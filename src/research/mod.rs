//! Research orchestrator — Phase 4: strategy engine ready; data summary active.
//!
//! Full backtest loop will be activated in later phases once:
//!   Phase 5: risk + cost model
//!   Phase 6: backtest engine
//!   Phase 7: report writers

use std::path::Path;

use crate::config::ResearchConfig;
use crate::core::Timeframe;
use crate::market::{CandleStore, DataQualityIssueKind, OhlcvLoader};

/// Run Phase 4 research summary.
///
/// Validates config, loads market data, builds candle store, and prints a
/// truthful data + indicator + strategy readiness summary.
/// Does not run a backtest. Does not generate fake results. Does not write reports.
pub fn run_research(cfg: &ResearchConfig) -> Result<(), String> {
    println!("=================================================================");
    println!(" Northflow — Phase 4: Strategy Engine");
    println!("=================================================================");
    println!();

    // Validate explicit timeframe roles.
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
    println!("  paper mode  DISABLED — research engine not yet validated");
    println!("  live mode   DISABLED — research engine not yet validated");
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
    println!("  No orders, no risk sizing, no backtest execution");
    println!();
    println!("Next: Phase 5 — risk and cost model");
    println!();

    Ok(())
}

fn run_symbol(cfg: &ResearchConfig, symbol: &str) {
    let csv_path = Path::new(&cfg.data_dir).join(format!("{symbol}.csv"));

    if !csv_path.exists() {
        println!("No historical CSV found for {symbol}.");
        println!("Expected path: {}", csv_path.display());
        println!("Place a 1m OHLCV CSV file with columns:");
        println!("  timestamp,open,high,low,close,volume");
        println!();
        return;
    }

    let load_result = match OhlcvLoader::load_file(&csv_path) {
        Ok(r) => r,
        Err(e) => {
            println!("Error loading {symbol}: {e}");
            return;
        }
    };

    let quality = &load_result.quality;
    let store = match CandleStore::build_from_1m(load_result.candles) {
        Ok(s) => s,
        Err(e) => {
            println!("Error building candle store for {symbol}: {e}");
            return;
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
    println!("Data quality issues:   {}", quality.error_count());
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
    }

    if !quality.missing_gaps.is_empty() {
        println!();
        println!("  Missing 1m gaps:");
        for gap in &quality.missing_gaps {
            println!(
                "    {} missing candle(s) after ts={}  (expected ts={})",
                gap.missing_count, gap.from_timestamp, gap.expected_next_timestamp
            );
        }
    }

    println!();
}
