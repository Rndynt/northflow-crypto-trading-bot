//! EmaTrendPullbackV1 — multi-timeframe EMA trend pullback strategy.
//!
//! Timeframe roles (explicit, never inferred from order):
//!   entry        = 1m  — entry and execution signal timeframe
//!   confirmation = 5m  — intermediate confirmation layer
//!   screening    = 15m — market regime / bias filter
//!
//! Concept:
//!   Long:  15m bullish → 5m bullish → 1m price pulls back near EMA/VWAP →
//!          1m bullish reclaim/rejection → ATR + reward large enough to cover cost.
//!   Short: mirror of long.
//!
//! This strategy trades less frequently than VWAP scalp strategies, requiring
//! stricter multi-timeframe alignment and a confirmed pullback trigger.
//!
//! This is a research-only strategy variant. Not a profitability claim.
//! No orders, no exchange calls, no LLMs, no auto-tuning.

use crate::config::EtpConfig;
use crate::core::{NorthflowError, Side, Signal, SignalId, StrategyId};
use crate::strategy::traits::{MultiTimeframeInput, Strategy, StrategyContext};

// ── EmaTrendPullbackV1 ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct EmaTrendPullbackV1 {
    pub cfg: EtpConfig,
}

impl EmaTrendPullbackV1 {
    pub fn new(cfg: EtpConfig) -> Self {
        Self { cfg }
    }
}

impl Strategy for EmaTrendPullbackV1 {
    fn strategy_id(&self) -> &'static str {
        "ema_trend_pullback_v1"
    }

    fn evaluate(
        &self,
        ctx: &StrategyContext,
        input: &MultiTimeframeInput,
    ) -> Result<Option<Signal>, NorthflowError> {
        // ── 0. Defensive candle validation ────────────────────────────────────
        input.entry_candle.validate()?;
        input.confirmation_candle.validate()?;
        input.screening_candle.validate()?;

        // ── 1. Required 1m entry indicators ───────────────────────────────────
        let ema_8 = match input.entry_indicators.ema_8 {
            Some(v) => v,
            None => return Ok(None),
        };
        let ema_21 = match input.entry_indicators.ema_21 {
            Some(v) => v,
            None => return Ok(None),
        };
        let ema_50 = match input.entry_indicators.ema_50 {
            Some(v) => v,
            None => return Ok(None),
        };
        // ema_200 is required (ensures indicator warmup); not directly used in filters.
        let _ema_200 = match input.entry_indicators.ema_200 {
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

        // ── 2. Required 5m confirmation indicators ────────────────────────────
        let ema_21_5m = match input.confirmation_indicators.ema_21 {
            Some(v) => v,
            None => return Ok(None),
        };
        let ema_50_5m = match input.confirmation_indicators.ema_50 {
            Some(v) => v,
            None => return Ok(None),
        };
        let ema_200_5m = match input.confirmation_indicators.ema_200 {
            Some(v) => v,
            None => return Ok(None),
        };

        // ── 3. Required 15m screening indicators ──────────────────────────────
        let ema_50_15m = match input.screening_indicators.ema_50 {
            Some(v) => v,
            None => return Ok(None),
        };
        let ema_200_15m = match input.screening_indicators.ema_200 {
            Some(v) => v,
            None => return Ok(None),
        };

        let entry_close = input.entry_candle.close;
        let confirmation_close = input.confirmation_candle.close;
        let screening_close = input.screening_candle.close;

        // ── 4. 15m trend filter ───────────────────────────────────────────────
        let is_bullish_15m = ema_50_15m > ema_200_15m && screening_close > ema_50_15m;
        let is_bearish_15m = ema_50_15m < ema_200_15m && screening_close < ema_50_15m;

        if !is_bullish_15m && !is_bearish_15m {
            return Ok(None);
        }

        let side = if is_bullish_15m {
            Side::Long
        } else {
            Side::Short
        };

        // ── 5. Direction enable/disable ───────────────────────────────────────
        match side {
            Side::Long if !self.cfg.allow_long => return Ok(None),
            Side::Short if !self.cfg.allow_short => return Ok(None),
            _ => {}
        }

        // ── 6. 5m confirmation filter ─────────────────────────────────────────
        let confirmation_ok = match side {
            Side::Long => {
                ema_21_5m > ema_50_5m && ema_50_5m > ema_200_5m && confirmation_close > ema_21_5m
            }
            Side::Short => {
                ema_21_5m < ema_50_5m && ema_50_5m < ema_200_5m && confirmation_close < ema_21_5m
            }
        };
        if !confirmation_ok {
            return Ok(None);
        }

        // ── 7. 1m EMA alignment ───────────────────────────────────────────────
        let ema_aligned = match side {
            Side::Long => ema_8 > ema_21 && ema_21 > ema_50 && entry_close >= ema_21,
            Side::Short => ema_8 < ema_21 && ema_21 < ema_50 && entry_close <= ema_21,
        };
        if self.cfg.require_entry_ema_alignment && !ema_aligned {
            return Ok(None);
        }

        // ── 8. Pullback distance filter ───────────────────────────────────────
        if atr <= 0.0 {
            return Ok(None);
        }

        let anchors: Vec<(&str, f64)> = match self.cfg.pullback_to.as_str() {
            "ema21" => vec![("ema21", ema_21)],
            "ema50" => vec![("ema50", ema_50)],
            "vwap" => vec![("vwap", vwap)],
            "ema21_or_vwap" => vec![("ema21", ema_21), ("vwap", vwap)],
            "ema21_or_ema50_or_vwap" => {
                vec![("ema21", ema_21), ("ema50", ema_50), ("vwap", vwap)]
            }
            other => {
                return Err(NorthflowError::ConfigError(format!(
                    "unknown etp_pullback_to: '{other}'"
                )));
            }
        };

        let mut nearest_name = "";
        let mut nearest_anchor = 0.0_f64;
        let mut nearest_distance = f64::MAX;
        for (name, anchor) in &anchors {
            let dist = (entry_close - anchor).abs();
            if dist < nearest_distance {
                nearest_distance = dist;
                nearest_anchor = *anchor;
                nearest_name = *name;
            }
        }

        let distance_atr = nearest_distance / atr;
        if distance_atr < self.cfg.min_pullback_distance_atr
            || distance_atr > self.cfg.max_pullback_distance_atr
        {
            return Ok(None);
        }

        // ── 9. Reclaim / rejection trigger ────────────────────────────────────
        let candle = input.entry_candle;
        let range = candle.high - candle.low;

        let close_reclaim_pass = match side {
            Side::Long => {
                candle.low <= nearest_anchor
                    && entry_close > nearest_anchor
                    && entry_close > candle.open
            }
            Side::Short => {
                candle.high >= nearest_anchor
                    && entry_close < nearest_anchor
                    && entry_close < candle.open
            }
        };

        let wick_rejection_pass = if range <= 0.0 {
            false
        } else {
            let body = (entry_close - candle.open).abs();
            let body_ratio = body / range;
            let upper_wick = candle.high - candle.open.max(entry_close);
            let lower_wick = candle.open.min(entry_close) - candle.low;
            let lower_wick_ratio = lower_wick / range;
            let upper_wick_ratio = upper_wick / range;

            match side {
                Side::Long => {
                    lower_wick_ratio >= self.cfg.min_wick_rejection_ratio
                        && body_ratio >= self.cfg.min_body_ratio
                        && entry_close > candle.open
                }
                Side::Short => {
                    upper_wick_ratio >= self.cfg.min_wick_rejection_ratio
                        && body_ratio >= self.cfg.min_body_ratio
                        && entry_close < candle.open
                }
            }
        };

        let trigger_pass = match self.cfg.reclaim_mode.as_str() {
            "close_reclaim" => close_reclaim_pass,
            "wick_rejection" => {
                if range <= 0.0 {
                    return Ok(None);
                }
                wick_rejection_pass
            }
            "close_reclaim_or_wick" => close_reclaim_pass || wick_rejection_pass,
            other => {
                return Err(NorthflowError::ConfigError(format!(
                    "unknown etp_reclaim_mode: '{other}'"
                )));
            }
        };

        if !trigger_pass {
            return Ok(None);
        }

        // ── 10. ATR bps filter ────────────────────────────────────────────────
        if entry_close <= 0.0 {
            return Ok(None);
        }
        let atr_bps = atr / entry_close * 10_000.0;
        if atr_bps < self.cfg.min_atr_bps || atr_bps > self.cfg.max_atr_bps {
            return Ok(None);
        }

        // ── 11. Volume ratio filter ───────────────────────────────────────────
        if volume_sma_20 <= 0.0 {
            return Ok(None);
        }
        let volume_ratio = candle.volume / volume_sma_20;
        if volume_ratio < self.cfg.min_volume_ratio {
            return Ok(None);
        }

        // ── 12. Signal geometry ───────────────────────────────────────────────
        let entry = entry_close;
        let (stop_loss, take_profit) = match side {
            Side::Long => (
                entry - atr * self.cfg.sl_atr_multiple,
                entry + atr * self.cfg.tp_atr_multiple,
            ),
            Side::Short => (
                entry + atr * self.cfg.sl_atr_multiple,
                entry - atr * self.cfg.tp_atr_multiple,
            ),
        };

        // ── 13. Reward / risk check ───────────────────────────────────────────
        let risk = (entry - stop_loss).abs();
        if risk <= 0.0 {
            return Ok(None);
        }
        let rr = (take_profit - entry).abs() / risk;
        if rr < self.cfg.min_reward_risk {
            return Ok(None);
        }

        // ── 14. Expected reward and edge ──────────────────────────────────────
        let expected_reward_bps = match side {
            Side::Long => (take_profit - entry) / entry * 10_000.0,
            Side::Short => (entry - take_profit) / entry * 10_000.0,
        };
        let expected_net_edge_bps = expected_reward_bps - ctx.estimated_cost_bps;

        if expected_reward_bps < self.cfg.min_expected_reward_bps {
            return Ok(None);
        }
        if expected_net_edge_bps < self.cfg.min_expected_net_edge_bps {
            return Ok(None);
        }

        // ── 15. Confidence scoring ────────────────────────────────────────────
        let mut confidence: i32 = 50;
        confidence += 10; // 15m trend passed
        confidence += 10; // 5m confirmation passed
        if ema_aligned {
            confidence += 10; // 1m EMA alignment passed
        }
        confidence += 10; // reclaim/rejection trigger passed
        confidence += 10; // expected net edge passed
        confidence += 5; // volume ratio ok
        confidence += 5; // ATR bps in range
        let confidence = confidence.clamp(0, 100) as u8;

        if confidence < ctx.min_confidence {
            return Ok(None);
        }

        // ── 16. Build filters_passed ──────────────────────────────────────────
        let mut filters_passed: Vec<String> = Vec::new();
        match side {
            Side::Long => filters_passed.push("15m_trend_bullish".to_string()),
            Side::Short => filters_passed.push("15m_trend_bearish".to_string()),
        }
        match side {
            Side::Long => filters_passed.push("5m_confirmation_bullish".to_string()),
            Side::Short => filters_passed.push("5m_confirmation_bearish".to_string()),
        }
        if ema_aligned {
            match side {
                Side::Long => filters_passed.push("1m_ema_alignment_long".to_string()),
                Side::Short => filters_passed.push("1m_ema_alignment_short".to_string()),
            }
        }
        match nearest_name {
            "ema21" => filters_passed.push("pullback_near_ema21".to_string()),
            "ema50" => filters_passed.push("pullback_near_ema50".to_string()),
            "vwap" => filters_passed.push("pullback_near_vwap".to_string()),
            _ => {}
        }
        if close_reclaim_pass {
            match side {
                Side::Long => filters_passed.push("close_reclaim_long".to_string()),
                Side::Short => filters_passed.push("close_reclaim_short".to_string()),
            }
        } else if wick_rejection_pass {
            match side {
                Side::Long => filters_passed.push("wick_rejection_long".to_string()),
                Side::Short => filters_passed.push("wick_rejection_short".to_string()),
            }
        }
        filters_passed.push("atr_bps_in_range".to_string());
        filters_passed.push("volume_ratio_ok".to_string());
        filters_passed.push("reward_risk_ok".to_string());
        filters_passed.push("expected_reward_ok".to_string());
        filters_passed.push("expected_net_edge_ok".to_string());
        filters_passed.push("direction_enabled".to_string());
        filters_passed.push("confidence_ok".to_string());

        // Regime and entry reason
        let regime = if is_bullish_15m { "bullish" } else { "bearish" }.to_string();
        let entry_reason = format!(
            "etp_{}_pullback_near_{}_{}",
            regime, nearest_name, self.cfg.reclaim_mode
        );

        // Deterministic signal ID
        let signal_id = SignalId::new(format!("SIG-BT-{:08X}", ctx.signal_index));

        Ok(Some(Signal {
            signal_id,
            symbol: ctx.symbol.clone(),
            strategy_id: StrategyId::new("ema_trend_pullback_v1"),
            side,
            entry_timeframe: ctx.entry_timeframe,
            screening_timeframe: ctx.screening_timeframe,
            confirmation_timeframe: ctx.confirmation_timeframe,
            entry_time: candle.timestamp,
            entry_price: entry,
            stop_loss,
            take_profit,
            confidence,
            regime,
            entry_reason,
            filters_passed,
            filters_failed: vec![],
            expected_reward_bps,
            estimated_cost_bps: ctx.estimated_cost_bps,
            expected_net_edge_bps,
        }))
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
        use crate::core::Timeframe;
    use crate::core::{Candle, Symbol};
    use crate::indicators::IndicatorSnapshot;
    use crate::strategy::traits::MultiTimeframeInput;

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn good_cfg() -> EtpConfig {
        EtpConfig::default()
    }

    fn ctx() -> StrategyContext {
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

    /// Build a candle at a given close with natural OHLC for a long setup.
    /// open < close (bullish), low dips below ema21 for close_reclaim.
    fn long_candle(open: f64, high: f64, low: f64, close: f64, vol: f64) -> Candle {
        Candle {
            timestamp: 1_700_000_000_000,
            open,
            high,
            low,
            close,
            volume: vol,
        }
    }

    fn short_candle(open: f64, high: f64, low: f64, close: f64, vol: f64) -> Candle {
        Candle {
            timestamp: 1_700_000_000_000,
            open,
            high,
            low,
            close,
            volume: vol,
        }
    }

    /// Snapshot with all indicators set for a bullish 1m setup near ema21.
    fn bullish_1m_snap(ema21: f64, atr: f64, vwap: f64) -> IndicatorSnapshot {
        IndicatorSnapshot {
            ema_8: Some(ema21 + 5.0),
            ema_21: Some(ema21),
            ema_50: Some(ema21 - 20.0),
            ema_200: Some(ema21 - 100.0),
            atr_14: Some(atr),
            vwap: Some(vwap),
            volume_sma_20: Some(100.0),
        }
    }

    fn bearish_1m_snap(ema21: f64, atr: f64, vwap: f64) -> IndicatorSnapshot {
        IndicatorSnapshot {
            ema_8: Some(ema21 - 5.0),
            ema_21: Some(ema21),
            ema_50: Some(ema21 + 20.0),
            ema_200: Some(ema21 + 100.0),
            atr_14: Some(atr),
            vwap: Some(vwap),
            volume_sma_20: Some(100.0),
        }
    }

    /// 5m snapshot bullish (ema21 > ema50 > ema200, close > ema21).
    fn bullish_5m_snap(close: f64) -> IndicatorSnapshot {
        IndicatorSnapshot {
            ema_21: Some(close - 10.0),
            ema_50: Some(close - 30.0),
            ema_200: Some(close - 80.0),
            ..Default::default()
        }
    }

    fn bearish_5m_snap(close: f64) -> IndicatorSnapshot {
        IndicatorSnapshot {
            ema_21: Some(close + 10.0),
            ema_50: Some(close + 30.0),
            ema_200: Some(close + 80.0),
            ..Default::default()
        }
    }

    /// 15m snapshot bullish (ema50 > ema200, close > ema50).
    fn bullish_15m_snap(close: f64) -> IndicatorSnapshot {
        IndicatorSnapshot {
            ema_50: Some(close - 50.0),
            ema_200: Some(close - 200.0),
            ..Default::default()
        }
    }

    fn bearish_15m_snap(close: f64) -> IndicatorSnapshot {
        IndicatorSnapshot {
            ema_50: Some(close + 50.0),
            ema_200: Some(close + 200.0),
            ..Default::default()
        }
    }

    /// Build a complete long MultiTimeframeInput for a close_reclaim trigger near ema21.
    ///
    /// close = 50_000, ema21 = 49_990 → pullback dips to 49_985 (below ema21) then closes above.
    fn good_long_input() -> MultiTimeframeInput {
        let ema21 = 49_990.0_f64;
        let close = 50_005.0_f64;
        let atr = 100.0_f64; // atr_bps = 100/50005 * 10000 ≈ 20 bps
        let vwap = 49_950.0_f64;

        MultiTimeframeInput {
            // low dips below ema21, close reclaims above ema21
            entry_candle: long_candle(49_985.0, 50_020.0, 49_975.0, close, 200.0),
            confirmation_candle: long_candle(49_900.0, 50_100.0, 49_850.0, 50_050.0, 150.0),
            screening_candle: long_candle(49_500.0, 50_200.0, 49_400.0, 50_100.0, 500.0),
            entry_indicators: bullish_1m_snap(ema21, atr, vwap),
            confirmation_indicators: bullish_5m_snap(50_050.0),
            screening_indicators: bullish_15m_snap(50_100.0),
        }
    }

    /// Build a complete short MultiTimeframeInput for a close_reclaim trigger near ema21.
    ///
    /// close = 49_995, ema21 = 50_010 → spike above ema21 then closes below.
    fn good_short_input() -> MultiTimeframeInput {
        let ema21 = 50_010.0_f64;
        let close = 49_995.0_f64;
        let atr = 100.0_f64;
        let vwap = 50_050.0_f64;

        MultiTimeframeInput {
            entry_candle: short_candle(50_020.0, 50_025.0, 49_980.0, close, 200.0),
            confirmation_candle: short_candle(50_100.0, 50_150.0, 49_900.0, 49_950.0, 150.0),
            screening_candle: short_candle(50_500.0, 50_600.0, 49_800.0, 49_900.0, 500.0),
            entry_indicators: bearish_1m_snap(ema21, atr, vwap),
            confirmation_indicators: bearish_5m_snap(49_950.0),
            screening_indicators: bearish_15m_snap(49_900.0),
        }
    }

    fn strategy() -> EmaTrendPullbackV1 {
        EmaTrendPullbackV1::new(good_cfg())
    }

    // ── Missing indicator tests ───────────────────────────────────────────────

    #[test]
    fn etp_returns_none_when_indicators_missing() {
        let strat = strategy();
        let mut input = good_long_input();
        input.entry_indicators.ema_8 = None;
        assert!(strat.evaluate(&ctx(), &input).unwrap().is_none());

        let mut input = good_long_input();
        input.entry_indicators.atr_14 = None;
        assert!(strat.evaluate(&ctx(), &input).unwrap().is_none());

        let mut input = good_long_input();
        input.entry_indicators.ema_200 = None;
        assert!(strat.evaluate(&ctx(), &input).unwrap().is_none());

        let mut input = good_long_input();
        input.confirmation_indicators.ema_21 = None;
        assert!(strat.evaluate(&ctx(), &input).unwrap().is_none());

        let mut input = good_long_input();
        input.screening_indicators.ema_50 = None;
        assert!(strat.evaluate(&ctx(), &input).unwrap().is_none());
    }

    // ── 15m trend tests ───────────────────────────────────────────────────────

    #[test]
    fn etp_returns_none_when_15m_trend_neutral() {
        let strat = strategy();
        let mut input = good_long_input();
        // ema50 > ema200 but close < ema50 → neutral
        input.screening_indicators = IndicatorSnapshot {
            ema_50: Some(50_200.0),
            ema_200: Some(49_000.0),
            ..Default::default()
        };
        input.screening_candle = long_candle(50_000.0, 50_100.0, 49_900.0, 50_100.0, 500.0);
        // close=50100 < ema50=50200 → neutral
        assert!(strat.evaluate(&ctx(), &input).unwrap().is_none());
    }

    #[test]
    fn etp_long_requires_bullish_15m_trend() {
        let strat = strategy();
        let mut input = good_long_input();
        // Flip to bearish 15m
        input.screening_indicators = bearish_15m_snap(49_900.0);
        input.screening_candle = long_candle(49_900.0, 50_000.0, 49_800.0, 49_900.0, 500.0);
        assert!(strat.evaluate(&ctx(), &input).unwrap().is_none());
    }

    #[test]
    fn etp_short_requires_bearish_15m_trend() {
        let strat = strategy();
        let mut input = good_short_input();
        // Flip to bullish 15m
        input.screening_indicators = bullish_15m_snap(50_100.0);
        input.screening_candle = long_candle(50_000.0, 50_200.0, 49_900.0, 50_100.0, 500.0);
        assert!(strat.evaluate(&ctx(), &input).unwrap().is_none());
    }

    // ── 5m confirmation tests ─────────────────────────────────────────────────

    #[test]
    fn etp_long_requires_5m_bullish_confirmation() {
        let strat = strategy();
        let mut input = good_long_input();
        // Flip 5m to bearish
        input.confirmation_indicators = bearish_5m_snap(49_950.0);
        input.confirmation_candle = long_candle(50_000.0, 50_100.0, 49_900.0, 49_950.0, 150.0);
        assert!(strat.evaluate(&ctx(), &input).unwrap().is_none());
    }

    #[test]
    fn etp_short_requires_5m_bearish_confirmation() {
        let strat = strategy();
        let mut input = good_short_input();
        // Flip 5m to bullish
        input.confirmation_indicators = bullish_5m_snap(50_050.0);
        input.confirmation_candle = short_candle(50_000.0, 50_200.0, 49_900.0, 50_050.0, 150.0);
        assert!(strat.evaluate(&ctx(), &input).unwrap().is_none());
    }

    // ── 1m EMA alignment tests ────────────────────────────────────────────────

    #[test]
    fn etp_long_requires_1m_ema_alignment() {
        let mut cfg = good_cfg();
        cfg.require_entry_ema_alignment = true;
        let strat = EmaTrendPullbackV1::new(cfg);
        let mut input = good_long_input();
        // Break alignment: ema_8 < ema_21
        input.entry_indicators.ema_8 = Some(49_980.0); // below ema_21=49_990
        assert!(strat.evaluate(&ctx(), &input).unwrap().is_none());
    }

    #[test]
    fn etp_short_requires_1m_ema_alignment() {
        let mut cfg = good_cfg();
        cfg.require_entry_ema_alignment = true;
        let strat = EmaTrendPullbackV1::new(cfg);
        let mut input = good_short_input();
        // Break alignment for short: ema_8 should be < ema_21; set it above
        input.entry_indicators.ema_8 = Some(50_020.0); // above ema_21=50_010
        assert!(strat.evaluate(&ctx(), &input).unwrap().is_none());
    }

    // ── Pullback distance tests ───────────────────────────────────────────────

    #[test]
    fn etp_rejects_pullback_too_far() {
        let mut cfg = good_cfg();
        cfg.pullback_to = "ema21".to_string();
        cfg.max_pullback_distance_atr = 0.5;
        let strat = EmaTrendPullbackV1::new(cfg);
        let mut input = good_long_input();
        // close=50005, ema21=49990, dist=15, atr=100 → dist_atr=0.15 → OK actually
        // Need to push it far: set ema21 very far from close
        input.entry_indicators.ema_21 = Some(49_000.0); // dist = 1005, atr=100 → 10.05 ATR
        // But also need to fix EMA alignment (ema_8 should be > new ema_21)
        input.entry_indicators.ema_8 = Some(49_100.0); // ema_8 > ema_21=49000
        // ema_50 needs to be < ema_21
        input.entry_indicators.ema_50 = Some(48_000.0);
        assert!(strat.evaluate(&ctx(), &input).unwrap().is_none());
    }

    #[test]
    fn etp_accepts_pullback_near_ema21() {
        let mut cfg = good_cfg();
        cfg.pullback_to = "ema21".to_string();
        cfg.require_entry_ema_alignment = false;
        let strat = EmaTrendPullbackV1::new(cfg);
        let input = good_long_input();
        // distance = |50005 - 49990| = 15, atr=100 → 0.15 ATR → within [0, 1.25]
        assert!(strat.evaluate(&ctx(), &input).unwrap().is_some());
    }

    #[test]
    fn etp_accepts_pullback_near_vwap() {
        let mut cfg = good_cfg();
        cfg.pullback_to = "vwap".to_string();
        cfg.require_entry_ema_alignment = false;
        let strat = EmaTrendPullbackV1::new(cfg);
        let mut input = good_long_input();
        // vwap = 49_950, close = 50_005 → dist = 55, atr=100 → 0.55 ATR → within range
        // But need close_reclaim near vwap: low <= vwap, close > vwap
        input.entry_candle = long_candle(49_940.0, 50_020.0, 49_940.0, 50_005.0, 200.0);
        assert!(strat.evaluate(&ctx(), &input).unwrap().is_some());
    }

    // ── Close reclaim trigger tests ───────────────────────────────────────────

    #[test]
    fn etp_long_close_reclaim_trigger() {
        let cfg = good_cfg(); // reclaim_mode = "close_reclaim"
        let strat = EmaTrendPullbackV1::new(cfg);
        let mut input = good_long_input();
        input.cfg_reclaim_mode_is_close_reclaim_or_wick_noop(); // no-op, just documenting
        // Valid long close_reclaim: low <= ema21, close > ema21, close > open
        assert!(strat.evaluate(&ctx(), &input).unwrap().is_some());
    }

    #[test]
    fn etp_short_close_reclaim_trigger() {
        let cfg = good_cfg();
        let strat = EmaTrendPullbackV1::new(cfg);
        let input = good_short_input();
        // Valid short: high >= ema21, close < ema21, close < open
        assert!(strat.evaluate(&ctx(), &input).unwrap().is_some());
    }

    // ── Wick rejection trigger tests ──────────────────────────────────────────

    #[test]
    fn etp_long_wick_rejection_trigger() {
        let mut cfg = good_cfg();
        cfg.reclaim_mode = "wick_rejection".to_string();
        cfg.min_wick_rejection_ratio = 0.20;
        cfg.min_body_ratio = 0.30;
        cfg.require_entry_ema_alignment = false;
        let strat = EmaTrendPullbackV1::new(cfg);

        let mut input = good_long_input();
        // Build a candle with large lower wick and good body:
        // open=49980, high=50020, low=49940, close=50010
        // range=80, body=|50010-49980|=30, body_ratio=0.375≥0.30 ✓
        // lower_wick=min(49980,50010)-49940=49980-49940=40, ratio=40/80=0.50≥0.20 ✓
        // close > open ✓
        input.entry_candle = long_candle(49_980.0, 50_020.0, 49_940.0, 50_010.0, 200.0);
        assert!(strat.evaluate(&ctx(), &input).unwrap().is_some());
    }

    #[test]
    fn etp_short_wick_rejection_trigger() {
        let mut cfg = good_cfg();
        cfg.reclaim_mode = "wick_rejection".to_string();
        cfg.min_wick_rejection_ratio = 0.20;
        cfg.min_body_ratio = 0.30;
        cfg.require_entry_ema_alignment = false;
        let strat = EmaTrendPullbackV1::new(cfg);

        let mut input = good_short_input();
        // Build short wick candle near ema21=50010:
        // open=50020, high=50060, low=49990, close=49998
        // range=70, body=|49998-50020|=22, body_ratio=22/70≈0.31≥0.30 ✓
        // upper_wick=50060-max(50020,49998)=50060-50020=40, ratio=40/70≈0.57≥0.20 ✓
        // close < open ✓
        input.entry_candle = short_candle(50_020.0, 50_060.0, 49_990.0, 49_998.0, 200.0);
        assert!(strat.evaluate(&ctx(), &input).unwrap().is_some());
    }

    // ── ATR bps filter tests ──────────────────────────────────────────────────

    #[test]
    fn etp_rejects_atr_bps_below_min() {
        let mut cfg = good_cfg();
        cfg.min_atr_bps = 50.0;
        let strat = EmaTrendPullbackV1::new(cfg);
        let mut input = good_long_input();
        // atr=100, close=50005 → atr_bps≈20 bps < 50
        input.entry_indicators.atr_14 = Some(10.0); // atr_bps≈2 < 50
        assert!(strat.evaluate(&ctx(), &input).unwrap().is_none());
    }

    #[test]
    fn etp_rejects_atr_bps_above_max() {
        let mut cfg = good_cfg();
        cfg.max_atr_bps = 10.0;
        let strat = EmaTrendPullbackV1::new(cfg);
        let input = good_long_input();
        // atr=100, close=50005 → atr_bps≈20 bps > 10
        assert!(strat.evaluate(&ctx(), &input).unwrap().is_none());
    }

    // ── Volume ratio filter tests ─────────────────────────────────────────────

    #[test]
    fn etp_rejects_volume_ratio_below_min() {
        let mut cfg = good_cfg();
        cfg.min_volume_ratio = 3.0;
        let strat = EmaTrendPullbackV1::new(cfg);
        let input = good_long_input();
        // candle volume=200, volume_sma=100 → ratio=2.0 < 3.0
        assert!(strat.evaluate(&ctx(), &input).unwrap().is_none());
    }

    // ── Expected reward and edge filter tests ─────────────────────────────────

    #[test]
    fn etp_rejects_expected_reward_below_min() {
        let mut cfg = good_cfg();
        cfg.min_expected_reward_bps = 10_000.0; // absurdly high
        let strat = EmaTrendPullbackV1::new(cfg);
        let input = good_long_input();
        assert!(strat.evaluate(&ctx(), &input).unwrap().is_none());
    }

    #[test]
    fn etp_rejects_expected_net_edge_below_min() {
        let mut cfg = good_cfg();
        cfg.min_expected_net_edge_bps = 10_000.0; // absurdly high
        let strat = EmaTrendPullbackV1::new(cfg);
        let input = good_long_input();
        assert!(strat.evaluate(&ctx(), &input).unwrap().is_none());
    }

    // ── TP/SL multiple config tests ───────────────────────────────────────────

    #[test]
    fn etp_uses_configurable_tp_atr_multiple() {
        let mut cfg = good_cfg();
        cfg.require_entry_ema_alignment = false;
        cfg.tp_atr_multiple = 5.0;
        let strat = EmaTrendPullbackV1::new(cfg);
        let input = good_long_input();
        let sig = strat.evaluate(&ctx(), &input).unwrap().unwrap();
        // entry≈50005, atr=100 → tp = 50005 + 500 = 50505
        let expected_tp = sig.entry_price + 500.0;
        assert!(
            (sig.take_profit - expected_tp).abs() < 1e-6,
            "tp={} expected={}",
            sig.take_profit,
            expected_tp
        );
    }

    #[test]
    fn etp_uses_configurable_sl_atr_multiple() {
        let mut cfg = good_cfg();
        cfg.require_entry_ema_alignment = false;
        cfg.sl_atr_multiple = 2.0;
        // tp=3.0, sl=2.0 → rr=1.5; lower min_reward_risk so the signal passes
        cfg.min_reward_risk = 1.0;
        let strat = EmaTrendPullbackV1::new(cfg);
        let input = good_long_input();
        let sig = strat.evaluate(&ctx(), &input).unwrap().unwrap();
        // entry≈50005, atr=100 → sl = 50005 - 200 = 49805
        let expected_sl = sig.entry_price - 200.0;
        assert!(
            (sig.stop_loss - expected_sl).abs() < 1e-6,
            "sl={} expected={}",
            sig.stop_loss,
            expected_sl
        );
    }

    // ── Full signal emission tests ─────────────────────────────────────────────

    #[test]
    fn etp_emits_long_signal_with_valid_geometry() {
        let mut cfg = good_cfg();
        cfg.require_entry_ema_alignment = false;
        let strat = EmaTrendPullbackV1::new(cfg);
        let input = good_long_input();
        let sig = strat.evaluate(&ctx(), &input).unwrap().unwrap();
        assert_eq!(sig.side, Side::Long);
        assert!(sig.valid_geometry(), "long geometry must be valid");
        assert!(sig.stop_loss < sig.entry_price);
        assert!(sig.take_profit > sig.entry_price);
    }

    #[test]
    fn etp_emits_short_signal_with_valid_geometry() {
        let mut cfg = good_cfg();
        cfg.require_entry_ema_alignment = false;
        let strat = EmaTrendPullbackV1::new(cfg);
        let input = good_short_input();
        let sig = strat.evaluate(&ctx(), &input).unwrap().unwrap();
        assert_eq!(sig.side, Side::Short);
        assert!(sig.valid_geometry(), "short geometry must be valid");
        assert!(sig.stop_loss > sig.entry_price);
        assert!(sig.take_profit < sig.entry_price);
    }

    #[test]
    fn etp_strategy_id_is_correct() {
        let strat = strategy();
        assert_eq!(strat.strategy_id(), "ema_trend_pullback_v1");
    }

    #[test]
    fn etp_filters_passed_are_populated() {
        let mut cfg = good_cfg();
        cfg.require_entry_ema_alignment = false;
        let strat = EmaTrendPullbackV1::new(cfg);
        let input = good_long_input();
        let sig = strat.evaluate(&ctx(), &input).unwrap().unwrap();
        assert!(
            !sig.filters_passed.is_empty(),
            "filters_passed must be populated"
        );
        assert!(
            sig.filters_passed
                .contains(&"15m_trend_bullish".to_string()),
            "must contain 15m_trend_bullish"
        );
        assert!(
            sig.filters_passed
                .contains(&"5m_confirmation_bullish".to_string()),
            "must contain 5m_confirmation_bullish"
        );
        assert!(
            sig.filters_failed.is_empty(),
            "filters_failed must be empty on emitted signal"
        );
    }
}

// Dummy trait impl for tests — allows documenting the reclaim_mode in test names.
#[cfg(test)]
trait EtpTestHelper {
    fn cfg_reclaim_mode_is_close_reclaim_or_wick_noop(&self) {}
}

#[cfg(test)]
impl EtpTestHelper for MultiTimeframeInput {}
