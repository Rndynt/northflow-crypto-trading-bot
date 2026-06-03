---
name: Phase 6 Backtest Engine
description: Design decisions for src/backtest/ — fill model, metrics, report, walk-forward, engine, and integration points.
---

## Key decisions

- 6 files: engine.rs, fill_model.rs, metrics.rs, report.rs, walk_forward.rs, mod.rs
- `Trade` gained `regime: String` field; all test helpers must include it
- `ResearchConfig` gained `max_bars_held: u32` (default 60), parsed from `[backtest]` section
- `ScreenedVwapScalp` is `Default` (unit struct) — use `ScreenedVwapScalp::default()`, not `::new()`

## Bugs fixed during implementation

- Parallel test race: report tests all used `format!("/tmp/.../{}",process::id())` → same dir → race.
  **Fix:** each test passes a unique tag string: `temp_dir("json")`, `temp_dir("trades")`, etc.
- `max_drawdown` zero when `trades` is empty: early-return path skipped equity_curve scan.
  **Fix:** compute `max_drawdown` from equity_curve fold even in the zero-trades branch.
- Walk-forward conflict: `total == train + test` produces exactly ONE window (not empty);
  `returns_empty_when_not_enough_data` test must only check strictly-less-than cases.

## Test count milestone

300 tests pass after Phase 6 completion (lib only, 0 ignored, 0 failed).

**Why:** deterministic no-lookahead backtest — 5m candle at 1m index i is only valid if
its timestamp ≤ (one_minute[i].timestamp − 240_000 ms); 15m uses −840_000 ms.
