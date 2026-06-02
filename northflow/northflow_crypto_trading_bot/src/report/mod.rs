use crate::core::Trade;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunReport {
    pub total_trades: usize,
    pub winning_trades: usize,
    pub losing_trades: usize,
    pub win_rate: f64,
    pub total_pnl: f64,
    pub avg_pnl: f64,
    pub max_drawdown: f64,
    pub sharpe_ratio: f64,
    pub initial_capital: f64,
    pub final_capital: f64,
    pub return_pct: f64,
}

impl RunReport {
    pub fn from_trades(trades: &[Trade], initial_capital: f64, final_capital: f64) -> Self {
        let total = trades.len();
        let winners = trades.iter().filter(|t| t.pnl > 0.0).count();
        let losers = total - winners;
        let win_rate = if total > 0 { winners as f64 / total as f64 } else { 0.0 };
        let total_pnl: f64 = trades.iter().map(|t| t.pnl).sum();
        let avg_pnl = if total > 0 { total_pnl / total as f64 } else { 0.0 };
        let return_pct = (final_capital - initial_capital) / initial_capital * 100.0;

        let max_drawdown = compute_max_drawdown(trades, initial_capital);
        let sharpe_ratio = compute_sharpe(trades);

        Self {
            total_trades: total,
            winning_trades: winners,
            losing_trades: losers,
            win_rate,
            total_pnl,
            avg_pnl,
            max_drawdown,
            sharpe_ratio,
            initial_capital,
            final_capital,
            return_pct,
        }
    }

    pub fn write_json<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn write_trades_csv<P: AsRef<Path>>(trades: &[Trade], path: P) -> Result<()> {
        let mut wtr = csv::Writer::from_path(path)?;
        wtr.write_record([
            "id", "symbol", "side", "entry_price", "exit_price",
            "quantity", "entry_time", "exit_time", "fee", "pnl",
        ])?;
        for t in trades {
            wtr.write_record([
                t.id.to_string(),
                t.symbol.clone(),
                format!("{:?}", t.side),
                t.entry_price.to_string(),
                t.exit_price.to_string(),
                t.quantity.to_string(),
                t.entry_time.to_rfc3339(),
                t.exit_time.to_rfc3339(),
                t.fee.to_string(),
                t.pnl.to_string(),
            ])?;
        }
        wtr.flush()?;
        Ok(())
    }
}

fn compute_max_drawdown(trades: &[Trade], initial_capital: f64) -> f64 {
    let mut equity = initial_capital;
    let mut peak = initial_capital;
    let mut max_dd = 0.0f64;
    for t in trades {
        equity += t.pnl;
        if equity > peak {
            peak = equity;
        }
        let dd = (peak - equity) / peak;
        if dd > max_dd {
            max_dd = dd;
        }
    }
    max_dd
}

fn compute_sharpe(trades: &[Trade]) -> f64 {
    if trades.len() < 2 {
        return 0.0;
    }
    let returns: Vec<f64> = trades.iter().map(|t| t.pnl).collect();
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>()
        / (returns.len() - 1) as f64;
    let std_dev = variance.sqrt();
    if std_dev == 0.0 {
        return 0.0;
    }
    mean / std_dev * (252f64.sqrt())
}
