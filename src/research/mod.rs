use crate::config::ResearchConfig;
use crate::core::SimTrade;
use crate::data::load_ohlcv_csv;
use crate::execution::SimExecutor;
use crate::report::{write_report_json, write_trades_csv, RunReport};
use crate::risk::{RiskManager, RiskParams};
use crate::strategy::{ScreenedVwapScalp, StrategyParams};
use std::path::Path;

pub fn run_research(cfg: &ResearchConfig) -> Result<(), String> {
    std::fs::create_dir_all(&cfg.reports_dir)
        .map_err(|e| format!("cannot create reports dir: {e}"))?;

    for symbol in &cfg.symbols {
        run_symbol(cfg, symbol)?;
    }
    Ok(())
}

fn run_symbol(cfg: &ResearchConfig, symbol: &str) -> Result<(), String> {
    let csv_path = Path::new(&cfg.data_dir).join(format!("{symbol}.csv"));
    eprintln!("[research] loading {}", csv_path.display());
    let candles = load_ohlcv_csv(&csv_path)?;
    if candles.is_empty() {
        return Err(format!("no valid candles in {}", csv_path.display()));
    }
    eprintln!("[research] {symbol}: {} candles loaded", candles.len());

    let strategy_params = StrategyParams {
        symbol: symbol.to_string(),
        ..StrategyParams::default()
    };
    let risk_params = RiskParams {
        initial_equity: cfg.initial_equity,
        risk_per_trade_pct: cfg.risk_per_trade_pct,
        max_open_positions: cfg.max_open_positions,
        max_leverage: cfg.max_leverage,
        min_reward_risk: cfg.min_reward_risk,
        taker_fee_bps: cfg.taker_fee_bps,
        slippage_bps: cfg.slippage_bps,
        spread_bps: cfg.spread_bps,
        market_impact_bps: cfg.market_impact_bps,
        ..RiskParams::default()
    };

    let mut strategy = ScreenedVwapScalp::new(strategy_params);
    let mut executor = SimExecutor::new(RiskManager::new(risk_params), cfg.conservative_intrabar);

    for &candle in &candles {
        let signal = strategy.next(candle);
        executor.feed(candle, signal);
    }
    if let Some(&last) = candles.last() {
        executor.force_close_all(last);
    }

    let trades = executor.trades;
    let final_equity = executor.risk.equity;
    let initial_equity = executor.risk.params.initial_equity;

    eprintln!(
        "[research] {symbol}: {} trades | final equity {:.2} | return {:.2}%",
        trades.len(),
        final_equity,
        (final_equity - initial_equity) / initial_equity * 100.0
    );

    save_results(cfg, symbol, &trades, initial_equity, final_equity)
}

fn save_results(
    cfg: &ResearchConfig,
    symbol: &str,
    trades: &[SimTrade],
    initial_equity: f64,
    final_equity: f64,
) -> Result<(), String> {
    let report = RunReport::build(trades, initial_equity, final_equity);

    let json_path = Path::new(&cfg.reports_dir).join(format!("{symbol}_report.json"));
    write_report_json(&report, &json_path)?;
    eprintln!("[research] report → {}", json_path.display());

    let csv_path = Path::new(&cfg.reports_dir).join(format!("{symbol}_trades.csv"));
    write_trades_csv(trades, &csv_path)?;
    eprintln!("[research] trades → {}", csv_path.display());

    Ok(())
}
