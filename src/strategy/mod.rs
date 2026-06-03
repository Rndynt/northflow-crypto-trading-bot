//! Strategy engine — Phase 4 placeholder.
//!
//! Will implement:
//!   - StrategyTrait for signal emission
//!   - screened_vwap_scalp: EMA crossover + VWAP filter
//!
//! Rules:
//!   - Strategies may only emit Signals (see core::Signal).
//!   - Strategies must NOT place orders, call exchanges, or mutate account state.
//!   - Only one strategy active in Phase 4: screened_vwap_scalp.
