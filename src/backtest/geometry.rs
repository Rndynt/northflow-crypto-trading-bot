//! Entry geometry mode — controls how SL/TP are adjusted relative to the actual entry fill price.
//!
//! After a signal is generated at signal-candle close, the actual fill happens at the
//! next 1m candle open with adverse slippage.  Two modes determine what happens to SL/TP:
//!
//! `PreserveSignalLevels` — current strict model.  SL/TP remain at their original absolute
//!   levels as computed at signal close.  Effective reward/risk can degrade when actual entry
//!   moves adversely.  This is the default.
//!
//! `ReanchorToActualEntry` — after the actual fill price is known, SL/TP are re-anchored
//!   around it using the original risk distance and original reward/risk ratio.  Simulates
//!   a bracket order placed after the fill price is confirmed.

use crate::core::{NorthflowError, Side, Signal};

// ── EntryGeometryMode ─────────────────────────────────────────────────────────

/// How SL/TP levels are handled relative to the actual adverse entry fill price.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryGeometryMode {
    /// Keep original absolute SL/TP from signal close.  Effective reward/risk
    /// degrades when actual entry moves adversely.  This is the strict default.
    PreserveSignalLevels,
    /// Re-anchor SL/TP around actual entry using original risk distance and
    /// original reward/risk ratio.  Simulates a bracket order placed after fill.
    ReanchorToActualEntry,
}

impl EntryGeometryMode {
    /// Return the canonical config string for this mode.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PreserveSignalLevels => "preserve_signal_levels",
            Self::ReanchorToActualEntry => "reanchor_to_actual_entry",
        }
    }

    /// Parse from a config string.
    ///
    /// Accepted values: `"preserve_signal_levels"`, `"reanchor_to_actual_entry"`.
    /// Returns `NorthflowError::ConfigError` for any other value — never silently defaults.
    pub fn parse(s: &str) -> Result<Self, NorthflowError> {
        match s {
            "preserve_signal_levels" => Ok(Self::PreserveSignalLevels),
            "reanchor_to_actual_entry" => Ok(Self::ReanchorToActualEntry),
            other => Err(NorthflowError::ConfigError(format!(
                "unknown entry_geometry_mode '{}': expected 'preserve_signal_levels' or \
                 'reanchor_to_actual_entry'",
                other
            ))),
        }
    }
}

// ── adjusted_signal_for_actual_entry ─────────────────────────────────────────

/// Adjust a signal to reflect the actual adverse fill price at the next candle open.
///
/// Always updates `entry_price`, `expected_reward_bps`, and `expected_net_edge_bps`.
/// Never modifies `signal_id`, `symbol`, `strategy_id`, `side`, `timeframes`,
/// `entry_time`, `confidence`, `regime`, `entry_reason`, `filters_*`, or
/// `estimated_cost_bps`.
///
/// ### `PreserveSignalLevels`
/// - `stop_loss` and `take_profit` remain at their original absolute levels.
/// - `entry_price` ← `actual_entry_price`.
/// - `expected_reward_bps` recalculated from new entry vs original TP.
///
/// ### `ReanchorToActualEntry`
/// - Original risk distance and reward/risk ratio derived from signal geometry.
/// - `stop_loss` ← `actual_entry ∓ original_risk` (long: minus, short: plus).
/// - `take_profit` ← `actual_entry ± original_risk × original_rr`.
/// - `expected_reward_bps` recalculated from new entry vs new TP.
pub fn adjusted_signal_for_actual_entry(
    signal: &Signal,
    actual_entry_price: f64,
    mode: EntryGeometryMode,
) -> Signal {
    match mode {
        EntryGeometryMode::PreserveSignalLevels => preserve_signal_levels(signal, actual_entry_price),
        EntryGeometryMode::ReanchorToActualEntry => reanchor_to_actual_entry(signal, actual_entry_price),
    }
}

// ── mode implementations ──────────────────────────────────────────────────────

fn preserve_signal_levels(signal: &Signal, actual_entry_price: f64) -> Signal {
    let expected_reward_bps = reward_bps(signal.side, actual_entry_price, signal.take_profit);
    let expected_net_edge_bps = expected_reward_bps - signal.estimated_cost_bps;
    Signal {
        entry_price: actual_entry_price,
        expected_reward_bps,
        expected_net_edge_bps,
        ..signal.clone()
    }
}

fn reanchor_to_actual_entry(signal: &Signal, actual_entry_price: f64) -> Signal {
    let (original_risk, original_rr) = match signal.side {
        Side::Long => {
            let risk = signal.entry_price - signal.stop_loss;
            let rr = if risk > 0.0 {
                (signal.take_profit - signal.entry_price) / risk
            } else {
                0.0
            };
            (risk, rr)
        }
        Side::Short => {
            let risk = signal.stop_loss - signal.entry_price;
            let rr = if risk > 0.0 {
                (signal.entry_price - signal.take_profit) / risk
            } else {
                0.0
            };
            (risk, rr)
        }
    };

    let (new_stop_loss, new_take_profit) = match signal.side {
        Side::Long => (
            actual_entry_price - original_risk,
            actual_entry_price + original_risk * original_rr,
        ),
        Side::Short => (
            actual_entry_price + original_risk,
            actual_entry_price - original_risk * original_rr,
        ),
    };

    let expected_reward_bps = reward_bps(signal.side, actual_entry_price, new_take_profit);
    let expected_net_edge_bps = expected_reward_bps - signal.estimated_cost_bps;

    Signal {
        entry_price: actual_entry_price,
        stop_loss: new_stop_loss,
        take_profit: new_take_profit,
        expected_reward_bps,
        expected_net_edge_bps,
        ..signal.clone()
    }
}

// ── reward_bps helper ─────────────────────────────────────────────────────────

fn reward_bps(side: Side, entry: f64, take_profit: f64) -> f64 {
    if entry <= 0.0 {
        return 0.0;
    }
    match side {
        Side::Long => (take_profit - entry) / entry * 10_000.0,
        Side::Short => (entry - take_profit) / entry * 10_000.0,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{SignalId, StrategyId, Symbol, Timeframe};

    fn long_signal() -> Signal {
        Signal {
            signal_id: SignalId::new("SIG-BT-00000001"),
            symbol: Symbol::new("BTCUSDT").unwrap(),
            strategy_id: StrategyId::new("screened_vwap_scalp"),
            side: Side::Long,
            entry_timeframe: Timeframe::OneMinute,
            screening_timeframe: Timeframe::FifteenMinute,
            confirmation_timeframe: Timeframe::FiveMinute,
            entry_time: 1_700_000_000_000,
            entry_price: 30_000.0,
            stop_loss: 29_700.0,
            take_profit: 30_600.0,
            confidence: 75,
            regime: "bullish".to_string(),
            entry_reason: "ema_cross".to_string(),
            filters_passed: vec![],
            filters_failed: vec![],
            expected_reward_bps: 200.0,
            estimated_cost_bps: 8.0,
            expected_net_edge_bps: 192.0,
        }
    }

    fn short_signal() -> Signal {
        Signal {
            signal_id: SignalId::new("SIG-BT-00000002"),
            symbol: Symbol::new("BTCUSDT").unwrap(),
            strategy_id: StrategyId::new("screened_vwap_scalp"),
            side: Side::Short,
            entry_timeframe: Timeframe::OneMinute,
            screening_timeframe: Timeframe::FifteenMinute,
            confirmation_timeframe: Timeframe::FiveMinute,
            entry_time: 1_700_000_000_000,
            entry_price: 30_000.0,
            stop_loss: 30_300.0,
            take_profit: 29_400.0,
            confidence: 75,
            regime: "bearish".to_string(),
            entry_reason: "ema_cross_down".to_string(),
            filters_passed: vec![],
            filters_failed: vec![],
            expected_reward_bps: 200.0,
            estimated_cost_bps: 8.0,
            expected_net_edge_bps: 192.0,
        }
    }

    // ── EntryGeometryMode::parse ──────────────────────────────────────────────

    #[test]
    fn parse_preserve_signal_levels() {
        let mode = EntryGeometryMode::parse("preserve_signal_levels").unwrap();
        assert_eq!(mode, EntryGeometryMode::PreserveSignalLevels);
    }

    #[test]
    fn parse_reanchor_to_actual_entry() {
        let mode = EntryGeometryMode::parse("reanchor_to_actual_entry").unwrap();
        assert_eq!(mode, EntryGeometryMode::ReanchorToActualEntry);
    }

    #[test]
    fn parse_unknown_value_returns_config_error() {
        let err = EntryGeometryMode::parse("unknown_mode").unwrap_err();
        assert!(
            matches!(err, NorthflowError::ConfigError(_)),
            "expected ConfigError, got: {err:?}"
        );
        let msg = err.to_string();
        assert!(msg.contains("unknown_mode"), "error should name the bad value: {msg}");
    }

    #[test]
    fn parse_empty_string_returns_config_error() {
        assert!(EntryGeometryMode::parse("").is_err());
    }

    #[test]
    fn as_str_round_trips() {
        for mode in [
            EntryGeometryMode::PreserveSignalLevels,
            EntryGeometryMode::ReanchorToActualEntry,
        ] {
            let s = mode.as_str();
            let parsed = EntryGeometryMode::parse(s).unwrap();
            assert_eq!(parsed, mode, "round-trip failed for: {s}");
        }
    }

    // ── PreserveSignalLevels — long ───────────────────────────────────────────

    #[test]
    fn preserve_long_updates_entry_price_and_reward() {
        let signal = long_signal();
        let actual = 30_006.0;
        let adjusted = adjusted_signal_for_actual_entry(&signal, actual, EntryGeometryMode::PreserveSignalLevels);

        assert_eq!(adjusted.entry_price, actual);
        assert_eq!(adjusted.stop_loss, signal.stop_loss, "SL must not change");
        assert_eq!(adjusted.take_profit, signal.take_profit, "TP must not change");
        let expected_reward = (30_600.0 - actual) / actual * 10_000.0;
        assert!(
            (adjusted.expected_reward_bps - expected_reward).abs() < 1e-6,
            "reward_bps mismatch"
        );
        let expected_net = expected_reward - signal.estimated_cost_bps;
        assert!(
            (adjusted.expected_net_edge_bps - expected_net).abs() < 1e-6,
            "net_edge_bps mismatch"
        );
    }

    #[test]
    fn preserve_long_entry_at_tp_makes_invalid_geometry() {
        let signal = long_signal();
        let adjusted = adjusted_signal_for_actual_entry(&signal, 30_600.0, EntryGeometryMode::PreserveSignalLevels);
        assert!(!adjusted.valid_geometry(), "entry == TP must be invalid for long");
    }

    #[test]
    fn preserve_long_identity_fields_unchanged() {
        let signal = long_signal();
        let adjusted = adjusted_signal_for_actual_entry(&signal, 30_006.0, EntryGeometryMode::PreserveSignalLevels);
        assert_eq!(adjusted.signal_id, signal.signal_id);
        assert_eq!(adjusted.side, signal.side);
        assert_eq!(adjusted.regime, signal.regime);
        assert_eq!(adjusted.estimated_cost_bps, signal.estimated_cost_bps);
    }

    // ── PreserveSignalLevels — short ──────────────────────────────────────────

    #[test]
    fn preserve_short_updates_entry_price_and_reward() {
        let signal = short_signal();
        let actual = 29_994.0;
        let adjusted = adjusted_signal_for_actual_entry(&signal, actual, EntryGeometryMode::PreserveSignalLevels);

        assert_eq!(adjusted.entry_price, actual);
        assert_eq!(adjusted.stop_loss, signal.stop_loss, "SL must not change");
        assert_eq!(adjusted.take_profit, signal.take_profit, "TP must not change");
        let expected_reward = (actual - 29_400.0) / actual * 10_000.0;
        assert!(
            (adjusted.expected_reward_bps - expected_reward).abs() < 1e-6,
            "short reward_bps mismatch"
        );
    }

    // ── ReanchorToActualEntry — long ──────────────────────────────────────────

    #[test]
    fn reanchor_long_sl_tp_shift_by_risk_distance() {
        let signal = long_signal();
        let actual = 30_006.0;
        let adjusted = adjusted_signal_for_actual_entry(&signal, actual, EntryGeometryMode::ReanchorToActualEntry);

        assert_eq!(adjusted.entry_price, actual);
        let expected_sl = actual - 300.0;
        assert!(
            (adjusted.stop_loss - expected_sl).abs() < 1e-6,
            "SL mismatch: {} != {}",
            adjusted.stop_loss,
            expected_sl
        );
        let expected_tp = actual + 300.0 * 2.0;
        assert!(
            (adjusted.take_profit - expected_tp).abs() < 1e-6,
            "TP mismatch: {} != {}",
            adjusted.take_profit,
            expected_tp
        );
    }

    #[test]
    fn reanchor_long_geometry_valid_after_normal_slippage() {
        let signal = long_signal();
        let actual = 30_006.0;
        let adjusted = adjusted_signal_for_actual_entry(&signal, actual, EntryGeometryMode::ReanchorToActualEntry);
        assert!(adjusted.valid_geometry(), "reanchored long must have valid geometry");
    }

    #[test]
    fn reanchor_long_reward_bps_uses_new_tp() {
        let signal = long_signal();
        let actual = 30_006.0;
        let adjusted = adjusted_signal_for_actual_entry(&signal, actual, EntryGeometryMode::ReanchorToActualEntry);
        let new_tp = actual + 600.0;
        let expected_reward = (new_tp - actual) / actual * 10_000.0;
        assert!(
            (adjusted.expected_reward_bps - expected_reward).abs() < 1e-6,
            "reward must be computed from new TP"
        );
    }

    #[test]
    fn reanchor_long_preserves_identity_fields() {
        let signal = long_signal();
        let adjusted = adjusted_signal_for_actual_entry(&signal, 30_006.0, EntryGeometryMode::ReanchorToActualEntry);
        assert_eq!(adjusted.signal_id, signal.signal_id);
        assert_eq!(adjusted.side, signal.side);
        assert_eq!(adjusted.estimated_cost_bps, signal.estimated_cost_bps);
        assert_eq!(adjusted.confidence, signal.confidence);
        assert_eq!(adjusted.regime, signal.regime);
        assert_eq!(adjusted.entry_reason, signal.entry_reason);
    }

    // ── ReanchorToActualEntry — short ─────────────────────────────────────────

    #[test]
    fn reanchor_short_sl_tp_shift_by_risk_distance() {
        let signal = short_signal();
        let actual = 29_994.0;
        let adjusted = adjusted_signal_for_actual_entry(&signal, actual, EntryGeometryMode::ReanchorToActualEntry);

        assert_eq!(adjusted.entry_price, actual);
        let expected_sl = actual + 300.0;
        assert!(
            (adjusted.stop_loss - expected_sl).abs() < 1e-6,
            "SL mismatch: {} != {}",
            adjusted.stop_loss,
            expected_sl
        );
        let expected_tp = actual - 300.0 * 2.0;
        assert!(
            (adjusted.take_profit - expected_tp).abs() < 1e-6,
            "TP mismatch: {} != {}",
            adjusted.take_profit,
            expected_tp
        );
    }

    #[test]
    fn reanchor_short_geometry_valid_after_normal_slippage() {
        let signal = short_signal();
        let actual = 29_994.0;
        let adjusted = adjusted_signal_for_actual_entry(&signal, actual, EntryGeometryMode::ReanchorToActualEntry);
        assert!(adjusted.valid_geometry(), "reanchored short must have valid geometry");
    }

    #[test]
    fn reanchor_short_reward_bps_uses_new_tp() {
        let signal = short_signal();
        let actual = 29_994.0;
        let adjusted = adjusted_signal_for_actual_entry(&signal, actual, EntryGeometryMode::ReanchorToActualEntry);
        let new_tp = actual - 600.0;
        let expected_reward = (actual - new_tp) / actual * 10_000.0;
        assert!(
            (adjusted.expected_reward_bps - expected_reward).abs() < 1e-6,
            "reward must be computed from new TP"
        );
    }

    // ── Zero / negative entry price guard ────────────────────────────────────

    #[test]
    fn preserve_zero_entry_gives_zero_reward() {
        let signal = long_signal();
        let adjusted = adjusted_signal_for_actual_entry(&signal, 0.0, EntryGeometryMode::PreserveSignalLevels);
        assert_eq!(adjusted.expected_reward_bps, 0.0);
    }

    #[test]
    fn reanchor_zero_entry_gives_zero_reward() {
        let signal = long_signal();
        let adjusted = adjusted_signal_for_actual_entry(&signal, 0.0, EntryGeometryMode::ReanchorToActualEntry);
        assert_eq!(adjusted.expected_reward_bps, 0.0);
    }
}
