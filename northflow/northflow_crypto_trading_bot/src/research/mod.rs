use crate::config::ResearchConfig;
use crate::core::Trade;
use crate::data::load_csv_ohlcv;
use crate::indicators::atr;
use crate::report::RunReport;
use crate::sim::run_simulation;
use crate::strategy::{ema_crossover, EmaCrossoverParams};
use anyhow::Result;

pub struct BacktestResult {
    pub trades: Vec<Trade>,
    pub final_capital: f64,
    pub initial_capital: f64,
    pub report: RunReport,
}

pub fn run_backtest(cfg: &ResearchConfig) -> Result<BacktestResult> {
    log::info!("Loading OHLCV data from {}", cfg.data_path);
    let candles = load_csv_ohlcv(&cfg.data_path)?;

    log::info!("Loaded {} candles for {}", candles.len(), cfg.symbol);

    let params = EmaCrossoverParams {
        fast_period: cfg.strategy.ema_fast,
        slow_period: cfg.strategy.ema_slow,
        atr_period: cfg.strategy.atr_period,
        vwap_period: cfg.strategy.vwap_period,
    };

    log::info!("Running strategy: {}", cfg.strategy.name);
    let strategy_output = ema_crossover(&candles, &params);
    let atr_vals = atr(&candles, cfg.strategy.atr_period);

    let sim = run_simulation(
        &candles,
        &strategy_output.signals,
        &atr_vals,
        cfg,
        &cfg.symbol,
    );

    let report = RunReport::from_trades(&sim.trades, cfg.initial_capital, sim.capital);
    log::info!(
        "Backtest complete — {} trades, PnL: {:.2}, Win rate: {:.1}%",
        report.total_trades,
        report.total_pnl,
        report.win_rate * 100.0
    );

    Ok(BacktestResult {
        trades: sim.trades,
        final_capital: sim.capital,
        initial_capital: cfg.initial_capital,
        report,
    })
}
