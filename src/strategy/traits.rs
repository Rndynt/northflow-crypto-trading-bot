//! Strategy trait — deterministic, single-candle, multi-timeframe signal evaluation.
//!
//! Rules:
//!   - Strategies may only read candles and indicator snapshots.
//!   - Strategies must not call exchange APIs, LLMs, or mutate account state.
//!   - Output is Result<Option<Signal>, NorthflowError> — a Signal, not an order.

use crate::core::{Candle, NorthflowError, Signal, Symbol, Timeframe};
use crate::indicators::IndicatorSnapshot;

// ── StrategyContext ───────────────────────────────────────────────────────────

/// Caller-supplied context for a single strategy evaluation.
///
/// Does not include candles or snapshots — those travel in [`MultiTimeframeInput`].
#[derive(Debug, Clone)]
pub struct StrategyContext {
    /// Symbol being evaluated.
    pub symbol: Symbol,
    /// Monotonically increasing index for deterministic signal ID generation.
    pub signal_index: u64,
    /// Estimated round-trip cost in basis points (taker fee + slippage + spread).
    pub estimated_cost_bps: f64,
    /// Minimum confidence score required to emit a signal (0–100).
    pub min_confidence: u8,
    /// Entry timeframe for this run (e.g. 1m, 5m).
    pub entry_timeframe: Timeframe,
    /// Confirmation timeframe for this run (e.g. 5m, 1h).
    pub confirmation_timeframe: Timeframe,
    /// Screening timeframe for this run (e.g. 15m, 4h).
    pub screening_timeframe: Timeframe,
}

// ── MultiTimeframeInput ───────────────────────────────────────────────────────

/// The complete, role-labelled multi-timeframe input for one evaluation.
///
/// Timeframe roles are explicit — never inferred from array order.
/// The actual timeframe values are configured at run time via ResearchConfig.
#[derive(Debug, Clone)]
pub struct MultiTimeframeInput {
    /// Entry-timeframe candle at the evaluation moment.
    pub entry_candle: Candle,
    /// Confirmation-timeframe candle at the evaluation moment.
    pub confirmation_candle: Candle,
    /// Screening-timeframe candle at the evaluation moment.
    pub screening_candle: Candle,
    /// Indicator snapshot computed from the entry-timeframe candle stream.
    pub entry_indicators: IndicatorSnapshot,
    /// Indicator snapshot computed from the confirmation-timeframe candle stream.
    pub confirmation_indicators: IndicatorSnapshot,
    /// Indicator snapshot computed from the screening-timeframe candle stream.
    pub screening_indicators: IndicatorSnapshot,
}

// ── Strategy trait ────────────────────────────────────────────────────────────

/// A deterministic, stateless signal generator.
///
/// Implementations must:
///   - Be deterministic: same inputs → same output.
///   - Never call external APIs.
///   - Never mutate account state.
///   - Never place orders.
///   - Emit only `Option<Signal>`, never an order or fill.
pub trait Strategy {
    /// Stable, unique identifier for this strategy (e.g. `"screened_vwap_scalp"`).
    fn strategy_id(&self) -> &'static str;

    /// Evaluate one multi-timeframe input and optionally emit a signal.
    ///
    /// Returns:
    ///   - `Ok(None)`       — conditions not met; no signal.
    ///   - `Ok(Some(sig))`  — all conditions met; signal ready for risk review.
    ///   - `Err(e)`         — invalid input or internal error.
    fn evaluate(
        &self,
        ctx: &StrategyContext,
        input: &MultiTimeframeInput,
    ) -> Result<Option<Signal>, NorthflowError>;
}
