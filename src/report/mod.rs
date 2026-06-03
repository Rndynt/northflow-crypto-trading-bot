use crate::core::{Side, SimTrade};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct RunReport {
    pub total_trades: usize,
    pub winners: usize,
    pub losers: usize,
    pub win_rate: f64,
    pub total_pnl: f64,
    pub avg_pnl: f64,
    pub best_trade: f64,
    pub worst_trade: f64,
    pub max_drawdown: f64,
    pub sharpe: f64,
    pub initial_equity: f64,
    pub final_equity: f64,
    pub return_pct: f64,
    pub long_trades: usize,
    pub short_trades: usize,
}

impl RunReport {
    pub fn build(trades: &[SimTrade], initial_equity: f64, final_equity: f64) -> Self {
        let n = trades.len();
        let winners = trades.iter().filter(|t| t.net_pnl > 0.0).count();
        let losers = n - winners;
        let win_rate = if n > 0 { winners as f64 / n as f64 } else { 0.0 };
        let total_pnl: f64 = trades.iter().map(|t| t.net_pnl).sum();
        let avg_pnl = if n > 0 { total_pnl / n as f64 } else { 0.0 };
        let best_trade = trades.iter().map(|t| t.net_pnl).fold(f64::NEG_INFINITY, f64::max);
        let worst_trade = trades.iter().map(|t| t.net_pnl).fold(f64::INFINITY, f64::min);
        let return_pct = (final_equity - initial_equity) / initial_equity * 100.0;
        let max_drawdown = compute_max_drawdown(trades, initial_equity);
        let sharpe = compute_sharpe(trades);
        let long_trades = trades.iter().filter(|t| t.side == Side::Buy).count();
        let short_trades = trades.iter().filter(|t| t.side == Side::Sell).count();

        Self {
            total_trades: n,
            winners,
            losers,
            win_rate,
            total_pnl,
            avg_pnl,
            best_trade: if n > 0 { best_trade } else { 0.0 },
            worst_trade: if n > 0 { worst_trade } else { 0.0 },
            max_drawdown,
            sharpe,
            initial_equity,
            final_equity,
            return_pct,
            long_trades,
            short_trades,
        }
    }

    pub fn print_summary(&self, symbol: &str) {
        println!();
        println!("╔══════════════════════════════════════╗");
        println!("║  Northflow Research Report — {symbol:<8} ║");
        println!("╠══════════════════════════════════════╣");
        println!("║  Trades     : {:>6}  (L:{} S:{}){}║",
            self.total_trades, self.long_trades, self.short_trades,
            " ".repeat(18usize.saturating_sub(format!("  (L:{} S:{})", self.long_trades, self.short_trades).len())));
        println!("║  Win Rate   : {:>6.1}%                  ║", self.win_rate * 100.0);
        println!("║  Total PnL  : {:>+10.4}              ║", self.total_pnl);
        println!("║  Avg PnL    : {:>+10.4}              ║", self.avg_pnl);
        println!("║  Best Trade : {:>+10.4}              ║", self.best_trade);
        println!("║  Worst Trade: {:>+10.4}              ║", self.worst_trade);
        println!("║  Max Drawdown: {:>5.2}%                 ║", self.max_drawdown * 100.0);
        println!("║  Sharpe     : {:>+10.4}              ║", self.sharpe);
        println!("║  Return     : {:>+7.2}%                ║", self.return_pct);
        println!("║  Final Eq   : {:>10.2}              ║", self.final_equity);
        println!("╚══════════════════════════════════════╝");
    }
}

pub fn write_report_json(report: &RunReport, path: &Path) -> Result<(), String> {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&fmt_field("total_trades", &report.total_trades.to_string(), false));
    out.push_str(&fmt_field("winners", &report.winners.to_string(), false));
    out.push_str(&fmt_field("losers", &report.losers.to_string(), false));
    out.push_str(&fmt_field("win_rate", &format!("{:.6}", report.win_rate), false));
    out.push_str(&fmt_field("total_pnl", &format!("{:.6}", report.total_pnl), false));
    out.push_str(&fmt_field("avg_pnl", &format!("{:.6}", report.avg_pnl), false));
    out.push_str(&fmt_field("best_trade", &format!("{:.6}", report.best_trade), false));
    out.push_str(&fmt_field("worst_trade", &format!("{:.6}", report.worst_trade), false));
    out.push_str(&fmt_field("max_drawdown", &format!("{:.6}", report.max_drawdown), false));
    out.push_str(&fmt_field("sharpe", &format!("{:.6}", report.sharpe), false));
    out.push_str(&fmt_field("initial_equity", &format!("{:.2}", report.initial_equity), false));
    out.push_str(&fmt_field("final_equity", &format!("{:.2}", report.final_equity), false));
    out.push_str(&fmt_field("return_pct", &format!("{:.4}", report.return_pct), false));
    out.push_str(&fmt_field("long_trades", &report.long_trades.to_string(), false));
    out.push_str(&fmt_field("short_trades", &report.short_trades.to_string(), true));
    out.push_str("}\n");
    std::fs::write(path, out).map_err(|e| format!("write report json: {e}"))
}

pub fn write_trades_csv(trades: &[SimTrade], path: &Path) -> Result<(), String> {
    let mut out = String::from("symbol,strategy,side,entry_time,exit_time,entry,exit,qty,net_pnl,exit_reason\n");
    for t in trades {
        out.push_str(&format!(
            "{},{},{},{},{},{:.6},{:.6},{:.8},{:.6},{}\n",
            t.symbol, t.strategy,
            t.side.as_str(),
            t.entry_time, t.exit_time,
            t.entry, t.exit, t.qty,
            t.net_pnl,
            t.exit_reason,
        ));
    }
    std::fs::write(path, out).map_err(|e| format!("write trades csv: {e}"))
}

fn fmt_field(key: &str, val: &str, last: bool) -> String {
    if last {
        format!("  \"{key}\": {val}\n")
    } else {
        format!("  \"{key}\": {val},\n")
    }
}

fn compute_max_drawdown(trades: &[SimTrade], initial_equity: f64) -> f64 {
    let mut equity = initial_equity;
    let mut peak = initial_equity;
    let mut max_dd = 0.0f64;
    for t in trades {
        equity += t.net_pnl;
        if equity > peak { peak = equity; }
        let dd = if peak > 0.0 { (peak - equity) / peak } else { 0.0 };
        if dd > max_dd { max_dd = dd; }
    }
    max_dd
}

fn compute_sharpe(trades: &[SimTrade]) -> f64 {
    if trades.len() < 2 { return 0.0; }
    let returns: Vec<f64> = trades.iter().map(|t| t.net_pnl).collect();
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>()
        / (returns.len() - 1) as f64;
    let std_dev = variance.sqrt();
    if std_dev == 0.0 { return 0.0; }
    mean / std_dev * (252f64.sqrt())
}
