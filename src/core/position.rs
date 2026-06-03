//! Position — an open or closed holding tied to a signal.

use std::fmt;

use crate::core::{
    error::NorthflowError,
    order::OrderId,
    side::Side,
    signal::SignalId,
    symbol::Symbol,
};

// ── PositionId ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PositionId(pub String);

impl PositionId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PositionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── PositionStatus ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionStatus {
    Open,
    PartiallyClosed,
    Closed,
}

// ── Position ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Position {
    pub position_id:    PositionId,
    pub signal_id:      SignalId,
    pub entry_order_id: OrderId,
    pub symbol:         Symbol,
    pub side:           Side,
    pub entry_price:    f64,
    pub quantity:       f64,
    pub stop_loss:      f64,
    pub take_profit:    f64,
    pub opened_at:      i64,
    pub status:         PositionStatus,
}

impl Position {
    /// Unrealized PnL at `current_price`, ignoring fees.
    pub fn unrealized_pnl(&self, current_price: f64) -> f64 {
        let diff = match self.side {
            Side::Long  => current_price - self.entry_price,
            Side::Short => self.entry_price - current_price,
        };
        diff * self.quantity
    }

    pub fn validate(&self) -> Result<(), NorthflowError> {
        if self.quantity <= 0.0 {
            return Err(NorthflowError::InvalidPosition(format!(
                "quantity must be > 0, got {}",
                self.quantity
            )));
        }
        let geometry_ok = match self.side {
            Side::Long  => self.stop_loss  < self.entry_price && self.entry_price < self.take_profit,
            Side::Short => self.take_profit < self.entry_price && self.entry_price < self.stop_loss,
        };
        if !geometry_ok {
            return Err(NorthflowError::InvalidPosition(format!(
                "SL/TP geometry invalid for {} position: entry={} sl={} tp={}",
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
    use crate::core::{order::OrderId, side::Side, signal::SignalId, symbol::Symbol};

    fn long_pos() -> Position {
        Position {
            position_id:    PositionId::new("POS-00000001"),
            signal_id:      SignalId::new("SIG-BT-00000001"),
            entry_order_id: OrderId::new("ORD-SIG-BT-00000001-ENTRY"),
            symbol:         Symbol::new("BTCUSDT").unwrap(),
            side:           Side::Long,
            entry_price:    30_000.0,
            quantity:       0.1,
            stop_loss:      29_700.0,
            take_profit:    30_600.0,
            opened_at:      1_700_000_000,
            status:         PositionStatus::Open,
        }
    }

    fn short_pos() -> Position {
        Position {
            position_id:    PositionId::new("POS-00000002"),
            signal_id:      SignalId::new("SIG-BT-00000002"),
            entry_order_id: OrderId::new("ORD-SIG-BT-00000002-ENTRY"),
            symbol:         Symbol::new("BTCUSDT").unwrap(),
            side:           Side::Short,
            entry_price:    30_000.0,
            quantity:       0.1,
            stop_loss:      30_300.0,
            take_profit:    29_400.0,
            opened_at:      1_700_000_060,
            status:         PositionStatus::Open,
        }
    }

    #[test]
    fn valid_long_position_passes() {
        assert!(long_pos().validate().is_ok());
    }

    #[test]
    fn valid_short_position_passes() {
        assert!(short_pos().validate().is_ok());
    }

    #[test]
    fn zero_quantity_fails() {
        let mut p = long_pos();
        p.quantity = 0.0;
        assert!(p.validate().is_err());
    }

    #[test]
    fn negative_quantity_fails() {
        let mut p = long_pos();
        p.quantity = -0.1;
        assert!(p.validate().is_err());
    }

    #[test]
    fn long_invalid_sl_above_entry_fails() {
        let mut p = long_pos();
        p.stop_loss = 30_100.0;
        assert!(p.validate().is_err());
    }

    #[test]
    fn short_invalid_sl_below_entry_fails() {
        let mut p = short_pos();
        p.stop_loss = 29_900.0;
        assert!(p.validate().is_err());
    }

    #[test]
    fn long_unrealized_pnl_profit() {
        // qty=0.1, entry=30000, current=30600 → +0.1*600 = +60
        let pnl = long_pos().unrealized_pnl(30_600.0);
        assert!((pnl - 60.0).abs() < 1e-9, "expected 60.0, got {pnl}");
    }

    #[test]
    fn long_unrealized_pnl_loss() {
        // qty=0.1, entry=30000, current=29700 → -0.1*300 = -30
        let pnl = long_pos().unrealized_pnl(29_700.0);
        assert!((pnl - (-30.0)).abs() < 1e-9, "expected -30.0, got {pnl}");
    }

    #[test]
    fn short_unrealized_pnl_profit() {
        // qty=0.1, entry=30000, current=29400 → +0.1*600 = +60
        let pnl = short_pos().unrealized_pnl(29_400.0);
        assert!((pnl - 60.0).abs() < 1e-9, "expected 60.0, got {pnl}");
    }

    #[test]
    fn short_unrealized_pnl_loss() {
        // qty=0.1, entry=30000, current=30300 → -0.1*300 = -30
        let pnl = short_pos().unrealized_pnl(30_300.0);
        assert!((pnl - (-30.0)).abs() < 1e-9, "expected -30.0, got {pnl}");
    }

    #[test]
    fn at_entry_price_pnl_is_zero() {
        assert_eq!(long_pos().unrealized_pnl(30_000.0), 0.0);
        assert_eq!(short_pos().unrealized_pnl(30_000.0), 0.0);
    }
}
