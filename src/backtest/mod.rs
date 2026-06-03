//! Backtest engine — Phase 6.
//!
//! Deterministic 1m replay with conservative intrabar fill model.
//! No live trading. No paper trading. No exchange calls.

pub mod engine;
pub mod fill_model;
pub mod metrics;
pub mod report;
pub mod walk_forward;

pub use engine::{BacktestConfig, BacktestEngine, BacktestResult};
pub use fill_model::{EntryFill, ExitFill, FillModel, OpenSimPosition};
pub use metrics::{BacktestSummary, EquityPoint, Metrics};
pub use report::ReportWriter;
pub use walk_forward::{WalkForwardWindow, build_walk_forward_windows};
