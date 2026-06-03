//! Backtest execution engine — Phase 6 placeholder.
//!
//! Will implement:
//!   - Intrabar stop-loss / take-profit simulation
//!   - Conservative fill model (worst-case price within the bar)
//!   - If both SL and TP touched in the same candle, assume SL first
//!   - Fill → Position → Trade conversion with full signal_id chain
