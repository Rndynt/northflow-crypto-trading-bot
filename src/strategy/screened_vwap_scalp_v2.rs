//! ScreenedVwapScalpV2 — stricter, cost-aware multi-timeframe scalp strategy.
//!
//! Timeframe roles (explicit, never inferred from order):
//!   entry        = 1m  — entry and execution signal timeframe
//!   confirmation = 5m  — intermediate confirmation layer
//!   screening    = 15m — market regime / bias filter
//!
//! V2 adds configurable ATR-based geometry, EMA ribbon alignment, VWAP/EMA21
//! distance filters, minimum expected edge filters, volume ratio, and cooldown.
//!
//! The strategy may only emit a Signal.  It does not:
//!   - place orders
//!   - call exchange APIs
//!   - call LLMs
//!   - calculate final position size
//!   - mutate account state
//!   - run a backtest
//!   - write reports
//!
//! This is a research/diagnostic variant only. Not a profitability claim.

use crate::config::V2Config;
use crate::core::{NorthflowError, Side, Signal, SignalId, StrategyId, Timeframe};
use crate::strategy::regime::{MarketRegime, classify_screening_regime};
use crate::strategy::traits::{MultiTimeframeInput, Strategy, StrategyContext};

// ── Public strategy struct ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ScreenedVwapScalpV2 {
    pub cfg: V2Config,
}

impl ScreenedVwapScalpV2 {
    pub fn new(cfg: V2Config) -> Self {
        Self { cfg }
    }
}

impl Strategy for ScreenedVwapScalpV2 {
    fn strategy_id(&self) -> &'static str {
        "screened_vwap_scalp_v2"
    }

    fn evaluate(
        &self,
        ctx: &StrategyContext,
        input: &MultiTimeframeInput,
    ) -> Result<Option<Signal>, NorthflowError> {
        // ── 0. Defensive candle validation ───────────────────────────────────
        input.entry_candle.validate()?;
        input.confirmation_candle.validate()?;
        input.screening_candle.validate()?;

        // ── 1. Required entry indicators ─────────────────────────────────────
        let ema_8 = match input.entry_indicators.ema_8 {
            Some(v) => v,
            None => return Ok(None),
        };
        let ema_21 = match input.entry_indicators.ema_21 {
            Some(v) => v,
            None => return Ok(None),
        };
        let ema_50_entry = match input.entry_indicators.ema_50 {
            Some(v) => v,
            None => return Ok(None),
        };
        let atr = match input.entry_indicators.atr_14 {
            Some(v) => v,
            None => return Ok(None),
        };
        let vwap = match input.entry_indicators.vwap {
            Some(v) => v,
            None => return Ok(None),
        };
        let volume_sma_20 = match input.entry_indicators.volume_sma_20 {
            Some(v) => v,
            None => return Ok(None),
        };

        // ── 2. Screening and confirmation regime ─────────────────────────────
        let screening_regime =
            classify_screening_regime(input.screening_candle, &input.screening_indicators);
        let confirmation_regime =
            classify_screening_regime(input.confirmation_candle, &input.confirmation_indicators);

        // ── 3. Screening gate: Neutral or Unknown → no signal ────────────────
        if screening_regime == MarketRegime::Neutral || screening_regime == MarketRegime::Unknown {
            return Ok(None);
        }

        // ── 4. Determine side from screening regime ───────────────────────────
        let side = match screening_regime {
            MarketRegime::Bullish => Side::Long,
            MarketRegime::Bearish => Side::Short,
            _ => return Ok(None),
        };

        // ── 5. Direction toggles ─────────────────────────────────────────────
        match side {
            Side::Long if !self.cfg.enable_long => return Ok(None),
            Side::Short if !self.cfg.enable_short => return Ok(None),
            _ => {}
        }

        // ── 6. Strict confirmation gate ──────────────────────────────────────
        let confirmation_ok = match side {
            Side::Long => {
                let strict = confirmation_regime == MarketRegime::Bullish;
                let neutral_allowed = !self.cfg.require_strict_confirmation
                    && self.cfg.allow_neutral_confirmation
                    && confirmation_regime == MarketRegime::Neutral;
                strict || neutral_allowed
            }
            Side::Short => {
                let strict = confirmation_regime == MarketRegime::Bearish;
                let neutral_allowed = !self.cfg.require_strict_confirmation
                    && self.cfg.allow_neutral_confirmation
                    && confirmation_regime == MarketRegime::Neutral;
                strict || neutral_allowed
            }
        };
        if !confirmation_ok {
            return Ok(None);
        }

        let close = input.entry_candle.close;

        // ── 7. EMA ribbon alignment ──────────────────────────────────────────
        if self.cfg.require_ema_ribbon_alignment {
            let ribbon_ok = match side {
                Side::Long => ema_8 > ema_21 && ema_21 > ema_50_entry && close > ema_21,
                Side::Short => ema_8 < ema_21 && ema_21 < ema_50_entry && close < ema_21,
            };
            if !ribbon_ok {
                return Ok(None);
            }
        }

        // ── 8. ATR validity ──────────────────────────────────────────────────
        if atr <= 0.0 {
            return Ok(None);
        }
        if close <= 0.0 {
            return Ok(None);
        }
        let atr_bps = atr / close * 10_000.0;
        if atr_bps < self.cfg.min_atr_bps || atr_bps > self.cfg.max_atr_bps {
            return Ok(None);
        }

        // ── 9. VWAP / EMA21 distance filter ─────────────────────────────────
        let distance_to_vwap = (close - vwap).abs();
        let distance_to_ema21 = (close - ema_21).abs();
        let nearest_distance = distance_to_vwap.min(distance_to_ema21);
        let distance_atr = nearest_distance / atr;
        if distance_atr < self.cfg.vwap_distance_atr_min
            || distance_atr > self.cfg.vwap_distance_atr_max
        {
            return Ok(None);
        }

        // ── 10. Volume ratio filter ──────────────────────────────────────────
        if volume_sma_20 <= 0.0 {
            return Ok(None);
        }
        let volume_ratio = input.entry_candle.volume / volume_sma_20;
        if volume_ratio < self.cfg.min_volume_ratio {
            return Ok(None);
        }

        // ── 11. Signal geometry ──────────────────────────────────────────────
        let (stop_loss, take_profit) = match side {
            Side::Long => (
                close - atr * self.cfg.sl_atr_multiple,
                close + atr * self.cfg.tp_atr_multiple,
            ),
            Side::Short => (
                close + atr * self.cfg.sl_atr_multiple,
                close - atr * self.cfg.tp_atr_multiple,
            ),
        };

        // ── 12. Expected edge filters ────────────────────────────────────────
        let expected_reward_bps = (take_profit - close).abs() / close * 10_000.0;
        let estimated_cost_bps = ctx.estimated_cost_bps;
        let expected_net_edge_bps = expected_reward_bps - estimated_cost_bps;

        if expected_reward_bps < self.cfg.min_expected_reward_bps {
            return Ok(None);
        }
        if expected_net_edge_bps < self.cfg.min_expected_net_edge_bps {
            return Ok(None);
        }

        // ── 13. Confidence scoring ───────────────────────────────────────────
        // All filters above are hard gates. Each passed filter contributes +10.
        let mut confidence: i16 = 50;
        confidence += 10; // screening and confirmation align
        confidence += 10; // EMA ribbon aligns (or not required, both add)
        confidence += 10; // volume_ratio passes
        confidence += 10; // expected_net_edge_bps passes
        confidence += 10; // VWAP/EMA21 distance passes
        let confidence = confidence.clamp(0, 100) as u8;

        if confidence < ctx.min_confidence {
            return Ok(None);
        }

        // ── 14. Filters ──────────────────────────────────────────────────────
        let mut filters_passed: Vec<String> = Vec::new();
        match side {
            Side::Long => {
                filters_passed.push("screening_bullish".to_string());
                filters_passed.push("confirmation_bullish".to_string());
                if self.cfg.require_ema_ribbon_alignment {
                    filters_passed.push("ema_ribbon_long".to_string());
                }
            }
            Side::Short => {
                filters_passed.push("screening_bearish".to_string());
                filters_passed.push("confirmation_bearish".to_string());
                if self.cfg.require_ema_ribbon_alignment {
                    filters_passed.push("ema_ribbon_short".to_string());
                }
            }
        }
        filters_passed.push("atr_bps_in_range".to_string());
        filters_passed.push("near_vwap_or_ema21".to_string());
        filters_passed.push("volume_ratio_ok".to_string());
        filters_passed.push("expected_reward_ok".to_string());
        filters_passed.push("expected_net_edge_ok".to_string());
        filters_passed.push("direction_enabled".to_string());
        filters_passed.push("confidence_ok".to_string());

        let filters_failed: Vec<String> = Vec::new();

        // ── 15. Entry reason ─────────────────────────────────────────────────
        let entry_reason = match side {
            Side::Long => format!(
                "15m bullish, 5m bullish, 1m ema_ribbon_long, \
                 near VWAP/EMA21, volume_ratio={:.2}, atr_bps={:.1}",
                volume_ratio, atr_bps
            ),
            Side::Short => format!(
                "15m bearish, 5m bearish, 1m ema_ribbon_short, \
                 near VWAP/EMA21, volume_ratio={:.2}, atr_bps={:.1}",
                volume_ratio, atr_bps
            ),
        };

        // ── 16. Build and validate signal ────────────────────────────────────
        let signal = Signal {
            signal_id: make_signal_id(ctx.signal_index),
            symbol: ctx.symbol.clone(),
            strategy_id: StrategyId::new(self.strategy_id()),
            side,
            entry_timeframe: Timeframe::OneMinute,
            screening_timeframe: Timeframe::FifteenMinute,
            confirmation_timeframe: Timeframe::FiveMinute,
            entry_time: input.entry_candle.timestamp,
            entry_price: close,
            stop_loss,
            take_profit,
            confidence,
            regime: screening_regime.as_str().to_string(),
            entry_reason,
            filters_passed,
            filters_failed,
            expected_reward_bps,
            estimated_cost_bps,
            expected_net_edge_bps,
        };

        signal.validate()?;
        Ok(Some(signal))
    }
}

// ── Signal ID generation ──────────────────────────────────────────────────────

fn make_signal_id(index: u64) -> SignalId {
    SignalId::new(format!("SIG-BT-{index:08}"))
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Candle, Symbol, Timeframe};
    use crate::indicators::IndicatorSnapshot;
    use crate::strategy::traits::{MultiTimeframeInput, StrategyContext};

    // ── Test helpers ──────────────────────────────────────────────────────────

    fn make_candle_v2(close: f64, volume: f64) -> Candle {
        Candle {
            timestamp: 1_700_000_000_000,
            open: close - 0.5,
            high: close + 1.0,
            low: close - 1.0,
            close,
            volume,
        }
    }

    fn default_ctx() -> StrategyContext {
        StrategyContext {
            symbol: Symbol::new("BTCUSDT").unwrap(),
            signal_index: 1,
            estimated_cost_bps: 9.0,
            min_confidence: 50,
        }
    }

    fn default_v2_cfg() -> V2Config {
        V2Config::default()
    }

    fn strategy() -> ScreenedVwapScalpV2 {
        ScreenedVwapScalpV2::new(default_v2_cfg())
    }

    // Long entry: close=102, ema_8=101.5 > ema_21=101.0 > ema_50=100.5
    // close(102) > ema_21(101) → ribbon long ok
    // atr=1.0, atr_bps = 1.0/102*10000 ≈ 98 bps → in [5, 150] ✓
    // vwap=102.1 → |102-102.1|/1.0 = 0.1 atr ≤ 2.0 ✓
    // volume=2000, sma20=1000 → ratio=2.0 ≥ 1.0 ✓
    // tp = 102 + 1.0*2.0 = 104, reward = 2/102*10000 ≈ 196 bps ≥ 20 ✓
    // net_edge = 196 - 9 = 187 ≥ 5 ✓
    fn long_entry_candle() -> Candle {
        make_candle_v2(102.0, 2000.0)
    }

    fn long_entry_snapshot() -> IndicatorSnapshot {
        IndicatorSnapshot {
            ema_8: Some(101.5),
            ema_21: Some(101.0),
            ema_50: Some(100.5),
            atr_14: Some(1.0),
            vwap: Some(102.1),
            volume_sma_20: Some(1000.0),
            ..Default::default()
        }
    }

    // Short entry: close=98, ema_8=98.5 < ema_21=99.0 < ema_50=99.5
    // close(98) < ema_21(99) → ribbon short ok
    // atr=1.0, atr_bps ≈ 102 bps → in [5, 150] ✓
    // vwap=97.9 → |98-97.9|/1.0 = 0.1 atr ≤ 2.0 ✓
    // volume=2000, sma20=1000 → ratio=2.0 ≥ 1.0 ✓
    // tp = 98 - 1.0*2.0 = 96, reward = 2/98*10000 ≈ 204 bps ≥ 20 ✓
    // net_edge ≈ 195 ≥ 5 ✓
    fn short_entry_candle() -> Candle {
        make_candle_v2(98.0, 2000.0)
    }

    fn short_entry_snapshot() -> IndicatorSnapshot {
        IndicatorSnapshot {
            ema_8: Some(98.5),
            ema_21: Some(99.0),
            ema_50: Some(99.5),
            atr_14: Some(1.0),
            vwap: Some(97.9),
            volume_sma_20: Some(1000.0),
            ..Default::default()
        }
    }

    // Bullish candle: close=105, ema_50=100 > ema_200=90 and close > ema_50 → Bullish
    fn bullish_candle() -> Candle {
        make_candle_v2(105.0, 1000.0)
    }

    fn bullish_snapshot() -> IndicatorSnapshot {
        IndicatorSnapshot {
            ema_50: Some(100.0),
            ema_200: Some(90.0),
            ..Default::default()
        }
    }

    // Bearish candle: close=85, ema_50=90 < ema_200=100 and close < ema_50 → Bearish
    fn bearish_candle() -> Candle {
        make_candle_v2(85.0, 1000.0)
    }

    fn bearish_snapshot() -> IndicatorSnapshot {
        IndicatorSnapshot {
            ema_50: Some(90.0),
            ema_200: Some(100.0),
            ..Default::default()
        }
    }

    // Neutral: ema_50=100 > ema_200=90 but close=95 < ema_50=100 → Neutral
    fn neutral_candle() -> Candle {
        make_candle_v2(95.0, 1000.0)
    }

    fn neutral_snapshot() -> IndicatorSnapshot {
        IndicatorSnapshot {
            ema_50: Some(100.0),
            ema_200: Some(90.0),
            ..Default::default()
        }
    }

    fn long_input() -> MultiTimeframeInput {
        MultiTimeframeInput {
            entry_candle: long_entry_candle(),
            confirmation_candle: bullish_candle(),
            screening_candle: bullish_candle(),
            entry_indicators: long_entry_snapshot(),
            confirmation_indicators: bullish_snapshot(),
            screening_indicators: bullish_snapshot(),
        }
    }

    fn short_input() -> MultiTimeframeInput {
        MultiTimeframeInput {
            entry_candle: short_entry_candle(),
            confirmation_candle: bearish_candle(),
            screening_candle: bearish_candle(),
            entry_indicators: short_entry_snapshot(),
            confirmation_indicators: bearish_snapshot(),
            screening_indicators: bearish_snapshot(),
        }
    }

    // ── Strategy ID ───────────────────────────────────────────────────────────

    #[test]
    fn v2_strategy_id_is_screened_vwap_scalp_v2() {
        assert_eq!(strategy().strategy_id(), "screened_vwap_scalp_v2");
    }

    #[test]
    fn v2_signal_id_is_deterministic() {
        let mut ctx = default_ctx();
        ctx.signal_index = 7;
        let sig = strategy().evaluate(&ctx, &long_input()).unwrap().unwrap();
        assert_eq!(sig.signal_id.as_str(), "SIG-BT-00000007");
    }

    // ── Missing indicators ────────────────────────────────────────────────────

    #[test]
    fn v2_returns_none_when_indicators_missing() {
        let mut input = long_input();
        input.entry_indicators = IndicatorSnapshot::default();
        let result = strategy().evaluate(&default_ctx(), &input).unwrap();
        assert!(result.is_none());
    }

    // ── Screening regime ──────────────────────────────────────────────────────

    #[test]
    fn v2_returns_none_when_screening_neutral() {
        let mut input = long_input();
        input.screening_candle = neutral_candle();
        input.screening_indicators = neutral_snapshot();
        let result = strategy().evaluate(&default_ctx(), &input).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn v2_long_requires_bullish_screening() {
        let mut input = long_input();
        // Use bearish screening with long entry — no signal
        input.screening_candle = bearish_candle();
        input.screening_indicators = bearish_snapshot();
        let result = strategy().evaluate(&default_ctx(), &input).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn v2_short_requires_bearish_screening() {
        let mut input = short_input();
        // Use bullish screening with short entry — no signal
        input.screening_candle = bullish_candle();
        input.screening_indicators = bullish_snapshot();
        let result = strategy().evaluate(&default_ctx(), &input).unwrap();
        assert!(result.is_none());
    }

    // ── Strict confirmation ───────────────────────────────────────────────────

    #[test]
    fn v2_long_requires_strict_confirmation_by_default() {
        let mut input = long_input();
        // bearish confirmation while screening is bullish → no signal
        input.confirmation_candle = bearish_candle();
        input.confirmation_indicators = bearish_snapshot();
        let result = strategy().evaluate(&default_ctx(), &input).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn v2_short_requires_strict_confirmation_by_default() {
        let mut input = short_input();
        // bullish confirmation while screening is bearish → no signal
        input.confirmation_candle = bullish_candle();
        input.confirmation_indicators = bullish_snapshot();
        let result = strategy().evaluate(&default_ctx(), &input).unwrap();
        assert!(result.is_none());
    }

    // ── EMA ribbon alignment ──────────────────────────────────────────────────

    #[test]
    fn v2_long_requires_ema_ribbon_alignment() {
        let mut input = long_input();
        // Break ribbon: ema_8 < ema_21 → fails long ribbon
        let mut snap = long_entry_snapshot();
        snap.ema_8 = Some(100.0); // below ema_21=101.0
        input.entry_indicators = snap;
        let result = strategy().evaluate(&default_ctx(), &input).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn v2_short_requires_ema_ribbon_alignment() {
        let mut input = short_input();
        // Break ribbon: ema_8 > ema_21 → fails short ribbon
        let mut snap = short_entry_snapshot();
        snap.ema_8 = Some(100.0); // above ema_21=99.0
        input.entry_indicators = snap;
        let result = strategy().evaluate(&default_ctx(), &input).unwrap();
        assert!(result.is_none());
    }

    // ── Expected reward / net edge filters ───────────────────────────────────

    #[test]
    fn v2_rejects_expected_reward_below_min() {
        // Use very high min_expected_reward_bps so nothing can pass
        let mut cfg = default_v2_cfg();
        cfg.min_expected_reward_bps = 9999.0;
        let strat = ScreenedVwapScalpV2::new(cfg);
        let result = strat.evaluate(&default_ctx(), &long_input()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn v2_rejects_expected_net_edge_below_min() {
        // cost_bps > reward_bps → net_edge negative → below any positive min
        let mut cfg = default_v2_cfg();
        cfg.min_expected_net_edge_bps = 9999.0;
        let strat = ScreenedVwapScalpV2::new(cfg);
        let result = strat.evaluate(&default_ctx(), &long_input()).unwrap();
        assert!(result.is_none());
    }

    // ── ATR bps filters ───────────────────────────────────────────────────────

    #[test]
    fn v2_rejects_atr_bps_below_min() {
        let mut cfg = default_v2_cfg();
        // atr_bps ≈ 98 but require ≥ 200
        cfg.min_atr_bps = 200.0;
        cfg.max_atr_bps = 300.0;
        let strat = ScreenedVwapScalpV2::new(cfg);
        let result = strat.evaluate(&default_ctx(), &long_input()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn v2_rejects_atr_bps_above_max() {
        let mut cfg = default_v2_cfg();
        // atr_bps ≈ 98 but require ≤ 50
        cfg.min_atr_bps = 10.0;
        cfg.max_atr_bps = 50.0;
        let strat = ScreenedVwapScalpV2::new(cfg);
        let result = strat.evaluate(&default_ctx(), &long_input()).unwrap();
        assert!(result.is_none());
    }

    // ── Volume ratio filter ───────────────────────────────────────────────────

    #[test]
    fn v2_rejects_volume_ratio_below_min() {
        let mut cfg = default_v2_cfg();
        cfg.min_volume_ratio = 5.0; // volume=2000, sma=1000 → ratio=2.0 < 5.0
        let strat = ScreenedVwapScalpV2::new(cfg);
        let result = strat.evaluate(&default_ctx(), &long_input()).unwrap();
        assert!(result.is_none());
    }

    // ── VWAP/EMA21 distance filter ────────────────────────────────────────────

    #[test]
    fn v2_rejects_too_far_from_vwap_or_ema21() {
        // min nearest distance must be > 5.0 atr — entry has dist 0.1 atr, so fails min
        let mut cfg = default_v2_cfg();
        cfg.vwap_distance_atr_min = 5.0;
        cfg.vwap_distance_atr_max = 10.0;
        let strat = ScreenedVwapScalpV2::new(cfg);
        let result = strat.evaluate(&default_ctx(), &long_input()).unwrap();
        assert!(result.is_none());
    }

    // ── Valid signal emission ─────────────────────────────────────────────────

    #[test]
    fn v2_emits_long_signal_with_valid_geometry() {
        let sig = strategy().evaluate(&default_ctx(), &long_input()).unwrap();
        assert!(sig.is_some());
        let sig = sig.unwrap();
        assert_eq!(sig.side, Side::Long);
        assert!(sig.stop_loss < sig.entry_price, "long: sl < entry");
        assert!(sig.entry_price < sig.take_profit, "long: entry < tp");
        assert!(sig.valid_geometry());
    }

    #[test]
    fn v2_emits_short_signal_with_valid_geometry() {
        let sig = strategy().evaluate(&default_ctx(), &short_input()).unwrap();
        assert!(sig.is_some());
        let sig = sig.unwrap();
        assert_eq!(sig.side, Side::Short);
        assert!(sig.entry_price < sig.stop_loss, "short: entry < sl");
        assert!(sig.take_profit < sig.entry_price, "short: tp < entry");
        assert!(sig.valid_geometry());
    }

    // ── ATR multipliers ───────────────────────────────────────────────────────

    #[test]
    fn v2_uses_configurable_tp_atr_multiple() {
        let mut cfg = default_v2_cfg();
        cfg.tp_atr_multiple = 3.0;
        cfg.sl_atr_multiple = 1.0;
        let strat = ScreenedVwapScalpV2::new(cfg);
        let sig = strat
            .evaluate(&default_ctx(), &long_input())
            .unwrap()
            .unwrap();
        // close=102, atr=1.0 → tp = 102 + 3.0 = 105.0
        assert!(
            (sig.take_profit - 105.0).abs() < 1e-9,
            "tp={}",
            sig.take_profit
        );
    }

    #[test]
    fn v2_uses_configurable_sl_atr_multiple() {
        let mut cfg = default_v2_cfg();
        cfg.tp_atr_multiple = 2.0;
        cfg.sl_atr_multiple = 0.5;
        let strat = ScreenedVwapScalpV2::new(cfg);
        let sig = strat
            .evaluate(&default_ctx(), &long_input())
            .unwrap()
            .unwrap();
        // close=102, atr=1.0 → sl = 102 - 0.5 = 101.5
        assert!((sig.stop_loss - 101.5).abs() < 1e-9, "sl={}", sig.stop_loss);
    }

    // ── Filters populated ─────────────────────────────────────────────────────

    #[test]
    fn v2_filters_passed_are_populated() {
        let sig = strategy()
            .evaluate(&default_ctx(), &long_input())
            .unwrap()
            .unwrap();
        assert!(
            sig.filters_passed
                .contains(&"screening_bullish".to_string())
        );
        assert!(
            sig.filters_passed
                .contains(&"confirmation_bullish".to_string())
        );
        assert!(sig.filters_passed.contains(&"ema_ribbon_long".to_string()));
        assert!(sig.filters_passed.contains(&"atr_bps_in_range".to_string()));
        assert!(
            sig.filters_passed
                .contains(&"near_vwap_or_ema21".to_string())
        );
        assert!(sig.filters_passed.contains(&"volume_ratio_ok".to_string()));
        assert!(
            sig.filters_passed
                .contains(&"expected_reward_ok".to_string())
        );
        assert!(
            sig.filters_passed
                .contains(&"expected_net_edge_ok".to_string())
        );
        assert!(
            sig.filters_passed
                .contains(&"direction_enabled".to_string())
        );
        assert!(sig.filters_passed.contains(&"confidence_ok".to_string()));
    }

    // ── Required timeframes ───────────────────────────────────────────────────

    #[test]
    fn v2_signal_has_required_timeframes() {
        let sig = strategy()
            .evaluate(&default_ctx(), &long_input())
            .unwrap()
            .unwrap();
        assert_eq!(sig.entry_timeframe, Timeframe::OneMinute);
        assert_eq!(sig.confirmation_timeframe, Timeframe::FiveMinute);
        assert_eq!(sig.screening_timeframe, Timeframe::FifteenMinute);
    }
}
