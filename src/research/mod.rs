//! Research orchestrator — Phase 2/3+ placeholder.
//!
//! Full backtest loop will be activated in Phase 6 once:
//!   Phase 2: market data loader + timeframe builder
//!   Phase 3: indicators (EMA, ATR, VWAP)
//!   Phase 4: screened_vwap_scalp strategy
//!   Phase 5: risk + cost model
//!   Phase 6: backtest engine (SimExecutor)
//!   Phase 7: report writers

use crate::config::ResearchConfig;

/// Phase 1: prints core domain status and Phase 2 next steps.
/// Does not run a backtest, does not generate fake results.
pub fn run_research(cfg: &ResearchConfig) -> Result<(), String> {
    println!("=================================================================");
    println!(" Northflow — Phase 1: Core Domain Foundation  [COMPLETE]");
    println!("=================================================================");
    println!();
    println!("  Core types       ready");
    println!("  signal_id chain  SIG-… → ORD-… → FILL-… → POS-… → TRD-…");
    println!("  Timeframe model  explicit roles (never inferred from order)");
    println!();
    println!("  entry_timeframe        = \"{}\"  (1m  → entry & execution)", cfg.entry_timeframe);
    println!("  screening_timeframe    = \"{}\" (15m → regime bias)", cfg.screening_timeframe);
    println!("  confirmation_timeframe = \"{}\"  (5m  → confirmation)", cfg.confirmation_timeframe);
    println!();
    println!("  Symbols:   {:?}", cfg.symbols);
    println!();
    println!("  paper mode  DISABLED — research engine not yet validated");
    println!("  live mode   DISABLED — research engine not yet validated");
    println!();
    println!("  Next: Phase 2 — market data loader + timeframe builder");
    println!("        cargo run -- research --config config/research.toml");
    println!("=================================================================");
    Ok(())
}
