//! Lesson extraction — turn aggregate stats into actionable rules.

use crate::learning::memory::PerformanceMemory;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum LessonKind {
    /// Recent loss streak — pause the offending bucket for `cooldown_minutes`.
    LoseStreak,
    /// Long-term win rate too low — raise TA threshold and shrink size.
    StrategyDerate,
    /// Long-term win rate good — slightly relax threshold and boost size.
    StrategyBoost,
    /// Strategy/regime combo is hopeless — drop from the regime selector.
    RegimeBlacklist,
    /// LLM is over-confident — calibrate `min_confidence` upward.
    LlmCalibration,
    /// Symbol losing money for multiple days — skip for a day.
    SymbolDerate,
    /// Sharp drawdown in the last hour — pause everything for an hour.
    DrawdownCooldown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lesson {
    pub kind: LessonKind,
    /// Optional bucket the lesson applies to. `None` = global.
    pub strategy: Option<String>,
    pub regime: Option<String>,
    pub symbol: Option<String>,
    pub size_multiplier: f64,
    pub ta_threshold_delta: i16,
    pub llm_min_confidence_floor: Option<u8>,
    pub valid_until: DateTime<Utc>,
    pub reason: String,
}

impl Lesson {
    pub fn applies(&self, strategy: &str, regime: &str, symbol: &str) -> bool {
        if self.valid_until < Utc::now() {
            return false;
        }
        if let Some(s) = &self.strategy {
            if s != strategy {
                return false;
            }
        }
        if let Some(r) = &self.regime {
            if r != regime {
                return false;
            }
        }
        if let Some(sym) = &self.symbol {
            if sym != symbol {
                return false;
            }
        }
        true
    }

    /// Returns true when the lesson **bans** this trade entirely.
    pub fn is_block(&self) -> bool {
        matches!(
            self.kind,
            LessonKind::LoseStreak
                | LessonKind::RegimeBlacklist
                | LessonKind::DrawdownCooldown
                | LessonKind::SymbolDerate
        )
    }
}

/// Configuration thresholds for the lesson extractor.
#[derive(Debug, Clone)]
pub struct LessonConfig {
    pub min_trades_for_significance: u32,
    pub lose_streak_trigger: i32,
    pub lose_streak_cooldown_minutes: i64,
    pub derate_win_rate: f64,
    pub boost_win_rate: f64,
    pub regime_blacklist_win_rate: f64,
    pub regime_blacklist_min_trades: u32,
    pub drawdown_cooldown_pct: f64,
    pub drawdown_cooldown_minutes: i64,
    pub llm_overconf_actual_wr: f64,
    pub equity_for_drawdown: f64,
}

impl Default for LessonConfig {
    fn default() -> Self {
        Self {
            min_trades_for_significance: 10, // was 30 — learn faster
            lose_streak_trigger: -5,         // was -8 — react sooner
            lose_streak_cooldown_minutes: 15,
            derate_win_rate: 0.35,
            boost_win_rate: 0.65,
            regime_blacklist_win_rate: 0.30,
            regime_blacklist_min_trades: 5, // was 12 — react sooner
            drawdown_cooldown_pct: -5.0,
            drawdown_cooldown_minutes: 60,
            llm_overconf_actual_wr: 0.40,
            equity_for_drawdown: 5000.0,
        }
    }
}

pub struct LessonExtractor {
    pub cfg: LessonConfig,
}

impl LessonExtractor {
    pub fn new(cfg: LessonConfig) -> Self {
        Self { cfg }
    }

    pub fn extract(&self, mem: &PerformanceMemory) -> Vec<Lesson> {
        let now = Utc::now();
        let mut out = Vec::new();

        // 1. Drawdown cooldown — reduce size, NEVER block completely
        let dd_pct = mem.recent_hour_pnl / self.cfg.equity_for_drawdown * 100.0;
        if dd_pct <= self.cfg.drawdown_cooldown_pct && mem.recent_hour_trades >= 2 {
            out.push(Lesson {
                kind: LessonKind::DrawdownCooldown,
                strategy: None,
                regime: None,
                symbol: None,
                size_multiplier: 0.3, // was 0.0 — reduce, don't kill
                ta_threshold_delta: 10,
                llm_min_confidence_floor: Some(65),
                valid_until: now + chrono::Duration::minutes(self.cfg.drawdown_cooldown_minutes),
                reason: format!(
                    "{:.2}% drawdown in last 60 min over {} trades — size reduced",
                    dd_pct, mem.recent_hour_trades
                ),
            });
        }

        // 2. Per-(strategy, symbol) lose streak — reduce size, keep trading
        for ((strategy, symbol), s) in &mem.by_strategy_symbol {
            if s.recent_streak <= self.cfg.lose_streak_trigger {
                out.push(Lesson {
                    kind: LessonKind::LoseStreak,
                    strategy: Some(strategy.clone()),
                    regime: None,
                    symbol: Some(symbol.clone()),
                    size_multiplier: 0.4, // was 0.0 — reduce, don't kill
                    ta_threshold_delta: 10,
                    llm_min_confidence_floor: Some(65),
                    valid_until: now
                        + chrono::Duration::minutes(self.cfg.lose_streak_cooldown_minutes),
                    reason: format!(
                        "lose-streak {} on {strategy}/{symbol} — size reduced",
                        s.recent_streak.abs()
                    ),
                });
            }
        }

        // 3. Strategy derate / boost — based on long-term win rate.
        for (strategy, s) in &mem.by_strategy {
            if s.trades < self.cfg.min_trades_for_significance {
                continue;
            }
            let wr = s.win_rate();
            if wr < self.cfg.derate_win_rate {
                out.push(Lesson {
                    kind: LessonKind::StrategyDerate,
                    strategy: Some(strategy.clone()),
                    regime: None,
                    symbol: None,
                    size_multiplier: 0.5,
                    ta_threshold_delta: 10,
                    llm_min_confidence_floor: Some(80),
                    valid_until: now + chrono::Duration::hours(6),
                    reason: format!(
                        "WR {:.1}% on {strategy} ({} trades) — derate",
                        wr * 100.0,
                        s.trades
                    ),
                });
            } else if wr >= self.cfg.boost_win_rate && s.profit_factor() >= 1.5 {
                out.push(Lesson {
                    kind: LessonKind::StrategyBoost,
                    strategy: Some(strategy.clone()),
                    regime: None,
                    symbol: None,
                    size_multiplier: 1.2,
                    ta_threshold_delta: -5,
                    llm_min_confidence_floor: None,
                    valid_until: now + chrono::Duration::hours(6),
                    reason: format!(
                        "WR {:.1}% PF {:.2} on {strategy} ({} trades) — boost",
                        wr * 100.0,
                        s.profit_factor(),
                        s.trades
                    ),
                });
            }
        }

        // 4. (Strategy, regime) derate — reduce size, never block
        for ((strategy, regime), s) in &mem.by_strategy_regime {
            if s.trades >= self.cfg.regime_blacklist_min_trades
                && s.win_rate() < self.cfg.regime_blacklist_win_rate
            {
                out.push(Lesson {
                    kind: LessonKind::RegimeBlacklist,
                    strategy: Some(strategy.clone()),
                    regime: Some(regime.clone()),
                    symbol: None,
                    size_multiplier: 0.2, // was 0.0 — reduce, don't kill
                    ta_threshold_delta: 15,
                    llm_min_confidence_floor: Some(75),
                    valid_until: now + chrono::Duration::hours(3), // was 12h
                    reason: format!(
                        "WR {:.1}% on {strategy} during {regime} ({} trades) — size reduced 3h",
                        s.win_rate() * 100.0,
                        s.trades
                    ),
                });
            }
        }

        // 5. LLM calibration — if the 80-100 confidence buckets are below
        //    `llm_overconf_actual_wr` actual WR, raise the gate.
        let high_conf = combine(&mem.llm_calibration[8..]);
        if high_conf.trades >= self.cfg.min_trades_for_significance
            && high_conf.win_rate() < self.cfg.llm_overconf_actual_wr
        {
            out.push(Lesson {
                kind: LessonKind::LlmCalibration,
                strategy: None,
                regime: None,
                symbol: None,
                size_multiplier: 1.0,
                ta_threshold_delta: 0,
                llm_min_confidence_floor: Some(90),
                valid_until: now + chrono::Duration::hours(12),
                reason: format!(
                    "high-conf LLM picks landing {:.1}% WR ({} trades) — raising gate to 90",
                    high_conf.win_rate() * 100.0,
                    high_conf.trades
                ),
            });
        }

        // 6. Per-symbol derate — reduce size, NEVER block completely
        for (symbol, s) in &mem.by_symbol {
            if s.trades >= self.cfg.min_trades_for_significance
                && s.net_pnl_usd < 0.0
                && s.win_rate() < self.cfg.derate_win_rate
            {
                out.push(Lesson {
                    kind: LessonKind::SymbolDerate,
                    strategy: None,
                    regime: None,
                    symbol: Some(symbol.clone()),
                    size_multiplier: 0.3, // was 0.0 — reduce, don't kill
                    ta_threshold_delta: 15,
                    llm_min_confidence_floor: Some(70),
                    valid_until: now + chrono::Duration::hours(4), // was 24h
                    reason: format!(
                        "{symbol} net {:+.2} over {} trades, WR {:.1}% — size reduced 4h",
                        s.net_pnl_usd,
                        s.trades,
                        s.win_rate() * 100.0
                    ),
                });
            }
        }

        out
    }
}

fn combine(
    stats: &[crate::learning::memory::StrategyStats],
) -> crate::learning::memory::StrategyStats {
    let mut out = crate::learning::memory::StrategyStats::default();
    for s in stats {
        out.trades += s.trades;
        out.wins += s.wins;
        out.losses += s.losses;
        out.net_pnl_usd += s.net_pnl_usd;
        out.gross_profit += s.gross_profit;
        out.gross_loss += s.gross_loss;
    }
    out
}
