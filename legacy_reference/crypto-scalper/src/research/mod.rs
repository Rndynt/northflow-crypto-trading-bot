pub mod decay;
pub mod export;
pub mod ic;
pub mod report;
pub mod sensitivity;
pub mod significance;
pub mod walk_forward;

pub use decay::{SignalObservation, compute_ic_decay};
pub use export::{ResearchReport, reports_to_json, reports_to_markdown};
pub use ic::IcTracker;
pub use report::{StrategyHealth, StrategyResearchSummary};
pub use sensitivity::{ParameterPoint, SensitivitySummary, summarize_parameter_sensitivity};
pub use significance::{permutation_p_value, win_rate_significance};
pub use walk_forward::{
    WalkForwardResult, WalkForwardSplit, WalkForwardWindow, evaluate_walk_forward,
    walk_forward_splits,
};
