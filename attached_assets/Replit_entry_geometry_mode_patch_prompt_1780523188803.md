# Northflow Entry Geometry Mode Patch Prompt

You are working on this repository:

https://github.com/Rndynt/northflow-crypto-trading-bot

Your task is to implement a focused research-core patch after Phase 7 and after the risk rejection attribution patch.

Do not implement a new phase.

Do not tune the strategy.

Do not change indicator formulas.

Do not change the screened_vwap_scalp strategy rules.

Do not optimize parameters.

Do not implement paper trading.

Do not implement live trading.

Do not implement exchange APIs.

Do not implement dashboard, Telegram, LLM decisions, or AI advisor.

This patch is only for making entry geometry explicit and comparable in backtests.

## Current observed result

After running:

```bash
cargo run --release -- research --config config/research.toml
```

on BTCUSDT 1m 2024 data, the system prints:

```json
{
  "signals_generated": 69061,
  "signals_preapproved": 8237,
  "signals_rejected_initial_risk": 60824,
  "signals_rejected_actual_entry": 8237,
  "trades_opened": 0,
  "trades_closed": 0,
  "risk_rejections": 71647,
  "rejections_max_drawdown": 0,
  "rejections_daily_loss": 0,
  "rejections_reward_risk": 8237,
  "rejections_expected_net_edge": 63410,
  "rejections_other": 0
}
```

Interpretation:

- The engine generated 69,061 strategy signals.
- 60,824 were rejected at initial risk.
- 8,237 were preapproved at signal close.
- All 8,237 failed at actual-entry re-risk.
- No trades were opened.

This is not an engine failure. It exposes an execution geometry issue:

- Strategy creates signal geometry from signal candle close.
- Backtest enters at next 1m candle open with adverse slippage.
- Current behavior preserves the original absolute SL/TP levels from signal close.
- After actual entry moves, effective reward/risk often falls below minimum.
- Result: all preapproved signals are rejected at actual-entry stage.

We need to make this behavior configurable and measurable.

## Goal

Add explicit config:

```toml
[backtest]
entry_geometry_mode = "preserve_signal_levels"
```

Supported values:

```text
preserve_signal_levels
reanchor_to_actual_entry
```

Meaning:

### preserve_signal_levels

Current strict behavior.

- Signal entry, stop_loss, take_profit are computed at signal candle close.
- Actual fill happens at next candle open with adverse slippage.
- SL/TP remain the original absolute levels.
- Effective reward/risk can degrade when actual entry price moves.
- This is useful for strict realism when SL/TP are decided before fill and are not adjusted.

### reanchor_to_actual_entry

Alternative execution model.

- Signal still comes from signal candle close.
- Actual fill still happens at next candle open with adverse slippage.
- After actual fill price is known, SL/TP are re-anchored around actual entry using the original risk distance and original reward/risk ratio.
- This simulates a bracket order placed after the market entry fill is known.

For long:

```text
original_risk = signal.entry_price - signal.stop_loss
original_reward_risk = (signal.take_profit - signal.entry_price) / original_risk

actual_entry = next_open * (1 + slippage_bps / 10000)

new_stop_loss   = actual_entry - original_risk
new_take_profit = actual_entry + original_risk * original_reward_risk
```

For short:

```text
original_risk = signal.stop_loss - signal.entry_price
original_reward_risk = (signal.entry_price - signal.take_profit) / original_risk

actual_entry = next_open * (1 - slippage_bps / 10000)

new_stop_loss   = actual_entry + original_risk
new_take_profit = actual_entry - original_risk * original_reward_risk
```

Do not hardcode 1.5. Use the original reward/risk implied by the signal.

## Files to read first

Read these files before changing anything:

- AGENTS.md
- docs/ROADMAP.md
- README.md
- docs/DATA_DOWNLOAD.md
- config/research.toml
- src/config/mod.rs
- src/backtest/engine.rs
- src/backtest/fill_model.rs
- src/backtest/risk_trace.rs
- src/backtest/report.rs
- src/backtest/mod.rs
- src/research/mod.rs
- src/report/manifest.rs
- src/risk/guard.rs
- src/core/signal.rs
- src/core/trade.rs

## Required change 1 - Add EntryGeometryMode enum

Add a deterministic enum.

Recommended location:

```text
src/backtest/engine.rs
```

or cleaner:

```text
src/backtest/geometry.rs
```

If creating a new file, update:

```text
src/backtest/mod.rs
```

Recommended enum:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryGeometryMode {
    PreserveSignalLevels,
    ReanchorToActualEntry,
}
```

Implement:

```rust
impl EntryGeometryMode {
    pub fn as_str(&self) -> &'static str;
    pub fn parse(s: &str) -> Result<Self, NorthflowError>;
}
```

Accepted config strings:

```text
preserve_signal_levels
reanchor_to_actual_entry
```

Reject unknown values with `NorthflowError::ConfigError`.

Do not silently default unknown strings.

## Required change 2 - Add config parsing

Update `src/config/mod.rs`.

Add field to `ResearchConfig`:

```rust
pub entry_geometry_mode: EntryGeometryMode
```

or if config module must avoid importing backtest types, use:

```rust
pub entry_geometry_mode: String
```

Preferred:

- Use the enum if this does not introduce circular dependencies.
- Otherwise keep string in config and parse in BacktestEngine.

Default must be:

```text
preserve_signal_levels
```

Update parser to read:

```toml
[backtest]
entry_geometry_mode = "preserve_signal_levels"
```

If missing, default to `preserve_signal_levels`.

If invalid, return a config error.

Update `config/research.toml`:

```toml
[backtest]
entry_geometry_mode = "preserve_signal_levels"
```

Keep existing fields such as:

```toml
data_dir
reports_dir
conservative_intrabar
max_bars_held
```

Do not break existing config parsing.

## Required change 3 - Implement geometry adjustment helper

In `src/backtest/engine.rs` or new `src/backtest/geometry.rs`, implement a helper.

Recommended function:

```rust
fn adjusted_signal_for_actual_entry(
    signal: &Signal,
    actual_entry_price: f64,
    mode: EntryGeometryMode,
) -> Signal
```

Behavior:

### preserve_signal_levels

- Clone signal.
- Set `entry_price = actual_entry_price`.
- Keep original `stop_loss`.
- Keep original `take_profit`.
- Recalculate:
  - `expected_reward_bps`
  - `expected_net_edge_bps`

### reanchor_to_actual_entry

- Clone signal.
- Set `entry_price = actual_entry_price`.
- Compute original risk distance and original reward/risk ratio from the original signal.
- Set new stop_loss and take_profit around actual entry.
- Recalculate:
  - `expected_reward_bps`
  - `expected_net_edge_bps`

Important:

Do not modify:

- signal_id
- symbol
- strategy_id
- side
- timeframes
- entry_time
- confidence
- regime
- entry_reason
- filters_passed
- filters_failed
- estimated_cost_bps

### Expected reward bps formula

For long:

```text
expected_reward_bps = (take_profit - actual_entry_price) / actual_entry_price * 10000
```

For short:

```text
expected_reward_bps = (actual_entry_price - take_profit) / actual_entry_price * 10000
```

Then:

```text
expected_net_edge_bps = expected_reward_bps - estimated_cost_bps
```

Note:

`RiskEngine::assess()` will later compute its own final cost model and expected net edge. The adjusted signal fields are still useful for trace/debug and invalid-geometry soft rejection.

## Required change 4 - Preserve mode behavior must remain current behavior

The default mode must keep current behavior:

```text
entry_geometry_mode = "preserve_signal_levels"
```

Current BTCUSDT run should still produce approximately:

```text
trades_opened = 0
rejected_actual_entry = 8237
```

Do not force reanchor as default.

Do not change current strict model behavior.

## Required change 5 - Reanchor mode behavior

When config says:

```toml
entry_geometry_mode = "reanchor_to_actual_entry"
```

then actual-entry re-risking should use the reanchored signal:

- actual entry price from `FillModel::adverse_entry_price`
- stop_loss shifted from actual entry by original risk distance
- take_profit shifted from actual entry by original risk distance * original reward/risk
- then `RiskEngine::assess()` on this adjusted signal
- if approved, open position using this adjusted signal
- `Trade.reward_risk` should reflect effective reward/risk at actual fill
- `trades.csv` should show the adjusted stop_loss and take_profit actually used in the simulated trade

This mode should allow us to compare whether the strategy fails because of execution geometry or because the signal itself is bad.

## Required change 6 - Add geometry mode to outputs

Update CLI output in `src/research/mod.rs`.

Near timeframe model or backtest summary, print:

```text
Entry geometry mode: preserve_signal_levels
```

or:

```text
Entry geometry mode: reanchor_to_actual_entry
```

Also include this mode in `signal_flow_summary.json`.

Update `SignalFlowSummary` in `src/backtest/risk_trace.rs`:

```rust
pub entry_geometry_mode: String,
```

Default can be empty or `preserve_signal_levels`, but before writing result it must be set to the actual mode.

Update `signal_flow_summary.json`:

```json
{
  "entry_geometry_mode": "preserve_signal_levels",
  "signals_generated": 0,
  ...
}
```

Keep existing field names. Add the new field at the top.

Update tests accordingly.

## Required change 7 - Add geometry mode to risk_rejections.csv

Update `RiskRejection` model:

```rust
pub entry_geometry_mode: String,
```

Update `risk_rejections.csv` header to include this column:

```csv
signal_id,stage,entry_geometry_mode,timestamp,side,regime,reason,equity,peak_equity,drawdown_pct,daily_realized_pnl,expected_reward_bps,expected_cost_bps,expected_net_edge_bps
```

Each rejection row should include the active mode.

Reason:

- When comparing preserve vs reanchor output files, each row must be self-describing.

Update tests.

## Required change 8 - Optional but recommended report naming

Do not change existing default filenames yet.

Keep:

```text
reports/risk_rejections.csv
reports/signal_flow_summary.json
reports/trades.csv
```

Do not create per-mode filenames in this patch.

Comparison can be done by rerunning with a different `reports_dir`.

Document this.

Example:

```toml
[backtest]
reports_dir = "reports/preserve"
entry_geometry_mode = "preserve_signal_levels"
```

and:

```toml
[backtest]
reports_dir = "reports/reanchor"
entry_geometry_mode = "reanchor_to_actual_entry"
```

## Required change 9 - Documentation

Update README.md.

Add short section under Phase 6 or Phase 7:

```markdown
### Entry geometry mode

Backtest supports two actual-entry geometry modes:

- `preserve_signal_levels` — default strict mode. Entry occurs at next candle open, but SL/TP remain the original signal levels.
- `reanchor_to_actual_entry` — after actual fill, SL/TP are re-anchored around the actual entry using original risk distance and original reward/risk.

Use preserve mode to test strict pre-defined signal levels.
Use reanchor mode to simulate market entry followed by bracket placement after the fill price is known.

`trades.csv` always reports the actual stop_loss/take_profit used by the simulated trade.
`reward_risk` is always effective reward/risk at simulated entry fill.
```

Update `docs/DATA_DOWNLOAD.md`.

Add a short note:

```markdown
For comparing entry geometry modes, set a different `reports_dir` before each run.
```

Example:

```toml
[backtest]
reports_dir = "reports/preserve"
entry_geometry_mode = "preserve_signal_levels"
```

```toml
[backtest]
reports_dir = "reports/reanchor"
entry_geometry_mode = "reanchor_to_actual_entry"
```

Do not rewrite whole docs.

## Required change 10 - Tests

Add focused tests.

### EntryGeometryMode tests

- entry_geometry_mode_parse_preserve
- entry_geometry_mode_parse_reanchor
- entry_geometry_mode_rejects_unknown
- entry_geometry_mode_as_str_is_stable

### Config tests

- config_defaults_entry_geometry_mode_to_preserve
- config_parses_preserve_entry_geometry_mode
- config_parses_reanchor_entry_geometry_mode
- config_rejects_unknown_entry_geometry_mode

### Adjusted signal tests

- preserve_mode_keeps_original_stop_loss_and_take_profit
- preserve_mode_updates_entry_price
- preserve_mode_recalculates_expected_reward_long
- preserve_mode_recalculates_expected_reward_short
- reanchor_mode_long_shifts_stop_loss_and_take_profit
- reanchor_mode_short_shifts_stop_loss_and_take_profit
- reanchor_mode_preserves_original_reward_risk
- reanchor_mode_recalculates_expected_reward_long
- reanchor_mode_recalculates_expected_reward_short

### Engine behavior tests

- preserve_mode_can_reject_actual_entry_reward_risk
- reanchor_mode_preserves_effective_reward_risk_after_actual_entry
- risk_rejection_records_entry_geometry_mode
- signal_flow_records_entry_geometry_mode

These tests do not need huge CSV fixtures. Prefer helper unit tests around adjusted signal behavior.

### Report tests

- risk_rejections_csv_header_includes_entry_geometry_mode
- risk_rejections_csv_row_includes_entry_geometry_mode
- signal_flow_summary_json_includes_entry_geometry_mode

Existing tests must continue to pass.

## Required commands

Run:

```bash
cargo fmt
cargo build
cargo test
cargo run -- help
```

If `data/historical/BTCUSDT.csv` exists, run both modes:

### Preserve mode

Set:

```toml
[backtest]
reports_dir = "reports/preserve"
entry_geometry_mode = "preserve_signal_levels"
```

Run:

```bash
cargo run --release -- research --config config/research.toml
```

Expected:

- likely `trades_opened = 0`
- many actual-entry reward/risk rejections

### Reanchor mode

Set:

```toml
[backtest]
reports_dir = "reports/reanchor"
entry_geometry_mode = "reanchor_to_actual_entry"
```

Run:

```bash
cargo run --release -- research --config config/research.toml
```

Expected:

- do not hardcode expected trade count
- should produce self-consistent signal flow and reports
- trades may or may not open depending on cost/risk
- audit must pass

If no CSV exists, friendly missing-data behavior must remain.

## Strictly forbidden

Do not implement:

- strategy tuning
- new indicators
- new strategy rules
- parameter optimization
- automatic mode selection
- paper trading
- live trading
- exchange adapter
- websocket
- database
- dashboard
- Telegram
- LLM trading decisions
- AI advisor

Do not mark either mode as profitable.

Do not claim reanchor is better. It is only a research comparison mode.

## Expected final result

At the end of this patch:

- Backtest config supports `entry_geometry_mode`.
- Default mode is `preserve_signal_levels`.
- Preserve mode keeps current strict behavior.
- Reanchor mode adjusts SL/TP around actual entry using original risk distance and original reward/risk.
- Signal flow summary reports the active mode.
- Risk rejection rows report the active mode.
- Documentation explains both modes.
- Tests cover parsing, config, geometry adjustment, reports, and summary.
- All tests pass.

## Commit message suggestion

research: add entry geometry modes
