//! Fill model — deterministic, conservative entry and exit simulation.
//!
//! Rules:
//!   - Entry at next 1m candle open with adverse slippage.
//!   - Exit at SL / TP / TimeExit / EndOfBacktest with adverse slippage.
//!   - Conservative intrabar: if SL and TP both touched, SL is assumed first.
//!   - No exchange calls. No live data. Backtest simulation only.

use crate::core::{Candle, Side, Signal, TradeExitReason};

// ── EntryFill ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct EntryFill {
    pub time: i64,
    pub price: f64,
    pub qty: f64,
    pub fee: f64,
    pub slippage: f64,
}

// ── ExitFill ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ExitFill {
    pub time: i64,
    pub price: f64,
    pub fee: f64,
    pub slippage: f64,
    pub reason: TradeExitReason,
    pub bars_held: u32,
}

// ── OpenSimPosition ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct OpenSimPosition {
    pub signal: Signal,
    pub qty: f64,
    pub entry_time: i64,
    pub entry_price: f64,
    pub entry_fee: f64,
    pub entry_slippage: f64,
    pub bars_held: u32,
}

// ── FillModel ─────────────────────────────────────────────────────────────────

pub struct FillModel;

impl FillModel {
    /// Simulate entry at `entry_candle.open` with adverse slippage.
    ///
    /// Long  → buy at a price ABOVE open (adverse: pay more).
    /// Short → sell at a price BELOW open (adverse: receive less).
    pub fn simulate_entry(
        signal: &Signal,
        qty: f64,
        entry_candle: &Candle,
        slippage_bps: f64,
        taker_fee_bps: f64,
    ) -> EntryFill {
        let raw = entry_candle.open;
        let factor = slippage_bps / 10_000.0;
        let price = match signal.side {
            Side::Long => raw * (1.0 + factor),
            Side::Short => raw * (1.0 - factor),
        };
        let fee = price * qty * taker_fee_bps / 10_000.0;
        let slippage = (price - raw).abs() * qty;
        EntryFill {
            time: entry_candle.timestamp,
            price,
            qty,
            fee,
            slippage,
        }
    }

    /// Check whether the position should exit on `candle`.
    ///
    /// Returns `Some(ExitFill)` when an exit condition is met, `None` otherwise.
    ///
    /// Conservative intrabar rule: if both SL and TP are touched in the same
    /// candle, SL is assumed to have been hit first.
    pub fn check_exit(
        pos: &OpenSimPosition,
        candle: &Candle,
        conservative_intrabar: bool,
        slippage_bps: f64,
        taker_fee_bps: f64,
        max_bars_held: u32,
    ) -> Option<ExitFill> {
        let sl = pos.signal.stop_loss;
        let tp = pos.signal.take_profit;

        let sl_touched = match pos.signal.side {
            Side::Long => candle.low <= sl,
            Side::Short => candle.high >= sl,
        };
        let tp_touched = match pos.signal.side {
            Side::Long => candle.high >= tp,
            Side::Short => candle.low <= tp,
        };

        let reason = if sl_touched && tp_touched && conservative_intrabar {
            TradeExitReason::StopLoss
        } else if sl_touched {
            TradeExitReason::StopLoss
        } else if tp_touched {
            TradeExitReason::TakeProfit
        } else if pos.bars_held >= max_bars_held {
            TradeExitReason::TimeExit
        } else {
            return None;
        };

        Some(Self::make_exit_fill(
            pos,
            candle.timestamp,
            Self::base_price(reason, sl, tp, candle.close),
            reason,
            slippage_bps,
            taker_fee_bps,
            pos.bars_held,
        ))
    }

    /// Exit at end of backtest — last candle close with adverse slippage.
    pub fn end_of_backtest_exit(
        pos: &OpenSimPosition,
        last_candle: &Candle,
        slippage_bps: f64,
        taker_fee_bps: f64,
    ) -> ExitFill {
        Self::make_exit_fill(
            pos,
            last_candle.timestamp,
            last_candle.close,
            TradeExitReason::EndOfBacktest,
            slippage_bps,
            taker_fee_bps,
            pos.bars_held,
        )
    }

    // ── helpers ───────────────────────────────────────────────────────────────

    fn base_price(reason: TradeExitReason, sl: f64, tp: f64, close: f64) -> f64 {
        match reason {
            TradeExitReason::StopLoss => sl,
            TradeExitReason::TakeProfit => tp,
            _ => close,
        }
    }

    fn make_exit_fill(
        pos: &OpenSimPosition,
        time: i64,
        base: f64,
        reason: TradeExitReason,
        slippage_bps: f64,
        taker_fee_bps: f64,
        bars_held: u32,
    ) -> ExitFill {
        let factor = slippage_bps / 10_000.0;
        let price = match pos.signal.side {
            Side::Long => base * (1.0 - factor),
            Side::Short => base * (1.0 + factor),
        };
        let fee = price * pos.qty * taker_fee_bps / 10_000.0;
        let slippage = (price - base).abs() * pos.qty;
        ExitFill {
            time,
            price,
            fee,
            slippage,
            reason,
            bars_held,
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{SignalId, StrategyId, Symbol, Timeframe};

    fn make_candle(ts: i64, open: f64, high: f64, low: f64, close: f64) -> Candle {
        Candle {
            timestamp: ts,
            open,
            high,
            low,
            close,
            volume: 1000.0,
        }
    }

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
            entry_reason: "ema_cross_below".to_string(),
            filters_passed: vec![],
            filters_failed: vec![],
            expected_reward_bps: 200.0,
            estimated_cost_bps: 8.0,
            expected_net_edge_bps: 192.0,
        }
    }

    fn open_long(bars_held: u32) -> OpenSimPosition {
        OpenSimPosition {
            signal: long_signal(),
            qty: 0.1,
            entry_time: 1_700_000_000_000,
            entry_price: 30_000.0,
            entry_fee: 1.2,
            entry_slippage: 0.6,
            bars_held,
        }
    }

    fn open_short(bars_held: u32) -> OpenSimPosition {
        OpenSimPosition {
            signal: short_signal(),
            qty: 0.1,
            entry_time: 1_700_000_000_000,
            entry_price: 30_000.0,
            entry_fee: 1.2,
            entry_slippage: 0.6,
            bars_held,
        }
    }

    // ── Entry tests ───────────────────────────────────────────────────────────

    #[test]
    fn entry_uses_next_candle_open() {
        let sig = long_signal();
        let candle = make_candle(1_700_000_060_000, 30_100.0, 30_200.0, 30_050.0, 30_150.0);
        let fill = FillModel::simulate_entry(&sig, 0.1, &candle, 0.0, 0.0);
        assert!(
            (fill.price - 30_100.0).abs() < 1e-9,
            "expected open price when slippage=0"
        );
        assert_eq!(fill.time, candle.timestamp);
    }

    #[test]
    fn long_entry_slippage_is_adverse() {
        let sig = long_signal();
        let candle = make_candle(1_700_000_060_000, 30_000.0, 30_100.0, 29_950.0, 30_050.0);
        let fill = FillModel::simulate_entry(&sig, 0.1, &candle, 2.0, 0.0);
        assert!(
            fill.price > candle.open,
            "long entry must be ABOVE open (adverse)"
        );
    }

    #[test]
    fn short_entry_slippage_is_adverse() {
        let sig = short_signal();
        let candle = make_candle(1_700_000_060_000, 30_000.0, 30_100.0, 29_950.0, 30_050.0);
        let fill = FillModel::simulate_entry(&sig, 0.1, &candle, 2.0, 0.0);
        assert!(
            fill.price < candle.open,
            "short entry must be BELOW open (adverse)"
        );
    }

    #[test]
    fn fee_is_applied() {
        let sig = long_signal();
        let candle = make_candle(1_700_000_060_000, 30_000.0, 30_100.0, 29_950.0, 30_050.0);
        // no slippage, only fee
        let fill = FillModel::simulate_entry(&sig, 0.1, &candle, 0.0, 4.0);
        let expected = 30_000.0 * 0.1 * 4.0 / 10_000.0;
        assert!(
            (fill.fee - expected).abs() < 1e-9,
            "fee mismatch: got {}",
            fill.fee
        );
    }

    #[test]
    fn slippage_cost_is_applied() {
        let sig = long_signal();
        let candle = make_candle(1_700_000_060_000, 30_000.0, 30_100.0, 29_950.0, 30_050.0);
        let fill = FillModel::simulate_entry(&sig, 0.1, &candle, 2.0, 0.0);
        let expected = 30_000.0 * (2.0 / 10_000.0) * 0.1;
        assert!(
            (fill.slippage - expected).abs() < 1e-9,
            "slippage mismatch: got {}",
            fill.slippage
        );
    }

    // ── Exit: Long ────────────────────────────────────────────────────────────

    #[test]
    fn long_stop_loss_exit() {
        let pos = open_long(1);
        // low <= 29700 → SL triggered
        let candle = make_candle(1_700_000_120_000, 30_000.0, 30_050.0, 29_650.0, 29_900.0);
        let exit = FillModel::check_exit(&pos, &candle, true, 2.0, 4.0, 60);
        assert!(exit.is_some());
        let exit = exit.unwrap();
        assert_eq!(exit.reason, TradeExitReason::StopLoss);
        // exit price = SL * (1 - slippage) for long sell
        let expected = 29_700.0 * (1.0 - 2.0 / 10_000.0);
        assert!((exit.price - expected).abs() < 1e-6, "price mismatch");
    }

    #[test]
    fn long_take_profit_exit() {
        let pos = open_long(1);
        // high >= 30600 → TP triggered
        let candle = make_candle(1_700_000_120_000, 30_000.0, 30_650.0, 29_950.0, 30_600.0);
        let exit = FillModel::check_exit(&pos, &candle, true, 2.0, 4.0, 60);
        assert!(exit.is_some());
        assert_eq!(exit.unwrap().reason, TradeExitReason::TakeProfit);
    }

    #[test]
    fn long_both_sl_tp_same_candle_assumes_stop_first() {
        let pos = open_long(1);
        // Both SL (low<=29700) and TP (high>=30600) touched
        let candle = make_candle(1_700_000_120_000, 30_000.0, 30_650.0, 29_650.0, 30_100.0);
        let exit = FillModel::check_exit(&pos, &candle, true, 2.0, 4.0, 60);
        assert!(exit.is_some());
        assert_eq!(
            exit.unwrap().reason,
            TradeExitReason::StopLoss,
            "conservative: SL first when both touched"
        );
    }

    // ── Exit: Short ───────────────────────────────────────────────────────────

    #[test]
    fn short_stop_loss_exit() {
        let pos = open_short(1);
        // high >= 30300 → SL triggered for short
        let candle = make_candle(1_700_000_120_000, 30_000.0, 30_350.0, 29_950.0, 30_200.0);
        let exit = FillModel::check_exit(&pos, &candle, true, 2.0, 4.0, 60);
        assert!(exit.is_some());
        assert_eq!(exit.unwrap().reason, TradeExitReason::StopLoss);
    }

    #[test]
    fn short_take_profit_exit() {
        let pos = open_short(1);
        // low <= 29400 → TP triggered for short
        let candle = make_candle(1_700_000_120_000, 30_000.0, 30_050.0, 29_350.0, 29_500.0);
        let exit = FillModel::check_exit(&pos, &candle, true, 2.0, 4.0, 60);
        assert!(exit.is_some());
        assert_eq!(exit.unwrap().reason, TradeExitReason::TakeProfit);
    }

    #[test]
    fn short_both_sl_tp_same_candle_assumes_stop_first() {
        let pos = open_short(1);
        // SL: high>=30300, TP: low<=29400 — both touched
        let candle = make_candle(1_700_000_120_000, 30_000.0, 30_350.0, 29_350.0, 30_000.0);
        let exit = FillModel::check_exit(&pos, &candle, true, 2.0, 4.0, 60);
        assert!(exit.is_some());
        assert_eq!(
            exit.unwrap().reason,
            TradeExitReason::StopLoss,
            "conservative: SL first"
        );
    }

    // ── Time exit ─────────────────────────────────────────────────────────────

    #[test]
    fn time_exit_after_max_bars() {
        let pos = open_long(60); // bars_held == max_bars_held
        // price does NOT touch SL or TP
        let candle = make_candle(1_700_000_120_000, 30_100.0, 30_200.0, 30_050.0, 30_150.0);
        let exit = FillModel::check_exit(&pos, &candle, true, 2.0, 4.0, 60);
        assert!(exit.is_some());
        assert_eq!(exit.unwrap().reason, TradeExitReason::TimeExit);
    }

    // ── Same-candle exit (bars_held = 0) — entry candle can trigger SL/TP ──────

    #[test]
    fn fill_model_can_exit_on_entry_candle_long_stop_loss() {
        // bars_held=0 simulates the entry candle: low touches SL
        let pos = open_long(0);
        let candle = make_candle(1_700_000_060_000, 30_000.0, 30_050.0, 29_650.0, 29_900.0);
        let exit = FillModel::check_exit(&pos, &candle, true, 2.0, 4.0, 60);
        assert!(exit.is_some(), "SL must fire on entry candle (bars_held=0)");
        assert_eq!(exit.unwrap().reason, TradeExitReason::StopLoss);
    }

    #[test]
    fn fill_model_can_exit_on_entry_candle_long_take_profit() {
        // bars_held=0 simulates the entry candle: high touches TP
        let pos = open_long(0);
        let candle = make_candle(1_700_000_060_000, 30_000.0, 30_650.0, 29_950.0, 30_600.0);
        let exit = FillModel::check_exit(&pos, &candle, true, 2.0, 4.0, 60);
        assert!(exit.is_some(), "TP must fire on entry candle (bars_held=0)");
        assert_eq!(exit.unwrap().reason, TradeExitReason::TakeProfit);
    }

    #[test]
    fn fill_model_can_exit_on_entry_candle_short_stop_loss() {
        // bars_held=0 simulates the entry candle: high touches SL for short
        let pos = open_short(0);
        let candle = make_candle(1_700_000_060_000, 30_000.0, 30_350.0, 29_950.0, 30_200.0);
        let exit = FillModel::check_exit(&pos, &candle, true, 2.0, 4.0, 60);
        assert!(exit.is_some(), "SL must fire on entry candle (bars_held=0)");
        assert_eq!(exit.unwrap().reason, TradeExitReason::StopLoss);
    }

    #[test]
    fn fill_model_can_exit_on_entry_candle_short_take_profit() {
        // bars_held=0 simulates the entry candle: low touches TP for short
        let pos = open_short(0);
        let candle = make_candle(1_700_000_060_000, 30_000.0, 30_050.0, 29_350.0, 29_500.0);
        let exit = FillModel::check_exit(&pos, &candle, true, 2.0, 4.0, 60);
        assert!(exit.is_some(), "TP must fire on entry candle (bars_held=0)");
        assert_eq!(exit.unwrap().reason, TradeExitReason::TakeProfit);
    }

    #[test]
    fn fill_model_entry_candle_both_sl_tp_assumes_stop_first() {
        // bars_held=0: both SL and TP touched — conservative rule applies
        let pos = open_long(0);
        let candle = make_candle(1_700_000_060_000, 30_000.0, 30_650.0, 29_650.0, 30_100.0);
        let exit = FillModel::check_exit(&pos, &candle, true, 2.0, 4.0, 60);
        assert!(exit.is_some(), "must exit when both SL and TP touched");
        assert_eq!(
            exit.unwrap().reason,
            TradeExitReason::StopLoss,
            "conservative intrabar: SL assumed first on entry candle"
        );
    }

    // ── End-of-backtest ───────────────────────────────────────────────────────

    #[test]
    fn end_of_backtest_exit() {
        let pos = open_long(5);
        let last = make_candle(1_700_003_600_000, 30_000.0, 30_100.0, 29_900.0, 30_050.0);
        let exit = FillModel::end_of_backtest_exit(&pos, &last, 2.0, 4.0);
        assert_eq!(exit.reason, TradeExitReason::EndOfBacktest);
        // Long: sell at close * (1 - slippage)
        let expected = 30_050.0 * (1.0 - 2.0 / 10_000.0);
        assert!((exit.price - expected).abs() < 1e-6);
    }
}
