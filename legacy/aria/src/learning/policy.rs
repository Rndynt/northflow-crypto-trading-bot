//! `LearningPolicy` — the runtime object the rest of the engine consults.
//!
//! Created from a `PerformanceMemory` + `LessonExtractor` and queried before
//! every decision: filter strategy candidates, compute size multiplier,
//! provide a human-readable summary for the LLM context.

use crate::learning::lessons::Lesson;
use crate::learning::memory::{PerformanceMemory, StrategyStats};
use chrono::Utc;
use parking_lot::RwLock;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize)]
pub struct PolicyVerdict {
    pub allowed: bool,
    pub size_multiplier: f64,
    pub ta_threshold_delta: i16,
    pub llm_min_confidence_floor: Option<u8>,
    pub matched_lessons: Vec<String>,
}

impl Default for PolicyVerdict {
    fn default() -> Self {
        Self {
            allowed: true,
            size_multiplier: 1.0,
            ta_threshold_delta: 0,
            llm_min_confidence_floor: None,
            matched_lessons: Vec::new(),
        }
    }
}

#[derive(Default)]
struct Inner {
    memory: PerformanceMemory,
    lessons: Vec<Lesson>,
}

/// Thread-safe policy. Cheap to clone (Arc).
#[derive(Clone, Default)]
pub struct LearningPolicy {
    inner: Arc<RwLock<Inner>>,
}

impl LearningPolicy {
    pub fn update(&self, memory: PerformanceMemory, lessons: Vec<Lesson>) {
        let mut g = self.inner.write();
        g.memory = memory;
        g.lessons = lessons;
    }

    pub fn evaluate(&self, strategy: &str, regime: &str, symbol: &str) -> PolicyVerdict {
        let g = self.inner.read();
        let mut verdict = PolicyVerdict::default();
        for l in &g.lessons {
            if !l.applies(strategy, regime, symbol) {
                continue;
            }
            verdict.matched_lessons.push(l.reason.clone());
            // NEVER block — always reduce size. is_block() removed.
            verdict.size_multiplier *= l.size_multiplier;
            verdict.ta_threshold_delta += l.ta_threshold_delta;
            if let Some(f) = l.llm_min_confidence_floor {
                verdict.llm_min_confidence_floor =
                    Some(verdict.llm_min_confidence_floor.unwrap_or(0).max(f));
            }
        }
        // Enforce minimum size multiplier — bot ALWAYS trades with at least 20% size
        verdict.size_multiplier = verdict.size_multiplier.max(0.1); // min 10% — allow learning to reduce bad setups
        verdict
    }

    /// Snapshot of all currently active lessons for telemetry.
    pub fn active_lessons(&self) -> Vec<Lesson> {
        let g = self.inner.read();
        let now = Utc::now();
        g.lessons
            .iter()
            .filter(|l| l.valid_until > now)
            .cloned()
            .collect()
    }

    /// Compact human-readable performance summary to inject into the LLM prompt.
    pub fn historical_summary(&self, strategy: &str, regime: &str, symbol: &str) -> String {
        let g = self.inner.read();
        let mut out = String::new();

        // ── 1. LLM-generated qualitative insights (most important — shown first) ──
        if !g.memory.llm_insights.is_empty() {
            out.push_str("=== LEARNING INSIGHTS (read carefully before deciding) ===\n");
            for (i, insight) in g.memory.llm_insights.iter().enumerate() {
                out.push_str(&format!("{}. {}\n", i + 1, insight));
            }
            out.push_str("=== END LEARNING INSIGHTS ===\n\n");
        }

        // ── 2. Quantitative stats ──
        if let Some(s) = g.by_overall() {
            out.push_str(&format!(
                "Overall: {} trades · WR {:.1}% · PF {:.2} · net ${:+.2}\n",
                s.trades,
                s.win_rate() * 100.0,
                s.profit_factor(),
                s.net_pnl_usd
            ));
        }
        if let Some(s) = g.memory.by_strategy.get(strategy) {
            let note = if s.net_pnl_usd > 0.0 {
                format!(" [net +${:.2} — profitable]", s.net_pnl_usd)
            } else if s.recent_streak <= -3 {
                format!(" [streak {} — reduce size to 0.5x]", s.recent_streak)
            } else {
                String::new()
            };
            out.push_str(&format!(
                "Strategy {strategy}: {} trades · WR {:.1}% · PF {:.2} · net ${:+.2} · streak {} · last5 {}{}\n",
                s.trades,
                s.win_rate() * 100.0,
                s.profit_factor(),
                s.net_pnl_usd,
                s.recent_streak,
                fmt_outcomes(s),
                note,
            ));
        }
        if let Some(s) = g
            .memory
            .by_strategy_regime
            .get(&(strategy.into(), regime.into()))
        {
            out.push_str(&format!(
                "Regime {regime} for {strategy}: {} trades · WR {:.1}% · net ${:+.2}\n",
                s.trades,
                s.win_rate() * 100.0,
                s.net_pnl_usd,
            ));
        }
        if let Some(s) = g
            .memory
            .by_strategy_symbol
            .get(&(strategy.into(), symbol.into()))
        {
            out.push_str(&format!(
                "{symbol} on {strategy}: {} trades · WR {:.1}% · streak {}\n",
                s.trades,
                s.win_rate() * 100.0,
                s.recent_streak,
            ));
        }

        // ── 3. Active mechanical lessons ──
        let active = g
            .lessons
            .iter()
            .filter(|l| l.valid_until > Utc::now() && l.applies(strategy, regime, symbol))
            .map(|l| format!("- {:?}: {}", l.kind, l.reason))
            .collect::<Vec<_>>()
            .join("\n");
        if !active.is_empty() {
            out.push_str("Active lessons:\n");
            out.push_str(&active);
            out.push('\n');
        }

        if out.is_empty() {
            out.push_str("No prior trades — running on TA + LLM only.\n");
        }
        out
    }

    /// Read-only access to per-strategy stats, mostly for `/metrics`.
    pub fn strategy_stats(&self) -> HashMap<String, StrategyStats> {
        let g = self.inner.read();
        g.memory.by_strategy.clone()
    }
}

impl Inner {
    fn by_overall(&self) -> Option<&StrategyStats> {
        if self.memory.overall.trades == 0 {
            None
        } else {
            Some(&self.memory.overall)
        }
    }
}

fn fmt_outcomes(s: &StrategyStats) -> String {
    s.last_5_outcomes
        .iter()
        .map(|w| if *w { 'W' } else { 'L' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learning::lessons::{LessonConfig, LessonExtractor};
    use crate::monitoring::logger::ClosedTrade;

    fn t(strat: &str, sym: &str, regime: &str, pnl: f64) -> ClosedTrade {
        ClosedTrade {
            symbol: sym.into(),
            direction: "LONG".into(),
            strategy: strat.into(),
            regime: regime.into(),
            entry_time: Utc::now(),
            exit_time: Utc::now(),
            pnl_usd: pnl,
            pnl_pct: pnl,
            ta_confidence: Some(70),
            llm_confidence: Some(80),
        }
    }

    #[test]
    fn lose_streak_reduces_size() {
        // 3 consecutive losses on same (strategy, symbol) triggers LoseStreak.
        let trades = vec![
            t("ema_ribbon", "BTCUSDT", "TRENDING", -1.0),
            t("ema_ribbon", "BTCUSDT", "TRENDING", -1.0),
            t("ema_ribbon", "BTCUSDT", "TRENDING", -1.0),
        ];
        let mem = PerformanceMemory::build(&trades);
        let extractor = LessonExtractor::new(LessonConfig::default());
        let lessons = extractor.extract(&mem);
        let policy = LearningPolicy::default();
        policy.update(mem, lessons);

        let v = policy.evaluate("ema_ribbon", "TRENDING", "BTCUSDT");
        // Policy no longer blocks, only reduces size (min 0.5 enforced)
        assert!(v.allowed);
        assert!(v.size_multiplier < 1.0); // size dikurangi
        assert!(v.size_multiplier >= 0.5); // tapi tidak pernah di bawah minimum
        assert!(!v.matched_lessons.is_empty());

        // Different symbol still at full size.
        let v2 = policy.evaluate("ema_ribbon", "TRENDING", "ETHUSDT");
        assert!(v2.allowed);
        assert!((v2.size_multiplier - 1.0).abs() < 0.01); // no lesson matched
    }

    #[test]
    fn boost_increases_size() {
        let mut trades = Vec::new();
        for _ in 0..7 {
            trades.push(t("vwap_scalp", "BTCUSDT", "RANGING", 5.0));
        }
        for _ in 0..2 {
            trades.push(t("vwap_scalp", "BTCUSDT", "RANGING", -1.0));
        }
        let mem = PerformanceMemory::build(&trades);
        let lessons = LessonExtractor::new(LessonConfig::default()).extract(&mem);
        let policy = LearningPolicy::default();
        policy.update(mem, lessons);
        let v = policy.evaluate("vwap_scalp", "RANGING", "BTCUSDT");
        assert!(v.allowed);
        assert!(v.size_multiplier > 1.0);
        assert!(v.ta_threshold_delta < 0);
    }
}
