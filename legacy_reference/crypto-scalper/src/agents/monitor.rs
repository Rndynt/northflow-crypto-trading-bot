//! Monitor agent — fans out events to the metrics state, the trade
//! journal, and Telegram. The other agents stay focused on their
//! domain; the Monitor is the only place where observability concerns
//! live.

use crate::agents::MessageBus;
use crate::agents::messages::{AgentEvent, AgentId, BrainOutcome, ControlCommand, ManagerAction};
use crate::llm::engine::Decision;
use crate::monitoring::chart;
use crate::monitoring::{MetricsState, TelegramNotifier, TradeJournal, TradeRecord};
use crate::strategy::state::StrategyName;
use chrono::{DateTime, Utc};
use parking_lot::Mutex as PlMutex;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

#[derive(Default, Clone, Copy)]
struct PriceSnapshot {
    price: f64,
    ts: Option<DateTime<Utc>>,
    ticks: u64,
}

#[derive(Default)]
struct StatusCounters {
    prices: HashMap<String, PriceSnapshot>,
    candles: HashMap<String, u64>,
    signals: u64,
    risk_allowed: u64,
    risk_blocked: u64,
    brain_calls: u64,
    brain_go: u64,
    brain_nogo: u64,
    brain_wait: u64,
    manager_calls: u64,
    manager_vetoes: u64,
    orders_filled: u64,
    trades_total: u64,
    wins: u64,
    losses: u64,
    daily_pnl: f64,
    last_signal: Option<SignalSnapshot>,
    last_signal_eval: Option<SignalEvalSnapshot>,
    last_block: Option<DecisionSnapshot>,
    last_brain: Option<DecisionSnapshot>,
    last_manager: Option<DecisionSnapshot>,
    open_times: HashMap<String, DateTime<Utc>>,
}

#[derive(Clone)]
struct SignalSnapshot {
    symbol: String,
    strategy: StrategyName,
    side: crate::data::Side,
    confidence: u8,
}

#[derive(Clone)]
struct SignalEvalSnapshot {
    symbol: String,
    timeframe_secs: i64,
    regime: String,
    candles: usize,
    strategies: String,
    reason: String,
    best: String,
}

#[derive(Clone)]
struct DecisionSnapshot {
    symbol: String,
    stage: &'static str,
    reason: String,
}

fn short_sym(s: &str) -> &str {
    s.strip_suffix("USDT").unwrap_or(s)
}

fn fmt_prices_compact(map: &HashMap<String, PriceSnapshot>, now: DateTime<Utc>) -> String {
    if map.is_empty() {
        return "—".to_string();
    }
    let mut entries: Vec<(&String, &PriceSnapshot)> = map.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    entries
        .iter()
        .map(|(sym, snap)| {
            let age = snap
                .ts
                .map(|t| (now - t).num_seconds().max(0))
                .unwrap_or(-1);
            let stale = if age > 10 { "⚠" } else { "" };
            format!("{}={:.2}{}", short_sym(sym), snap.price, stale)
        })
        .collect::<Vec<_>>()
        .join("  ")
}

fn fmt_candles_compact(map: &HashMap<String, u64>) -> String {
    if map.is_empty() {
        return "0".to_string();
    }
    let mut entries: Vec<(&String, &u64)> = map.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    entries
        .iter()
        .map(|(s, n)| format!("{}:{}", short_sym(s), n))
        .collect::<Vec<_>>()
        .join(" ")
}

fn fmt_signal_compact(s: &Option<SignalSnapshot>) -> String {
    match s {
        Some(s) => format!(
            "{} {} {} @{}%",
            short_sym(&s.symbol),
            s.strategy.as_str(),
            s.side.as_str(),
            s.confidence
        ),
        None => "—".to_string(),
    }
}

fn fmt_block_compact(s: &Option<DecisionSnapshot>) -> String {
    match s {
        Some(s) => format!("{} {}: {}", short_sym(&s.symbol), s.stage, s.reason),
        None => "—".to_string(),
    }
}

fn fmt_brain_compact(s: &Option<DecisionSnapshot>) -> String {
    match s {
        Some(s) => format!("{} {}", short_sym(&s.symbol), s.reason),
        None => "—".to_string(),
    }
}

fn emit_status_line(line: impl std::fmt::Display) {
    info!("{}", line);
}

fn fmt_signal_eval(s: &Option<SignalEvalSnapshot>) -> String {
    match s {
        Some(s) => format!(
            "{}:{}m:{}:{}c:{}:best={}:{}",
            s.symbol,
            s.timeframe_secs / 60,
            s.regime,
            s.candles,
            s.strategies,
            s.best,
            s.reason
        ),
        None => "-".to_string(),
    }
}

/// Truncate a string to `max_len` chars, appending "…" if truncated.
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len.saturating_sub(1)])
    }
}

/// Escape HTML special chars for Telegram parse_mode=HTML.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn spawn(
    bus: MessageBus,
    metrics: Arc<MetricsState>,
    journal: Arc<TradeJournal>,
    telegram: Arc<TelegramNotifier>,
    max_leverage: f64,
) -> JoinHandle<()> {
    let mut rx = bus.subscribe();
    let last_brain: Arc<PlMutex<HashMap<String, BrainOutcome>>> =
        Arc::new(PlMutex::new(HashMap::new()));
    let counters: Arc<PlMutex<StatusCounters>> = Arc::new(PlMutex::new(StatusCounters::default()));
    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default();

    // Periodic status — every 60s, emit 4 compact lines instead of one giant blob
    {
        let counters = counters.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
            tick.tick().await;
            loop {
                tick.tick().await;
                let c = counters.lock();
                let now = Utc::now();
                emit_status_line("┌─ ARIA STATUS ─────────────────────────────");
                emit_status_line(format_args!("│ 💹 {}", fmt_prices_compact(&c.prices, now)));
                emit_status_line(format_args!(
                    "│ 📊 candles={}  signals={}  allowed={}  blocked={}  fills={}  trades={}",
                    fmt_candles_compact(&c.candles),
                    c.signals,
                    c.risk_allowed,
                    c.risk_blocked,
                    c.orders_filled,
                    c.trades_total,
                ));
                emit_status_line(format_args!(
                    "│ 🧠 brain={}  go={}  nogo={}  wait={}  manager={}  vetoes={}",
                    c.brain_calls,
                    c.brain_go,
                    c.brain_nogo,
                    c.brain_wait,
                    c.manager_calls,
                    c.manager_vetoes,
                ));
                if c.last_signal.is_some() {
                    emit_status_line(format_args!(
                        "│ 🔍 last_signal : {}",
                        fmt_signal_compact(&c.last_signal)
                    ));
                }
                if c.last_signal_eval.is_some() {
                    emit_status_line(format_args!(
                        "│ 🧭 last_eval   : {}",
                        fmt_signal_eval(&c.last_signal_eval)
                    ));
                }
                if c.last_block.is_some() {
                    emit_status_line(format_args!(
                        "│ 🚫 last_block  : {}",
                        fmt_block_compact(&c.last_block)
                    ));
                }
                if c.last_brain.is_some() {
                    emit_status_line(format_args!(
                        "│ 🤖 last_brain  : {}",
                        fmt_brain_compact(&c.last_brain)
                    ));
                }
                if let Some(m) = c.last_manager.as_ref() {
                    emit_status_line(format_args!(
                        "│ 👔 last_manager: {} {}: {}",
                        short_sym(&m.symbol),
                        m.stage,
                        m.reason
                    ));
                }
                emit_status_line("└───────────────────────────────────────────");
            }
        });
    }

    tokio::spawn(async move {
        info!("monitor agent starting");
        crate::agents::heartbeat::spawn(bus.clone(), AgentId::Monitor);
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
                AgentEvent::Shutdown => break,
                AgentEvent::Tick { symbol, trade } => {
                    if trade.price <= 0.0 {
                        continue; // drop zero-price WS artifacts
                    }
                    let mut c = counters.lock();
                    let snap = c.prices.entry(symbol).or_default();
                    snap.price = trade.price;
                    snap.ts = Some(trade.ts);
                    snap.ticks += 1;
                }
                AgentEvent::CandleClosed {
                    symbol,
                    timeframe_secs,
                    ..
                } => {
                    *counters
                        .lock()
                        .candles
                        .entry(format!("{symbol}:{}m", timeframe_secs / 60))
                        .or_insert(0) += 1;
                }
                AgentEvent::PreSignalEmitted { signal, .. } => {
                    metrics.update(|m| m.signals_today += 1);
                    let mut c = counters.lock();
                    c.signals += 1;
                    c.last_signal = Some(SignalSnapshot {
                        symbol: signal.symbol.clone(),
                        strategy: signal.strategy,
                        side: signal.side,
                        confidence: signal.ta_confidence,
                    });
                }
                AgentEvent::SignalEvaluation(e) => {
                    let mut c = counters.lock();
                    let regime = e.regime.map(|r| r.as_str()).unwrap_or("SKIPPED");
                    let strategies = if e.strategies.is_empty() {
                        "-".to_string()
                    } else {
                        e.strategies
                            .iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join("|")
                    };
                    let best = match (e.best_strategy, e.best_confidence) {
                        (Some(strategy), Some(confidence)) => {
                            format!("{}:{confidence}", strategy.as_str())
                        }
                        _ => "-".to_string(),
                    };
                    c.last_signal_eval = Some(SignalEvalSnapshot {
                        symbol: e.symbol.clone(),
                        timeframe_secs: e.timeframe_secs,
                        regime: regime.to_string(),
                        candles: e.candles,
                        strategies,
                        reason: e.reason.clone(),
                        best,
                    });
                    debug!(
                        symbol = %e.symbol,
                        timeframe = %format!("{}m", e.timeframe_secs / 60),
                        regime = %regime,
                        candles = e.candles,
                        reason = %e.reason,
                        "signal: no trade candidate"
                    );
                }
                AgentEvent::RiskVerdict(risk) => {
                    let mut c = counters.lock();
                    match risk.outcome {
                        crate::agents::messages::RiskOutcome::Allowed => {
                            c.risk_allowed += 1;
                            c.last_block = None;
                            info!(
                                symbol   = %risk.signal.symbol,
                                strategy = %risk.signal.strategy.as_str(),
                                side     = %risk.signal.side.as_str(),
                                size     = risk.size,
                                ta       = risk.signal.ta_confidence,
                                "risk: allowed signal"
                            );
                        }
                        crate::agents::messages::RiskOutcome::Blocked => {
                            c.risk_blocked += 1;
                            let reason = risk.reason.clone().unwrap_or_else(|| "blocked".into());
                            c.last_block = Some(DecisionSnapshot {
                                symbol: risk.signal.symbol.clone(),
                                stage: "risk",
                                reason: reason.clone(),
                            });
                            info!(
                                symbol   = %risk.signal.symbol,
                                strategy = %risk.signal.strategy.as_str(),
                                side     = %risk.signal.side.as_str(),
                                ta       = risk.signal.ta_confidence,
                                reason   = %reason,
                                "risk: blocked signal"
                            );
                        }
                    }
                }
                AgentEvent::BrainOutcomeReady(brain) => {
                    last_brain
                        .lock()
                        .insert(brain.signal.symbol.clone(), brain.clone());
                    record_brain(&metrics, &brain);
                    {
                        let mut c = counters.lock();
                        c.brain_calls += 1;
                        match brain.decision.decision {
                            Decision::Go => c.brain_go += 1,
                            Decision::NoGo => c.brain_nogo += 1,
                            Decision::Wait => c.brain_wait += 1,
                        }
                        c.last_brain = Some(DecisionSnapshot {
                            symbol: brain.signal.symbol.clone(),
                            stage: "brain",
                            reason: format!(
                                "{:?}/{}: {}",
                                brain.decision.decision,
                                brain.decision.confidence,
                                brain.decision.reasoning.summary
                            ),
                        });
                    } // guard dropped here, before .await

                    // ─── Signal Notification (only GO/WAIT to group) ───────
                    // Spawn to background — never block the event loop with a
                    // Telegram HTTP call (can take 100-500 ms, causing lag).
                    if !matches!(brain.decision.decision, Decision::NoGo) {
                        let tg = telegram.clone();
                        let b = brain.clone();
                        let hc = http_client.clone();
                        tokio::spawn(async move {
                            send_signal_notification(&tg, &b, &hc).await;
                        });
                    }
                }
                AgentEvent::ManagerVerdictEmitted(v) => {
                    {
                        let mut c = counters.lock();
                        c.manager_calls += 1;
                        if matches!(v.action, ManagerAction::Veto { .. }) {
                            c.manager_vetoes += 1;
                        }
                        c.last_manager = Some(DecisionSnapshot {
                            symbol: v.proposal.symbol.clone(),
                            stage: "manager",
                            reason: manager_action_summary(&v.action),
                        });
                    }
                    if matches!(v.action, ManagerAction::Veto { .. }) {
                        info!(
                            symbol = %v.proposal.symbol,
                            reason = %manager_action_summary(&v.action),
                            "monitor: trade vetoed by manager"
                        );
                    }
                }
                AgentEvent::ControlCommand(ControlCommand::StatusRequest) => {
                    let c = counters.lock();
                    let now = Utc::now();
                    emit_status_line("┌─ ARIA STATUS (on-demand) ──────────────────");
                    emit_status_line(format_args!("│ 💹 {}", fmt_prices_compact(&c.prices, now)));
                    emit_status_line(format_args!(
                        "│ 📊 candles={}  signals={}  allowed={}  blocked={}  fills={}  trades={}",
                        fmt_candles_compact(&c.candles),
                        c.signals,
                        c.risk_allowed,
                        c.risk_blocked,
                        c.orders_filled,
                        c.trades_total,
                    ));
                    emit_status_line(format_args!(
                        "│ 🧠 brain={}  go={}  nogo={}  wait={}  manager={}  vetoes={}",
                        c.brain_calls,
                        c.brain_go,
                        c.brain_nogo,
                        c.brain_wait,
                        c.manager_calls,
                        c.manager_vetoes,
                    ));
                    emit_status_line(format_args!(
                        "│ 🔍 last_signal : {}",
                        fmt_signal_compact(&c.last_signal)
                    ));
                    emit_status_line(format_args!(
                        "│ 🧭 last_eval   : {}",
                        fmt_signal_eval(&c.last_signal_eval)
                    ));
                    emit_status_line(format_args!(
                        "│ 🚫 last_block  : {}",
                        fmt_block_compact(&c.last_block)
                    ));
                    emit_status_line(format_args!(
                        "│ 🤖 last_brain  : {}",
                        fmt_brain_compact(&c.last_brain)
                    ));
                    if let Some(m) = c.last_manager.as_ref() {
                        emit_status_line(format_args!(
                            "│ 👔 last_manager: {} {}: {}",
                            short_sym(&m.symbol),
                            m.stage,
                            m.reason
                        ));
                    }
                    emit_status_line("└────────────────────────────────────────────");
                }
                AgentEvent::OrderFilled {
                    client_id,
                    symbol,
                    side,
                    size,
                    ack,
                    signal_id,
                } => {
                    // Scope lock strictly — must not cross .await boundary
                    let _trade_no = {
                        let mut c = counters.lock();
                        c.orders_filled += 1;
                        c.trades_total += 1;
                        c.open_times.insert(client_id.clone(), Utc::now());
                        c.trades_total
                    };

                    let brain = { last_brain.lock().get(&symbol).cloned() };
                    let (sl, tp, strategy) = brain
                        .as_ref()
                        .map(|b| {
                            (
                                b.signal.stop_loss,
                                b.signal.take_profit,
                                b.signal.strategy.as_str().to_string(),
                            )
                        })
                        .unwrap_or((0.0, 0.0, "—".to_string()));

                    // Always insert into the journal — brain data is optional enrichment.
                    // Previously: insert was SKIPPED if brain=None (timing race between
                    // BrainOutcomeReady and OrderFilled could leave last_brain empty for
                    // the symbol). Result: close_trade UPDATE found 0 rows → history stayed
                    // empty despite dozens of trades executing.
                    let log_result = if let Some(b) = &brain {
                        log_open_trade(&journal, &client_id, &symbol, side, size, &ack, b)
                    } else {
                        log_open_trade_minimal(&journal, &client_id, &symbol, side, size, &ack)
                    };
                    if let Err(e) = log_result {
                        warn!(error = %e, "monitor: insert_trade failed (client_id={client_id})");
                    }

                    let side_label = if side == crate::data::Side::Long {
                        "BUY"
                    } else {
                        "SELL"
                    };
                    let sl_line = if sl > 0.0 {
                        format!("{:.4}", sl)
                    } else {
                        "—".to_string()
                    };
                    let tp_line = if tp > 0.0 {
                        format!("{:.4}", tp)
                    } else {
                        "—".to_string()
                    };

                    // ─── Enhanced open notification ────────────────────────
                    let msg = build_open_notification(
                        side_label,
                        &symbol,
                        ack.avg_fill_price,
                        &sl_line,
                        &tp_line,
                        size,
                        short_sym(&symbol),
                        &strategy,
                        brain.as_ref(),
                        max_leverage,
                        &signal_id,
                    );
                    // Spawn — don't block the event loop
                    let tg = telegram.clone();
                    tokio::spawn(async move {
                        let _ = tg.send(&msg).await;
                    });
                }
                AgentEvent::PositionClosed {
                    client_id,
                    symbol,
                    side,
                    size,
                    entry_price,
                    exit_price,
                    pnl_usd,
                    reason,
                    strategy: _,
                    signal_id,
                } => {
                    // BUG FIX: old formula (exit-entry)/entry gives negative pnl_pct for
                    // profitable SHORT positions. Use pnl_usd/notional which is always
                    // correct regardless of direction: positive for wins, negative for losses.
                    let notional = entry_price * size;
                    let pnl_pct = if notional > 0.0 { pnl_usd / notional * 100.0 } else { 0.0 };
                    if let Err(e) = journal.close_trade(
                        &client_id,
                        Utc::now(),
                        exit_price,
                        reason.as_str(),
                        pnl_usd,
                        pnl_pct,
                        0.0,
                    ) {
                        warn!(error = %e, client_id = %client_id, "monitor: close_trade failed");
                    }
                    metrics.update(|m| {
                        m.daily_pnl += pnl_usd;
                        m.trades_today += 1;
                    });

                    // Update counters with win/loss tracking
                    let (trade_no, win_rate, daily_pnl, total_wins, total_losses, open_time) = {
                        let mut c = counters.lock();
                        c.daily_pnl += pnl_usd;
                        if pnl_usd >= 0.0 {
                            c.wins += 1;
                        } else {
                            c.losses += 1;
                        }
                        let total = c.wins + c.losses;
                        let wr = if total > 0 {
                            c.wins as f64 / total as f64 * 100.0
                        } else {
                            0.0
                        };
                        let ot = c.open_times.remove(&client_id);
                        (c.trades_total, wr, c.daily_pnl, c.wins, c.losses, ot)
                    };

                    let side_label = if side == crate::data::Side::Long {
                        "BUY"
                    } else {
                        "SELL"
                    };
                    let is_win = pnl_usd >= 0.0;

                    let header = match reason {
                        crate::execution::PositionExitReason::TakeProfit => {
                            "🎯 <b>TAKE PROFIT HIT!</b>"
                        }
                        crate::execution::PositionExitReason::StopLoss => "🛑 <b>STOP LOSS HIT</b>",
                        crate::execution::PositionExitReason::Trailing => "🔄 <b>TRAILING STOP</b>",
                        crate::execution::PositionExitReason::TimeExit => "⏰ <b>TIME EXIT</b>",
                        crate::execution::PositionExitReason::Manual => "🔧 <b>MANUAL CLOSE</b>",
                        crate::execution::PositionExitReason::Breakeven => {
                            "🔒 <b>BREAKEVEN EXIT</b>"
                        }
                        crate::execution::PositionExitReason::PartialTP => {
                            "🎯 <b>PARTIAL TAKE PROFIT</b>"
                        }
                    };
                    let result_emoji = if is_win { "🏆" } else { "💔" };
                    let result_text = if is_win { "WIN" } else { "LOSS" };
                    let pnl_sign = if pnl_usd >= 0.0 { "+" } else { "" };
                    let pnl_emoji = if is_win { "📈" } else { "📉" };

                    // Duration
                    let duration_str = if let Some(opened) = open_time {
                        let dur = Utc::now() - opened;
                        let mins = dur.num_minutes();
                        if mins >= 60 {
                            format!("{}h {}m", mins / 60, mins % 60)
                        } else {
                            format!("{}m", mins)
                        }
                    } else {
                        "—".to_string()
                    };

                    let daily_pnl_sign = if daily_pnl >= 0.0 { "+" } else { "" };

                    // Calculate ROE and margin (same logic as open notification)
                    let notional = size * entry_price;
                    let close_max_lev = max_leverage;
                    let margin_usd = if close_max_lev > 0.0 {
                        notional / close_max_lev
                    } else {
                        notional
                    };
                    let roe_pct = pnl_pct.abs() * close_max_lev; // ROE = price change × leverage

                    let msg = format!(
                        "{header}\n\
                         ──────────\n\
                         🆔 <code>{signal_id}</code>\n\
                         📊 <b>{side_label}</b> · <code>{sym}</code>\n\
                         📍 Entry: <code>{entry:.4}</code>\n\
                         🏁 Exit:  <code>{exit:.4}</code>\n\
                         💼 Size:  <code>{size:.4}</code> {sym_short} (<code>${notional:.2}</code>)\n\
                         ⚡ Leverage: <code>{leverage:.0}x</code> · Margin: <code>${margin:.2}</code>\n\
                         {pnl_emoji} PnL:   <code>{pnl_sign}{pnl:.2}$</code> ({pnl_sign}{pnl_pct_val:.4}% price · {pnl_sign}{roe:.2}% ROE)\n\
                         ⏱ Duration: <code>{duration}</code>\n\
                         {result_emoji} Result: <b>{result_text}</b>\n\
                         ──────────\n\
                         📊 <b>Session Stats</b>\n\
                         ├ Daily PnL: <code>{daily_sign}{daily:.2}$</code>\n\
                         ├ Win Rate: <code>{wr:.1}%</code> ({wins}W/{losses}L)\n\
                         └ Total Trades: {total}\n\
                         🤖 ARIA v1.0",
                        header = header,
                        signal_id = signal_id,
                        side_label = side_label,
                        sym = short_sym(&symbol),
                        entry = entry_price,
                        exit = exit_price,
                        size = size,
                        sym_short = short_sym(&symbol),
                        notional = notional,
                        leverage = close_max_lev,
                        margin = margin_usd,
                        pnl_emoji = pnl_emoji,
                        pnl_sign = pnl_sign,
                        pnl = pnl_usd,
                        pnl_pct_val = pnl_pct.abs(),
                        roe = roe_pct,
                        duration = duration_str,
                        result_emoji = result_emoji,
                        result_text = result_text,
                        daily_sign = daily_pnl_sign,
                        daily = daily_pnl,
                        wr = win_rate,
                        wins = total_wins,
                        losses = total_losses,
                        total = trade_no,
                    );
                    // Spawn both sends — don't block the event loop
                    let tg = telegram.clone();
                    let msg_clone = msg.clone();
                    tokio::spawn(async move {
                        let _ = tg.send(&msg_clone).await;
                        // Also send to group topic
                        let _ = tg.send_signal(&msg_clone).await;
                    });
                }
                AgentEvent::PolicyRefreshed { lessons_count, .. } => {
                    metrics.update(|m| m.active_lessons = lessons_count as u64);
                }
                AgentEvent::PositionReduced {
                    client_id,
                    symbol,
                    side,
                    reduced_size,
                    remaining_size,
                    entry_price,
                    exit_price,
                    pnl_usd,
                    reason,
                    signal_id,
                    strategy,
                    ..
                } => {
                    // BUG FIX: update session daily_pnl counter so Session Stats
                    // in the NEXT close notification reflects partial TP profit.
                    {
                        let mut c = counters.lock();
                        c.daily_pnl += pnl_usd;
                    }
                    metrics.update(|m| m.daily_pnl += pnl_usd);

                    // Persist partial close to journal so it appears in /history
                    let side_str = if side == crate::data::Side::Long { "LONG" } else { "SHORT" };
                    if let Err(e) = journal.log_partial_close(
                        &signal_id,
                        &client_id,
                        &symbol,
                        side_str,
                        &strategy,
                        "UNKNOWN",
                        entry_price,
                        exit_price,
                        reduced_size,
                        pnl_usd,
                    ) {
                        warn!(error = %e, "monitor: log_partial_close failed");
                    }

                    let side_label = if side == crate::data::Side::Long {
                        "BUY"
                    } else {
                        "SELL"
                    };
                    info!(
                        symbol = %symbol,
                        side = %side_label,
                        reduced = %format!("{:.4}", reduced_size),
                        remaining = %format!("{:.4}", remaining_size),
                        pnl = %format!("{:+.4}", pnl_usd),
                        reason = %reason.as_str(),
                        "📉 partial TP taken"
                    );
                    // Running session daily PnL for display
                    let session_daily = counters.lock().daily_pnl;
                    let daily_sign = if session_daily >= 0.0 { "+" } else { "" };
                    let msg = format!(
                        "🎯 <b>PARTIAL TP</b> · <code>{sym}</code> {side}\n\
                         🆔 <code>{signal_id}</code>\n\
                         📍 Entry: <code>{entry:.4}</code> → Exit: <code>{exit:.4}</code>\n\
                         📦 Reduced: <code>{red:.4}</code> · Remaining: <code>{rem:.4}</code>\n\
                         💵 Partial PnL: <code>{pnl:+.4}$</code>\n\
                         🛡 Runner SL → Breakeven (entry)\n\
                         📊 Session Daily: <code>{daily_sign}{session_daily:.2}$</code>\n\
                         🤖 ARIA v1.0",
                        sym = short_sym(&symbol),
                        side = side_label,
                        signal_id = signal_id,
                        entry = entry_price,
                        exit = exit_price,
                        red = reduced_size,
                        rem = remaining_size,
                        pnl = pnl_usd,
                        daily_sign = daily_sign,
                        session_daily = session_daily,
                    );
                    let tg = telegram.clone();
                    tokio::spawn(async move {
                        let _ = tg.send(&msg).await;
                    });
                }
                AgentEvent::ExecutionFailed { symbol, reason } => {
                    warn!(symbol = %symbol, %reason, "⚠️ execution failed — pending lock released");
                    let msg = format!(
                        "⚠️ <b>EXECUTION FAILED</b>\n\
                         Symbol: <code>{sym}</code>\n\
                         Reason: <code>{reason}</code>\n\
                         🤖 ARIA v1.0",
                        sym = short_sym(&symbol),
                        reason = reason,
                    );
                    let tg = telegram.clone();
                    tokio::spawn(async move {
                        let _ = tg.send(&msg).await;
                    });
                }
                AgentEvent::StopMoved {
                    symbol,
                    old_stop,
                    new_stop,
                    reason,
                    ..
                } => {
                    info!(
                        symbol = %symbol,
                        old = %format!("{:.4}", old_stop),
                        new = %format!("{:.4}", new_stop),
                        %reason,
                        "🔒 stop loss moved"
                    );
                }
                AgentEvent::ScreeningUpdated { symbol, bias, .. } => {
                    info!(symbol = %symbol, bias = %bias.as_str(), "📡 screening bias updated");
                }
                _ => {}
            }
        }
    })
}

/// Build a rich HTML notification for an opened position.
#[allow(clippy::too_many_arguments)]
fn build_open_notification(
    side_label: &str,
    symbol: &str,
    entry_price: f64,
    sl_line: &str,
    tp_line: &str,
    size: f64,
    sym_short: &str,
    strategy: &str,
    brain: Option<&BrainOutcome>,
    max_leverage: f64,
    signal_id: &str,
) -> String {
    let side_emoji = if side_label == "BUY" { "🟢" } else { "🔴" };

    // Partial TP levels
    let partial_tp_section = if let Some(b) = brain {
        let entry = entry_price;
        let sl_raw = b.signal.stop_loss;
        let tp_raw = b.signal.take_profit;
        if entry > 0.0 && sl_raw > 0.0 && tp_raw > 0.0 {
            let risk_dist = (sl_raw - entry).abs();
            let is_long = b.signal.side == crate::data::Side::Long;
            // Partial TP at 1R (50% close)
            let partial_tp_price = if is_long {
                entry + risk_dist
            } else {
                entry - risk_dist
            };
            // Breakeven SL (move SL to entry after partial TP)
            let be_sl = entry;
            format!(
                "\n📊 <b>TP Plan</b>\n\
                 ├ 🎯 TP₁ (50%): <code>{tp1:.4}</code> @ 1R\n\
                 ├ 🛡 After TP₁: SL → BE <code>{be:.4}</code>\n\
                 ├ 🎯 TP₂ (50%): <code>{tp2:.4}</code>\n\
                 └ 📐 Risk: <code>{risk:.4}$</code>\n",
                tp1 = partial_tp_price,
                be = be_sl,
                tp2 = tp_raw,
                risk = risk_dist * size,
            )
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Risk:Reward ratio
    let rr_line = if let Some(b) = brain {
        let rr = b.signal.rr();
        if rr > 0.0 {
            format!("⚖ R:R Ratio: <code>1:{:.1}</code>\n", rr)
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // AI reasoning section
    let ai_section = if let Some(b) = brain {
        let decision_emoji = match b.decision.decision {
            Decision::Go => "✅",
            Decision::NoGo => "❌",
            Decision::Wait => "⏳",
        };
        let decision_label = match b.decision.decision {
            Decision::Go => "GO",
            Decision::NoGo => "NO-GO",
            Decision::Wait => "WAIT",
        };
        let summary = truncate(&b.decision.reasoning.summary, 150);
        format!(
            "\n🧠 <b>AI Analysis</b>\n\
             ├ Decision: {decision_emoji} <b>{decision_label}</b> (confidence: {conf}%)\n\
             ├ Summary: <i>{summary}</i>\n\
             ├ TA Score: <code>{ta}</code> · Sentiment: <code>{sent}</code> · Risk: <code>{risk_s}</code>\n\
             └ Composite: <code>{comp}</code>/100\n",
            conf = b.decision.confidence,
            summary = html_escape(&summary),
            ta = b.decision.market_context_score.ta_score,
            sent = b.decision.market_context_score.sentiment_score,
            risk_s = b.decision.market_context_score.risk_score,
            comp = b.decision.market_context_score.composite_score,
        )
    } else {
        String::new()
    };

    // Market context section
    let context_section = if let Some(b) = brain {
        format!(
            "\n🌐 <b>Market Context</b>\n\
             ├ Regime: <code>{regime}</code>\n\
             └ Strategy: <code>{strategy}</code>\n",
            regime = b.regime.as_str(),
            strategy = strategy,
        )
    } else {
        format!(
            "\n🔧 Strategy: <code>{strategy}</code>\n",
            strategy = strategy,
        )
    };

    // Risk factors
    let risk_section = if let Some(b) = brain {
        let risks = truncate(&b.decision.reasoning.risk_factors, 100);
        if !risks.is_empty() {
            format!("⚠ <i>Risks: {risks}</i>\n", risks = html_escape(&risks))
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Calculate notional value and margin for display
    let notional = size * entry_price;
    let margin = if max_leverage > 0.0 {
        notional / max_leverage
    } else {
        notional
    };

    format!(
        "{side_emoji} <b>POSITION OPENED</b>\n\
         ──────────\n\
         🆔 <code>{signal_id}</code>\n\
         📊 <b>{side_label}</b> · <code>{sym}</code>\n\
         📍 Entry: <code>{entry:.4}</code>\n\
         🛡 SL: <code>{sl}</code>\n\
         🎯 TP: <code>{tp}</code>\n\
         💼 Size: <code>{size:.4}</code> {sym_short} (<code>${notional:.2}</code>)\n\
         ⚡ Leverage: <code>{leverage:.0}x</code> · Margin: <code>${margin:.2}</code>\n\
         {rr_line}\
         {partial_tp_section}\
         {ai_section}\
         {context_section}\
         {risk_section}\
         🤖 ARIA v1.0",
        side_emoji = side_emoji,
        side_label = side_label,
        signal_id = signal_id,
        sym = short_sym(symbol),
        entry = entry_price,
        sl = sl_line,
        tp = tp_line,
        size = size,
        notional = notional,
        leverage = max_leverage,
        margin = margin,
        sym_short = sym_short,
        rr_line = rr_line,
        partial_tp_section = partial_tp_section,
        ai_section = ai_section,
        context_section = context_section,
        risk_section = risk_section,
    )
}

/// Send a signal notification when the brain produces an outcome.
async fn send_signal_notification(
    telegram: &TelegramNotifier,
    brain: &BrainOutcome,
    http_client: &reqwest::Client,
) {
    let symbol = &brain.signal.symbol;
    let side_label = if brain.signal.side == crate::data::Side::Long {
        "📈 LONG"
    } else {
        "📉 SHORT"
    };
    let side_emoji = if brain.signal.side == crate::data::Side::Long {
        "📈"
    } else {
        "📉"
    };
    let decision_emoji = match brain.decision.decision {
        Decision::Go => "✅ GO",
        Decision::NoGo => "🚫 NO-GO",
        Decision::Wait => "⏳ WAIT",
    };
    let decision_label = match brain.decision.decision {
        Decision::Go => "<b>APPROVED</b>",
        Decision::NoGo => "<b>VETOED</b>",
        Decision::Wait => "<b>WAITING</b>",
    };
    let summary = truncate(&brain.decision.reasoning.summary, 150);
    let ta_analysis = truncate(&brain.decision.reasoning.ta_analysis, 150);
    let microstructure = truncate(&brain.decision.reasoning.microstructure, 120);
    let risks = truncate(&brain.decision.reasoning.risk_factors, 120);
    let invalidation = truncate(&brain.decision.reasoning.invalidation, 100);

    let fallback = if brain.offline_fallback {
        " ⚠ fallback"
    } else {
        ""
    };

    let msg = format!(
        "🔔 <b>SIGNAL DETECTED</b>\n\
         ──────────\n\
         {side_emoji} <b>{sym}</b> · {side_label} · {decision_emoji} {decision_label}\n\
         \n\
         ├ 🎯 Confidence: <code>{conf}%</code>\n\
         ├ 📋 Strategy: <code>{strat}</code>\n\
         ├ 🧭 Regime: <code>{regime}</code>\n\
         ├ 💰 Entry: <code>{entry:.6}</code>\n\
         ├ 🛑 SL: <code>{sl:.6}</code>\n\
         ├ 🎯 TP: <code>{tp:.6}</code>\n\
         └ 📐 R:R: <code>1:{rr:.1}</code>\n\
         \n\
         📊 <b>Scores</b>\n\
         ├ TA: <code>{ta}</code> · Micro: <code>{micro}</code>\n\
         ├ Sentiment: <code>{sent}</code> · Risk: <code>{risk_s}</code>\n\
         └ Composite: <code>{comp}</code>/100\n\
         \n\
         🧠 <b>AI Reasoning</b>\n\
         ├ <i>Summary:</i> {summary}\n\
         ├ <i>TA:</i> {ta_analysis}\n\
         ├ <i>Micro:</i> {microstructure}\n\
         ├ <i>Risks:</i> {risks}\n\
         └ <i>Invalidate:</i> {invalidation}\n\
         \n\
         ⏱ Latency: <code>{latency}ms</code>{fallback}",
        side_emoji = side_emoji,
        sym = short_sym(symbol),
        side_label = side_label,
        decision_emoji = decision_emoji,
        decision_label = decision_label,
        conf = brain.decision.confidence,
        strat = brain.signal.strategy.as_str(),
        regime = brain.regime.as_str(),
        entry = brain.signal.entry,
        sl = brain.signal.stop_loss,
        tp = brain.signal.take_profit,
        rr = brain.signal.rr(),
        ta = brain.decision.market_context_score.ta_score,
        micro = brain.decision.market_context_score.microstructure_score,
        sent = brain.decision.market_context_score.sentiment_score,
        risk_s = brain.decision.market_context_score.risk_score,
        comp = brain.decision.market_context_score.composite_score,
        summary = html_escape(&summary),
        ta_analysis = html_escape(&ta_analysis),
        microstructure = html_escape(&microstructure),
        risks = html_escape(&risks),
        invalidation = html_escape(&invalidation),
        latency = brain.latency_ms,
        fallback = fallback,
    );
    // ─── Generate chart and send as single photo message ────
    let chart_result = async {
        let candles = chart::fetch_klines(http_client, &brain.signal.symbol, "5m", 100).await?;
        let chart_candles: Vec<chart::ChartCandle> = candles
            .iter()
            .map(|c| chart::ChartCandle {
                open_time: c.open_time,
                open: c.open,
                high: c.high,
                low: c.low,
                close: c.close,
                volume: c.volume,
            })
            .collect();

        let img = chart::generate_signal_chart(
            &brain.signal.symbol,
            brain.signal.side,
            brain.signal.entry,
            brain.signal.stop_loss,
            brain.signal.take_profit,
            &chart_candles,
        )?;

        // 1 photo with full signal text as caption = 1 bubble
        telegram.send_photo(&img, &msg).await?;
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;

    // Fallback: if chart fails, send text only
    if let Err(e) = chart_result {
        warn!("chart generation/send failed: {} — sending text only", e);
        let _ = telegram.send_signal(&msg).await;
    }
}

fn record_brain(metrics: &MetricsState, brain: &BrainOutcome) {
    metrics.update(|m| {
        let n = m.llm_go + m.llm_nogo + m.llm_wait;
        let avg = m.llm_avg_confidence * n as f64 + brain.decision.confidence as f64;
        match brain.decision.decision {
            Decision::Go => m.llm_go += 1,
            Decision::NoGo => m.llm_nogo += 1,
            Decision::Wait => m.llm_wait += 1,
        }
        m.llm_avg_confidence = avg / ((n + 1) as f64).max(1.0);
        let total = m.llm_go + m.llm_nogo + m.llm_wait;
        m.llm_avg_latency_ms =
            (m.llm_avg_latency_ms * total.saturating_sub(1) + brain.latency_ms) / total.max(1);
        if brain.offline_fallback {
            m.llm_offline_fallbacks += 1;
        }
    });
}

fn manager_action_summary(action: &ManagerAction) -> String {
    match action {
        ManagerAction::Approve => "approve".to_string(),
        ManagerAction::Veto { reason } => format!("veto: {reason}"),
        ManagerAction::Adjust {
            size_multiplier,
            sl_offset_bps,
            tp_offset_bps,
            reason,
        } => format!(
            "adjust size={size_multiplier:.2} sl={sl_offset_bps:.1}bps tp={tp_offset_bps:.1}bps: {reason}"
        ),
    }
}

/// Insert a minimal trade record when brain outcome is unavailable.
/// All LLM/TA fields are NULL — close_trade will fill in exit fields later.
/// This ensures every OrderFilled has a matching DB row so close_trade UPDATE
/// always succeeds and the trade appears in /history.
fn log_open_trade_minimal(
    journal: &TradeJournal,
    client_id: &str,
    symbol: &str,
    side: crate::data::Side,
    size: f64,
    ack: &crate::execution::OrderAck,
) -> anyhow::Result<()> {
    let record = TradeRecord {
        client_order_id: client_id.to_string(),
        signal_id: String::new(), // minimal record — no signal context available
        symbol: symbol.to_string(),
        direction: side.as_str().to_string(),
        strategy: "—".to_string(),
        market_regime: "—".to_string(),
        entry_time: Utc::now(),
        entry_price: ack.avg_fill_price,
        size,
        stop_loss: 0.0,
        take_profit: 0.0,
        exit_time: None,
        exit_price: None,
        exit_reason: None,
        pnl_usd: None,
        pnl_pct: None,
        fees_paid: None,
        ta_confidence: None,
        rsi: None,
        adx: None,
        vwap_delta_pct: None,
        ema_alignment: None,
        llm_model: None,
        llm_decision: None,
        llm_confidence: None,
        llm_ta_score: None,
        llm_sentiment_score: None,
        llm_fundamental_score: None,
        llm_composite: None,
        llm_summary: None,
        llm_ta_analysis: None,
        llm_sentiment: None,
        llm_fundamental: None,
        llm_risks: None,
        llm_invalidation: None,
        llm_latency_ms: None,
        fear_greed: None,
        social_sentiment: None,
        news_score: None,
        funding_rate: None,
        top_news_titles: None,
        user_id: 7773988648,
    };
    journal.insert_trade(&record)?;
    Ok(())
}

fn log_open_trade(
    journal: &TradeJournal,
    client_id: &str,
    symbol: &str,
    side: crate::data::Side,
    size: f64,
    ack: &crate::execution::OrderAck,
    brain: &BrainOutcome,
) -> anyhow::Result<()> {
    let signal = &brain.signal;
    let record = TradeRecord {
        client_order_id: client_id.to_string(),
        signal_id: signal.signal_id.clone(),
        symbol: symbol.to_string(),
        direction: side.as_str().to_string(),
        strategy: signal.strategy.as_str().to_string(),
        market_regime: brain.regime.as_str().to_string(),
        entry_time: Utc::now(),
        entry_price: ack.avg_fill_price,
        size,
        stop_loss: signal.stop_loss,
        take_profit: signal.take_profit,
        exit_time: None,
        exit_price: None,
        exit_reason: None,
        pnl_usd: None,
        pnl_pct: None,
        fees_paid: Some(ack.fee_usd),
        ta_confidence: Some(signal.ta_confidence),
        rsi: None,
        adx: None,
        vwap_delta_pct: None,
        ema_alignment: Some(brain.regime.as_str().to_string()),
        llm_model: Some(brain.decision.direction.clone()),
        llm_decision: Some(format!("{:?}", brain.decision.decision)),
        llm_confidence: Some(brain.decision.confidence),
        llm_ta_score: Some(brain.decision.market_context_score.ta_score),
        llm_sentiment_score: Some(brain.decision.market_context_score.sentiment_score),
        llm_fundamental_score: Some(brain.decision.market_context_score.microstructure_score),
        llm_composite: Some(brain.decision.market_context_score.composite_score),
        llm_summary: Some(brain.decision.reasoning.summary.clone()),
        llm_ta_analysis: Some(brain.decision.reasoning.ta_analysis.clone()),
        llm_sentiment: Some(brain.decision.reasoning.microstructure.clone()),
        llm_fundamental: Some(String::new()),
        llm_risks: Some(brain.decision.reasoning.risk_factors.clone()),
        llm_invalidation: Some(brain.decision.reasoning.invalidation.clone()),
        llm_latency_ms: Some(brain.latency_ms),
        fear_greed: None,
        social_sentiment: None,
        news_score: None,
        funding_rate: None,
        top_news_titles: None,
        user_id: 7773988648,
    };
    journal.insert_trade(&record)?;
    Ok(())
}
