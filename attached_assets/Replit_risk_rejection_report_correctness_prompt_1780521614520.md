# Northflow Risk Rejection Report Correctness Patch Prompt

You are working on this repository:

https://github.com/Rndynt/northflow-crypto-trading-bot

Your task is to perform a small correctness and documentation patch after the risk attribution / actual-entry re-risking patch.

Do not implement a new phase.

Do not tune the strategy.

Do not change indicator formulas.

Do not change risk formulas.

Do not implement paper trading.

Do not implement live trading.

Do not implement exchange APIs.

Do not implement dashboard, Telegram, LLM decisions, or AI advisor.

This patch is only for report correctness, risk rejection traceability, and documentation.

## Current observed result

After running:

```bash
cargo run --release -- research --config config/research.toml
```

on BTCUSDT 1m 2024 data, the system prints:

```text
Backtest complete: 0 trades, final equity 5000.00

Signal flow:
  signals generated:          69061
  signals preapproved:        8237
  rejected initial risk:      60824
  rejected actual entry:      8237
  trades opened:              0
  trades closed:              0
  risk rejection rows:        71647
    max_drawdown:             0
    daily_loss:               0
    reward_risk:              8237
    expected_net_edge:        63410
    other:                    0

Audit report:
  passed:   true
  errors:   0
  warnings: 0
```

This is useful and confirms the actual-entry re-risking patch is working.

However, two report correctness issues remain:

1. `risk_rejections.csv` does not include a `stage` column, so it is harder to distinguish `initial_risk` rejections from `actual_entry` rejections.
2. For normal `RiskEngine::assess()` rejections, `risk_rejections.csv` may still write expected cost and net edge values from the `Signal` fields instead of the final `RiskAssessment` fields. This can make rows look inconsistent, for example reason `expected_net_edge_not_positive` while the CSV field appears positive.

Documentation also needs to mention the new files:

- `reports/risk_rejections.csv`
- `reports/signal_flow_summary.json`

## Files to read first

Read these files before changing anything:

- AGENTS.md
- docs/ROADMAP.md
- README.md
- docs/DATA_DOWNLOAD.md
- src/backtest/engine.rs
- src/backtest/risk_trace.rs
- src/backtest/report.rs
- src/backtest/mod.rs
- src/research/mod.rs
- src/report/manifest.rs
- src/risk/guard.rs
- src/core/signal.rs
- src/core/trade.rs

## Required fix 1 - Add stage to RiskRejection

Update `src/backtest/risk_trace.rs`.

Add a new field to `RiskRejection`:

```rust
pub stage: String,
```

Use stable stage values:

```text
initial_risk
actual_entry
```

Meaning:

- `initial_risk`: signal was rejected during the first risk assessment at signal candle close.
- `actual_entry`: signal was preapproved initially, then rejected when re-risked using actual next-candle open entry price.

Update all constructors, helper functions, and tests that build `RiskRejection`.

## Required fix 2 - Update build_rejection helper

In `src/backtest/engine.rs`, update `build_rejection()` to accept a `stage` argument:

```rust
fn build_rejection(
    signal: &Signal,
    stage: &str,
    timestamp: i64,
    equity: f64,
    peak_equity: f64,
    daily_realized_pnl: f64,
    reason: &str,
    expected_reward_bps: f64,
    expected_cost_bps: f64,
    expected_net_edge_bps: f64,
) -> RiskRejection
```

Set:

```rust
stage: stage.to_string()
```

Every call site must pass either:

```rust
"initial_risk"
```

or:

```rust
"actual_entry"
```

## Required fix 3 - Use RiskAssessment fields for normal rejections

This is the most important correctness fix.

For every normal rejection from:

```rust
Ok(assessment) if !assessment.approved => { ... }
```

when pushing `RiskRejection`, use values from `assessment`, not stale values from the `Signal`.

Use:

```rust
assessment.expected_reward_bps
assessment.expected_cost_bps
assessment.expected_net_edge_bps
```

This applies to both:

1. Initial risk rejection.
2. Actual-entry re-risk rejection.

Do not use these for invalid geometry soft rejection, because there is no `RiskAssessment` in that path. For invalid geometry, keep using adjusted signal values.

### Initial rejection behavior

Current style may look like this:

```rust
risk_rejections.push(build_rejection(
    &signal,
    candle.timestamp,
    equity,
    peak_equity,
    daily_realized_pnl,
    reason,
    signal.expected_reward_bps,
    signal.estimated_cost_bps,
    signal.expected_net_edge_bps,
));
```

Change it to:

```rust
risk_rejections.push(build_rejection(
    &signal,
    "initial_risk",
    candle.timestamp,
    equity,
    peak_equity,
    daily_realized_pnl,
    reason,
    assessment.expected_reward_bps,
    assessment.expected_cost_bps,
    assessment.expected_net_edge_bps,
));
```

### Actual-entry normal rejection behavior

Current style may look like this:

```rust
risk_rejections.push(build_rejection(
    &adjusted,
    candle.timestamp,
    equity,
    peak_equity,
    daily_realized_pnl,
    reason,
    adjusted.expected_reward_bps,
    adjusted.estimated_cost_bps,
    adjusted.expected_net_edge_bps,
));
```

Change it to:

```rust
risk_rejections.push(build_rejection(
    &adjusted,
    "actual_entry",
    candle.timestamp,
    equity,
    peak_equity,
    daily_realized_pnl,
    reason,
    assessment.expected_reward_bps,
    assessment.expected_cost_bps,
    assessment.expected_net_edge_bps,
));
```

### Actual-entry invalid geometry behavior

For invalid geometry, use:

```rust
build_rejection(
    &adjusted,
    "actual_entry",
    candle.timestamp,
    equity,
    peak_equity,
    daily_realized_pnl,
    "actual_entry_invalid_geometry",
    adjusted.expected_reward_bps,
    adjusted.estimated_cost_bps,
    adjusted.expected_net_edge_bps,
)
```

### Actual-entry invalid signal behavior

For `Err(NorthflowError::InvalidSignal(_))`, use:

```rust
build_rejection(
    &adjusted,
    "actual_entry",
    candle.timestamp,
    equity,
    peak_equity,
    daily_realized_pnl,
    "actual_entry_risk_error",
    adjusted.expected_reward_bps,
    adjusted.estimated_cost_bps,
    adjusted.expected_net_edge_bps,
)
```

Do not swallow config errors.

Config/cost/context errors must still return `Err(e)`.

## Required fix 4 - Update risk_rejections.csv

Update `src/backtest/report.rs`.

Change risk rejections CSV header from:

```csv
signal_id,timestamp,side,regime,reason,equity,peak_equity,drawdown_pct,daily_realized_pnl,expected_reward_bps,expected_cost_bps,expected_net_edge_bps
```

to:

```csv
signal_id,stage,timestamp,side,regime,reason,equity,peak_equity,drawdown_pct,daily_realized_pnl,expected_reward_bps,expected_cost_bps,expected_net_edge_bps
```

Write `r.stage` after `signal_id`.

Rules:

- Keep header even if no rejections.
- CSV escaping must apply to `stage` too.
- Keep deterministic row order.
- Do not change existing report filenames.

## Required fix 5 - Update signal_flow_summary if needed

No schema change is required for `signal_flow_summary.json`.

But ensure counts still mean:

- `signals_rejected_initial_risk` counts rejected signals at initial risk stage.
- `signals_rejected_actual_entry` counts rejected signals at actual-entry stage.
- `risk_rejections` counts rejection rows, not rejected signals.
- Reason counts count rejection rows by reason.

Do not change current JSON field names.

## Required fix 6 - Update manifest row count

`src/report/manifest.rs` already includes:

```text
reports/risk_rejections.csv
reports/signal_flow_summary.json
```

Keep that behavior.

No schema change is needed.

Ensure tests still pass after `RiskRejection` gains `stage`.

## Required fix 7 - Update README

Update README.md.

Do not rewrite the whole file.

In the report files table, add these rows:

```markdown
| `reports/risk_rejections.csv` | Every rejected signal reason with stage, equity, drawdown, and expected edge/cost context |
| `reports/signal_flow_summary.json` | Signal funnel counts: generated, preapproved, rejected, opened, and closed |
```

Add a short subsection under Phase 7, or near report files:

```markdown
### Risk rejection attribution

`risk_rejections.csv` explains why signals did not become trades.

The `stage` column can be:

- `initial_risk` — rejected at signal close before a pending entry is created.
- `actual_entry` — initially approved, then rejected after the engine recalculates risk using actual next-candle open entry price.

Normal RiskEngine rejections use `RiskAssessment.expected_reward_bps`, `RiskAssessment.expected_cost_bps`, and `RiskAssessment.expected_net_edge_bps` so the rejection reason and edge fields match.

`signal_flow_summary.json` summarizes the funnel:

signal generated -> preapproved -> rejected at initial risk / rejected at actual entry -> trade opened -> trade closed
```

Also add one sentence:

```markdown
`trades.csv` reward_risk is the effective reward/risk at the simulated entry fill price.
```

Do not remove existing content.

## Required fix 8 - Update docs/DATA_DOWNLOAD.md

Update expected report files list to include:

```text
reports/risk_rejections.csv
reports/signal_flow_summary.json
```

Add a short note:

```markdown
For large datasets, if the strategy opens no trades, inspect:

- `reports/signal_flow_summary.json`
- `reports/risk_rejections.csv`

These files show whether signals were rejected by initial risk checks or actual-entry re-risking.
```

Do not rewrite the whole file.

## Required tests

Add or update tests.

### RiskRejection model / report tests

- risk_rejection_has_stage
- writes_risk_rejections_csv_with_stage_header
- writes_risk_rejections_csv_stage_value
- writes_empty_risk_rejections_csv_with_stage_header
- risk_rejections_csv_escapes_stage

### Engine behavior tests

Add or update tests to verify:

- initial_risk_rejection_uses_assessment_cost_fields
- actual_entry_risk_rejection_uses_assessment_cost_fields
- initial_risk_rejection_stage_is_initial_risk
- actual_entry_rejection_stage_is_actual_entry

These tests do not need huge CSV fixtures. Prefer unit-testing helper behavior if possible.

### Signal flow tests

Ensure existing tests still verify:

- generated / preapproved / rejected / opened / closed counts.
- reason counts are stable.

### Documentation tests

No documentation tests required.

## Required commands

Run:

```bash
cargo fmt
cargo build
cargo test
cargo run -- help
```

If `data/historical/BTCUSDT.csv` exists, also run:

```bash
cargo run --release -- research --config config/research.toml
```

Expected behavior after real BTCUSDT run:

- reports are generated successfully.
- `reports/risk_rejections.csv` header includes `stage`.
- rows contain either `initial_risk` or `actual_entry`.
- `expected_net_edge_not_positive` rows should have `expected_net_edge_bps <= 0` when produced by normal RiskEngine rejection.
- `signal_flow_summary.json` still exists.
- audit remains passed.
- no paper/live/exchange/LLM behavior is added.

## Strictly forbidden

Do not implement:

- strategy tuning
- new indicators
- new strategy rules
- parameter optimization
- entry_geometry_mode
- reanchored SL/TP
- paper trading
- live trading
- exchange adapter
- websocket
- database
- dashboard
- Telegram
- LLM trading decisions
- AI advisor

This patch is only for report correctness and documentation.

## Expected final result

At the end of this patch:

- `risk_rejections.csv` has a `stage` column.
- Normal RiskEngine rejections use `RiskAssessment` edge/cost fields.
- Rejection rows are easier to analyze by stage.
- README documents risk rejection attribution.
- DATA_DOWNLOAD docs list the new report files.
- Existing Phase 1-7 behavior remains intact.
- All tests pass.

## Commit message suggestion

reports: clarify risk rejection stages
