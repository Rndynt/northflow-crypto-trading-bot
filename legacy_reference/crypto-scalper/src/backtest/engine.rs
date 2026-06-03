//! Minimal backtest runner. Replays candles through all configured strategies
//! and simulates SL/TP fills on the next candle.
//!
//! **P0-9 fix**: The engine now uses the live quant strategies (OrderFlow,
//! TradeFlow, KalmanTrend, MicrostructureReversion, Squeeze) instead of the
//! legacy TA aliases (EmaRibbon, Momentum, VwapScalp, MeanReversion) so that
//! backtest results reflect the same signal logic used in production.

use crate::backtest::metrics::PerformanceMetrics;
use crate::data::{Candle, Side};
use crate::errors::Result;
use crate::execution::tcm::TransactionCostModel;
use crate::strategy::{
    RegimeDetector, Strategy,
    kalman_trend::KalmanTrendStrategy,
    microstructure_reversion::MicrostructureReversion,
    order_flow::OrderFlow,
    select_strategies,
    squeeze::Squeeze,
    state::{PreSignal, StrategyName, SymbolState},
    trade_flow::TradeFlow,
};
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimTrade {
    pub symbol: String,
    pub strategy: String,
    pub side: String,
    pub entry: f64,
    pub exit: f64,
    pub pnl: f64,
    pub pnl_pct: f64,
    pub bars_held: u32,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestResult {
    pub symbol: String,
    pub trades: Vec<SimTrade>,
    pub metrics: PerformanceMetrics,
}

pub struct BacktestEngine {
    pub symbol: String,
    pub active: Vec<StrategyName>,
    pub min_ta_confidence: u8,
    pub risk_per_trade_usd: f64,
    pub fee_bps: f64,
    pub slippage_bps: f64,
    pub market_impact_bps: f64,
    pub min_reward_risk: f64,
    pub max_position_notional_pct: f64,
    pub min_net_edge_bps: f64,
    pub assumed_daily_volume_usd: f64,
    pub equity_usd: f64,
    pub trading_days_per_year: f64,
    pub trades_per_day: f64,
}

impl BacktestEngine {
    pub fn run(&self, candles: &[Candle]) -> Result<BacktestResult> {
        let mut state = SymbolState::new(&self.symbol);
        let mut open: Option<(PreSignal, u32)> = None;
        let mut sim_trades: Vec<SimTrade> = Vec::new();

        for (i, c) in candles.iter().enumerate() {
            state.on_closed(*c);

            // Exit check first
            if let Some((sig, bars)) = open.clone() {
                let (exit_price, exit_reason) = match sig.side {
                    Side::Long => {
                        if c.low <= sig.stop_loss {
                            (sig.stop_loss, "SL".to_string())
                        } else if c.high >= sig.take_profit {
                            (sig.take_profit, "TP".to_string())
                        } else {
                            // Noop — still open
                            open = Some((sig.clone(), bars + 1));
                            continue;
                        }
                    }
                    Side::Short => {
                        if c.high >= sig.stop_loss {
                            (sig.stop_loss, "SL".to_string())
                        } else if c.low <= sig.take_profit {
                            (sig.take_profit, "TP".to_string())
                        } else {
                            open = Some((sig.clone(), bars + 1));
                            continue;
                        }
                    }
                };
                let size = self.signal_size(&sig);
                let tcm = self.tcm();
                let slip = (self.slippage_bps
                    + tcm.market_impact_bps(size * sig.entry, self.assumed_daily_volume_usd))
                    / 10_000.0;
                let slipped_exit = match sig.side {
                    Side::Long => exit_price * (1.0 - slip),
                    Side::Short => exit_price * (1.0 + slip),
                };
                let gross_pnl = match sig.side {
                    Side::Long => (slipped_exit - sig.entry) * size,
                    Side::Short => (sig.entry - slipped_exit) * size,
                };
                let notional = sig.entry * size;
                let fee = notional * self.fee_bps / 10_000.0 * 2.0; // round-trip
                let pnl = gross_pnl - fee;
                let pnl_pct = pnl / (sig.entry * size) * 100.0;
                sim_trades.push(SimTrade {
                    symbol: self.symbol.clone(),
                    strategy: sig.strategy.as_str().to_string(),
                    side: format!("{:?}", sig.side),
                    entry: sig.entry,
                    exit: slipped_exit,
                    pnl,
                    pnl_pct,
                    bars_held: bars,
                    reason: exit_reason,
                });
                open = None;
                continue;
            }

            // Skip if not enough candles
            if i < 5 {
                continue;
            }

            let regime = RegimeDetector::detect(&state);
            let chosen = select_strategies(&self.active, regime);
            if chosen.is_empty() {
                continue;
            }

            // Evaluate quant strategies in regime-preferred order, take the highest confidence.
            // P0-9: use the live quant strategy implementations, not legacy TA aliases.
            let mut best: Option<PreSignal> = None;
            for &name in &chosen {
                let sig = match name {
                    StrategyName::EmaRibbon => OrderFlow.evaluate(&state, c),
                    StrategyName::Momentum => TradeFlow.evaluate(&state, c),
                    StrategyName::VwapScalp => KalmanTrendStrategy.evaluate(&state, c),
                    StrategyName::MeanReversion => MicrostructureReversion.evaluate(&state, c),
                    StrategyName::Squeeze => Squeeze.evaluate(&state, c),
                    StrategyName::ScreenedVwapScalp => {
                        crate::strategy::screened_vwap_scalp::ScreenedVwapScalp.evaluate(&state, c)
                    }
                };
                if let Some(s) = sig {
                    if best
                        .as_ref()
                        .map(|b| s.ta_confidence > b.ta_confidence)
                        .unwrap_or(true)
                    {
                        best = Some(s);
                    }
                }
            }

            let sig = match best {
                Some(s) => s,
                None => continue,
            };
            if sig.ta_confidence < self.min_ta_confidence {
                continue;
            }
            if sig.entry <= 0.0 || sig.stop_loss <= 0.0 || sig.take_profit <= 0.0 {
                continue;
            }

            // R:R gate
            let risk = (sig.entry - sig.stop_loss).abs();
            let reward = (sig.take_profit - sig.entry).abs();
            if risk <= 0.0 || reward / risk < self.min_reward_risk {
                continue;
            }

            // Net edge gate (P0-10: re-enabled when min_net_edge_bps > 0)
            if self.min_net_edge_bps > 0.0 {
                let size = self.signal_size(&sig);
                let tcm = self.tcm();
                let gross_edge_bps = reward / sig.entry * 10_000.0;
                let net_edge_bps = gross_edge_bps
                    - tcm.round_trip_cost_bps(size * sig.entry, self.assumed_daily_volume_usd);
                if net_edge_bps < self.min_net_edge_bps {
                    continue;
                }
            }

            open = Some((sig, 1));
        }

        // Close any still-open position at end of data using the last candle close
        if let Some((sig, bars)) = open.take() {
            if let Some(last) = candles.last() {
                let exit_price = last.close;
                let size = self.signal_size(&sig);
                let pnl = match sig.side {
                    Side::Long => (exit_price - sig.entry) * size,
                    Side::Short => (sig.entry - exit_price) * size,
                };
                sim_trades.push(SimTrade {
                    symbol: self.symbol.clone(),
                    strategy: sig.strategy.as_str().to_string(),
                    side: format!("{:?}", sig.side),
                    entry: sig.entry,
                    exit: exit_price,
                    pnl,
                    pnl_pct: pnl / (sig.entry * size) * 100.0,
                    bars_held: bars,
                    reason: "END".to_string(),
                });
            }
        }

        let pnls: Vec<f64> = sim_trades.iter().map(|t| t.pnl).collect();
        let periods_per_year = self.trading_days_per_year * self.trades_per_day;
        let metrics = PerformanceMetrics::from_trades_annualized(&pnls, periods_per_year);
        info!(
            symbol = %self.symbol,
            trades = sim_trades.len(),
            win_rate = %format!("{:.1}%", metrics.win_rate * 100.0),
            "backtest done"
        );
        Ok(BacktestResult {
            symbol: self.symbol.clone(),
            trades: sim_trades,
            metrics,
        })
    }

    fn signal_size(&self, sig: &PreSignal) -> f64 {
        if sig.entry <= 0.0 {
            return 0.0;
        }
        let notional =
            self.risk_per_trade_usd / ((sig.entry - sig.stop_loss).abs() / sig.entry).max(0.001);
        let max_notional = self.equity_usd * self.max_position_notional_pct / 100.0;
        (notional.min(max_notional) / sig.entry).max(0.0)
    }

    fn tcm(&self) -> TransactionCostModel {
        TransactionCostModel {
            taker_fee_bps: self.fee_bps,
            maker_fee_bps: -1.0,
            avg_slippage_bps: self.slippage_bps,
            market_impact_bps: self.market_impact_bps,
        }
    }
}
