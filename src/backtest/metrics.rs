//! Backtest metrics — summarises closed trades and equity curve.
//!
//! No external dependencies.  All computation is from pure std.

use crate::core::Trade;

// ── EquityPoint ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct EquityPoint {
    pub timestamp: i64,
    pub equity: f64,
    pub drawdown_pct: f64,
}

// ── BacktestSummary ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct BacktestSummary {
    pub total_trades: usize,
    pub win_rate: f64,
    pub net_pnl: f64,
    pub gross_pnl: f64,
    pub total_fee: f64,
    pub total_slippage: f64,
    pub profit_factor: f64,
    pub expectancy: f64,
    pub avg_win: f64,
    pub avg_loss: f64,
    pub max_drawdown: f64,
    pub max_consecutive_losses: usize,
    pub avg_trade_duration: f64,
}

// ── Metrics ───────────────────────────────────────────────────────────────────

pub struct Metrics;

impl Metrics {
    pub fn summarize(trades: &[Trade], equity_curve: &[EquityPoint]) -> BacktestSummary {
        let total_trades = trades.len();

        if total_trades == 0 {
            let max_drawdown = equity_curve
                .iter()
                .map(|p| p.drawdown_pct)
                .fold(0.0_f64, f64::max);
            return BacktestSummary {
                total_trades: 0,
                win_rate: 0.0,
                net_pnl: 0.0,
                gross_pnl: 0.0,
                total_fee: 0.0,
                total_slippage: 0.0,
                profit_factor: 0.0,
                expectancy: 0.0,
                avg_win: 0.0,
                avg_loss: 0.0,
                max_drawdown,
                max_consecutive_losses: 0,
                avg_trade_duration: 0.0,
            };
        }

        let wins: Vec<&Trade> = trades.iter().filter(|t| t.net_pnl > 0.0).collect();
        let losses: Vec<&Trade> = trades.iter().filter(|t| t.net_pnl <= 0.0).collect();

        let win_rate = wins.len() as f64 / total_trades as f64 * 100.0;
        let net_pnl: f64 = trades.iter().map(|t| t.net_pnl).sum();
        let gross_pnl: f64 = trades.iter().map(|t| t.gross_pnl).sum();
        let total_fee: f64 = trades.iter().map(|t| t.fee).sum();
        let total_slippage: f64 = trades.iter().map(|t| t.slippage).sum();

        let total_win_pnl: f64 = wins.iter().map(|t| t.net_pnl).sum();
        let total_loss_pnl: f64 = losses.iter().map(|t| t.net_pnl.abs()).sum();

        let profit_factor = if total_loss_pnl <= 0.0 {
            if total_win_pnl > 0.0 {
                f64::INFINITY
            } else {
                0.0
            }
        } else {
            total_win_pnl / total_loss_pnl
        };

        let expectancy = net_pnl / total_trades as f64;

        let avg_win = if wins.is_empty() {
            0.0
        } else {
            total_win_pnl / wins.len() as f64
        };

        let avg_loss = if losses.is_empty() {
            0.0
        } else {
            losses.iter().map(|t| t.net_pnl).sum::<f64>() / losses.len() as f64
        };

        let max_drawdown = equity_curve
            .iter()
            .map(|p| p.drawdown_pct)
            .fold(0.0_f64, f64::max);

        let max_consecutive_losses = Self::max_consecutive_losses(trades);

        let avg_trade_duration = if total_trades == 0 {
            0.0
        } else {
            trades
                .iter()
                .map(|t| t.duration_seconds() as f64)
                .sum::<f64>()
                / total_trades as f64
        };

        BacktestSummary {
            total_trades,
            win_rate,
            net_pnl,
            gross_pnl,
            total_fee,
            total_slippage,
            profit_factor,
            expectancy,
            avg_win,
            avg_loss,
            max_drawdown,
            max_consecutive_losses,
            avg_trade_duration,
        }
    }

    fn max_consecutive_losses(trades: &[Trade]) -> usize {
        let mut max = 0usize;
        let mut current = 0usize;
        for t in trades {
            if t.net_pnl <= 0.0 {
                current += 1;
                if current > max {
                    max = current;
                }
            } else {
                current = 0;
            }
        }
        max
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        PositionId, Side, SignalId, StrategyId, Symbol, Trade, TradeExitReason, TradeId,
    };

    fn make_trade(net_pnl: f64, gross_pnl: f64, entry_time: i64, exit_time: i64) -> Trade {
        Trade {
            trade_id: TradeId::new("TRD-SIG-BT-00000001"),
            signal_id: SignalId::new("SIG-BT-00000001"),
            position_id: PositionId::new("POS-SIG-BT-00000001"),
            symbol: Symbol::new("BTCUSDT").unwrap(),
            strategy_id: StrategyId::new("screened_vwap_scalp"),
            regime: "bullish".to_string(),
            side: Side::Long,
            entry_time,
            exit_time,
            entry_price: 30_000.0,
            exit_price: 30_600.0,
            stop_loss: 29_700.0,
            take_profit: 30_600.0,
            quantity: 0.1,
            gross_pnl,
            fee: 1.0,
            slippage: 0.5,
            net_pnl,
            reward_risk: 2.0,
            bars_held: 10,
            exit_reason: TradeExitReason::TakeProfit,
            entry_reason: "ema_cross".to_string(),
            filters_passed: vec![],
            filters_failed: vec![],
            expected_edge_bps: 192.0,
            actual_edge_bps: net_pnl / (30_000.0 * 0.1) * 10_000.0,
        }
    }

    fn equity_point(ts: i64, equity: f64, drawdown: f64) -> EquityPoint {
        EquityPoint {
            timestamp: ts,
            equity,
            drawdown_pct: drawdown,
        }
    }

    #[test]
    fn summary_zero_trades() {
        let s = Metrics::summarize(&[], &[]);
        assert_eq!(s.total_trades, 0);
        assert_eq!(s.win_rate, 0.0);
        assert_eq!(s.net_pnl, 0.0);
        assert_eq!(s.profit_factor, 0.0);
    }

    #[test]
    fn summary_calculates_total_trades() {
        let trades = vec![
            make_trade(50.0, 60.0, 0, 600),
            make_trade(-20.0, -15.0, 600, 1200),
        ];
        let s = Metrics::summarize(&trades, &[]);
        assert_eq!(s.total_trades, 2);
    }

    #[test]
    fn summary_calculates_win_rate() {
        let trades = vec![
            make_trade(50.0, 60.0, 0, 600),
            make_trade(-20.0, -15.0, 600, 1200),
        ];
        let s = Metrics::summarize(&trades, &[]);
        assert!((s.win_rate - 50.0).abs() < 1e-9);
    }

    #[test]
    fn summary_calculates_net_pnl() {
        let trades = vec![
            make_trade(50.0, 60.0, 0, 600),
            make_trade(-20.0, -15.0, 600, 1200),
        ];
        let s = Metrics::summarize(&trades, &[]);
        assert!((s.net_pnl - 30.0).abs() < 1e-9);
    }

    #[test]
    fn summary_calculates_gross_pnl() {
        let trades = vec![
            make_trade(50.0, 60.0, 0, 600),
            make_trade(-20.0, -15.0, 600, 1200),
        ];
        let s = Metrics::summarize(&trades, &[]);
        assert!((s.gross_pnl - 45.0).abs() < 1e-9);
    }

    #[test]
    fn summary_calculates_total_fee() {
        let trades = vec![
            make_trade(50.0, 60.0, 0, 600),
            make_trade(-20.0, -15.0, 600, 1200),
        ];
        let s = Metrics::summarize(&trades, &[]);
        assert!((s.total_fee - 2.0).abs() < 1e-9);
    }

    #[test]
    fn summary_calculates_total_slippage() {
        let trades = vec![
            make_trade(50.0, 60.0, 0, 600),
            make_trade(-20.0, -15.0, 600, 1200),
        ];
        let s = Metrics::summarize(&trades, &[]);
        assert!((s.total_slippage - 1.0).abs() < 1e-9);
    }

    #[test]
    fn summary_calculates_profit_factor() {
        let trades = vec![
            make_trade(60.0, 70.0, 0, 600),
            make_trade(-20.0, -15.0, 600, 1200),
        ];
        let s = Metrics::summarize(&trades, &[]);
        // pf = 60 / 20 = 3.0
        assert!((s.profit_factor - 3.0).abs() < 1e-9);
    }

    #[test]
    fn summary_profit_factor_inf_when_no_losses() {
        let trades = vec![make_trade(50.0, 60.0, 0, 600)];
        let s = Metrics::summarize(&trades, &[]);
        assert!(s.profit_factor.is_infinite());
    }

    #[test]
    fn summary_calculates_expectancy() {
        let trades = vec![
            make_trade(50.0, 60.0, 0, 600),
            make_trade(-20.0, -15.0, 600, 1200),
        ];
        let s = Metrics::summarize(&trades, &[]);
        // expectancy = 30 / 2 = 15
        assert!((s.expectancy - 15.0).abs() < 1e-9);
    }

    #[test]
    fn summary_calculates_avg_win_and_avg_loss() {
        let trades = vec![
            make_trade(40.0, 50.0, 0, 600),
            make_trade(60.0, 70.0, 600, 1200),
            make_trade(-30.0, -25.0, 1200, 1800),
        ];
        let s = Metrics::summarize(&trades, &[]);
        assert!((s.avg_win - 50.0).abs() < 1e-9);
        assert!((s.avg_loss - (-30.0)).abs() < 1e-9);
    }

    #[test]
    fn summary_calculates_max_drawdown() {
        let curve = vec![
            equity_point(0, 1000.0, 0.0),
            equity_point(60, 950.0, 5.0),
            equity_point(120, 900.0, 10.0),
            equity_point(180, 920.0, 8.0),
        ];
        let s = Metrics::summarize(&[], &curve);
        assert!((s.max_drawdown - 10.0).abs() < 1e-9);
    }

    #[test]
    fn summary_calculates_max_consecutive_losses() {
        let trades = vec![
            make_trade(-10.0, -5.0, 0, 600),
            make_trade(-10.0, -5.0, 600, 1200),
            make_trade(50.0, 60.0, 1200, 1800),
            make_trade(-10.0, -5.0, 1800, 2400),
        ];
        let s = Metrics::summarize(&trades, &[]);
        assert_eq!(s.max_consecutive_losses, 2);
    }

    #[test]
    fn summary_calculates_avg_trade_duration() {
        let trades = vec![
            make_trade(50.0, 60.0, 0, 600),
            make_trade(-20.0, -15.0, 600, 1200),
        ];
        let s = Metrics::summarize(&trades, &[]);
        // both trades held 600 seconds
        assert!((s.avg_trade_duration - 600.0).abs() < 1e-9);
    }
}
