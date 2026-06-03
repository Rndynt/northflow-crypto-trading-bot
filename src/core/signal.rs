//! Signal — the pure output of a strategy evaluation.
//!
//! A Signal must never place orders, call an exchange, or mutate account state.
//! It carries every field needed for downstream risk validation, order creation,
//! and full trade attribution.
//!
//! ID chain:
//!   signal_id → order_id → fill_id → position_id → exit_order_id → trade_id

use std::fmt;

use crate::core::{
    error::NorthflowError,
    side::Side,
    symbol::Symbol,
    timeframe::Timeframe,
};

// ── Signal identity types ────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SignalId(pub String);

impl SignalId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SignalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StrategyId(pub String);

impl StrategyId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StrategyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── Signal ───────────────────────────────────────────────────────────────────

/// The complete, immutable output of one strategy evaluation.
/// Every downstream object (Order, Fill, Position, Trade) must trace back to
/// this `signal_id`.
#[derive(Debug, Clone)]
pub struct Signal {
    pub signal_id:               SignalId,
    pub symbol:                  Symbol,
    pub strategy_id:             StrategyId,
    pub side:                    Side,
    /// 1m — entry and execution.
    pub entry_timeframe:         Timeframe,
    /// 15m — screening and regime bias.
    pub screening_timeframe:     Timeframe,
    /// 5m — confirmation.
    pub confirmation_timeframe:  Timeframe,
    pub entry_time:              i64,
    pub entry_price:             f64,
    pub stop_loss:               f64,
    pub take_profit:             f64,
    /// Strategy confidence 0–100.
    pub confidence:              u8,
    /// Market regime label (e.g. "bullish", "bearish", "ranging").
    pub regime:                  String,
    pub entry_reason:            String,
    pub filters_passed:          Vec<String>,
    pub filters_failed:          Vec<String>,
    pub expected_reward_bps:     f64,
    pub estimated_cost_bps:      f64,
    pub expected_net_edge_bps:   f64,
}

impl Signal {
    /// |TP − entry| / |entry − SL|.  Returns 0.0 when risk is zero.
    pub fn reward_risk(&self) -> f64 {
        let risk = (self.entry_price - self.stop_loss).abs();
        if risk <= 0.0 {
            return 0.0;
        }
        (self.take_profit - self.entry_price).abs() / risk
    }

    /// `true` when SL / TP geometry is valid for the declared Side:
    ///   Long  → SL < entry < TP
    ///   Short → TP < entry < SL
    pub fn valid_geometry(&self) -> bool {
        if self.entry_price <= 0.0 || self.stop_loss <= 0.0 || self.take_profit <= 0.0 {
            return false;
        }
        match self.side {
            Side::Long  => self.stop_loss  < self.entry_price && self.entry_price < self.take_profit,
            Side::Short => self.take_profit < self.entry_price && self.entry_price < self.stop_loss,
        }
    }

    pub fn validate(&self) -> Result<(), NorthflowError> {
        if self.signal_id.0.is_empty() {
            return Err(NorthflowError::InvalidSignal(
                "signal_id must not be empty".to_string(),
            ));
        }
        if !self.valid_geometry() {
            return Err(NorthflowError::InvalidSignal(format!(
                "invalid {} geometry: entry={} sl={} tp={}",
                self.side, self.entry_price, self.stop_loss, self.take_profit
            )));
        }
        Ok(())
    }
}

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{side::Side, symbol::Symbol, timeframe::Timeframe};

    fn make_long() -> Signal {
        Signal {
            signal_id:              SignalId::new("SIG-BT-00000001"),
            symbol:                 Symbol::new("BTCUSDT").unwrap(),
            strategy_id:            StrategyId::new("screened_vwap_scalp"),
            side:                   Side::Long,
            entry_timeframe:        Timeframe::OneMinute,
            screening_timeframe:    Timeframe::FifteenMinute,
            confirmation_timeframe: Timeframe::FiveMinute,
            entry_time:             1_700_000_000,
            entry_price:            30_000.0,
            stop_loss:              29_700.0,
            take_profit:            30_600.0,
            confidence:             70,
            regime:                 "bullish".to_string(),
            entry_reason:           "ema_cross_above_vwap".to_string(),
            filters_passed:         vec!["vwap_filter".to_string()],
            filters_failed:         vec![],
            expected_reward_bps:    200.0,
            estimated_cost_bps:     8.0,
            expected_net_edge_bps:  192.0,
        }
    }

    fn make_short() -> Signal {
        Signal {
            signal_id:              SignalId::new("SIG-BT-00000002"),
            symbol:                 Symbol::new("BTCUSDT").unwrap(),
            strategy_id:            StrategyId::new("screened_vwap_scalp"),
            side:                   Side::Short,
            entry_timeframe:        Timeframe::OneMinute,
            screening_timeframe:    Timeframe::FifteenMinute,
            confirmation_timeframe: Timeframe::FiveMinute,
            entry_time:             1_700_000_060,
            entry_price:            30_000.0,
            stop_loss:              30_300.0,
            take_profit:            29_400.0,
            confidence:             65,
            regime:                 "bearish".to_string(),
            entry_reason:           "ema_cross_below_vwap".to_string(),
            filters_passed:         vec!["vwap_filter".to_string()],
            filters_failed:         vec![],
            expected_reward_bps:    200.0,
            estimated_cost_bps:     8.0,
            expected_net_edge_bps:  192.0,
        }
    }

    #[test]
    fn valid_long_passes_geometry() {
        assert!(make_long().valid_geometry());
        assert!(make_long().validate().is_ok());
    }

    #[test]
    fn invalid_long_sl_above_entry_fails() {
        let mut s = make_long();
        s.stop_loss = 30_100.0;
        assert!(!s.valid_geometry());
        assert!(s.validate().is_err());
    }

    #[test]
    fn invalid_long_tp_below_entry_fails() {
        let mut s = make_long();
        s.take_profit = 29_800.0;
        assert!(!s.valid_geometry());
    }

    #[test]
    fn invalid_long_sl_equals_entry_fails() {
        let mut s = make_long();
        s.stop_loss = 30_000.0;
        assert!(!s.valid_geometry());
    }

    #[test]
    fn valid_short_passes_geometry() {
        assert!(make_short().valid_geometry());
        assert!(make_short().validate().is_ok());
    }

    #[test]
    fn invalid_short_sl_below_entry_fails() {
        let mut s = make_short();
        s.stop_loss = 29_900.0;
        assert!(!s.valid_geometry());
    }

    #[test]
    fn invalid_short_tp_above_entry_fails() {
        let mut s = make_short();
        s.take_profit = 30_100.0;
        assert!(!s.valid_geometry());
    }

    #[test]
    fn reward_risk_long() {
        // TP–entry = 600, entry–SL = 300 → RR = 2.0
        let rr = make_long().reward_risk();
        assert!((rr - 2.0).abs() < 1e-9, "expected 2.0, got {rr}");
    }

    #[test]
    fn reward_risk_short() {
        // |TP–entry| = 600, |entry–SL| = 300 → RR = 2.0
        let rr = make_short().reward_risk();
        assert!((rr - 2.0).abs() < 1e-9, "expected 2.0, got {rr}");
    }

    #[test]
    fn reward_risk_zero_when_sl_equals_entry() {
        let mut s = make_long();
        s.stop_loss = s.entry_price;
        assert_eq!(s.reward_risk(), 0.0);
    }

    #[test]
    fn signal_id_is_present_and_nonempty() {
        let s = make_long();
        assert!(!s.signal_id.0.is_empty());
        assert_eq!(s.signal_id.as_str(), "SIG-BT-00000001");
    }

    #[test]
    fn empty_signal_id_fails_validation() {
        let mut s = make_long();
        s.signal_id = SignalId::new("");
        assert!(s.validate().is_err());
    }

    #[test]
    fn explicit_timeframe_roles() {
        let s = make_long();
        assert_eq!(s.entry_timeframe,        Timeframe::OneMinute,     "entry must be 1m");
        assert_eq!(s.screening_timeframe,    Timeframe::FifteenMinute, "screening must be 15m");
        assert_eq!(s.confirmation_timeframe, Timeframe::FiveMinute,    "confirmation must be 5m");
    }

    #[test]
    fn zero_entry_price_fails_geometry() {
        let mut s = make_long();
        s.entry_price = 0.0;
        assert!(!s.valid_geometry());
    }
}
