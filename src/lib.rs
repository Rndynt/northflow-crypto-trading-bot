//! Northflow crypto trading engine — library root.
//!
//! Phase 1: core domain types are complete.
//! Phase 2+: market data, indicators, strategy, risk, backtest, reports.

pub mod advisor;
pub mod config;
pub mod core;
pub mod data;
pub mod execution;
pub mod indicators;
pub mod journal;
pub mod report;
pub mod research;
pub mod risk;
pub mod strategy;
