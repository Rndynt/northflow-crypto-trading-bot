//! ControlAgent — operator command surface.
//!
//! Provides three ingress paths:
//!
//! 1. **Telegram bot long-poll** (`/status`, `/positions`, `/freeze`,
//!    `/unfreeze`, `/flat`, `/health`, `/help`, etc.).
//! 2. **Terminal stdin** (`status`, `positions`, `freeze`, `unfreeze`,
//!    `flat`, `health`, `help`) when running interactively.
//! 3. **Internal control file** at `/tmp/aria.control` — write a
//!    single line (`freeze`, `flat`, `unfreeze`, `status`) and the
//!    agent picks it up. Useful for headless servers without
//!    Telegram.
//!
//! Commands are translated into typed `ControlCommand` events on the
//! bus; downstream agents (`ExecutionAgent`, `SurvivalAgent`,
//! `MonitorAgent`) act on them.

use crate::agents::MessageBus;
use crate::agents::messages::{AgentEvent, AgentId, BrainOutcome, ControlCommand, SurvivalState};
use crate::config::ControlCfg;
use crate::execution::{Exchange, PositionBook, PositionConfig, RiskManager};
use crate::monitoring::{MetricsState, TradeJournal, telegram::InlineButton};
use chrono::Utc;
use parking_lot::{Mutex, RwLock};
use reqwest::Client;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt};
use tokio::task::JoinHandle;
use tracing::{info, warn};

/// Max recent brain outcomes to keep in memory for the /brain command.
const MAX_RECENT_BRAINS: usize = 20;

pub struct ControlAgentDeps {
    pub bus: MessageBus,
    pub cfg: ControlCfg,
    pub telegram_token: String,
    pub telegram_chat_id: String,
    /// Optional group+topic for signal notifications.
    pub telegram_signal_group_id: String,
    pub telegram_signal_topic_id: Option<i64>,
    pub risk: Arc<RiskManager>,
    pub book: Arc<PositionBook>,
    pub exchange: Arc<dyn Exchange>,
    /// Optional path for the file-based ingress. `None` = disable.
    pub control_file: Option<PathBuf>,
    /// Shared metrics for performance stats.
    pub metrics: Arc<MetricsState>,
    /// Shared survival state (updated by SurvivalAgent events).
    pub survival_state: Arc<RwLock<Option<SurvivalState>>>,
    /// Trade journal for /history command.
    pub journal: Option<Arc<TradeJournal>>,
    /// Initial equity from config, used for /reset command.
    pub initial_equity: f64,
    /// Shared position config — /hold command updates this.
    pub pos_cfg: Arc<parking_lot::RwLock<PositionConfig>>,
}

/// State tracked by the control agent from bus events.
struct ControlState {
    /// Recent brain outcomes, keyed by symbol (latest per symbol).
    recent_brains: Vec<BrainOutcome>, // kept short via MAX_RECENT_BRAINS
    /// Latest survival state.
    survival: Option<SurvivalState>,
    /// Latest mid-prices by symbol (updated from L2 events).
    prices: HashMap<String, f64>,
    /// Operator-command stats derived directly from the bus subscriber.
    ///
    /// `/status` and `/performance` must not depend solely on MonitorAgent's
    /// MetricsState because Telegram commands can arrive before that async
    /// consumer has processed the same event (or after it lagged/dropped one).
    stats: ControlStats,
    /// Initial equity from config, used for /reset command.
    initial_equity: f64,
}

#[derive(Debug, Clone, Default)]
struct ControlStats {
    signals_today: u64,
    trades_today: u64,
    llm_go: u64,
    llm_nogo: u64,
    llm_wait: u64,
    llm_avg_confidence: f64,
    llm_avg_latency_ms: u64,
    llm_offline_fallbacks: u64,
    active_lessons: u64,
    daily_pnl: f64,
    wins: u64,
    losses: u64,
    consecutive_losses: u64,
}

impl ControlStats {
    fn record_brain(&mut self, brain: &BrainOutcome) {
        let n = self.llm_go + self.llm_nogo + self.llm_wait;
        let avg = self.llm_avg_confidence * n as f64 + brain.decision.confidence as f64;
        match brain.decision.decision {
            crate::llm::engine::Decision::Go => self.llm_go += 1,
            crate::llm::engine::Decision::NoGo => self.llm_nogo += 1,
            crate::llm::engine::Decision::Wait => self.llm_wait += 1,
        }
        let total = self.llm_go + self.llm_nogo + self.llm_wait;
        self.llm_avg_confidence = avg / total.max(1) as f64;
        self.llm_avg_latency_ms =
            (self.llm_avg_latency_ms * total.saturating_sub(1) + brain.latency_ms) / total.max(1);
        if brain.offline_fallback {
            self.llm_offline_fallbacks += 1;
        }
    }

    fn record_closed_trade(&mut self, pnl_usd: f64) {
        self.trades_today += 1;
        self.daily_pnl += pnl_usd;
        if pnl_usd >= 0.0 {
            self.wins += 1;
            self.consecutive_losses = 0;
        } else {
            self.losses += 1;
            self.consecutive_losses += 1;
        }
    }
}

pub fn spawn(deps: ControlAgentDeps) -> JoinHandle<()> {
    let ControlAgentDeps {
        bus,
        cfg,
        telegram_token,
        telegram_chat_id,
        telegram_signal_group_id: _,
        telegram_signal_topic_id: _,
        risk,
        book,
        exchange: _exchange,
        control_file,
        metrics,
        survival_state,
        journal,
        initial_equity,
        pos_cfg,
    } = deps;

    let allowed: HashSet<i64> = cfg.allowed_user_ids.iter().copied().collect();

    // Shared control state — updated by a bus subscriber task.
    let ctrl_state: Arc<Mutex<ControlState>> = Arc::new(Mutex::new(ControlState {
        recent_brains: Vec::new(),
        survival: None,
        prices: HashMap::new(),
        stats: ControlStats::default(),
        initial_equity,
    }));

    // Bus subscriber to track brain outcomes and survival updates.
    {
        let bus_sub = bus.clone();
        let ctrl_state = ctrl_state.clone();
        let survival_state = survival_state.clone();
        tokio::spawn(async move {
            let mut rx = bus_sub.subscribe();
            loop {
                let ev = match rx.recv().await {
                    Ok(ev) => ev,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "broadcast lagged — skipping events");
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                };
                match ev {
                    AgentEvent::BrainOutcomeReady(brain) => {
                        // Signal notification is handled by monitor.rs (inline, ordered).
                        // Do NOT send here — fire-and-forget causes race with position notif.

                        let mut st = ctrl_state.lock();
                        st.stats.record_brain(&brain);
                        // Deduplicate: keep only latest per symbol
                        st.recent_brains
                            .retain(|b| b.signal.symbol != brain.signal.symbol);
                        st.recent_brains.push(brain);
                        while st.recent_brains.len() > MAX_RECENT_BRAINS {
                            st.recent_brains.remove(0);
                        }
                    }
                    AgentEvent::BookTicker {
                        symbol,
                        best_bid,
                        best_ask,
                        ..
                    } if best_bid > 0.0 && best_ask > 0.0 => {
                        let mid = (best_bid + best_ask) / 2.0;
                        ctrl_state.lock().prices.insert(symbol, mid);
                    }
                    AgentEvent::PreSignalEmitted { .. } => {
                        ctrl_state.lock().stats.signals_today += 1;
                    }
                    AgentEvent::PositionClosed { pnl_usd, .. } => {
                        ctrl_state.lock().stats.record_closed_trade(pnl_usd);
                    }
                    // BUG FIX: partial TP must add to daily_pnl shown in Session Stats.
                    // Does NOT increment trade count or win/loss — the position is not
                    // fully closed yet. Only daily_pnl needs updating here.
                    AgentEvent::PositionReduced { pnl_usd, .. } => {
                        ctrl_state.lock().stats.daily_pnl += pnl_usd;
                    }
                    AgentEvent::PolicyRefreshed { lessons_count, .. } => {
                        ctrl_state.lock().stats.active_lessons = lessons_count as u64;
                    }
                    AgentEvent::SurvivalUpdated(s) => {
                        ctrl_state.lock().survival = Some(s.clone());
                        *survival_state.write() = Some(s);
                    }
                    AgentEvent::Shutdown => break,
                    _ => {}
                }
            }
        });
    }

    if cfg.telegram_commands_enabled && !telegram_token.is_empty() && !telegram_chat_id.is_empty() {
        info!(
            "telegram loop spawning token_len={} chat_id={}",
            telegram_token.len(),
            telegram_chat_id
        );
        let bus_t = bus.clone();
        let risk_t = risk.clone();
        let book_t = book.clone();
        let metrics_t = metrics.clone();
        let ctrl_state_t = ctrl_state.clone();
        let journal_t = journal.clone();
        let token = telegram_token.clone();
        let chat_id = telegram_chat_id.clone();
        let poll_secs = cfg.poll_secs.max(1);
        let pos_cfg_t = pos_cfg.clone();
        tokio::spawn(async move {
            telegram_loop(
                bus_t,
                token,
                chat_id,
                allowed,
                risk_t,
                book_t,
                metrics_t,
                ctrl_state_t,
                journal_t,
                poll_secs,
                pos_cfg_t,
            )
            .await;
        });
    }

    {
        let bus_s = bus.clone();
        let risk_s = risk.clone();
        let book_s = book.clone();
        let metrics_s = metrics.clone();
        let ctrl_state_s = ctrl_state.clone();
        let journal_s = journal.clone();
        let pos_cfg_s = pos_cfg.clone();
        tokio::spawn(async move {
            stdin_loop(
                bus_s,
                risk_s,
                book_s,
                metrics_s,
                ctrl_state_s,
                journal_s,
                pos_cfg_s,
            )
            .await;
        });
    }

    // File-based control surface.
    if let Some(path) = control_file {
        let bus_f = bus.clone();
        tokio::spawn(async move {
            file_loop(bus_f, path).await;
        });
    }

    // Watchdog → freeze/unfreeze handler. Keeps RiskManager in sync
    // with operator commands routed through the bus.
    let mut rx = bus.subscribe();
    let risk_ev = risk.clone();
    tokio::spawn(async move {
        info!("control agent starting");
        crate::agents::heartbeat::spawn(bus.clone(), AgentId::Control);
        loop {
            let ev = match rx.recv().await {
                Ok(ev) => ev,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    warn!(skipped = n, "broadcast lagged — skipping events");
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            };
            match ev {
                AgentEvent::ControlCommand(ControlCommand::Freeze { reason }) => {
                    risk_ev.freeze(reason);
                }
                AgentEvent::ControlCommand(ControlCommand::Unfreeze) => {
                    risk_ev.unfreeze();
                }
                AgentEvent::Shutdown => break,
                _ => {}
            }
        }
    })
}

async fn telegram_loop(
    bus: MessageBus,
    token: String,
    chat_id: String,
    allowed: HashSet<i64>,
    risk: Arc<RiskManager>,
    book: Arc<PositionBook>,
    metrics: Arc<MetricsState>,
    ctrl_state: Arc<Mutex<ControlState>>,
    journal: Option<Arc<TradeJournal>>,
    poll_secs: u64,
    pos_cfg: Arc<parking_lot::RwLock<PositionConfig>>,
) {
    #![allow(clippy::too_many_arguments)]
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(poll_secs * 4))
        .build()
        .unwrap_or_default();
    let last_update_id: Arc<Mutex<i64>> = Arc::new(Mutex::new(0));

    // Register bot commands with Telegram so they appear in the "/" menu
    let cmd_url = format!("https://api.telegram.org/bot{token}/setMyCommands");
    let cmd_body = serde_json::json!({
        "commands": [
            {"command": "status", "description": "📊 Full bot status & risk overview"},
            {"command": "positions", "description": "📈 Open positions with P&L"},
            {"command": "signals", "description": "🔔 Recent AI signal analysis"},
            {"command": "brain", "description": "🧠 Last AI brain analysis"},
            {"command": "performance", "description": "📋 Daily/weekly performance"},
            {"command": "config", "description": "⚙ Config panel (leverage, risk, thresholds)"},
            {"command": "lessons", "description": "🧠 Learning system & active lessons"},
            {"command": "reset", "description": "🔄 Reset equity & clear lessons"},
            {"command": "leverage", "description": "⚡ View/change leverage"},
            {"command": "hold", "description": "⏱ View/change max hold time"},
            {"command": "breakeven", "description": "🔒 View/change breakeven threshold"},
            {"command": "risk", "description": "🛡 Risk metrics & limits"},
            {"command": "survival", "description": "🏥 Survival mode details"},
            {"command": "history", "description": "📜 Recent trade history"},
            {"command": "health", "description": "💚 System health check"},
            {"command": "freeze", "description": "⏸ Pause trading"},
            {"command": "unfreeze", "description": "▶ Resume trading"},
            {"command": "flat", "description": "🚨 Close ALL positions"},
            {"command": "help", "description": "📖 Command list"}
        ]
    });
    if let Err(e) = client.post(&cmd_url).json(&cmd_body).send().await {
        warn!(error = %e, "setMyCommands failed");
    } else {
        info!("registered {} bot commands with Telegram", 15);
    }

    // ── Startup drain ──────────────────────────────────────────────────────────
    // On restart, Telegram re-queues every unacknowledged update since the bot
    // was last running (offset=0 → all pending). We skip them without processing
    // by calling getUpdates?timeout=0 once and advancing past the highest id.
    {
        let drain_url =
            format!("https://api.telegram.org/bot{token}/getUpdates?timeout=0&limit=100");
        match client.get(&drain_url).send().await {
            Ok(resp) => {
                if let Ok(body) = resp.json::<Value>().await {
                    let max_id = body
                        .get("result")
                        .and_then(|v| v.as_array())
                        .and_then(|arr| {
                            arr.iter()
                                .filter_map(|u| u.get("update_id").and_then(|v| v.as_i64()))
                                .max()
                        })
                        .unwrap_or(0);
                    if max_id > 0 {
                        *last_update_id.lock() = max_id;
                        info!(
                            drained_up_to = max_id,
                            "telegram: drained stale queued updates"
                        );
                    }
                }
            }
            Err(e) => warn!(error = %e, "telegram startup drain failed"),
        }
    }

    // In-session dedup guard — prevents double-processing if the same update_id
    // somehow appears in two consecutive poll responses (network retry race).
    let mut seen_ids: std::collections::HashSet<i64> = std::collections::HashSet::new();

    loop {
        let offset = *last_update_id.lock() + 1;
        let url = format!(
            "https://api.telegram.org/bot{token}/getUpdates?offset={offset}&timeout={poll_secs}"
        );
        match client.get(&url).send().await {
            Ok(resp) => {
                let body: Value = match resp.json().await {
                    Ok(v) => v,
                    Err(e) => {
                        warn!(error = %e, "telegram getUpdates parse failed");
                        tokio::time::sleep(std::time::Duration::from_secs(poll_secs)).await;
                        continue;
                    }
                };
                let updates = body
                    .get("result")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                for upd in updates {
                    let update_id = upd.get("update_id").and_then(|v| v.as_i64()).unwrap_or(0);
                    // Always advance the stored offset so the next poll starts past this id
                    if update_id > *last_update_id.lock() {
                        *last_update_id.lock() = update_id;
                    }
                    // Skip if we already processed this update in this session
                    if !seen_ids.insert(update_id) {
                        warn!(update_id, "telegram: duplicate update_id — skipped");
                        continue;
                    }
                    // Keep seen_ids bounded to last 500 entries
                    if seen_ids.len() > 500 {
                        seen_ids.clear();
                    }

                    // ─── Handle callback queries (inline button clicks) ───
                    if let Some(cb) = upd.get("callback_query") {
                        let cb_id = cb.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        let data = cb.get("data").and_then(|v| v.as_str()).unwrap_or("");
                        let cb_from = cb
                            .get("from")
                            .and_then(|f| f.get("id"))
                            .and_then(|i| i.as_i64())
                            .unwrap_or(0);
                        let cb_chat = cb
                            .get("message")
                            .and_then(|m| m.get("chat"))
                            .and_then(|c| c.get("id"))
                            .and_then(|i| i.as_i64())
                            .map(|id| id.to_string())
                            .unwrap_or_else(|| chat_id.clone());

                        if !allowed.is_empty() && !allowed.contains(&cb_from) {
                            let _ =
                                send_telegram(&client, &token, &cb_chat, "⛔ not allowed").await;
                            continue;
                        }

                        // Map callback data to command
                        let cmd = match data {
                            "btn_status" => "/status",
                            "btn_positions" => "/positions",
                            "btn_refresh_positions" => "/positions",
                            "btn_signals" => "/signals",
                            "btn_performance" => "/performance",
                            "btn_risk" => "/risk",
                            "btn_history" => "/history",
                            "btn_leverage" => "/leverage",
                            "btn_hold" => "/hold",
                            "hold_5m" => "/hold 5m",
                            "hold_15m" => "/hold 15m",
                            "hold_30m" => "/hold 30m",
                            "hold_1h" => "/hold 1h",
                            "be_0.3" => "/breakeven 0.3",
                            "be_0.5" => "/breakeven 0.5",
                            "be_0.6" => "/breakeven 0.6",
                            "be_1.0" => "/breakeven 1.0",
                            "btn_survival" => "/survival",
                            "btn_brain" => "/brain",
                            "btn_health" => "/health",
                            "btn_freeze" => "/freeze",
                            "btn_unfreeze" => "/unfreeze",
                            "btn_flat" => "/flat",
                            "btn_help" => "/help",
                            "btn_lessons" => "/lessons",
                            "btn_config" => "/config",
                            _ if data.starts_with("cfg_lev_") => {
                                // Leverage preset: cfg_lev_20, cfg_lev_50, etc.
                                let val = data.strip_prefix("cfg_lev_").unwrap_or("");
                                let reply = cmd_leverage(&risk, &format!("/leverage {val}"));
                                let _ = send_telegram_html(&client, &token, &cb_chat, &reply).await;
                                let answer_url = format!(
                                    "https://api.telegram.org/bot{token}/answerCallbackQuery"
                                );
                                let _ = client
                                    .post(&answer_url)
                                    .json(&serde_json::json!({ "callback_query_id": cb_id }))
                                    .send()
                                    .await;
                                continue;
                            }
                            _ if data.starts_with("cfg_risk_") => {
                                // Risk preset: cfg_risk_1, cfg_risk_2, cfg_risk_5
                                let val = data.strip_prefix("cfg_risk_").unwrap_or("1");
                                if let Ok(pct) = val.parse::<f64>() {
                                    risk.set_risk_per_trade_pct(pct);
                                    let reply = format!(
                                        "✅ <b>Risk Updated</b>\n──────────\n📐 Risk per trade: <code>{pct:.1}%</code>\n🤖 ARIA v1.0"
                                    );
                                    let _ =
                                        send_telegram_html(&client, &token, &cb_chat, &reply).await;
                                }
                                let answer_url = format!(
                                    "https://api.telegram.org/bot{token}/answerCallbackQuery"
                                );
                                let _ = client
                                    .post(&answer_url)
                                    .json(&serde_json::json!({ "callback_query_id": cb_id }))
                                    .send()
                                    .await;
                                continue;
                            }
                            _ if data.starts_with("cfg_maxpos_") => {
                                let val = data.strip_prefix("cfg_maxpos_").unwrap_or("3");
                                if let Ok(n) = val.parse::<u32>() {
                                    risk.set_max_open_positions(n);
                                    let reply = format!(
                                        "✅ <b>Max Positions Updated</b>\n──────────\n📊 Max open: <code>{n}</code>\n🤖 ARIA v1.0"
                                    );
                                    let _ =
                                        send_telegram_html(&client, &token, &cb_chat, &reply).await;
                                }
                                let answer_url = format!(
                                    "https://api.telegram.org/bot{token}/answerCallbackQuery"
                                );
                                let _ = client
                                    .post(&answer_url)
                                    .json(&serde_json::json!({ "callback_query_id": cb_id }))
                                    .send()
                                    .await;
                                continue;
                            }
                            _ if data.starts_with("close_") => {
                                let sym = data.strip_prefix("close_").unwrap_or("");
                                let full_sym = format!("{}USDT", sym.to_uppercase());
                                bus.publish(AgentEvent::ControlCommand(
                                    ControlCommand::ClosePosition {
                                        symbol: full_sym.clone(),
                                    },
                                ));
                                let reply = format!(
                                    "🔧 <b>Closing {}</b>...\n🤖 ARIA v1.0",
                                    sym.to_uppercase()
                                );
                                let _ = send_telegram_html(&client, &token, &cb_chat, &reply).await;
                                let answer_url = format!(
                                    "https://api.telegram.org/bot{token}/answerCallbackQuery"
                                );
                                let _ = client
                                    .post(&answer_url)
                                    .json(&serde_json::json!({ "callback_query_id": cb_id }))
                                    .send()
                                    .await;
                                continue;
                            }
                            _ => "",
                        };

                        if !cmd.is_empty() {
                            let reply = handle_command(
                                cmd,
                                &bus,
                                &risk,
                                &book,
                                &metrics,
                                &ctrl_state,
                                &journal,
                                &pos_cfg,
                            );
                            if !reply.is_empty() {
                                // Reattach inline keyboard buttons (same as text command path)
                                let mut buttons = command_buttons(cmd);

                                // Add close buttons for each open position
                                if cmd == "/positions" || cmd == "positions" {
                                    let positions = book.snapshot();
                                    let mut close_row = Vec::new();
                                    for p in &positions {
                                        let sym = p.symbol.replace("USDT", "");
                                        close_row.push(InlineButton {
                                            text: format!("❌ Close {}", sym),
                                            callback_data: format!("close_{}", sym.to_lowercase()),
                                        });
                                    }
                                    if !close_row.is_empty() {
                                        buttons.insert(0, close_row);
                                    }
                                }

                                if !buttons.is_empty() {
                                    send_telegram_html_with_buttons(
                                        &client, &token, &cb_chat, &reply, buttons,
                                    )
                                    .await;
                                } else {
                                    send_telegram_html(&client, &token, &cb_chat, &reply).await;
                                }
                            }
                        }

                        // Answer callback to remove loading indicator
                        let answer_url =
                            format!("https://api.telegram.org/bot{token}/answerCallbackQuery");
                        let _ = client
                            .post(&answer_url)
                            .json(&serde_json::json!({ "callback_query_id": cb_id }))
                            .send()
                            .await;
                        continue;
                    }

                    // ─── Handle text messages ───
                    let msg = upd.get("message").cloned().unwrap_or(Value::Null);
                    let from_id = msg
                        .get("from")
                        .and_then(|f| f.get("id"))
                        .and_then(|i| i.as_i64())
                        .unwrap_or(0);
                    // Get originating chat_id (could be DM or group)
                    let origin_chat_id = msg
                        .get("chat")
                        .and_then(|c| c.get("id"))
                        .and_then(|i| i.as_i64())
                        .map(|id| id.to_string())
                        .unwrap_or_else(|| chat_id.clone());
                    // Get thread_id if message is in a forum topic
                    let origin_thread_id = msg.get("message_thread_id").and_then(|t| t.as_i64());
                    let text = msg
                        .get("text")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .to_string();
                    if !allowed.is_empty() && !allowed.contains(&from_id) {
                        send_telegram(
                            &client,
                            &token,
                            &origin_chat_id,
                            &format!("⛔ user {from_id} not allowed"),
                        )
                        .await;
                        continue;
                    }
                    let reply = handle_command(
                        &text,
                        &bus,
                        &risk,
                        &book,
                        &metrics,
                        &ctrl_state,
                        &journal,
                        &pos_cfg,
                    );
                    if !reply.is_empty() {
                        let cmd_lower = text.trim().to_lowercase();

                        // Get contextual buttons for this command
                        let mut buttons = command_buttons(&cmd_lower);

                        // Add close buttons for each open position
                        if cmd_lower == "/positions" || cmd_lower == "positions" {
                            let positions = book.snapshot();
                            let mut close_row = Vec::new();
                            for p in &positions {
                                let sym = p.symbol.replace("USDT", "");
                                close_row.push(InlineButton {
                                    text: format!("❌ Close {}", sym),
                                    callback_data: format!("close_{}", sym.to_lowercase()),
                                });
                            }
                            if !close_row.is_empty() {
                                buttons.insert(0, close_row);
                            }
                        }

                        // Reply to originating chat (DM or group topic)
                        if let Some(thread_id) = origin_thread_id {
                            send_telegram_html_to_topic(
                                &client,
                                &token,
                                &origin_chat_id,
                                thread_id,
                                &reply,
                            )
                            .await;
                        } else if !buttons.is_empty() {
                            send_telegram_html_with_buttons(
                                &client,
                                &token,
                                &origin_chat_id,
                                &reply,
                                buttons,
                            )
                            .await;
                        } else {
                            send_telegram_html(&client, &token, &origin_chat_id, &reply).await;
                        }
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "telegram getUpdates failed");
                tokio::time::sleep(std::time::Duration::from_secs(poll_secs)).await;
            }
        }
    }
}

async fn send_telegram(client: &Client, token: &str, chat_id: &str, text: &str) {
    let url = format!("https://api.telegram.org/bot{token}/sendMessage");
    let body = serde_json::json!({
        "chat_id": chat_id,
        "text": text,
        "disable_web_page_preview": true,
        "parse_mode": "Markdown",
    });
    if let Err(e) = client.post(&url).json(&body).send().await {
        warn!(error = %e, "telegram send failed");
    }
}

async fn send_telegram_html(client: &Client, token: &str, chat_id: &str, text: &str) {
    let url = format!("https://api.telegram.org/bot{token}/sendMessage");
    let body = serde_json::json!({
        "chat_id": chat_id,
        "text": text,
        "disable_web_page_preview": true,
        "parse_mode": "HTML",
    });
    if let Err(e) = client.post(&url).json(&body).send().await {
        warn!(error = %e, "telegram send failed");
    }
}

/// Send HTML message to a forum topic (message_thread_id).
async fn send_telegram_html_to_topic(
    client: &Client,
    token: &str,
    group_id: &str,
    thread_id: i64,
    text: &str,
) {
    let url = format!("https://api.telegram.org/bot{token}/sendMessage");
    let body = serde_json::json!({
        "chat_id": group_id,
        "message_thread_id": thread_id,
        "text": text,
        "disable_web_page_preview": true,
        "parse_mode": "HTML",
    });
    if let Err(e) = client.post(&url).json(&body).send().await {
        warn!(error = %e, "telegram topic send failed");
    }
}

/// Send HTML message with inline keyboard buttons.
async fn send_telegram_html_with_buttons(
    client: &Client,
    token: &str,
    chat_id: &str,
    text: &str,
    buttons: Vec<Vec<InlineButton>>,
) {
    let url = format!("https://api.telegram.org/bot{token}/sendMessage");
    let keyboard: Vec<Vec<serde_json::Value>> = buttons
        .iter()
        .map(|row| {
            row.iter()
                .map(|btn| {
                    serde_json::json!({
                        "text": btn.text,
                        "callback_data": btn.callback_data,
                    })
                })
                .collect()
        })
        .collect();
    let body = serde_json::json!({
        "chat_id": chat_id,
        "text": text,
        "disable_web_page_preview": true,
        "parse_mode": "HTML",
        "reply_markup": {
            "inline_keyboard": keyboard,
        },
    });
    if let Err(e) = client.post(&url).json(&body).send().await {
        warn!(error = %e, "telegram buttons send failed");
    }
}

async fn stdin_loop(
    bus: MessageBus,
    risk: Arc<RiskManager>,
    book: Arc<PositionBook>,
    metrics: Arc<MetricsState>,
    ctrl_state: Arc<Mutex<ControlState>>,
    journal: Option<Arc<TradeJournal>>,
    pos_cfg: Arc<parking_lot::RwLock<PositionConfig>>,
) {
    let mut lines = io::BufReader::new(io::stdin()).lines();
    info!("stdin control ready — type `help`, then press Enter");
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                let reply = handle_command(
                    &line,
                    &bus,
                    &risk,
                    &book,
                    &metrics,
                    &ctrl_state,
                    &journal,
                    &pos_cfg,
                );
                if !reply.is_empty() {
                    // Strip HTML tags for terminal output
                    let plain = strip_html(&reply);
                    println!("{plain}");
                    info!(reply = %plain, "control command");
                }
            }
            Ok(None) => break,
            Err(e) => {
                warn!(error = %e, "stdin control read failed");
                break;
            }
        }
    }
}

/// Strip HTML tags for plain-text terminal output.
fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

fn handle_command(
    text: &str,
    bus: &MessageBus,
    risk: &Arc<RiskManager>,
    book: &Arc<PositionBook>,
    metrics: &Arc<MetricsState>,
    ctrl_state: &Arc<Mutex<ControlState>>,
    journal: &Option<Arc<TradeJournal>>,
    pos_cfg: &Arc<parking_lot::RwLock<PositionConfig>>,
) -> String {
    let cmd = text.trim().to_lowercase();
    match cmd.as_str() {
        "/status" | "status" => cmd_status(bus, risk, book, metrics, ctrl_state),
        "/positions" | "positions" => {
            let prices = ctrl_state.lock().prices.clone();
            cmd_positions(book, &prices, risk)
        }
        "/signals" | "signals" => cmd_signals(ctrl_state),
        "/performance" | "performance" => cmd_performance(risk, metrics, ctrl_state),
        "/survival" | "survival" => cmd_survival(ctrl_state),
        "/freeze" | "freeze" => cmd_freeze(bus),
        "/unfreeze" | "unfreeze" => cmd_unfreeze(bus),
        "/flat" | "flat" => cmd_flat(bus),
        "/health" | "health" => cmd_health(bus, risk, metrics),
        "/brain" | "brain" => cmd_brain(ctrl_state),
        "/risk" | "risk" => cmd_risk(risk),
        "/history" | "history" => cmd_history(journal),
        "/leverage" | "leverage" if !cmd.contains(' ') => cmd_leverage(risk, ""),
        _ if cmd.starts_with("/leverage ") || cmd.starts_with("leverage ") => {
            cmd_leverage(risk, &cmd)
        }
        "/hold" | "hold" if !cmd.contains(' ') => cmd_hold(bus, pos_cfg, ""),
        _ if cmd.starts_with("/hold ") || cmd.starts_with("hold ") => cmd_hold(bus, pos_cfg, &cmd),
        "/breakeven" | "breakeven" if !cmd.contains(' ') => cmd_breakeven(pos_cfg, ""),
        _ if cmd.starts_with("/breakeven ") || cmd.starts_with("breakeven ") => {
            cmd_breakeven(pos_cfg, &cmd)
        }
        "/config" | "config" => cmd_config(risk),
        "/lessons" | "lessons" => cmd_lessons(),
        "/reset" | "reset" => {
            let initial_equity = ctrl_state.lock().initial_equity;
            cmd_reset(risk, book, bus, initial_equity)
        }
        "/help" | "help" | "/start" | "start" => cmd_help(),
        _ => String::new(),
    }
}

// ─── Command implementations ───────────────────────────────────────

fn cmd_help() -> String {
    "🤖 <b>ARIA Command Center</b>\n\
     ──────────\n\
     \n\
     📊 <b>Monitoring</b>\n\
     ├ <code>/status</code> — Full bot status & risk overview\n\
     ├ <code>/positions</code> — List open positions with P&L\n\
     ├ <code>/signals</code> — Recent AI signal analysis\n\
     ├ <code>/brain</code> — Last AI brain analysis per symbol\n\
     ├ <code>/performance</code> — Daily/weekly performance stats\n\
     ├ <code>/survival</code> — Survival mode details\n\
     ├ <code>/risk</code> — Current risk metrics & limits\n\
     ├ <code>/leverage</code> — View/change leverage settings\n\
     ├ <code>/config</code> — ⚙ Config panel (all settings)\n\
     ├ <code>/history</code> — Recent trade history (NeonDB)\n\
     ├ <code>/lessons</code> — 🧠 Learning system & active lessons\n\
     ├ <code>/reset</code> — 🔄 Reset equity & clear lessons (fresh start)\n\
     └ <code>/health</code> — System health check\n\
     \n\
     🎮 <b>Control</b>\n\
     ├ <code>/freeze</code> — Pause trading (block new entries)\n\
     ├ <code>/unfreeze</code> — Resume trading\n\
     └ <code>/flat</code> — ⚠ Close ALL positions immediately\n\
     \n\
     👆 <b>Tap the buttons below for quick access!</b>\n\
     \n\
     🤖 ARIA v1.0"
        .to_string()
}

fn cmd_reset(
    risk: &Arc<RiskManager>,
    book: &Arc<PositionBook>,
    bus: &MessageBus,
    initial_equity: f64,
) -> String {
    // 1. Close all positions
    let positions = book.snapshot();
    let pos_count = positions.len();
    for p in &positions {
        book.close(&p.client_id);
    }

    // 2. Reset equity to configured initial value (not hardcoded)
    risk.set_equity(initial_equity);

    // 3. Clear learning state
    let _ = std::fs::remove_file("data/learning_state.json");

    // 4. Clear positions file (already cleared by book.close, but just in case)
    let _ = std::fs::remove_file("data/positions.json");

    // 5. Publish flat all to execution agent
    bus.publish(AgentEvent::ControlCommand(ControlCommand::FlatAll {
        reason: "operator /reset".into(),
    }));

    format!(
        "🔄 <b>RESET COMPLETE</b>\n──────────\n         ├ Closed positions: <code>{pos_count}</code>\n         ├ Equity reset: <code>${initial_equity:.2}</code>\n         ├ Learning state: <b>cleared</b>\n\
         ├ Trade history: preserved in DB\n\
         └ Fresh start ready\n\
         🤖 ARIA v1.0"
    )
}

fn cmd_lessons() -> String {
    const LEARNING_STATE_PATH: &str = "data/learning_state.json";
    let data = match std::fs::read_to_string(LEARNING_STATE_PATH) {
        Ok(d) => d,
        Err(_) => {
            return "📭 <b>No learning data yet</b>
Bot needs more trades to generate lessons.
🤖 ARIA v1.0"
                .to_string();
        }
    };
    let snap: serde_json::Value = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(e) => {
            return format!(
                "⚠ <b>Error reading lessons:</b> {e}
🤖 ARIA v1.0"
            );
        }
    };

    let overall_trades = snap
        .get("overall_trades")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let overall_wins = snap
        .get("overall_wins")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let overall_losses = snap
        .get("overall_losses")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let overall_pnl = snap
        .get("overall_net_pnl")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let lessons_count = snap
        .get("lessons_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let wr = if overall_trades > 0 {
        overall_wins as f64 / overall_trades as f64 * 100.0
    } else {
        0.0
    };
    let pnl_sign = if overall_pnl >= 0.0 { "+" } else { "" };

    let mut lines = vec![
        "🧠 <b>Learning System</b>".to_string(),
        "──────────".to_string(),
        format!(
            "📊 Overall: <code>{overall_trades}</code> trades (<code>{overall_wins}W/{overall_losses}L</code> · <code>{wr:.1}%</code>)"
        ),
        format!("💰 Net PnL: <code>{pnl_sign}{overall_pnl:.2}$</code>"),
        format!("📋 Active Lessons: <code>{lessons_count}</code>"),
        "".to_string(),
    ];

    if let Some(lessons) = snap.get("lessons").and_then(|v| v.as_array()) {
        if lessons.is_empty() {
            lines.push("  └ (no active lessons)".to_string());
        } else {
            lines.push("📋 <b>Active Lessons</b>".to_string());
            lines.push("──────────".to_string());
            for (i, lesson) in lessons.iter().enumerate().take(10) {
                let kind = lesson.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
                let strategy = lesson
                    .get("strategy")
                    .and_then(|v| v.as_str())
                    .unwrap_or("*");
                let symbol = lesson.get("symbol").and_then(|v| v.as_str()).unwrap_or("*");
                let regime = lesson.get("regime").and_then(|v| v.as_str()).unwrap_or("*");
                let size_mult = lesson
                    .get("size_multiplier")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(1.0);
                let reason = lesson.get("reason").and_then(|v| v.as_str()).unwrap_or("");
                let valid_until = lesson
                    .get("valid_until")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let kind_emoji = match kind {
                    "Derate" => "🟡",
                    "Boost" => "🟢",
                    "Cooldown" => "🟠",
                    "Blacklist" => "🔴",
                    "LlmCalibration" => "🧠",
                    _ => "⚪",
                };
                lines.push(format!(
                    "{kind_emoji} <b>#{}</b> {kind}
  ├ Strategy: <code>{}</code> · Symbol: <code>{}</code>
  ├ Regime: <code>{}</code> · Size: <code>{:.2}×</code>
  ├ <i>{}</i>
  └ Expires: <code>{}</code>",
                    i + 1,
                    strategy,
                    symbol,
                    regime,
                    size_mult,
                    reason,
                    &valid_until[..16.min(valid_until.len())]
                ));
            }
        }
    }

    lines.push("".to_string());
    lines.push("🤖 ARIA v1.0".to_string());
    lines.join(
        "
",
    )
}

/// Build inline keyboard buttons for the /help message.
fn help_buttons() -> Vec<Vec<InlineButton>> {
    vec![
        // Row 1: Monitoring
        vec![
            InlineButton {
                text: "📊 Status".into(),
                callback_data: "btn_status".into(),
            },
            InlineButton {
                text: "📈 Positions".into(),
                callback_data: "btn_positions".into(),
            },
            InlineButton {
                text: "🔄 Refresh Pos".into(),
                callback_data: "btn_refresh_positions".into(),
            },
            InlineButton {
                text: "🔔 Signals".into(),
                callback_data: "btn_signals".into(),
            },
        ],
        // Row 2: Analytics
        vec![
            InlineButton {
                text: "🧠 Brain".into(),
                callback_data: "btn_brain".into(),
            },
            InlineButton {
                text: "📋 Performance".into(),
                callback_data: "btn_performance".into(),
            },
            InlineButton {
                text: "📜 History".into(),
                callback_data: "btn_history".into(),
            },
            InlineButton {
                text: "🎓 Lessons".into(),
                callback_data: "btn_lessons".into(),
            },
        ],
        // Row 3: Risk & Survival
        vec![
            InlineButton {
                text: "🛡 Risk".into(),
                callback_data: "btn_risk".into(),
            },
            InlineButton {
                text: "⚙ Config".into(),
                callback_data: "btn_config".into(),
            },
            InlineButton {
                text: "🏥 Survival".into(),
                callback_data: "btn_survival".into(),
            },
        ],
        // Row 4: Control (danger zone)
        vec![
            InlineButton {
                text: "💚 Health".into(),
                callback_data: "btn_health".into(),
            },
            InlineButton {
                text: "⏸ Freeze".into(),
                callback_data: "btn_freeze".into(),
            },
            InlineButton {
                text: "▶ Unfreeze".into(),
                callback_data: "btn_unfreeze".into(),
            },
        ],
        // Row 5: Emergency
        vec![InlineButton {
            text: "🚨 FLAT ALL POSITIONS".into(),
            callback_data: "btn_flat".into(),
        }],
    ]
}

/// Get contextual buttons for a given command. NO duplicates — each button appears once max.
fn command_buttons(cmd: &str) -> Vec<Vec<InlineButton>> {
    // Shared: single Help button for the nav row
    let help_btn = InlineButton {
        text: "🏠 Help".into(),
        callback_data: "btn_help".into(),
    };

    match cmd {
        "/help" | "help" | "/start" | "start" => help_buttons(),
        "/status" | "status" => vec![vec![
            InlineButton {
                text: "📈 Positions".into(),
                callback_data: "btn_positions".into(),
            },
            InlineButton {
                text: "📋 Performance".into(),
                callback_data: "btn_performance".into(),
            },
            InlineButton {
                text: "💚 Health".into(),
                callback_data: "btn_health".into(),
            },
            help_btn,
        ]],
        "/positions" | "positions" => vec![vec![
            InlineButton {
                text: "🔄 Refresh".into(),
                callback_data: "btn_refresh_positions".into(),
            },
            InlineButton {
                text: "📊 Status".into(),
                callback_data: "btn_status".into(),
            },
            InlineButton {
                text: "📜 History".into(),
                callback_data: "btn_history".into(),
            },
            InlineButton {
                text: "🚨 FLAT ALL".into(),
                callback_data: "btn_flat".into(),
            },
            help_btn,
        ]],
        "/signals" | "signals" => vec![vec![
            InlineButton {
                text: "🧠 Brain".into(),
                callback_data: "btn_brain".into(),
            },
            InlineButton {
                text: "📊 Status".into(),
                callback_data: "btn_status".into(),
            },
            InlineButton {
                text: "🛡 Risk".into(),
                callback_data: "btn_risk".into(),
            },
            help_btn,
        ]],
        "/brain" | "brain" => vec![vec![
            InlineButton {
                text: "🔔 Signals".into(),
                callback_data: "btn_signals".into(),
            },
            InlineButton {
                text: "📋 Performance".into(),
                callback_data: "btn_performance".into(),
            },
            InlineButton {
                text: "🛡 Risk".into(),
                callback_data: "btn_risk".into(),
            },
            help_btn,
        ]],
        "/performance" | "performance" => vec![vec![
            InlineButton {
                text: "📜 History".into(),
                callback_data: "btn_history".into(),
            },
            InlineButton {
                text: "🏥 Survival".into(),
                callback_data: "btn_survival".into(),
            },
            InlineButton {
                text: "📊 Status".into(),
                callback_data: "btn_status".into(),
            },
            help_btn,
        ]],
        "/risk" | "risk" => vec![vec![
            InlineButton {
                text: "⚙ Leverage".into(),
                callback_data: "btn_leverage".into(),
            },
            InlineButton {
                text: "📈 Positions".into(),
                callback_data: "btn_positions".into(),
            },
            InlineButton {
                text: "🏥 Survival".into(),
                callback_data: "btn_survival".into(),
            },
            help_btn,
        ]],
        "/leverage" | "leverage" => vec![vec![
            InlineButton {
                text: "🛡 Risk".into(),
                callback_data: "btn_risk".into(),
            },
            InlineButton {
                text: "📊 Status".into(),
                callback_data: "btn_status".into(),
            },
            InlineButton {
                text: "📈 Positions".into(),
                callback_data: "btn_positions".into(),
            },
            help_btn,
        ]],
        "/survival" | "survival" => vec![vec![
            InlineButton {
                text: "🛡 Risk".into(),
                callback_data: "btn_risk".into(),
            },
            InlineButton {
                text: "💚 Health".into(),
                callback_data: "btn_health".into(),
            },
            InlineButton {
                text: "📋 Performance".into(),
                callback_data: "btn_performance".into(),
            },
            help_btn,
        ]],
        "/history" | "history" => vec![vec![
            InlineButton {
                text: "📋 Performance".into(),
                callback_data: "btn_performance".into(),
            },
            InlineButton {
                text: "📈 Positions".into(),
                callback_data: "btn_positions".into(),
            },
            InlineButton {
                text: "📊 Status".into(),
                callback_data: "btn_status".into(),
            },
            help_btn,
        ]],
        "/health" | "health" => vec![vec![
            InlineButton {
                text: "📊 Status".into(),
                callback_data: "btn_status".into(),
            },
            InlineButton {
                text: "🏥 Survival".into(),
                callback_data: "btn_survival".into(),
            },
            InlineButton {
                text: "🧠 Brain".into(),
                callback_data: "btn_brain".into(),
            },
            help_btn,
        ]],
        "/freeze" | "freeze" => vec![vec![
            InlineButton {
                text: "▶ Unfreeze".into(),
                callback_data: "btn_unfreeze".into(),
            },
            InlineButton {
                text: "📊 Status".into(),
                callback_data: "btn_status".into(),
            },
            InlineButton {
                text: "🏥 Survival".into(),
                callback_data: "btn_survival".into(),
            },
            help_btn,
        ]],
        "/unfreeze" | "unfreeze" => vec![vec![
            InlineButton {
                text: "⏸ Freeze".into(),
                callback_data: "btn_freeze".into(),
            },
            InlineButton {
                text: "📊 Status".into(),
                callback_data: "btn_status".into(),
            },
            InlineButton {
                text: "📈 Positions".into(),
                callback_data: "btn_positions".into(),
            },
            help_btn,
        ]],
        "/flat" | "flat" => vec![vec![
            InlineButton {
                text: "📊 Status".into(),
                callback_data: "btn_status".into(),
            },
            InlineButton {
                text: "📈 Positions".into(),
                callback_data: "btn_positions".into(),
            },
            InlineButton {
                text: "⏸ Freeze".into(),
                callback_data: "btn_freeze".into(),
            },
            help_btn,
        ]],
        "/lessons" | "lessons" => vec![vec![
            InlineButton {
                text: "📊 Status".into(),
                callback_data: "btn_status".into(),
            },
            InlineButton {
                text: "📋 Performance".into(),
                callback_data: "btn_performance".into(),
            },
            InlineButton {
                text: "📜 History".into(),
                callback_data: "btn_history".into(),
            },
            help_btn,
        ]],
        "/config" | "config" => config_buttons(),
        _ => vec![],
    }
}

fn cmd_status(
    bus: &MessageBus,
    risk: &Arc<RiskManager>,
    book: &Arc<PositionBook>,
    metrics: &Arc<MetricsState>,
    ctrl_state: &Arc<Mutex<ControlState>>,
) -> String {
    bus.publish(AgentEvent::ControlCommand(ControlCommand::StatusRequest));
    let s = risk.snapshot();
    let limits = risk.limits();
    let positions = book.snapshot();
    let m = metrics.snapshot();
    let ctrl = ctrl_state.lock();
    let ctrl_stats = ctrl.stats.clone();
    let initial_equity = ctrl.initial_equity;
    drop(ctrl);
    let display_pnl = [ctrl_stats.daily_pnl, s.realized_pnl_today, m.daily_pnl]
        .into_iter()
        .max_by(|a, b| a.abs().total_cmp(&b.abs()))
        .unwrap_or(0.0);

    let status_emoji = if s.tripped {
        "🚨"
    } else if s.frozen {
        "🧊"
    } else {
        "✅"
    };
    let status_text = if s.tripped {
        "TRIPPED"
    } else if s.frozen {
        "FROZEN"
    } else {
        "ACTIVE"
    };
    let pnl_sign = if display_pnl >= 0.0 { "+" } else { "" };

    // Build positions summary
    let pos_lines: Vec<String> = positions
        .iter()
        .map(|p| {
            let side = if p.side == crate::data::Side::Long {
                "🟢 L"
            } else {
                "🔴 S"
            };
            format!(
                "  ├ {} <code>{}</code> size={:.4} @ {:.2}",
                side,
                short_sym_ctrl(&p.symbol),
                p.size,
                p.entry_price,
            )
        })
        .collect();
    let pos_section = if pos_lines.is_empty() {
        "  └ (none)".to_string()
    } else {
        pos_lines.join("\n")
    };

    format!(
        "{status_emoji} <b>ARIA STATUS</b> — {status_text}\n\
         ──────────\n\
         💰 <b>Account</b>\n\
         ├ Equity: <code>${equity:.2}</code>\n\
         ├ Peak: <code>${peak:.2}</code>\n\
         ├ Daily PnL: <code>{pnl_sign}{pnl:.2}$</code> ({pnl_pct:.2}%)\n\
         └ Drawdown: <code>{dd:.2}%</code>\n\
         \n\
         📊 <b>Trading</b>\n\
         ├ Positions: <code>{open_pos}</code> / {max_pos}\n\
         {pos_section}\n\
         ├ Signals Today: <code>{signals}</code>\n\
         ├ Trades Today: <code>{trades}</code>\n\
         └ Avg LLM Confidence: <code>{avg_conf:.0}%</code>\n\
         \n\
         🧠 <b>AI Pipeline</b>\n\
         ├ GO: <code>{go}</code> · NO-GO: <code>{nogo}</code> · WAIT: <code>{wait}</code>\n\
         └ Offline Fallbacks: <code>{offline}</code>\n\
         \n\
         ⚙ <b>Limits</b>\n\
         ├ Max DD: <code>{max_dd}%</code> · Max Daily Loss: <code>{max_dl}%</code>\n\
         ├ Risk/Trade: <code>{risk_pct}%</code> · Min R:R: <code>{min_rr}</code>\n\
         └ Frozen: <code>{frozen}</code> · Tripped: <code>{tripped}</code>\n\
         \n\
         🤖 ARIA v1.0",
        status_emoji = status_emoji,
        status_text = status_text,
        equity = s.equity,
        peak = s.peak_equity,
        pnl_sign = pnl_sign,
        pnl = display_pnl,
        pnl_pct = if initial_equity > 0.0 {
            display_pnl / initial_equity * 100.0
        } else {
            s.daily_loss_pct
        },
        dd = s.drawdown_pct,
        open_pos = positions.len(),
        max_pos = limits.max_open_positions,
        pos_section = pos_section,
        signals = m.signals_today.max(ctrl_stats.signals_today),
        trades = m.trades_today.max(ctrl_stats.trades_today),
        avg_conf = m.llm_avg_confidence.max(ctrl_stats.llm_avg_confidence),
        go = m.llm_go.max(ctrl_stats.llm_go),
        nogo = m.llm_nogo.max(ctrl_stats.llm_nogo),
        wait = m.llm_wait.max(ctrl_stats.llm_wait),
        offline = m
            .llm_offline_fallbacks
            .max(ctrl_stats.llm_offline_fallbacks),
        max_dd = limits.max_drawdown_pct,
        max_dl = limits.max_daily_loss_pct,
        risk_pct = limits.risk_per_trade_pct,
        min_rr = limits.min_reward_risk,
        frozen = s.frozen,
        tripped = s.tripped,
    )
}

fn cmd_positions(
    book: &Arc<PositionBook>,
    prices: &HashMap<String, f64>,
    risk: &Arc<RiskManager>,
) -> String {
    let positions = book.snapshot();
    if positions.is_empty() {
        let risk_open = risk.snapshot().open_positions;
        if risk_open > 0 {
            return format!(
                "⚠️ <b>Position state syncing</b>\n──────────\nRisk engine still reports <code>{risk_open}</code> open position(s), but the local position book is empty right now. Try again after the next market tick or check exchange reconciliation.\n🤖 ARIA v1.0"
            );
        }
        return "📭 <b>No open positions</b>\n🤖 ARIA v1.0".to_string();
    }

    let mut lines = Vec::new();
    lines.push("📋 <b>Open Positions</b>".to_string());
    lines.push("──────────".to_string());

    let mut total_pnl = 0.0f64;

    for (i, p) in positions.iter().enumerate() {
        let side_emoji = if p.side == crate::data::Side::Long {
            "🟢"
        } else {
            "🔴"
        };
        let side_label = if p.side == crate::data::Side::Long {
            "LONG"
        } else {
            "SHORT"
        };
        let trailing = if p.trailing_activated { " 🔄" } else { "" };
        let be = if p.breakeven_activated { " 🔒" } else { "" };

        // Current price and unrealized PnL
        let current = prices.get(&p.symbol).copied().unwrap_or(0.0);
        let (pnl_str, pnl_emoji, pnl_pct_str) = if current > 0.0 && p.entry_price > 0.0 {
            let price_pct = match p.side {
                crate::data::Side::Long => (current - p.entry_price) / p.entry_price * 100.0,
                crate::data::Side::Short => (p.entry_price - current) / p.entry_price * 100.0,
            };
            let pnl_usd = price_pct / 100.0 * p.size * p.entry_price;
            // ROE = price change × leverage
            let max_lev = risk.limits().max_leverage as f64;
            let roe_pct = price_pct * max_lev;
            total_pnl += pnl_usd;
            let sign = if pnl_usd >= 0.0 { "+" } else { "" };
            let emoji = if pnl_usd >= 0.0 { "📈" } else { "📉" };
            (
                format!("{}{:.2}$", sign, pnl_usd),
                emoji,
                format!("({}{:.2}% ROE)", sign, roe_pct),
            )
        } else {
            ("—".to_string(), "⚪", "".to_string())
        };

        // Duration
        let now = Utc::now();
        let dur = now - p.opened_at;
        let mins = dur.num_minutes();
        let duration = if mins >= 60 {
            format!("{}h {}m", mins / 60, mins % 60)
        } else {
            format!("{}m", mins)
        };

        // Price change from entry
        let price_line = if current > 0.0 {
            format!("├ Current: <code>{:.4}</code>\n", current)
        } else {
            String::new()
        };

        // Notional value in USD
        let notional_usd = p.size * p.entry_price;
        let max_lev = risk.limits().max_leverage as f64;
        let margin_usd = if max_lev > 0.0 {
            notional_usd / max_lev
        } else {
            notional_usd
        };

        // Partial TP realized PnL — only show when partial was taken
        let partial_line = if p.partial_taken && p.partial_realized_pnl.abs() > 0.001 {
            let sign = if p.partial_realized_pnl >= 0.0 { "+" } else { "" };
            format!(
                "├ 💰 Partial Realized: <code>{}{:.2}$</code>\n",
                sign, p.partial_realized_pnl
            )
        } else {
            String::new()
        };

        // Add partial realized to the position total so the footer "Total PnL"
        // reflects runner unrealized + what was already banked on this position.
        // (Runner unrealized was already accumulated in total_pnl above.)
        total_pnl += p.partial_realized_pnl;

        lines.push(format!(
            "{side_emoji} <b>#{idx} {sym}</b> — {side_label}{trailing}{be}\n\
             🆔 <code>{signal_id}</code>\n\
             ├ Entry: <code>{entry:.4}</code>\n\
             {price_line}\
             ├ SL: <code>{sl:.4}</code> · TP: <code>{tp:.4}</code>\n\
             ├ Size: <code>{size:.4}</code> ({notional:.2}$)\n\
             ├ ⚡ {leverage:.0}x · Margin: <code>{margin:.2}$</code>\n\
             {partial_line}\
             ├ {pnl_emoji} PnL: <code>{pnl}</code> {pnl_pct}\n\
             └ Duration: <code>{duration}</code> · Opened: <code>{opened}</code>",
            idx = i + 1,
            sym = short_sym_ctrl(&p.symbol),
            signal_id = if p.signal_id.is_empty() { "—" } else { &p.signal_id },
            entry = p.entry_price,
            price_line = price_line,
            sl = p.stop_loss,
            tp = p.take_profit,
            size = p.size,
            notional = notional_usd,
            leverage = max_lev,
            margin = margin_usd,
            partial_line = partial_line,
            pnl_emoji = pnl_emoji,
            pnl = pnl_str,
            pnl_pct = pnl_pct_str,
            duration = duration,
            opened = p.opened_at.format("%H:%M UTC"),
        ));
    }

    // Total PnL = unrealized (runners) + partial realized (banked from partial TP closes)
    let total_sign = if total_pnl >= 0.0 { "+" } else { "" };
    let total_emoji = if total_pnl >= 0.0 { "📈" } else { "📉" };
    lines.push("──────────".to_string());
    lines.push(format!(
        "{emoji} <b>Total PnL (Unrealized + Partial):</b> <code>{sign}{pnl:.2}$</code>",
        emoji = total_emoji,
        sign = total_sign,
        pnl = total_pnl
    ));
    lines.push("🤖 ARIA v1.0".to_string());
    lines.join("\n")
}

fn cmd_signals(ctrl_state: &Arc<Mutex<ControlState>>) -> String {
    let st = ctrl_state.lock();
    if st.recent_brains.is_empty() {
        return "📭 <b>No recent signals</b>\n🤖 ARIA v1.0".to_string();
    }

    let mut lines = Vec::new();
    lines.push("🔔 <b>Recent Signals</b>".to_string());
    lines.push("──────────".to_string());

    for brain in st.recent_brains.iter().rev().take(10) {
        let decision_emoji = match brain.decision.decision {
            crate::llm::engine::Decision::Go => "✅",
            crate::llm::engine::Decision::NoGo => "🚫",
            crate::llm::engine::Decision::Wait => "⏳",
        };
        let side = if brain.signal.side == crate::data::Side::Long {
            "📈 L"
        } else {
            "📉 S"
        };
        let summary = truncate_ctrl(&brain.decision.reasoning.summary, 80);
        lines.push(format!(
            "{decision_emoji} <b>{sym}</b> {side} · conf={conf}%\n\
             ├ Strategy: <code>{strat}</code>\n\
             ├ Scores: TA={ta} Sent={sent} Comp={comp}\n\
             └ <i>{summary}</i>",
            sym = short_sym_ctrl(&brain.signal.symbol),
            side = side,
            conf = brain.decision.confidence,
            strat = brain.signal.strategy.as_str(),
            ta = brain.decision.market_context_score.ta_score,
            sent = brain.decision.market_context_score.sentiment_score,
            comp = brain.decision.market_context_score.composite_score,
            summary = html_escape_ctrl(&summary),
        ));
    }

    lines.push("──────────".to_string());
    lines.push("🤖 ARIA v1.0".to_string());
    lines.join("\n")
}

fn cmd_performance(
    risk: &Arc<RiskManager>,
    metrics: &Arc<MetricsState>,
    ctrl_state: &Arc<Mutex<ControlState>>,
) -> String {
    let s = risk.snapshot();
    let m = metrics.snapshot();
    let st = ctrl_state.lock();
    let survival = st.survival.as_ref();
    let ctrl_stats = st.stats.clone();
    let display_pnl = [ctrl_stats.daily_pnl, s.realized_pnl_today, m.daily_pnl]
        .into_iter()
        .max_by(|a, b| a.abs().total_cmp(&b.abs()))
        .unwrap_or(0.0);

    let pnl_sign = if display_pnl >= 0.0 { "+" } else { "" };
    let wr = if m.trades_today > 0 {
        // Estimate win rate from brain GO vs trades
        // (we don't have direct win count here, so use survival if available)
        0.0 // Will be filled from survival if available
    } else {
        0.0
    };

    let (_win_rate, consec_losses) = if let Some(sv) = survival {
        let wr_est = if sv.open_positions > 0 || sv.consecutive_losses > 0 {
            // Approximate from consecutive losses
            0.0
        } else {
            0.0
        };
        (
            wr_est,
            sv.consecutive_losses
                .max(ctrl_stats.consecutive_losses as u32),
        )
    } else {
        (wr, ctrl_stats.consecutive_losses as u32)
    };

    let pnl_pct = if st.initial_equity > 0.0 {
        display_pnl / st.initial_equity * 100.0
    } else if s.equity > 0.0 {
        display_pnl / s.equity * 100.0
    } else {
        0.0
    };
    let total_closed = ctrl_stats.wins + ctrl_stats.losses;
    let win_rate = if total_closed > 0 {
        ctrl_stats.wins as f64 / total_closed as f64 * 100.0
    } else {
        0.0
    };

    format!(
        "📊 <b>Performance Summary</b>\n\
         ──────────\n\
         💰 <b>Today</b>\n\
         ├ PnL: <code>{pnl_sign}{pnl:.2}$</code> ({pnl_sign}{pnl_pct:.2}%)\n\
         ├ Equity: <code>${equity:.2}</code>\n\
         ├ Peak: <code>${peak:.2}</code>\n\
         └ Drawdown: <code>{dd:.2}%</code>\n\
         \n\
         📈 <b>Activity</b>\n\
         ├ Trades Today: <code>{trades}</code>\n\
         ├ Win Rate: <code>{win_rate:.1}%</code> (<code>{wins}W/{losses}L</code>)\n\
         ├ Signals Today: <code>{signals}</code>\n\
         ├ AI GO/NOGO/WAIT: <code>{go}</code>/<code>{nogo}</code>/<code>{wait}</code>\n\
         ├ Avg LLM Latency: <code>{latency}ms</code>\n\
         └ Consecutive Losses: <code>{consec}</code>\n\
         \n\
         🧠 <b>AI Stats</b>\n\
         ├ Avg Confidence: <code>{avg_conf:.0}%</code>\n\
         ├ Active Lessons: <code>{lessons}</code>\n\
         └ Offline Fallbacks: <code>{offline}</code>\n\
         \n\
         🤖 ARIA v1.0",
        pnl_sign = pnl_sign,
        pnl = display_pnl,
        pnl_pct = pnl_pct,
        equity = s.equity,
        peak = s.peak_equity,
        dd = s.drawdown_pct,
        trades = m.trades_today.max(ctrl_stats.trades_today),
        win_rate = win_rate,
        wins = ctrl_stats.wins,
        losses = ctrl_stats.losses,
        signals = m.signals_today.max(ctrl_stats.signals_today),
        go = m.llm_go.max(ctrl_stats.llm_go),
        nogo = m.llm_nogo.max(ctrl_stats.llm_nogo),
        wait = m.llm_wait.max(ctrl_stats.llm_wait),
        latency = m.llm_avg_latency_ms.max(ctrl_stats.llm_avg_latency_ms),
        consec = consec_losses,
        avg_conf = m.llm_avg_confidence.max(ctrl_stats.llm_avg_confidence),
        lessons = m.active_lessons.max(ctrl_stats.active_lessons),
        offline = m
            .llm_offline_fallbacks
            .max(ctrl_stats.llm_offline_fallbacks),
    )
}

fn cmd_survival(ctrl_state: &Arc<Mutex<ControlState>>) -> String {
    let st = ctrl_state.lock();
    let s = match &st.survival {
        Some(s) => s,
        None => {
            return "📭 <b>Survival data not yet available</b>\nWaiting for first update...\n🤖 ARIA v1.0"
                .to_string();
        }
    };

    let mode_emoji = match s.mode {
        crate::agents::messages::SurvivalMode::Healthy => "🟢",
        crate::agents::messages::SurvivalMode::Cautious => "🟡",
        crate::agents::messages::SurvivalMode::Defensive => "🟠",
        crate::agents::messages::SurvivalMode::Frozen => "🧊",
        crate::agents::messages::SurvivalMode::Dead => "💀",
    };
    let pnl_sign = if s.realized_pnl_today >= 0.0 { "+" } else { "" };
    let score_bar = progress_bar(s.score, 20);

    let reasons = if s.reasons.is_empty() {
        "  └ (none active)".to_string()
    } else {
        s.reasons
            .iter()
            .map(|r| format!("  ├ {}", r))
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "{mode_emoji} <b>Survival Mode</b>\n\
         ──────────\n\
         🏥 Score: <code>{score}</code>/100 {bar}\n\
         🔧 Mode: <b>{mode}</b>\n\
         📏 Size Multiplier: <code>{mult:.2}×</code>\n\
         \n\
         💰 <b>Account</b>\n\
         ├ Equity: <code>${equity:.2}</code>\n\
         ├ Initial: <code>${initial:.2}</code>\n\
         ├ Peak: <code>${peak:.2}</code>\n\
         ├ Death Line: <code>${death:.2}</code>\n\
         └ Daily PnL: <code>{pnl_sign}{pnl:.2}$</code> ({pnl_pct:.2}%)\n\
         \n\
         📊 <b>Risk</b>\n\
         ├ Drawdown: <code>{dd:.2}%</code>\n\
         ├ Open Positions: <code>{pos}</code>\n\
         └ Consecutive Losses: <code>{losses}</code>\n\
         \n\
         📋 <b>Active Rules</b>\n\
         {reasons}\n\
         \n\
         🤖 ARIA v1.0",
        mode_emoji = mode_emoji,
        score = s.score,
        bar = score_bar,
        mode = s.mode.as_str().to_uppercase(),
        mult = s.size_multiplier,
        equity = s.equity_usd,
        initial = s.initial_equity_usd,
        peak = s.peak_equity_usd,
        death = s.death_line_usd,
        pnl_sign = pnl_sign,
        pnl = s.realized_pnl_today,
        pnl_pct = if s.initial_equity_usd > 0.0 {
            s.realized_pnl_today / s.initial_equity_usd * 100.0
        } else {
            0.0
        },
        dd = s.drawdown_pct,
        pos = s.open_positions,
        losses = s.consecutive_losses,
        reasons = reasons,
    )
}

fn cmd_freeze(bus: &MessageBus) -> String {
    bus.publish(AgentEvent::ControlCommand(ControlCommand::Freeze {
        reason: "operator command".into(),
    }));
    "🧊 <b>Trading FROZEN</b>\nNew entries are now blocked.\n🤖 ARIA v1.0".to_string()
}

fn cmd_unfreeze(bus: &MessageBus) -> String {
    bus.publish(AgentEvent::ControlCommand(ControlCommand::Unfreeze));
    "✅ <b>Trading RESUMED</b>\nEntries are now allowed.\n🤖 ARIA v1.0".to_string()
}

fn cmd_flat(bus: &MessageBus) -> String {
    bus.publish(AgentEvent::ControlCommand(ControlCommand::FlatAll {
        reason: "operator /flat".into(),
    }));
    "🚨 <b>FLAT ALL — dispatched</b>\nClosing all positions at market.\n🤖 ARIA v1.0".to_string()
}

fn cmd_health(bus: &MessageBus, risk: &Arc<RiskManager>, metrics: &Arc<MetricsState>) -> String {
    bus.publish(AgentEvent::ControlCommand(ControlCommand::StatusRequest));
    let s = risk.snapshot();
    let m = metrics.snapshot();

    let risk_ok = !s.tripped && !s.frozen;
    let risk_icon = if risk_ok { "✅" } else { "⚠" };
    let dd_ok = s.drawdown_pct < 5.0;
    let dd_icon = if dd_ok { "✅" } else { "⚠" };
    let llm_ok = m.llm_avg_latency_ms < 10_000;
    let llm_icon = if llm_ok { "✅" } else { "⚠" };

    format!(
        "🏥 <b>Health Check</b>\n\
         ──────────\n\
         {risk_icon} Risk Gate: <code>{risk_status}</code>\n\
         {dd_icon} Drawdown: <code>{dd:.2}%</code>\n\
         {llm_icon} LLM Latency: <code>{latency}ms</code>\n\
         ✅ Event Bus: <code>active</code>\n\
         ✅ Position Book: <code>{pos} open</code>\n\
         ✅ Metrics: <code>updated</code>\n\
         \n\
         🤖 ARIA v1.0",
        risk_icon = risk_icon,
        risk_status = if risk_ok { "OK" } else { "BLOCKED" },
        dd_icon = dd_icon,
        dd = s.drawdown_pct,
        llm_icon = llm_icon,
        latency = m.llm_avg_latency_ms,
        pos = s.open_positions,
    )
}

fn cmd_brain(ctrl_state: &Arc<Mutex<ControlState>>) -> String {
    let st = ctrl_state.lock();
    if st.recent_brains.is_empty() {
        return "📭 <b>No brain analyses yet</b>\n🤖 ARIA v1.0".to_string();
    }

    let mut lines = Vec::new();
    lines.push("🧠 <b>Last AI Analysis</b>".to_string());
    lines.push("──────────".to_string());

    for brain in st.recent_brains.iter().rev().take(5) {
        let decision_emoji = match brain.decision.decision {
            crate::llm::engine::Decision::Go => "✅ GO",
            crate::llm::engine::Decision::NoGo => "🚫 NO-GO",
            crate::llm::engine::Decision::Wait => "⏳ WAIT",
        };
        let ta_analysis = truncate_ctrl(&brain.decision.reasoning.ta_analysis, 100);
        let sentiment = truncate_ctrl(&brain.decision.reasoning.microstructure, 80);
        let risks = truncate_ctrl(&brain.decision.reasoning.risk_factors, 80);

        lines.push(format!(
            "<b>{sym}</b> — {decision}\n\
             ├ Confidence: <code>{conf}%</code> · Regime: <code>{regime}</code>\n\
             ├ TA: <code>{ta_score}</code> · Sent: <code>{sent_score}</code> · Comp: <code>{comp}</code>\n\
             ├ <i>TA:</i> {ta_analysis}\n\
             ├ <i>Sentiment:</i> {sentiment}\n\
             ├ <i>Risks:</i> {risks}\n\
             └ Latency: <code>{latency}ms</code>{fallback}",
            sym = short_sym_ctrl(&brain.signal.symbol),
            decision = decision_emoji,
            conf = brain.decision.confidence,
            regime = brain.regime.as_str(),
            ta_score = brain.decision.market_context_score.ta_score,
            sent_score = brain.decision.market_context_score.sentiment_score,
            comp = brain.decision.market_context_score.composite_score,
            ta_analysis = html_escape_ctrl(&ta_analysis),
            sentiment = html_escape_ctrl(&sentiment),
            risks = html_escape_ctrl(&risks),
            latency = brain.latency_ms,
            fallback = if brain.offline_fallback { " ⚠ fallback" } else { "" },
        ));
    }

    lines.push("──────────".to_string());
    lines.push("🤖 ARIA v1.0".to_string());
    lines.join("\n")
}

fn cmd_risk(risk: &Arc<RiskManager>) -> String {
    let s = risk.snapshot();
    let limits = risk.limits();
    let size_mult = risk.size_multiplier();

    let status_emoji = if s.tripped {
        "🚨"
    } else if s.frozen {
        "🧊"
    } else {
        "✅"
    };
    let pnl_sign = if s.realized_pnl_today >= 0.0 { "+" } else { "" };

    format!(
        "{status_emoji} <b>Risk Metrics</b>\n\
         ──────────\n\
         💰 <b>Account State</b>\n\
         ├ Equity: <code>${equity:.2}</code>\n\
         ├ Peak: <code>${peak:.2}</code>\n\
         ├ Daily PnL: <code>{pnl_sign}{pnl:.2}$</code>\n\
         ├ Drawdown: <code>{dd:.2}%</code>\n\
         └ Daily Loss: <code>{dl:.2}%</code>\n\
         \n\
         ⚙ <b>Risk Limits</b>\n\
         ├ Risk/Trade: <code>{risk_pct}%</code>\n\
         ├ Max Positions: <code>{max_pos}</code>\n\
         ├ Max Drawdown: <code>{max_dd}%</code>\n\
         ├ Max Daily Loss: <code>{max_dl}%</code>\n\
         ├ Max Leverage: <code>{max_lev}×</code>\n\
         ├ Min R:R: <code>{min_rr}</code>\n\
         └ Size Multiplier: <code>{size_mult:.2}×</code>\n\
         \n\
         🔒 <b>Status</b>\n\
         ├ Open Positions: <code>{open_pos}</code>\n\
         ├ Frozen: <code>{frozen}</code>\n\
         ├ Tripped: <code>{tripped}</code>\
         {trip_reason}\
         {freeze_reason}\n\
         \n\
         🤖 ARIA v1.0",
        status_emoji = status_emoji,
        equity = s.equity,
        peak = s.peak_equity,
        pnl_sign = pnl_sign,
        pnl = s.realized_pnl_today,
        dd = s.drawdown_pct,
        dl = s.daily_loss_pct,
        risk_pct = limits.risk_per_trade_pct,
        max_pos = limits.max_open_positions,
        max_dd = limits.max_drawdown_pct,
        max_dl = limits.max_daily_loss_pct,
        max_lev = limits.max_leverage,
        min_rr = limits.min_reward_risk,
        size_mult = size_mult,
        open_pos = s.open_positions,
        frozen = s.frozen,
        tripped = s.tripped,
        trip_reason = s
            .trip_reason
            .as_ref()
            .map(|r| format!("\n├ Trip Reason: <code>{}</code>", html_escape_ctrl(r)))
            .unwrap_or_default(),
        freeze_reason = s
            .freeze_reason
            .as_ref()
            .map(|r| format!("\n├ Freeze Reason: <code>{}</code>", html_escape_ctrl(r)))
            .unwrap_or_default(),
    )
}

fn cmd_history(journal: &Option<Arc<TradeJournal>>) -> String {
    let j = match journal {
        Some(j) => j,
        None => {
            return "📭 <b>Trade History</b>\n──────────\n⚠ Journal not configured\n🤖 ARIA v1.0"
                .to_string();
        }
    };

    let trades = match j.closed_trades(20) {
        Ok(t) => t,
        Err(e) => return format!("❌ Error: {e}"),
    };

    if trades.is_empty() {
        return "📭 <b>Trade History</b>\n──────────\nNo closed trades yet\n🤖 ARIA v1.0"
            .to_string();
    }

    let total_trades = trades.len();
    let wins = trades.iter().filter(|t| t.is_win()).count();
    let losses = total_trades - wins;
    let total_pnl: f64 = trades.iter().map(|t| t.pnl_usd).sum();
    let win_rate = if total_trades > 0 {
        wins as f64 / total_trades as f64 * 100.0
    } else {
        0.0
    };
    let pnl_sign = if total_pnl >= 0.0 { "+" } else { "" };

    let mut lines = Vec::new();
    for t in &trades {
        let emoji = if t.is_win() { "🟢" } else { "🔴" };
        let pnl_s = if t.pnl_usd >= 0.0 { "+" } else { "" };
        let ts = t.entry_time.format("%m/%d %H:%M").to_string();
        let sid = if t.signal_id.is_empty() { "—" } else { &t.signal_id };
        lines.push(format!(
            "{emoji} {ts} · {sid} · {dir} <code>{sym}</code> · {pnl_s}{pnl:.2}$",
            dir = t.direction,
            sym = short_sym_ctrl(&t.symbol),
            pnl_s = pnl_s,
            pnl = t.pnl_usd,
            sid = sid,
        ));
    }

    format!(
        "📜 <b>Trade History</b> (last {count})\n\
         ──────────\n\
         {lines}\n\
         ──────────\n\
         📊 Total: {total} ({wins}W/{losses}L)\n\
         💰 PnL: <code>{pnl_sign}{pnl:.2}$</code>\n\
         🎯 Win Rate: <code>{wr:.1}%</code>\n\
         🤖 ARIA v1.0",
        count = total_trades,
        lines = lines.join("\n"),
        total = total_trades,
        wins = wins,
        losses = losses,
        pnl_sign = pnl_sign,
        pnl = total_pnl,
        wr = win_rate,
    )
}

fn cmd_leverage(risk: &Arc<RiskManager>, args: &str) -> String {
    let limits = risk.limits();
    let current = limits.max_leverage;

    // Parse new leverage from args: "/leverage 100" or "leverage 100"
    let new_lev = args
        .split_whitespace()
        .last()
        .and_then(|s| s.parse::<u32>().ok());

    if let Some(lev) = new_lev {
        if lev < 1 || lev > 200 {
            return format!(
                "⚠ Leverage must be 1-200x. You entered: <code>{}</code>\n🤖 ARIA v1.0",
                lev
            );
        }
        risk.set_max_leverage(lev);
        format!(
            "✅ <b>Leverage Updated</b>\n\
             ──────────\n\
             ⚡ Max Leverage: <code>{old}x</code> → <code>{new}x</code>\n\
             \n\
             📐 <b>Impact at {new}x</b>\n\
             ├ SL 0.3% = <code>{sl_loss:.1}%</code> position loss\n\
             ├ TP 0.6% = <code>{tp_gain:.1}%</code> position gain\n\
             └ Max notional = equity × {new}\n\
             \n\
             🤖 ARIA v1.0",
            old = current,
            new = lev,
            sl_loss = 0.3 * lev as f64,
            tp_gain = 0.6 * lev as f64,
        )
    } else {
        format!(
            "⚙ <b>Leverage Settings</b>\n\
             ──────────\n\
             📊 Current Max Leverage: <code>{current}x</code>\n\
             \n\
             ⚡ <b>Quick Presets</b>\n\
             ├ 🟢 <code>20x</code> — Conservative\n\
             ├ 🟡 <code>50x</code> — Moderate\n\
             ├ 🟠 <code>75x</code> — Aggressive\n\
             └ 🔴 <code>100x</code> — Maximum HFT\n\
             \n\
             📐 <b>SL/TP at {current}x</b>\n\
             ├ SL 0.3% = <code>{sl_loss:.1}%</code> position loss\n\
             ├ TP 0.6% = <code>{tp_gain:.1}%</code> position gain\n\
             └ R:R = <code>1:2.0</code>\n\
             \n\
             💡 To change: <code>/leverage 100</code>\n\
             \n\
             🤖 ARIA v1.0",
            current = current,
            sl_loss = 0.3 * current as f64,
            tp_gain = 0.6 * current as f64,
        )
    }
}

fn format_hold_duration(secs: i64) -> String {
    if secs == 0 {
        return "disabled".to_string();
    }
    if secs % 3600 == 0 {
        return format!("{}h", secs / 3600);
    }
    if secs % 60 == 0 {
        return format!("{}m", secs / 60);
    }
    format!("{}s", secs)
}

fn cmd_hold(
    bus: &MessageBus,
    pos_cfg: &Arc<parking_lot::RwLock<PositionConfig>>,
    args: &str,
) -> String {
    let current = pos_cfg.read().max_hold_secs;

    // Parse new hold time: "/hold 1800", "hold 30m", "hold 1h", or "hold 0".
    let new_secs = args.split_whitespace().last().and_then(|s| {
        if let Some(mins) = s.strip_suffix('m') {
            mins.parse::<i64>().ok().map(|m| m * 60)
        } else if let Some(hrs) = s.strip_suffix('h') {
            hrs.parse::<i64>().ok().map(|h| h * 3600)
        } else {
            s.parse::<i64>().ok()
        }
    });

    if let Some(secs) = new_secs {
        if secs != 0 && !(60..=7200).contains(&secs) {
            return format!(
                "⚠ Hold time must be <code>0</code> or 60s–7200s (1m–2h). You entered: <code>{}s</code>\n🤖 ARIA v1.0",
                secs
            );
        }

        pos_cfg.write().max_hold_secs = secs;
        bus.publish(AgentEvent::ControlCommand(ControlCommand::SetMaxHold {
            secs,
        }));

        let old_label = format_hold_duration(current);
        let new_label = format_hold_duration(secs);
        let behavior = if secs == 0 {
            "💡 Time exit is now disabled. Positions will only close by SL/TP/trailing/manual."
                .to_string()
        } else {
            format!("💡 Positions open longer than {new_label} will be time-exited.")
        };

        format!(
            "✅ <b>Max Hold Time Updated</b>\n\
             ──────────\n\
             ⏱ Max Hold: <code>{old}</code> → <code>{new}</code> ({new_s}s)\n\
             \n\
             {behavior}\n\
             Set <code>/hold 0</code> to disable time exit.\n\
             \n\
             🤖 ARIA v1.0",
            old = old_label,
            new = new_label,
            new_s = secs,
            behavior = behavior,
        )
    } else {
        let current_label = format_hold_duration(current);
        format!(
            "⏱ <b>Max Hold Time Settings</b>\n\
             ──────────\n\
             📊 Current: <code>{current_label}</code> ({current}s)\n\
             \n\
             ⚡ <b>Quick Presets</b>\n\
             ├ 🟢 <code>/hold 5m</code> — 5 min\n\
             ├ 🟡 <code>/hold 15m</code> — 15 min\n\
             ├ 🟠 <code>/hold 30m</code> — 30 min\n\
             ├ 🔴 <code>/hold 1h</code> — 1 hour\n\
             └ ⛔ <code>/hold 0</code> — disable time exit\n\
             \n\
             💡 To change: <code>/hold 1h</code>, <code>/hold 3600</code>, or <code>/hold 0</code>\n\
             \n\
             🤖 ARIA v1.0",
            current = current,
            current_label = current_label,
        )
    }
}

fn cmd_breakeven(pos_cfg: &Arc<parking_lot::RwLock<PositionConfig>>, args: &str) -> String {
    let current = pos_cfg.read().breakeven_r;
    let arg = args
        .trim_start_matches("/breakeven")
        .trim_start_matches("breakeven")
        .trim();
    if arg.is_empty() {
        return format!(
            "🔒 <b>Breakeven Settings</b>\n\
             ──────────\n\
             \n\
             Current: <code>{current:.1}R</code>\n\
             \n\
             Breakeven moves SL to entry price when profit reaches this R-multiple.\n\
             \n\
             🕐 <b>Presets</b>:\n\
             ├ <code>/breakeven 0.3</code> — Aggressive (lock early)\n\
             ├ <code>/breakeven 0.5</code> — Moderate\n\
             ├ <code>/breakeven 0.6</code> — Conservative (default)\n\
             ├ <code>/breakeven 1.0</code> — Very conservative\n\
             └ <code>/breakeven 0</code> — Disable breakeven\n\
             \n\
             Or use <code>/config</code> buttons.\n\
             \n\
             🤖 ARIA v1.0",
            current = current,
        );
    }
    let new_r: f64 = match arg.parse() {
        Ok(v) => v,
        Err(_) => {
            return "⚠ Invalid value. Use e.g. <code>/breakeven 0.6</code>\n🤖 ARIA v1.0"
                .to_string();
        }
    };
    if new_r < 0.0 || new_r > 5.0 {
        return "⚠ Breakeven R must be 0.0–5.0. Set 0 to disable.\n🤖 ARIA v1.0".to_string();
    }
    let old_r = pos_cfg.read().breakeven_r;
    pos_cfg.write().breakeven_r = new_r;
    if new_r == 0.0 {
        "✅ <b>Breakeven Disabled</b>\n──────────\n🔒 SL will NOT move to entry.\n🤖 ARIA v1.0"
            .to_string()
    } else {
        format!(
            "✅ <b>Breakeven Updated</b>\n\
             ──────────\n\
             🔒 Breakeven: <code>{old:.1}R</code> → <code>{new:.1}R</code>\n\
             \n\
             SL moves to entry when profit reaches <code>{new:.1}R</code>.\n\
             🤖 ARIA v1.0",
            old = old_r,
            new = new_r,
        )
    }
}

fn cmd_config(risk: &Arc<RiskManager>) -> String {
    let limits = risk.limits();
    format!(
        "⚙ <b>ARIA Config Panel</b>\n\
         ──────────\n\
         \n\
         ⚡ <b>Leverage</b>: <code>{lev}x</code>\n\
         📐 <b>Risk/Trade</b>: <code>{risk_pct:.1}%</code>\n\
         📊 <b>Max Positions</b>: <code>{max_pos}</code>\n\
         🛡 <b>Max Daily Loss</b>: <code>{daily_loss:.1}%</code>\n\
         📉 <b>Max Spread</b>: <code>{spread:.2}%</code>\n\
         🎯 <b>Min R:R</b>: <code>{rr:.1}</code>\n\
         \n\
         👆 <b>Tap buttons below to change settings!</b>\n\
         \n\
         🤖 ARIA v1.0",
        lev = limits.max_leverage,
        risk_pct = limits.risk_per_trade_pct,
        max_pos = limits.max_open_positions,
        daily_loss = limits.max_daily_loss_pct,
        spread = limits.max_spread_pct,
        rr = limits.min_reward_risk,
    )
}

fn config_buttons() -> Vec<Vec<InlineButton>> {
    vec![
        // Row 1: Leverage presets
        vec![
            InlineButton {
                text: "🟢 20x".into(),
                callback_data: "cfg_lev_20".into(),
            },
            InlineButton {
                text: "🟡 50x".into(),
                callback_data: "cfg_lev_50".into(),
            },
            InlineButton {
                text: "🟠 75x".into(),
                callback_data: "cfg_lev_75".into(),
            },
            InlineButton {
                text: "🔴 100x".into(),
                callback_data: "cfg_lev_100".into(),
            },
        ],
        // Row 2: Risk per trade presets
        vec![
            InlineButton {
                text: "0.5% Risk".into(),
                callback_data: "cfg_risk_0.5".into(),
            },
            InlineButton {
                text: "1% Risk".into(),
                callback_data: "cfg_risk_1".into(),
            },
            InlineButton {
                text: "2% Risk".into(),
                callback_data: "cfg_risk_2".into(),
            },
            InlineButton {
                text: "5% Risk".into(),
                callback_data: "cfg_risk_5".into(),
            },
        ],
        // Row 3: Max positions
        vec![
            InlineButton {
                text: "📍 1 Pos".into(),
                callback_data: "cfg_maxpos_1".into(),
            },
            InlineButton {
                text: "📍 2 Pos".into(),
                callback_data: "cfg_maxpos_2".into(),
            },
            InlineButton {
                text: "📍 3 Pos".into(),
                callback_data: "cfg_maxpos_3".into(),
            },
            InlineButton {
                text: "📍 5 Pos".into(),
                callback_data: "cfg_maxpos_5".into(),
            },
        ],
        // Row 4: Hold time presets
        vec![
            InlineButton {
                text: "⏱ 5m".into(),
                callback_data: "hold_5m".into(),
            },
            InlineButton {
                text: "⏱ 15m".into(),
                callback_data: "hold_15m".into(),
            },
            InlineButton {
                text: "⏱ 30m".into(),
                callback_data: "hold_30m".into(),
            },
            InlineButton {
                text: "⏱ 1h".into(),
                callback_data: "hold_1h".into(),
            },
        ],
        // Row 5: Breakeven presets
        vec![
            InlineButton {
                text: "🔒 0.3R".into(),
                callback_data: "be_0.3".into(),
            },
            InlineButton {
                text: "🔒 0.5R".into(),
                callback_data: "be_0.5".into(),
            },
            InlineButton {
                text: "🔒 0.6R".into(),
                callback_data: "be_0.6".into(),
            },
            InlineButton {
                text: "🔒 1.0R".into(),
                callback_data: "be_1.0".into(),
            },
        ],
        // Row 6: Navigation
        vec![
            InlineButton {
                text: "⚡ Leverage".into(),
                callback_data: "btn_leverage".into(),
            },
            InlineButton {
                text: "⏱ Hold".into(),
                callback_data: "btn_hold".into(),
            },
            InlineButton {
                text: "🛡 Risk".into(),
                callback_data: "btn_risk".into(),
            },
            InlineButton {
                text: "🏠 Help".into(),
                callback_data: "btn_help".into(),
            },
        ],
    ]
}

// ─── Helpers ───────────────────────────────────────────────────────

fn short_sym_ctrl(s: &str) -> &str {
    s.strip_suffix("USDT").unwrap_or(s)
}

fn truncate_ctrl(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len.saturating_sub(1)])
    }
}

fn html_escape_ctrl(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Generate a simple text progress bar.
fn progress_bar(value: u8, width: usize) -> String {
    let filled = (value as usize * width) / 100;
    let empty = width.saturating_sub(filled);
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}

/// Build a signal notification for Telegram when BrainOutcomeReady is received.
#[allow(dead_code)]
fn build_signal_notification(brain: &BrainOutcome) -> String {
    let decision_emoji = match brain.decision.decision {
        crate::llm::engine::Decision::Go => "✅ GO",
        crate::llm::engine::Decision::NoGo => "🚫 NO-GO",
        crate::llm::engine::Decision::Wait => "⏳ WAIT",
    };
    let side_emoji = if brain.signal.side == crate::data::Side::Long {
        "📈"
    } else {
        "📉"
    };
    let side_label = if brain.signal.side == crate::data::Side::Long {
        "LONG"
    } else {
        "SHORT"
    };
    let ta = brain.decision.market_context_score.ta_score;
    let sent = brain.decision.market_context_score.sentiment_score;
    let comp = brain.decision.market_context_score.composite_score;
    let summary = truncate_ctrl(&brain.decision.reasoning.summary, 120);

    // Key reasoning points
    let ta_analysis = truncate_ctrl(&brain.decision.reasoning.ta_analysis, 100);
    let risks = truncate_ctrl(&brain.decision.reasoning.risk_factors, 80);

    let fallback = if brain.offline_fallback {
        " ⚠ fallback"
    } else {
        ""
    };

    format!(
        "🔔 <b>AI Signal Detected</b>\n\
         ──────────\n\
         {side_emoji} <b>{sym}</b> · {side_label} · {decision_emoji}\n\
         ├ Confidence: <code>{conf}%</code>\n\
         ├ Strategy: <code>{strat}</code>\n\
         ├ Regime: <code>{regime}</code>\n\
         ├ Scores: TA=<code>{ta}</code> Sent=<code>{sent}</code> Comp=<code>{comp}</code>\n\
         ├ Entry: <code>{entry:.4}</code>\n\
         ├ SL: <code>{sl:.4}</code> · TP: <code>{tp:.4}</code>\n\
         ├ R:R: <code>1:{rr:.1}</code>\n\
         ├ <i>TA:</i> {ta_analysis}\n\
         ├ <i>Risks:</i> {risks}\n\
         └ <i>{summary}</i>\n\
         ──────────\n\
         ⏱ Latency: <code>{latency}ms</code>{fallback}\n\
         🤖 ARIA v1.0",
        side_emoji = side_emoji,
        sym = short_sym_ctrl(&brain.signal.symbol),
        side_label = side_label,
        decision_emoji = decision_emoji,
        conf = brain.decision.confidence,
        strat = brain.signal.strategy.as_str(),
        regime = brain.regime.as_str(),
        ta = ta,
        sent = sent,
        comp = comp,
        entry = brain.signal.entry,
        sl = brain.signal.stop_loss,
        tp = brain.signal.take_profit,
        rr = brain.signal.rr(),
        ta_analysis = html_escape_ctrl(&ta_analysis),
        risks = html_escape_ctrl(&risks),
        summary = html_escape_ctrl(&summary),
        latency = brain.latency_ms,
        fallback = fallback,
    )
}

async fn file_loop(bus: MessageBus, path: PathBuf) {
    let mut last_size: u64 = 0;
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let meta = match tokio::fs::metadata(&path).await {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.len() == last_size {
            continue;
        }
        let content = match tokio::fs::read_to_string(&path).await {
            Ok(s) => s,
            Err(_) => continue,
        };
        let _ = meta.len(); // read above; fall through.
        for line in content.lines() {
            let cmd = line.trim().to_lowercase();
            match cmd.as_str() {
                "freeze" => bus.publish(AgentEvent::ControlCommand(ControlCommand::Freeze {
                    reason: "control file".into(),
                })),
                "unfreeze" => bus.publish(AgentEvent::ControlCommand(ControlCommand::Unfreeze)),
                "flat" => bus.publish(AgentEvent::ControlCommand(ControlCommand::FlatAll {
                    reason: "control file".into(),
                })),
                "status" | "health" => {
                    bus.publish(AgentEvent::ControlCommand(ControlCommand::StatusRequest))
                }
                _ => {}
            }
        }
        // Truncate the file so we don't replay commands.
        let _ = tokio::fs::write(&path, "").await;
        last_size = 0;
    }
}
