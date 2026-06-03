//! Backtest engine — Phase 6 + risk attribution patch + entry geometry mode patch.
//!
//! Deterministic 1m replay with conservative intrabar fill model.
//! No live trading. No paper trading. No exchange calls.

pub mod engine;
pub mod fill_model;
pub mod geometry;
pub mod metrics;
pub mod report;
pub mod risk_trace;
pub mod walk_forward;

pub use engine::{BacktestConfig, BacktestEngine, BacktestResult};
pub use fill_model::{EntryFill, ExitFill, FillModel, OpenSimPosition};
pub use geometry::EntryGeometryMode;
pub use metrics::{BacktestSummary, EquityPoint, Metrics};
pub use report::ReportWriter;
pub use risk_trace::{RiskRejection, SignalFlowSummary};
pub use walk_forward::{WalkForwardWindow, build_walk_forward_windows};
