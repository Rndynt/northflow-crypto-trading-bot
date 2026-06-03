//! Layer 6 — Learning system.
//!
//! Reads the trade journal and derives:
//!   1. **Performance memory** — aggregate stats (per strategy / regime /
//!      symbol / time-of-day, plus loss-streaks).
//!   2. **Lessons** — actionable rules turned from those stats (cooldowns,
//!      derates, blacklists, calibration adjustments).
//!   3. **Policy** — the runtime object the rest of the engine consults
//!      before every signal: `is this allowed? what size multiplier?`.
//!
//! The policy is refreshed periodically by the orchestrator (default every
//! 5 minutes) so the bot adapts to its own performance over time.

pub mod lessons;
pub mod memory;
pub mod policy;

pub use lessons::{Lesson, LessonKind};
pub use memory::{PerformanceMemory, StrategyStats};
pub use policy::{LearningPolicy, PolicyVerdict};
