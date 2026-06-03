//! Shared State — Central coordination context accessible by ALL agents.
//!
//! This module provides a thread-safe shared state that all agents can read/write
//! to enable proper coordination, feedback loops, and collective decision-making.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Strategy health tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyHealth {
    pub name: String,
    pub total_trades: u64,
    pub wins: u64,
    pub losses: u64,
    pub total_pnl: f64,
    pub win_rate: f64,
    pub avg_pnl: f64,
    pub loss_streak: u64,
    pub max_loss_streak: u64,
    pub last_trade_ts: Option<String>,
    pub last_trade_pnl: f64,
    pub enabled: bool,
    pub disable_reason: Option<String>,
}

/// Overall trading statistics
#[derive(Debug, Clone, Default)]
pub struct OverallStats {
    pub total_trades: u64,
    pub win_rate: f64,
    pub total_pnl: f64,
    pub last_trade_pnl: f64,
}

impl StrategyHealth {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            total_trades: 0,
            wins: 0,
            losses: 0,
            total_pnl: 0.0,
            win_rate: 0.0,
            avg_pnl: 0.0,
            loss_streak: 0,
            max_loss_streak: 0,
            last_trade_ts: None,
            last_trade_pnl: 0.0,
            enabled: true,
            disable_reason: None,
        }
    }

    pub fn record_trade(&mut self, pnl: f64) {
        self.total_trades += 1;
        self.total_pnl += pnl;
        self.avg_pnl = self.total_pnl / self.total_trades as f64;
        self.last_trade_pnl = pnl;
        self.last_trade_ts = Some(chrono::Utc::now().to_rfc3339());

        if pnl > 0.0 {
            self.wins += 1;
            self.loss_streak = 0;
        } else {
            self.losses += 1;
            self.loss_streak += 1;
            if self.loss_streak > self.max_loss_streak {
                self.max_loss_streak = self.loss_streak;
            }
        }

        self.win_rate = if self.total_trades > 0 {
            self.wins as f64 / self.total_trades as f64
        } else {
            0.0
        };
    }

    pub fn should_disable(&self) -> bool {
        // Disable only when catastrophically broken — bot must keep trading.
        // Win rate alone is never enough: a 20% WR with 5:1 RR is profitable.
        // Only disable on extreme loss streak OR deep dollar loss.
        self.loss_streak >= 20
            || (self.total_trades >= 30 && self.win_rate < 0.10)
            || (self.total_trades >= 15 && self.total_pnl < -80.0)
    }

    pub fn should_reduce_size(&self) -> bool {
        // Reduce size on extended streaks — never stop trading entirely.
        self.loss_streak >= 8 || (self.total_trades >= 15 && self.win_rate < 0.20)
    }

    pub fn size_multiplier(&self) -> f64 {
        if !self.enabled {
            return 0.0;
        }
        // Gentle reduction — keep bot trading at all times.
        // Sizing is the lever, not trade blocking.
        if self.loss_streak >= 10 {
            0.3
        } else if self.loss_streak >= 7 {
            0.5
        } else if self.loss_streak >= 4 {
            0.7
        } else if self.total_pnl > 0.0 && self.total_trades >= 10 {
            1.1 // Slight boost for profitable strategies
        } else {
            1.0
        }
    }
}

/// Symbol-level position tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolState {
    pub symbol: String,
    pub open_positions: u64,
    pub unrealized_pnl: f64,
    pub realized_pnl_today: f64,
    pub last_signal_time: Option<String>,
    pub current_regime: String,
}

/// Shared state accessible by all agents
#[derive(Debug)]
pub struct SharedState {
    // === CAPITAL & RISK ===
    pub equity: RwLock<f64>,
    pub initial_equity: RwLock<f64>,
    pub peak_equity: RwLock<f64>,
    pub realized_pnl_today: RwLock<f64>,
    pub unrealized_pnl: RwLock<f64>,

    // === SURVIVAL ===
    // Note: SurvivalAgent (agents/survival.rs) is the authoritative source for survival
    // decisions. These fields are display-only, updated by SurvivalAgent events.
    pub survival_mode: RwLock<SurvivalMode>,
    pub survival_score: RwLock<f64>,
    pub drawdown_pct: RwLock<f64>,

    // === STRATEGY HEALTH ===
    pub strategy_health: RwLock<HashMap<String, StrategyHealth>>,

    // === POSITION TRACKING ===
    pub open_positions: RwLock<u64>,
    pub max_open_positions: RwLock<u64>,
    pub symbol_states: RwLock<HashMap<String, SymbolState>>,

    // === MARKET CONTEXT ===
    pub current_regime: RwLock<String>,
    pub fear_greed: RwLock<i32>,
    pub funding_rate: RwLock<f64>,

    // === AGENT COORDINATION ===
    pub last_heartbeat: RwLock<HashMap<String, Instant>>,
    pub agent_errors: RwLock<HashMap<String, String>>,
    pub freeze_reason: RwLock<Option<String>>,

    // === LEARNING FEEDBACK ===
    pub strategy_adjustments: RwLock<HashMap<String, f64>>,
    pub recent_lessons: RwLock<Vec<String>>,

    // === SIGNAL ID COUNTER ===
    /// Global sequential signal counter for generating "S-00001" style IDs.
    /// Loaded from DB on startup, incremented atomically per new signal.
    pub signal_counter: AtomicU64,
}

/// Local survival mode for SharedState display only.
/// The authoritative survival decisions come from SurvivalAgent (agents/survival.rs).
/// This enum uses simple Normal/Defensive/Critical/Dead labels for Telegram status display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SurvivalMode {
    Normal,
    Defensive,
    Critical,
    Dead,
}

impl SurvivalMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            SurvivalMode::Normal => "Normal",
            SurvivalMode::Defensive => "Defensive",
            SurvivalMode::Critical => "Critical",
            SurvivalMode::Dead => "Dead",
        }
    }

    pub fn size_multiplier(&self) -> f64 {
        match self {
            SurvivalMode::Normal => 1.0,
            SurvivalMode::Defensive => 0.5,
            SurvivalMode::Critical => 0.2,
            SurvivalMode::Dead => 0.0,
        }
    }
}

impl SharedState {
    pub fn new(initial_equity: f64, max_open_positions: u64) -> Arc<Self> {
        Arc::new(Self {
            equity: RwLock::new(initial_equity),
            initial_equity: RwLock::new(initial_equity),
            peak_equity: RwLock::new(initial_equity),
            realized_pnl_today: RwLock::new(0.0),
            unrealized_pnl: RwLock::new(0.0),

            survival_mode: RwLock::new(SurvivalMode::Normal),
            survival_score: RwLock::new(100.0),
            drawdown_pct: RwLock::new(0.0),

            strategy_health: RwLock::new(HashMap::new()),

            open_positions: RwLock::new(0),
            max_open_positions: RwLock::new(max_open_positions),
            symbol_states: RwLock::new(HashMap::new()),

            current_regime: RwLock::new("UNKNOWN".to_string()),
            fear_greed: RwLock::new(50),
            funding_rate: RwLock::new(0.0),

            last_heartbeat: RwLock::new(HashMap::new()),
            agent_errors: RwLock::new(HashMap::new()),
            freeze_reason: RwLock::new(None),

            strategy_adjustments: RwLock::new(HashMap::new()),
            recent_lessons: RwLock::new(Vec::new()),
            signal_counter: AtomicU64::new(0),
        })
    }

    // === EQUITY METHODS ===

    /// Sync equity from persisted equity.json on disk (paper mode).
    /// Call after RiskManager::load_equity_from_disk to keep SharedState in sync.
    pub fn sync_from_persisted(&self) {
        const EQUITY_FILE: &str = "data/equity.json";
        if let Ok(data) = std::fs::read_to_string(EQUITY_FILE) {
            if let Ok(snap) = serde_json::from_str::<serde_json::Value>(&data) {
                let eq = snap.get("equity").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let peak = snap
                    .get("peak_equity")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let rpnl = snap
                    .get("realized_pnl_today")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                if eq > 0.0 {
                    *self.equity.write() = eq;
                    *self.peak_equity.write() = peak.max(eq);
                    *self.realized_pnl_today.write() = rpnl;
                    let dd = if peak > 0.0 {
                        ((peak - eq) / peak * 100.0).max(0.0)
                    } else {
                        0.0
                    };
                    *self.drawdown_pct.write() = dd;
                    tracing::info!(
                        equity = eq,
                        peak,
                        rpnl,
                        "SharedState synced from persisted equity"
                    );
                }
            }
        }
    }

    pub fn update_equity(&self, realized_pnl: f64) {
        let mut eq = self.equity.write();
        *eq += realized_pnl;
        let mut peak = self.peak_equity.write();
        if *eq > *peak {
            *peak = *eq;
        }
        let mut dd = self.drawdown_pct.write();
        *dd = if *peak > 0.0 {
            ((*peak - *eq) / *peak * 100.0).max(0.0)
        } else {
            0.0
        };
        let mut rpnl = self.realized_pnl_today.write();
        *rpnl += realized_pnl;
    }

    pub fn update_unrealized_pnl(&self, unrealized: f64) {
        let mut upnl = self.unrealized_pnl.write();
        *upnl = unrealized;
    }

    pub fn total_equity(&self) -> f64 {
        let eq = self.equity.read();
        let upnl = self.unrealized_pnl.read();
        *eq + *upnl
    }

    // === POSITION TRACKING ===

    pub fn can_open_position(&self) -> bool {
        let open = *self.open_positions.read();
        let max = *self.max_open_positions.read();
        open < max
    }

    // === STRATEGY HEALTH METHODS ===

    pub fn record_strategy_trade(&self, strategy: &str, pnl: f64) {
        let mut health = self.strategy_health.write();
        let entry = health
            .entry(strategy.to_string())
            .or_insert_with(|| StrategyHealth::new(strategy));
        entry.record_trade(pnl);

        // Auto-disable if needed
        if entry.should_disable() && entry.enabled {
            entry.enabled = false;
            entry.disable_reason = Some(format!(
                "Disabled: {} trades, {:.0}% win rate, ${:.2} PnL, {} loss streak",
                entry.total_trades,
                entry.win_rate * 100.0,
                entry.total_pnl,
                entry.loss_streak
            ));
        }
    }

    pub fn get_strategy_size_multiplier(&self, strategy: &str) -> f64 {
        let health = self.strategy_health.read();
        match health.get(strategy) {
            Some(h) => h.size_multiplier(),
            None => 1.0,
        }
    }

    pub fn is_strategy_enabled(&self, strategy: &str) -> bool {
        let health = self.strategy_health.read();
        match health.get(strategy) {
            Some(h) => h.enabled,
            None => true,
        }
    }

    pub fn get_strategy_summary(&self) -> String {
        let health = self.strategy_health.read();
        let mut summary = String::new();
        for (name, h) in health.iter() {
            summary.push_str(&format!(
                "{}: {:.0}% win, {} trades, ${:.2} PnL, streak {} | ",
                name,
                h.win_rate * 100.0,
                h.total_trades,
                h.total_pnl,
                h.loss_streak
            ));
        }
        if summary.is_empty() {
            "No data yet".to_string()
        } else {
            summary
        }
    }

    /// Get strategy health data for LLM context
    pub fn get_strategy_health(&self, strategy: &str) -> StrategyHealth {
        let health = self.strategy_health.read();
        match health.get(strategy) {
            Some(h) => h.clone(),
            None => StrategyHealth::new(strategy),
        }
    }

    /// Get overall trading stats
    pub fn get_overall_stats(&self) -> OverallStats {
        let health = self.strategy_health.read();
        let mut total_trades = 0u64;
        let mut total_wins = 0u64;
        let mut total_pnl = 0.0f64;
        let mut last_trade_pnl = 0.0f64;

        for h in health.values() {
            total_trades += h.total_trades;
            total_wins += h.wins;
            total_pnl += h.total_pnl;
            if h.last_trade_ts.is_some() {
                last_trade_pnl = h.last_trade_pnl;
            }
        }

        OverallStats {
            total_trades,
            win_rate: if total_trades > 0 {
                total_wins as f64 / total_trades as f64
            } else {
                0.0
            },
            total_pnl,
            last_trade_pnl,
        }
    }

    // === AGENT COORDINATION ===

    pub fn heartbeat(&self, agent_name: &str) {
        let mut hb = self.last_heartbeat.write();
        hb.insert(agent_name.to_string(), Instant::now());
    }

    pub fn report_error(&self, agent_name: &str, error: &str) {
        let mut errors = self.agent_errors.write();
        errors.insert(agent_name.to_string(), error.to_string());
    }

    pub fn clear_error(&self, agent_name: &str) {
        let mut errors = self.agent_errors.write();
        errors.remove(agent_name);
    }

    // === POSITION TRACKING ===

    pub fn on_position_opened(&self) {
        let mut open = self.open_positions.write();
        *open += 1;
    }

    pub fn set_open_positions(&self, n: u64) {
        *self.open_positions.write() = n;
    }

    pub fn on_position_closed(&self) {
        let mut open = self.open_positions.write();
        if *open > 0 {
            *open -= 1;
        }
    }

    // === LEARNING FEEDBACK ===

    pub fn add_lesson(&self, lesson: String) {
        let mut lessons = self.recent_lessons.write();
        lessons.push(lesson);
        // Keep only last 20 lessons
        if lessons.len() > 20 {
            lessons.remove(0);
        }
    }

    pub fn get_lessons(&self) -> Vec<String> {
        self.recent_lessons.read().clone()
    }

    // === STATUS ===

    pub fn status_summary(&self) -> String {
        let equity = *self.equity.read();
        let total_eq = self.total_equity();
        let upnl = *self.unrealized_pnl.read();
        let rpnl = *self.realized_pnl_today.read();
        let mode = *self.survival_mode.read();
        let score = *self.survival_score.read();
        let dd = *self.drawdown_pct.read();
        let open = *self.open_positions.read();
        let regime = self.current_regime.read().clone();

        format!(
            "Equity: ${:.2} (Total: ${:.2}) | Unrealized: ${:.2} | Today: ${:.2}\n\
             Survival: {} (score {:.0}) | DD: {:.1}% | Positions: {}\n\
             Regime: {}",
            equity,
            total_eq,
            upnl,
            rpnl,
            mode.as_str(),
            score,
            dd,
            open,
            regime
        )
    }

    // === SIGNAL ID ===

    /// Generate the next sequential signal ID in "S-00001" format.
    /// Thread-safe via atomic increment. Counter persists across restarts
    /// by loading from DB on startup.
    pub fn next_signal_id(&self) -> String {
        let n = self.signal_counter.fetch_add(1, Ordering::SeqCst) + 1;
        format!("S-{:05}", n)
    }

    /// Set the signal counter to a specific value (used on startup to sync with DB).
    pub fn set_signal_counter(&self, value: u64) {
        self.signal_counter.store(value, Ordering::SeqCst);
    }
}
