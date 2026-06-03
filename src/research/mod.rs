//! Research orchestrator — Phase 6: Backtest Engine.
//!
//! Runs the deterministic backtest for each configured symbol and writes:
//!   reports/backtest_summary.json
//!   reports/trades.csv
//!   reports/equity_curve.csv
//!
//! Paper and live modes remain disabled.

use std::path::Path;

use crate::backtest::{BacktestEngine, ReportWriter};
use crate::config::ResearchConfig;
use crate::core::Timeframe;
use crate::market::{DataQualityIssueKind, OhlcvLoader};

/// Run Phase 6 research: deterministic backtest + report generation.
///
/// Validates config, loads market data, runs the backtest engine, prints a
/// truthful summary, and writes report files.  Does not claim the strategy is
/// profitable.  Does not give trading advice.
pub fn run_research(cfg: &ResearchConfig) -> Result<(), String> {
    println!("=================================================================");
    println!(" Northflow — Phase 6: Backtest Engine");
    println!("=================================================================");
    println!();

    cfg.validate_timeframes().map_err(|e| format!("{e}"))?;

    println!("  Timeframe model:");
    println!(
        "    entry_timeframe        = \"{}\"  (1m  → entry & execution)",
        cfg.entry_timeframe
    );
    println!(
        "    screening_timeframe    = \"{}\" (15m → regime bias)",
        cfg.screening_timeframe
    );
    println!(
        "    confirmation_timeframe = \"{}\"  (5m  → confirmation)",
        cfg.confirmation_timeframe
    );
    println!();
    println!("  paper mode  DISABLED — research engine not yet validated for paper");
    println!("  live mode   DISABLED — paper/live parity not yet proven");
    println!();
    println!("  Note: backtest results are historical simulation only.");
    println!("        Do not use as financial advice or profitability claims.");
    println!();

    for symbol in &cfg.symbols {
        run_symbol(cfg, symbol);
    }

    println!("Indicators ready:");
    println!("  EMA 8 / 21 / 50 / 200");
    println!("  ATR 14 (Wilder smoothing)");
    println!("  VWAP (session-cumulative)");
    println!("  Volume SMA 20");
    println!();
    println!("Strategy engine ready:");
    println!("  screened_vwap_scalp");
    println!("  Output: Signal only");
    println!();
    println!("Risk model ready:");
    println!("  position sizing");
    println!("  cost model");
    println!("  risk guards");
    println!("  Output: RiskAssessment only");
    println!();
    println!("Backtest engine ready:");
    println!("  conservative intrabar fill model");
    println!("  no lookahead across 5m / 15m candles");
    println!("  deterministic signal IDs (SIG-BT-XXXXXXXX)");
    println!();

    Ok(())
}

fn run_symbol(cfg: &ResearchConfig, symbol: &str) {
    let csv_path = Path::new(&cfg.data_dir).join(format!("{symbol}.csv"));

    if !csv_path.exists() {
        println!("Symbol: {symbol}");
        println!("  No historical CSV found.");
        println!("  Expected path: {}", csv_path.display());
        println!("  Place a 1m OHLCV CSV file with columns:");
        println!("    timestamp,open,high,low,close,volume");
        println!();
        return;
    }

    // Print data quality summary (mirrors Phase 5 output).
    let data_quality_ok = print_data_quality(cfg, symbol, &csv_path);
    if !data_quality_ok {
        println!("  Skipping backtest — fix data quality errors first.");
        println!();
        return;
    }

    // Run backtest.
    match BacktestEngine::run(cfg, symbol) {
        Err(e) => {
            println!("  Backtest error: {e}");
            println!();
        }
        Ok(None) => {
            println!("  No data returned from backtest.");
            println!();
        }
        Ok(Some(result)) => {
            let s = &result.summary;
            println!("  Backtest complete:");
            println!("    Total trades:           {}", s.total_trades);
            println!("    Win rate:               {:.2}%", s.win_rate);
            println!("    Net PnL:                {:.2}", s.net_pnl);
            println!("    Gross PnL:              {:.2}", s.gross_pnl);
            println!("    Total fees:             {:.2}", s.total_fee);
            println!("    Total slippage:         {:.2}", s.total_slippage);
            let pf_str = if result.summary.profit_factor.is_infinite() {
                "inf".to_string()
            } else {
                format!("{:.4}", result.summary.profit_factor)
            };
            println!("    Profit factor:          {pf_str}");
            println!("    Max drawdown:           {:.2}%", s.max_drawdown);
            println!("    Max consecutive losses: {}", s.max_consecutive_losses);
            println!();

            // Write report files.
            match ReportWriter::write_all(
                &cfg.reports_dir,
                &result.summary,
                &result.trades,
                &result.equity_curve,
            ) {
                Ok(()) => {
                    println!("  Reports written:");
                    println!("    {}/backtest_summary.json", cfg.reports_dir);
                    println!("    {}/trades.csv", cfg.reports_dir);
                    println!("    {}/equity_curve.csv", cfg.reports_dir);
                }
                Err(e) => {
                    println!("  Warning: could not write reports: {e}");
                }
            }
            println!();
        }
    }
}

/// Print data quality for the symbol.  Returns `true` if no errors.
fn print_data_quality(_cfg: &ResearchConfig, symbol: &str, csv_path: &Path) -> bool {
    use crate::market::CandleStore;

    let load_result = match OhlcvLoader::load_file(csv_path) {
        Ok(r) => r,
        Err(e) => {
            println!("  Error loading {symbol}: {e}");
            return false;
        }
    };

    let quality = &load_result.quality;
    let store = match CandleStore::build_from_1m(load_result.candles) {
        Ok(s) => s,
        Err(e) => {
            println!("  Error building candle store: {e}");
            return false;
        }
    };

    let dup_count = quality
        .issues
        .iter()
        .filter(|i| i.kind == DataQualityIssueKind::DuplicateTimestamp)
        .count();

    println!("Symbol:                {symbol}");
    println!("Source:                {}", csv_path.display());
    println!("1m candles:            {}", store.len(Timeframe::OneMinute));
    println!(
        "5m candles:            {}",
        store.len(Timeframe::FiveMinute)
    );
    println!(
        "15m candles:           {}",
        store.len(Timeframe::FifteenMinute)
    );
    println!("Data quality errors:   {}", quality.error_count());
    println!("Duplicate timestamps:  {dup_count}");
    println!("Missing gaps:          {}", quality.missing_gaps.len());

    if quality.error_count() > 0 {
        println!();
        println!("  Data quality errors:");
        for issue in quality.issues.iter().filter(|i| i.kind.is_error()) {
            match issue.row {
                Some(row) => println!("    [{}] row {row}: {}", issue.kind, issue.message),
                None => println!("    [{}] {}", issue.kind, issue.message),
            }
        }
        return false;
    }

    if !quality.missing_gaps.is_empty() {
        println!();
        println!("  Missing 1m gaps (warnings):");
        for gap in &quality.missing_gaps {
            println!(
                "    {} missing candle(s) after ts={}  (expected ts={})",
                gap.missing_count, gap.from_timestamp, gap.expected_next_timestamp
            );
        }
    }

    println!();
    true
}
