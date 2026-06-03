//! Northflow crypto trading engine — library root.
//!
//! Phase 1: core domain types — COMPLETE
//! Phase 2: market data loader + timeframe builder — COMPLETE
//! Phase 3+: indicators, strategy, risk, backtest, reports — pending

pub mod advisor;
pub mod config;
pub mod core;
pub mod data;
pub mod execution;
pub mod indicators;
pub mod journal;
pub mod market;
pub mod report;
pub mod research;
pub mod risk;
pub mod strategy;
