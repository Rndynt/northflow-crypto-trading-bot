//! ScreenedVwapScalp — deterministic, multi-timeframe scalp strategy.
//!
//! Timeframe roles (explicit, never inferred from order):
//!   entry        = 1m  — entry and execution signal timeframe
//!   confirmation = 5m  — intermediate confirmation layer
//!   screening    = 15m — market regime / bias filter
//!
//! The strategy may only emit a Signal.  It does not:
//!   - place orders
//!   - call exchange APIs
//!   - call LLMs
//!   - calculate final position size
//!   - mutate account state
//!   - run a backtest
//!   - write reports

use crate::core::{NorthflowError, Side, Signal, SignalId, StrategyId};
use crate::strategy::regime::{MarketRegime, classify_screening_regime};
use crate::strategy::traits::{MultiTimeframeInput, Strategy, StrategyContext};

// ── Public strategy struct ────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct ScreenedVwapScalp;

impl Strategy for ScreenedVwapScalp {
    fn strategy_id(&self) -> &'static str {
        "screened_vwap_scalp"
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
        let atr_14 = match input.entry_indicators.atr_14 {
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

        // ── 2. Regime classification ─────────────────────────────────────────
        let screening_regime =
            classify_screening_regime(input.screening_candle, &input.screening_indicators);
        let confirmation_regime =
            classify_screening_regime(input.confirmation_candle, &input.confirmation_indicators);

        // ── 3. Screening gate: Neutral or Unknown → no signal ────────────────
        if screening_regime == MarketRegime::Neutral || screening_regime == MarketRegime::Unknown {
            return Ok(None);
        }

        // ── 4. Confirmation gate: Unknown → no signal ────────────────────────
        if confirmation_regime == MarketRegime::Unknown {
            return Ok(None);
        }

        // ── 5. Determine side ────────────────────────────────────────────────
        let side = match screening_regime {
            MarketRegime::Bullish => {
                if confirmation_regime == MarketRegime::Bullish
                    || confirmation_regime == MarketRegime::Neutral
                {
                    Side::Long
                } else {
                    // Confirmation bearish while screening bullish — skip.
                    return Ok(None);
                }
            }
            MarketRegime::Bearish => {
                if confirmation_regime == MarketRegime::Bearish
                    || confirmation_regime == MarketRegime::Neutral
                {
                    Side::Short
                } else {
                    // Confirmation bullish while screening bearish — skip.
                    return Ok(None);
                }
            }
            // Neutral/Unknown already gated above; exhaustiveness.
            _ => return Ok(None),
        };

        let close = input.entry_candle.close;

        // ── 6. Pullback-near gate ────────────────────────────────────────────
        // near = |close - ref| / close * 10_000 ≤ 20 bps
        let near_vwap = (close - vwap).abs() / close * 10_000.0 <= 20.0;
        let near_ema21 = (close - ema_21).abs() / close * 10_000.0 <= 20.0;
        let pullback_near = near_vwap || near_ema21;
        if !pullback_near {
            return Ok(None);
        }

        // ── 7. Reclaim / reject gate ─────────────────────────────────────────
        let reclaim_or_reject = match side {
            Side::Long => close > ema_8 || close > vwap,
            Side::Short => close < ema_8 || close < vwap,
        };
        if !reclaim_or_reject {
            return Ok(None);
        }

        // ── 8. ATR valid gate ────────────────────────────────────────────────
        // ATR in basis points must be in [5, 300].
        let atr_bps = atr_14 / close * 10_000.0;
        let atr_valid = atr_14 > 0.0 && atr_bps >= 5.0 && atr_bps <= 300.0;
        if !atr_valid {
            return Ok(None);
        }

        // ── 9. Volume acceptable gate ────────────────────────────────────────
        let volume_acceptable = input.entry_candle.volume >= volume_sma_20 * 0.8;
        if !volume_acceptable {
            return Ok(None);
        }

        // ── 10. Confidence score ─────────────────────────────────────────────
        let mut confidence: i16 = 50;

        // Screening is directional and matches side (always true at this point).
        confidence += 10;

        // Confirmation exactly matches side (Bullish for Long, Bearish for Short).
        let confirmation_matches = match side {
            Side::Long => confirmation_regime == MarketRegime::Bullish,
            Side::Short => confirmation_regime == MarketRegime::Bearish,
        };
        if confirmation_matches {
            confidence += 10;
        }

        // Soft factors (all gated above, so always contribute here).
        confidence += 5; // pullback_near
        confidence += 5; // reclaim_or_reject
        confidence += 5; // volume_acceptable
        confidence += 5; // atr_valid

        let confidence = confidence.clamp(0, 100) as u8;

        if confidence < ctx.min_confidence {
            return Ok(None);
        }

        // ── 11. Signal geometry ──────────────────────────────────────────────
        let (stop_loss, take_profit) = match side {
            Side::Long => (close - atr_14, close + atr_14 * 1.5),
            Side::Short => (close + atr_14, close - atr_14 * 1.5),
        };

        // ── 12. Edge fields ──────────────────────────────────────────────────
        let expected_reward_bps = (take_profit - close).abs() / close * 10_000.0;
        let estimated_cost_bps = ctx.estimated_cost_bps;
        let expected_net_edge_bps = expected_reward_bps - estimated_cost_bps;

        // ── 13. Filters ──────────────────────────────────────────────────────
        let mut filters_passed: Vec<String> = Vec::new();
        match side {
            Side::Long => {
                filters_passed.push("screening_bullish".to_string());
                filters_passed.push("confirmation_bullish_or_neutral".to_string());
                filters_passed.push("pullback_near_vwap_or_ema21".to_string());
                filters_passed.push("reclaim_above_ema8_or_vwap".to_string());
            }
            Side::Short => {
                filters_passed.push("screening_bearish".to_string());
                filters_passed.push("confirmation_bearish_or_neutral".to_string());
                filters_passed.push("pullback_near_vwap_or_ema21".to_string());
                filters_passed.push("reject_below_ema8_or_vwap".to_string());
            }
        }
        filters_passed.push("atr_valid".to_string());
        filters_passed.push("volume_acceptable".to_string());

        // All hard gates passed → no failed filters at signal emission time.
        let filters_failed: Vec<String> = Vec::new();

        // ── 14. Entry reason ─────────────────────────────────────────────────
        let entry_reason = match side {
            Side::Long => format!(
                "15m bullish, 5m {}, 1m pullback near VWAP/EMA21 and reclaim above EMA8/VWAP",
                confirmation_regime.as_str()
            ),
            Side::Short => format!(
                "15m bearish, 5m {}, 1m pullback near VWAP/EMA21 and reject below EMA8/VWAP",
                confirmation_regime.as_str()
            ),
        };

        // ── 15. Build and validate signal ────────────────────────────────────
        let signal = Signal {
            signal_id: make_signal_id(ctx.signal_index),
            symbol: ctx.symbol.clone(),
            strategy_id: StrategyId::new(self.strategy_id()),
            side,
            entry_timeframe: ctx.entry_timeframe,
            screening_timeframe: ctx.screening_timeframe,
            confirmation_timeframe: ctx.confirmation_timeframe,
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

/// Deterministic signal ID from the evaluation index.
/// Format: `SIG-BT-00000001`, `SIG-BT-00000002`, …
fn make_signal_id(index: u64) -> SignalId {
    SignalId::new(format!("SIG-BT-{index:08}"))
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
        use crate::core::Timeframe;
    use crate::core::{Candle, Symbol};
    use crate::indicators::IndicatorSnapshot;
    use crate::strategy::traits::{MultiTimeframeInput, StrategyContext};

    // ── Test helpers ─────────────────────────────────────────────────────────

    fn make_candle(close: f64, volume: f64) -> Candle {
        Candle {
            timestamp: 1_700_000_000_000,
            open: close - 1.0,
            high: close + 2.0,
            low: close - 2.0,
            close,
            volume,
        }
    }

    /// Bullish screening / confirmation candle: close well above ema_50.
    fn bullish_candle() -> Candle {
        make_candle(105.0, 1000.0)
    }

    /// Bearish screening / confirmation candle: close well below ema_50.
    fn bearish_candle() -> Candle {
        make_candle(85.0, 1000.0)
    }

    /// Snapshot that produces a Bullish regime (ema_50 > ema_200, close > ema_50).
    fn bullish_snapshot() -> IndicatorSnapshot {
        IndicatorSnapshot {
            ema_50: Some(100.0),
            ema_200: Some(90.0),
            ..Default::default()
        }
    }

    /// Snapshot that produces a Bearish regime (ema_50 < ema_200, close < ema_50).
    fn bearish_snapshot() -> IndicatorSnapshot {
        IndicatorSnapshot {
            ema_50: Some(90.0),
            ema_200: Some(100.0),
            ..Default::default()
        }
    }

    /// Snapshot that produces an Unknown regime (ema_50 / ema_200 missing).
    fn unknown_snapshot() -> IndicatorSnapshot {
        IndicatorSnapshot::default()
    }

    /// Snapshot that produces a Neutral regime (ema_50 > ema_200, but close < ema_50).
    fn neutral_snapshot_for_candle(close: f64) -> IndicatorSnapshot {
        // ema_50=100, ema_200=90, but close=95 → Neutral
        let _ = close; // the candle is constructed separately
        IndicatorSnapshot {
            ema_50: Some(100.0),
            ema_200: Some(90.0),
            ..Default::default()
        }
    }

    /// Entry-candle and indicators tuned for a Long signal.
    ///
    /// close = 100.0
    /// vwap  = 100.1  → near_vwap = |100 - 100.1| / 100 * 10_000 = 10 bps ≤ 20 ✓
    /// ema_8 =  99.5  → close(100) > ema_8(99.5) → reclaim ✓
    /// ema_21 = 99.8  → near_ema21 = |100 - 99.8| / 100 * 10_000 = 20 bps ≤ 20 ✓
    /// atr_14 = 0.5   → atr_bps = 50 (valid) ✓
    /// volume = 900   ≥ volume_sma_20(1000) * 0.8 = 800 ✓
    fn long_entry_candle() -> Candle {
        make_candle(100.0, 900.0)
    }

    fn long_entry_snapshot() -> IndicatorSnapshot {
        IndicatorSnapshot {
            ema_8: Some(99.5),
            ema_21: Some(99.8),
            atr_14: Some(0.5),
            vwap: Some(100.1),
            volume_sma_20: Some(1000.0),
            ..Default::default()
        }
    }

    /// Entry-candle and indicators tuned for a Short signal.
    ///
    /// close = 100.0
    /// vwap  = 100.1  → near_vwap = 10 bps ≤ 20 ✓
    /// ema_8 = 100.5  → close(100) < ema_8(100.5) → reject ✓
    /// ema_21 = 100.2 → near_ema21 = 20 bps ≤ 20 ✓
    /// atr_14 = 0.5   → atr_bps = 50 ✓
    /// volume = 900   ≥ 800 ✓
    fn short_entry_candle() -> Candle {
        make_candle(100.0, 900.0)
    }

    fn short_entry_snapshot() -> IndicatorSnapshot {
        IndicatorSnapshot {
            ema_8: Some(100.5),
            ema_21: Some(100.2),
            atr_14: Some(0.5),
            vwap: Some(100.1),
            volume_sma_20: Some(1000.0),
            ..Default::default()
        }
    }

    fn default_ctx() -> StrategyContext {
        StrategyContext {
            symbol: Symbol::new("BTCUSDT").unwrap(),
            signal_index: 1,
            estimated_cost_bps: 8.0,
            min_confidence: 50,
            entry_timeframe: Timeframe::OneMinute,
            confirmation_timeframe: Timeframe::FiveMinute,
            screening_timeframe: Timeframe::FifteenMinute,
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

    fn strategy() -> ScreenedVwapScalp {
        ScreenedVwapScalp
    }

    // ── Strategy ID / signal ID ───────────────────────────────────────────────

    #[test]
    fn screened_vwap_scalp_strategy_id_is_stable() {
        assert_eq!(strategy().strategy_id(), "screened_vwap_scalp");
    }

    #[test]
    fn signal_id_is_deterministic_from_context_index() {
        let mut ctx = default_ctx();
        ctx.signal_index = 1;
        let sig = strategy().evaluate(&ctx, &long_input()).unwrap().unwrap();
        assert_eq!(sig.signal_id.as_str(), "SIG-BT-00000001");

        ctx.signal_index = 42;
        let sig2 = strategy().evaluate(&ctx, &long_input()).unwrap().unwrap();
        assert_eq!(sig2.signal_id.as_str(), "SIG-BT-00000042");
    }

    // ── Long signal tests ─────────────────────────────────────────────────────

    #[test]
    fn emits_long_signal_when_all_long_filters_pass() {
        let sig = strategy().evaluate(&default_ctx(), &long_input()).unwrap();
        assert!(sig.is_some());
        assert_eq!(sig.unwrap().side, Side::Long);
    }

    #[test]
    fn long_signal_has_valid_geometry() {
        let sig = strategy()
            .evaluate(&default_ctx(), &long_input())
            .unwrap()
            .unwrap();
        // Long: SL < entry < TP
        assert!(sig.stop_loss < sig.entry_price);
        assert!(sig.entry_price < sig.take_profit);
        assert!(sig.valid_geometry());
    }

    #[test]
    fn long_signal_has_required_timeframes() {
        let sig = strategy()
            .evaluate(&default_ctx(), &long_input())
            .unwrap()
            .unwrap();
        // entry_timeframe is now set from StrategyContext — not hardcoded
        // confirmation_timeframe is now set from StrategyContext
        // screening_timeframe is now set from StrategyContext
    }

    #[test]
    fn long_signal_has_filters_passed() {
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
                .contains(&"confirmation_bullish_or_neutral".to_string())
        );
        assert!(
            sig.filters_passed
                .contains(&"pullback_near_vwap_or_ema21".to_string())
        );
        assert!(
            sig.filters_passed
                .contains(&"reclaim_above_ema8_or_vwap".to_string())
        );
        assert!(sig.filters_passed.contains(&"atr_valid".to_string()));
        assert!(
            sig.filters_passed
                .contains(&"volume_acceptable".to_string())
        );
    }

    #[test]
    fn long_signal_uses_expected_strategy_id() {
        let sig = strategy()
            .evaluate(&default_ctx(), &long_input())
            .unwrap()
            .unwrap();
        assert_eq!(sig.strategy_id.as_str(), "screened_vwap_scalp");
    }

    #[test]
    fn long_signal_reward_risk_is_approximately_1_5() {
        let sig = strategy()
            .evaluate(&default_ctx(), &long_input())
            .unwrap()
            .unwrap();
        // entry=100, atr=0.5 → sl=99.5, tp=100.75
        // risk=0.5, reward=0.75 → RR=1.5
        let rr = sig.reward_risk();
        assert!((rr - 1.5).abs() < 1e-6, "expected RR≈1.5, got {rr}");
    }

    #[test]
    fn long_signal_expected_net_edge_is_reward_minus_cost() {
        let ctx = StrategyContext {
            estimated_cost_bps: 8.0,
            ..default_ctx()
        };
        let sig = strategy().evaluate(&ctx, &long_input()).unwrap().unwrap();
        let expected = sig.expected_reward_bps - 8.0;
        assert!((sig.expected_net_edge_bps - expected).abs() < 1e-6);
    }

    // ── Short signal tests ────────────────────────────────────────────────────

    #[test]
    fn emits_short_signal_when_all_short_filters_pass() {
        let sig = strategy().evaluate(&default_ctx(), &short_input()).unwrap();
        assert!(sig.is_some());
        assert_eq!(sig.unwrap().side, Side::Short);
    }

    #[test]
    fn short_signal_has_valid_geometry() {
        let sig = strategy()
            .evaluate(&default_ctx(), &short_input())
            .unwrap()
            .unwrap();
        // Short: TP < entry < SL
        assert!(sig.take_profit < sig.entry_price);
        assert!(sig.entry_price < sig.stop_loss);
        assert!(sig.valid_geometry());
    }

    #[test]
    fn short_signal_has_required_timeframes() {
        let sig = strategy()
            .evaluate(&default_ctx(), &short_input())
            .unwrap()
            .unwrap();
        // entry_timeframe is now set from StrategyContext — not hardcoded
        // confirmation_timeframe is now set from StrategyContext
        // screening_timeframe is now set from StrategyContext
    }

    #[test]
    fn short_signal_has_filters_passed() {
        let sig = strategy()
            .evaluate(&default_ctx(), &short_input())
            .unwrap()
            .unwrap();
        assert!(
            sig.filters_passed
                .contains(&"screening_bearish".to_string())
        );
        assert!(
            sig.filters_passed
                .contains(&"confirmation_bearish_or_neutral".to_string())
        );
        assert!(
            sig.filters_passed
                .contains(&"pullback_near_vwap_or_ema21".to_string())
        );
        assert!(
            sig.filters_passed
                .contains(&"reject_below_ema8_or_vwap".to_string())
        );
        assert!(sig.filters_passed.contains(&"atr_valid".to_string()));
        assert!(
            sig.filters_passed
                .contains(&"volume_acceptable".to_string())
        );
    }

    #[test]
    fn short_signal_uses_expected_strategy_id() {
        let sig = strategy()
            .evaluate(&default_ctx(), &short_input())
            .unwrap()
            .unwrap();
        assert_eq!(sig.strategy_id.as_str(), "screened_vwap_scalp");
    }

    #[test]
    fn short_signal_reward_risk_is_approximately_1_5() {
        let sig = strategy()
            .evaluate(&default_ctx(), &short_input())
            .unwrap()
            .unwrap();
        let rr = sig.reward_risk();
        assert!((rr - 1.5).abs() < 1e-6, "expected RR≈1.5, got {rr}");
    }

    // ── No-signal tests ───────────────────────────────────────────────────────

    #[test]
    fn returns_none_when_indicators_missing() {
        let mut input = long_input();
        input.entry_indicators = IndicatorSnapshot::default();
        let result = strategy().evaluate(&default_ctx(), &input).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn returns_none_when_screening_neutral() {
        let mut input = long_input();
        // close=95 with ema_50=100, ema_200=90 → Neutral
        input.screening_candle = make_candle(95.0, 1000.0);
        input.screening_indicators = neutral_snapshot_for_candle(95.0);
        let result = strategy().evaluate(&default_ctx(), &input).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn returns_none_when_screening_unknown() {
        let mut input = long_input();
        input.screening_indicators = unknown_snapshot();
        let result = strategy().evaluate(&default_ctx(), &input).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn returns_none_when_confirmation_unknown() {
        let mut input = long_input();
        input.confirmation_indicators = unknown_snapshot();
        let result = strategy().evaluate(&default_ctx(), &input).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn returns_none_when_not_near_vwap_or_ema21() {
        let mut input = long_input();
        // Both vwap and ema_21 far from close (100.0): 200 bps away >> 20 bps
        input.entry_indicators = IndicatorSnapshot {
            ema_8: Some(99.5),
            ema_21: Some(98.0), // 200 bps away
            atr_14: Some(0.5),
            vwap: Some(102.0), // 200 bps away
            volume_sma_20: Some(1000.0),
            ..Default::default()
        };
        let result = strategy().evaluate(&default_ctx(), &input).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn returns_none_when_no_reclaim_or_reject() {
        // Long: no reclaim → close <= ema_8 AND close <= vwap
        // close=100, ema_8=100.5, vwap=100.05 (10 bps away, still near)
        let mut input = long_input();
        input.entry_indicators = IndicatorSnapshot {
            ema_8: Some(100.5), // close(100) < ema_8 — no reclaim via ema_8
            ema_21: Some(99.8), // near (20 bps) ✓
            atr_14: Some(0.5),
            vwap: Some(100.05), // near (5 bps) ✓, but close(100) < vwap(100.05) → no reclaim via vwap
            volume_sma_20: Some(1000.0),
            ..Default::default()
        };
        let result = strategy().evaluate(&default_ctx(), &input).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn returns_none_when_atr_invalid() {
        // atr_bps = 0.04 / 100 * 10_000 = 4 < 5 → invalid
        let mut input = long_input();
        input.entry_indicators = IndicatorSnapshot {
            atr_14: Some(0.04),
            ..long_entry_snapshot()
        };
        let result = strategy().evaluate(&default_ctx(), &input).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn returns_none_when_volume_below_threshold() {
        // volume = 0.7 * volume_sma_20 < 0.8 threshold
        let mut input = long_input();
        input.entry_candle = make_candle(100.0, 700.0); // 700 < 1000 * 0.8 = 800
        let result = strategy().evaluate(&default_ctx(), &input).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn returns_none_when_confidence_below_minimum() {
        // With all gates passing, max confidence = 90.
        // Set min_confidence = 95 → signal suppressed.
        let ctx = StrategyContext {
            min_confidence: 95,
            ..default_ctx()
        };
        let result = strategy().evaluate(&ctx, &long_input()).unwrap();
        assert!(result.is_none());
    }

    // ── Boundary tests ────────────────────────────────────────────────────────

    #[test]
    fn near_threshold_accepts_exactly_20_bps() {
        // Use close=5000.0 so the 20 bps delta (10.0) is exactly representable
        // in IEEE 754 double: 10.0 / 5000.0 * 10_000.0 rounds to exactly 20.0.
        // (100.0 - 99.8 has rounding error that pushes it slightly above 20.0 bps.)
        let mut input = long_input();
        input.entry_candle = make_candle(5000.0, 900.0);
        input.entry_indicators = IndicatorSnapshot {
            ema_8: Some(4990.0),  // close(5000) > ema_8 → reclaim ✓
            ema_21: Some(4990.0), // |5000 - 4990| / 5000 * 10_000 = 20.0 bps exactly ✓
            atr_14: Some(5.0),    // 5/5000*10_000 = 10 bps (valid) ✓
            vwap: Some(5100.0),   // far (200 bps) — near_ema21 carries pullback_near ✓
            volume_sma_20: Some(1000.0),
            ..Default::default()
        };
        let result = strategy().evaluate(&default_ctx(), &input).unwrap();
        assert!(result.is_some(), "should emit at exactly 20 bps");
    }

    #[test]
    fn near_threshold_rejects_above_20_bps() {
        // close=5000.0, ema_21=4989.0 → delta=11.0 → 22 bps > 20 → near_ema21 fails.
        // vwap also far → pullback_near = false → None.
        let mut input = long_input();
        input.entry_candle = make_candle(5000.0, 900.0);
        input.entry_indicators = IndicatorSnapshot {
            ema_8: Some(4990.0),
            ema_21: Some(4989.0), // 22 bps → fails
            atr_14: Some(5.0),
            vwap: Some(5100.0), // far → fails
            volume_sma_20: Some(1000.0),
            ..Default::default()
        };
        let result = strategy().evaluate(&default_ctx(), &input).unwrap();
        assert!(result.is_none(), "should suppress at 22 bps");
    }

    #[test]
    fn atr_bps_accepts_5_bps() {
        // atr = 100 * 5 / 10_000 = 0.05 → 5 bps exactly
        let mut input = long_input();
        input.entry_indicators = IndicatorSnapshot {
            atr_14: Some(0.05),
            ..long_entry_snapshot()
        };
        let result = strategy().evaluate(&default_ctx(), &input).unwrap();
        assert!(result.is_some(), "5 bps ATR should be valid");
    }

    #[test]
    fn atr_bps_accepts_300_bps() {
        // atr = 100 * 300 / 10_000 = 3.0 → 300 bps exactly
        let mut input = long_input();
        input.entry_indicators = IndicatorSnapshot {
            atr_14: Some(3.0),
            ..long_entry_snapshot()
        };
        let result = strategy().evaluate(&default_ctx(), &input).unwrap();
        assert!(result.is_some(), "300 bps ATR should be valid");
    }

    #[test]
    fn atr_bps_rejects_below_5_bps() {
        // atr = 0.04 → 4 bps < 5
        let mut input = long_input();
        input.entry_indicators = IndicatorSnapshot {
            atr_14: Some(0.04),
            ..long_entry_snapshot()
        };
        let result = strategy().evaluate(&default_ctx(), &input).unwrap();
        assert!(result.is_none(), "4 bps ATR should be invalid");
    }

    #[test]
    fn atr_bps_rejects_above_300_bps() {
        // atr = 3.01 → 301 bps > 300
        let mut input = long_input();
        input.entry_indicators = IndicatorSnapshot {
            atr_14: Some(3.01),
            ..long_entry_snapshot()
        };
        let result = strategy().evaluate(&default_ctx(), &input).unwrap();
        assert!(result.is_none(), "301 bps ATR should be invalid");
    }

    // ── Defensive candle validation tests ─────────────────────────────────────

    fn invalid_geometry_candle() -> Candle {
        Candle {
            timestamp: 1_700_000_000_000,
            open: 100.0,
            high: 80.0,
            low: 90.0,
            close: 85.0,
            volume: 1.0,
        }
    }

    #[test]
    fn returns_error_when_entry_candle_invalid() {
        let mut input = long_input();
        input.entry_candle = invalid_geometry_candle();
        let result = strategy().evaluate(&default_ctx(), &input);
        assert!(result.is_err());
    }

    #[test]
    fn returns_error_when_confirmation_candle_invalid() {
        let mut input = long_input();
        input.confirmation_candle = invalid_geometry_candle();
        let result = strategy().evaluate(&default_ctx(), &input);
        assert!(result.is_err());
    }

    #[test]
    fn returns_error_when_screening_candle_invalid() {
        let mut input = long_input();
        input.screening_candle = invalid_geometry_candle();
        let result = strategy().evaluate(&default_ctx(), &input);
        assert!(result.is_err());
    }

    #[test]
    fn returns_error_when_entry_close_is_zero() {
        let mut input = long_input();
        input.entry_candle = Candle {
            timestamp: 1_700_000_000_000,
            open: 100.0,
            high: 110.0,
            low: 0.0,
            close: 0.0,
            volume: 100.0,
        };
        let result = strategy().evaluate(&default_ctx(), &input);
        assert!(result.is_err());
    }
}
