//! Trade — the immutable closed-trade record.
//! Every field needed for attribution, reporting, and journal is present.

use std::fmt;

use crate::core::{
    position::PositionId,
    side::Side,
    signal::{SignalId, StrategyId},
    symbol::Symbol,
};

// ── TradeId ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TradeId(pub String);

impl TradeId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TradeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── TradeExitReason ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeExitReason {
    StopLoss,
    TakeProfit,
    PartialTakeProfit,
    TimeExit,
    ManualClose,
    RiskClose,
    EndOfBacktest,
}

impl TradeExitReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StopLoss => "stop_loss",
            Self::TakeProfit => "take_profit",
            Self::PartialTakeProfit => "partial_take_profit",
            Self::TimeExit => "time_exit",
            Self::ManualClose => "manual_close",
            Self::RiskClose => "risk_close",
            Self::EndOfBacktest => "end_of_backtest",
        }
    }
}

impl fmt::Display for TradeExitReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ── Trade ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Trade {
    pub trade_id: TradeId,
    pub signal_id: SignalId,
    pub position_id: PositionId,
    pub symbol: Symbol,
    pub strategy_id: StrategyId,
    pub side: Side,
    pub entry_time: i64,
    pub exit_time: i64,
    pub entry_price: f64,
    pub exit_price: f64,
    pub stop_loss: f64,
    pub take_profit: f64,
    pub quantity: f64,
    pub gross_pnl: f64,
    pub fee: f64,
    pub slippage: f64,
    pub net_pnl: f64,
    pub reward_risk: f64,
    pub bars_held: u32,
    pub exit_reason: TradeExitReason,
    pub entry_reason: String,
    pub filters_passed: Vec<String>,
    pub filters_failed: Vec<String>,
    pub expected_edge_bps: f64,
    pub actual_edge_bps: f64,
}

impl Trade {
    pub fn is_win(&self) -> bool {
        self.net_pnl > 0.0
    }

    /// Holding duration in seconds.
    pub fn duration_seconds(&self) -> i64 {
        self.exit_time - self.entry_time
    }

    /// Recompute net PnL from gross components (read-only; does not mutate).
    pub fn computed_net_pnl(&self) -> f64 {
        self.gross_pnl - self.fee - self.slippage
    }
}

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        position::PositionId,
        side::Side,
        signal::{SignalId, StrategyId},
        symbol::Symbol,
    };

    fn make_trade(net_pnl: f64, gross_pnl: f64, fee: f64, slippage: f64) -> Trade {
        Trade {
            trade_id: TradeId::new("TRD-SIG-BT-00000001"),
            signal_id: SignalId::new("SIG-BT-00000001"),
            position_id: PositionId::new("POS-00000001"),
            symbol: Symbol::new("BTCUSDT").unwrap(),
            strategy_id: StrategyId::new("screened_vwap_scalp"),
            side: Side::Long,
            entry_time: 1_700_000_000,
            exit_time: 1_700_000_600,
            entry_price: 30_000.0,
            exit_price: 30_600.0,
            stop_loss: 29_700.0,
            take_profit: 30_600.0,
            quantity: 0.1,
            gross_pnl,
            fee,
            slippage,
            net_pnl,
            reward_risk: 2.0,
            bars_held: 10,
            exit_reason: TradeExitReason::TakeProfit,
            entry_reason: "ema_cross_above_vwap".to_string(),
            filters_passed: vec!["vwap_filter".to_string()],
            filters_failed: vec![],
            expected_edge_bps: 192.0,
            actual_edge_bps: 195.0,
        }
    }

    #[test]
    fn winning_trade_is_win() {
        assert!(make_trade(55.0, 60.0, 3.0, 2.0).is_win());
    }

    #[test]
    fn losing_trade_not_win() {
        assert!(!make_trade(-35.0, -30.0, 3.0, 2.0).is_win());
    }

    #[test]
    fn break_even_not_win() {
        assert!(!make_trade(0.0, 5.0, 3.0, 2.0).is_win());
    }

    #[test]
    fn computed_net_pnl_correct() {
        let t = make_trade(55.0, 60.0, 3.0, 2.0);
        assert!((t.computed_net_pnl() - 55.0).abs() < 1e-9);
    }

    #[test]
    fn computed_net_pnl_loss() {
        let t = make_trade(-35.0, -30.0, 3.0, 2.0);
        assert!((t.computed_net_pnl() - (-35.0)).abs() < 1e-9);
    }

    #[test]
    fn duration_seconds_correct() {
        assert_eq!(make_trade(0.0, 0.0, 0.0, 0.0).duration_seconds(), 600);
    }

    #[test]
    fn signal_id_traceability() {
        let t = make_trade(55.0, 60.0, 3.0, 2.0);
        assert_eq!(t.signal_id.as_str(), "SIG-BT-00000001");
        assert_eq!(t.position_id.as_str(), "POS-00000001");
        assert_eq!(t.trade_id.as_str(), "TRD-SIG-BT-00000001");
    }

    #[test]
    fn exit_reason_str() {
        assert_eq!(TradeExitReason::StopLoss.as_str(), "stop_loss");
        assert_eq!(TradeExitReason::TakeProfit.as_str(), "take_profit");
        assert_eq!(TradeExitReason::EndOfBacktest.as_str(), "end_of_backtest");
        assert_eq!(TradeExitReason::RiskClose.as_str(), "risk_close");
    }
}
