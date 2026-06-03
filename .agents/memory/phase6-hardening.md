---
name: Phase 6 Hardening
description: Correctness fixes applied to the Phase 6 backtest engine — risk error propagation, same-candle exit after entry, and associated tests.
---

## Rules

### Risk error propagation
`RiskEngine::assess()` returns two distinct failure modes:
- `Ok(assessment { approved: false })` — normal rejection, skip the signal silently.
- `Err(e)` — invalid input or config, must stop the backtest with `return Err(e)`.

The original engine swallowed `Err(_)` with an empty arm. Fixed via `try_assess_risk()` private helper that maps `Err(e) => Err(e)` and `Ok(rejected) => Ok(None)`.

**Why:** Silent risk errors hide misconfigured costs, zero equity, or invalid signals — all of which indicate a broken backtest, not just a rejected trade.

**How to apply:** Any call to `RiskEngine::assess()` inside the engine loop must go through `try_assess_risk()` and propagate with `?`.

### Same-candle exit after entry
After a pending entry is filled at a candle's open, SL/TP checks must run on that same candle.

Fix: replaced `continue` after pending entry with an `entered_this_bar: bool` flag. Section B (exit checks) always runs. Section C (strategy evaluation) is gated on `!entered_this_bar`.

**Why:** Entering at the open and skipping intrabar high/low is unrealistic. The conservative SL-first rule (intrabar both touched → SL wins) still applies.

**How to apply:** The loop order is always A (entry) → B (exit check, including entry candle) → C (strategy, skipped on entry candle).

### test helpers
- `try_assess_risk(risk_cfg, cost_cfg, risk_ctx, signal)` — private fn in engine.rs, unit-testable without CSV.
- Fill model tests at `bars_held=0` prove `check_exit` works on the entry candle directly.
- `engine_does_not_skip_exit_check_on_entry_candle` directly simulates the A→B flow without CSV.
