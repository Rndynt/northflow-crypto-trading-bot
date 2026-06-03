//! Risk and cost model — Phase 5.
//!
//! Modules:
//!   position_sizing — equity-based position sizing (risk_pct / stop_distance)
//!   cost_model      — round-trip cost estimation (fee + slippage + spread + impact)
//!   guard           — signal validation against risk guards; produces RiskAssessment

pub mod cost_model;
pub mod guard;
pub mod position_sizing;

pub use cost_model::{CostBreakdown, CostModel, CostModelConfig, CostModelInput};
pub use guard::{RiskAssessment, RiskConfig, RiskContext, RiskEngine};
pub use position_sizing::{PositionSize, PositionSizer, PositionSizingConfig, PositionSizingInput};
