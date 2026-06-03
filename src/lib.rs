//! Northflow crypto trading engine — library root.
//!
//! Phase 1: core domain types — COMPLETE
//! Phase 2: market data loader + timeframe builder — COMPLETE
//! Phase 3: indicators (EMA 8/21/50/200, ATR 14, VWAP, Volume SMA 20) — COMPLETE
//! Phase 4: strategy engine (screened_vwap_scalp) — COMPLETE
//! Phase 5: risk and cost model — COMPLETE
//! Phase 6: backtest engine — COMPLETE

pub mod advisor;
pub mod backtest;
pub mod config;
pub mod core;
pub mod execution;
pub mod indicators;
pub mod journal;
pub mod market;
pub mod report;
pub mod research;
pub mod risk;
pub mod strategy;
