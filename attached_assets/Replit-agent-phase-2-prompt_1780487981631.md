# Northflow Phase 2 Build Prompt

You are working on this repository:

https://github.com/Rndynt/northflow-crypto-trading-bot

Your task is to implement **Phase 2: Market Data Foundation** exactly according to the repository documentation.

Read and follow these files first:

- `AGENTS.md`
- `docs/ROADMAP.md`
- `config/research.toml`
- `README.md`
- Existing Phase 1 core files under `src/core/`

Do not ignore those files. They define the allowed scope, architecture boundaries, and development rules.

## Project mission

Northflow is not a dashboard, not a React app, not a Telegram bot, and not an AI trading agent.

Northflow is a deterministic research-first crypto trading engine.

The current goal is to build a truthful historical market data foundation that later phases can use for indicators, strategy evaluation, risk validation, backtesting, and reporting.

In Phase 2, do **not** implement live trading, paper trading, dashboard, Telegram integration, LLM decision making, strategy logic, risk sizing, backtest execution, or report generation.

## Current phase

Implement:

```text
Phase 2 — Market Data
```

From `docs/ROADMAP.md`, Phase 2 must:

- Load 1m OHLCV CSV.
- Support `timestamp,open,high,low,close,volume`.
- Support `open_time,open,high,low,close,volume`.
- Reject invalid candles.
- Detect duplicate timestamps.
- Detect missing candles.
- Build 5m and 15m candles from 1m candles.

Do not implement Phase 3 indicators, Phase 4 strategy, Phase 5 risk, Phase 6 backtest, or Phase 7 reports except for small placeholders or module exports needed to keep the project compiling.

## Important rule about legacy code

The previous project may exist under:

```text
legacy/aria/
```

Legacy code is reference-only.

You may inspect legacy code to understand existing concepts, but you must not blindly copy it into the active source tree.

For every piece of legacy code you want to reuse, validate it first:

- Is it relevant to Phase 2 market data?
- Is it deterministic?
- Is it small enough?
- Does it avoid LLM, agents, Telegram, dashboard, and live exchange side effects?
- Can it be simplified?
- Can it be tested?
- Does it match the new Northflow architecture?

If the answer is no, do not reuse it.

If a needed module does not exist in legacy, implement it cleanly from scratch.

Do not import from `legacy/` into active `src/`.

Active production code must never depend on legacy modules.

## Mandatory Phase 1 preservation

Phase 1 core domain must remain intact.

Do not break or rewrite these modules unless a tiny compatibility adjustment is absolutely required:

```text
src/core/candle.rs
src/core/side.rs
src/core/symbol.rs
src/core/timeframe.rs
src/core/signal.rs
src/core/order.rs
src/core/fill.rs
src/core/position.rs
src/core/trade.rs
src/core/error.rs
```

The following Phase 1 concepts must remain valid:

```text
signal_id -> order_id -> fill_id -> position_id -> exit_order_id -> trade_id
```

The `Signal` type must keep mandatory `signal_id`.

The `Timeframe` type must keep support for:

```text
1m
5m
15m
1h
```

The project must still compile and all Phase 1 tests must keep passing.

## Mandatory timeframe model

The config must explicitly use:

```toml
entry_timeframe = "1m"
screening_timeframe = "15m"
confirmation_timeframe = "5m"
```

Meaning:

- `1m` is for entry and execution signals.
- `15m` is for screening and regime bias.
- `5m` is for confirmation.

Never infer timeframe roles from array order.

In Phase 2, market data must be built from the explicit timeframe model:

- Load base 1m candles.
- Build 5m candles from 1m candles.
- Build 15m candles from 1m candles.

Do not load 5m or 15m files as the primary source in this phase.

The source of truth is 1m OHLCV.

## Required active structure for Phase 2

Create this active source structure:

```text
src/market/mod.rs
src/market/ohlcv_loader.rs
src/market/candle_store.rs
src/market/timeframe_builder.rs
src/market/data_quality.rs
```

Also update:

```text
src/lib.rs
src/research/mod.rs
README.md
docs/ROADMAP.md if needed only to mark Phase 2 status or clarify usage
```

If `src/data/mod.rs` already exists, do not keep Phase 2 functionality there as the main implementation.

Preferred handling:

- Move the real Phase 2 implementation into `src/market/`.
- Keep `src/data/mod.rs` only as a thin compatibility wrapper or mark it deprecated.
- Do not duplicate two independent loaders.

## Required Phase 2 types

Implement simple deterministic Rust types.

Avoid unnecessary external dependencies.

Use the existing `crate::core::Candle`, `crate::core::Timeframe`, and `crate::core::NorthflowError` where appropriate.

Add these types:

```text
OhlcvLoadResult
OhlcvLoader
CandleStore
TimeframeBuilder
DataQualityReport
DataQualityIssue
DataQualityIssueKind
MissingCandleGap
```

You may adjust names slightly if there is a clearer Rust naming pattern, but the final module responsibilities must be obvious.

## OHLCV loader requirements

Implement `src/market/ohlcv_loader.rs`.

It must load 1m OHLCV CSV files from disk.

Supported headers:

```text
timestamp,open,high,low,close,volume
open_time,open,high,low,close,volume
```

Header handling rules:

- Header names should be case-insensitive.
- Header names should tolerate surrounding whitespace.
- Required columns are timestamp or open_time, open, high, low, close, volume.
- If required columns are missing, return a data error.
- Do not silently guess missing OHLCV columns.

Timestamp rules:

- Accept integer Unix timestamps.
- Accept both seconds and milliseconds.
- Normalize timestamps internally to milliseconds.
- If timestamp is seconds, convert to milliseconds.
- If timestamp is milliseconds, keep it as milliseconds.
- Reject non-numeric timestamps in Phase 2.
- Do not add ISO timestamp parsing unless implemented with tests and no new dependency.

CSV parsing rules:

- Keep parsing deterministic and simple.
- No async.
- No network.
- No exchange API.
- No live data.
- No external API calls.
- Empty lines can be ignored.
- Malformed rows must be reported as data quality issues.
- Invalid candles must be rejected and reported.
- Do not silently skip invalid candles without recording the reason.

Candle validation:

Use the existing `Candle::validate()` logic from Phase 1.

Reject any candle where:

- open is not finite or <= 0
- high is not finite or <= 0
- low is not finite or <= 0
- close is not finite or <= 0
- volume is not finite or < 0
- high < low
- open is outside low/high
- close is outside low/high

The loader must return both:

- valid candles
- data quality report

Do not panic on bad data.

## Data quality requirements

Implement `src/market/data_quality.rs`.

Data quality must detect and report:

```text
missing_required_column
malformed_row
invalid_number
invalid_timestamp
invalid_candle
duplicate_timestamp
non_monotonic_timestamp
missing_candle_gap
empty_file
```

Recommended type:

```text
pub struct DataQualityReport {
    pub source: String,
    pub total_rows: usize,
    pub valid_candles: usize,
    pub rejected_rows: usize,
    pub issues: Vec<DataQualityIssue>,
}
```

Recommended issue type:

```text
pub struct DataQualityIssue {
    pub kind: DataQualityIssueKind,
    pub row: Option<usize>,
    pub timestamp: Option<i64>,
    pub message: String,
}
```

Recommended issue kind enum:

```text
pub enum DataQualityIssueKind {
    MissingRequiredColumn,
    MalformedRow,
    InvalidNumber,
    InvalidTimestamp,
    InvalidCandle,
    DuplicateTimestamp,
    NonMonotonicTimestamp,
    MissingCandleGap,
    EmptyFile,
}
```

The exact implementation may vary, but the behavior must be testable.

The report must provide helper methods:

```text
has_errors()
error_count()
warning_count()
```

For Phase 2, treat these as errors:

- missing required column
- malformed row
- invalid number
- invalid timestamp
- invalid candle
- duplicate timestamp
- non-monotonic timestamp

For missing candle gaps, report them clearly. Whether they are fatal can be configurable later, but in Phase 2 they must be detected and visible.

## Duplicate timestamp detection

If two valid rows have the same normalized timestamp, report `DuplicateTimestamp`.

Do not keep both candles.

Use deterministic behavior:

- Keep the first valid candle.
- Reject the later duplicate row.
- Record the rejected row and timestamp in the data quality report.

## Timestamp ordering

The final candle list must be sorted by timestamp ascending.

If input rows are not monotonic, report `NonMonotonicTimestamp`.

After sorting, duplicate detection must still be correct.

Do not assume exchange data is already sorted.

## Missing candle detection

For base 1m candles, detect missing candles using 60,000 milliseconds interval.

Example:

```text
00:00
00:01
00:03
```

This has a missing candle gap between `00:01` and `00:03`.

Create a `MissingCandleGap` record that includes:

```text
from_timestamp
to_timestamp
expected_next_timestamp
missing_count
```

Rules:

- Missing count = `(actual_delta / 60000) - 1`
- Only detect gaps after valid candles are sorted and duplicates are removed.
- Record each gap as a `MissingCandleGap`.
- Also add a `DataQualityIssue` with kind `MissingCandleGap`.

## CandleStore requirements

Implement `src/market/candle_store.rs`.

`CandleStore` should hold candles by timeframe.

Minimum behavior:

```text
pub struct CandleStore {
    pub one_minute: Vec<Candle>,
    pub five_minute: Vec<Candle>,
    pub fifteen_minute: Vec<Candle>,
}
```

It must provide helper methods:

```text
get(Timeframe) -> Option<&[Candle]>
len(Timeframe) -> usize
is_empty(Timeframe) -> bool
```

It must be built from 1m candles using the timeframe builder.

The store must not mutate candles unpredictably.

No global state.

No static mutable state.

No exchange calls.

## Timeframe builder requirements

Implement `src/market/timeframe_builder.rs`.

It must build higher timeframe candles from 1m candles.

Required output:

- 5m candles
- 15m candles

Input:

- valid sorted 1m candles

Rules for aggregation:

For each bucket:

```text
open = first candle open
high = max high
low = min low
close = last candle close
volume = sum volume
timestamp = bucket start timestamp
```

Bucket alignment:

Use timestamp flooring based on timeframe seconds.

For 5m:

```text
bucket_start = timestamp - (timestamp % 300000)
```

For 15m:

```text
bucket_start = timestamp - (timestamp % 900000)
```

The builder must use the existing `Timeframe::to_seconds()` where appropriate.

Important:

- Only build from 1m source candles.
- Do not build 15m from 5m in this phase.
- Build both 5m and 15m directly from 1m.
- Validate every aggregated candle with `Candle::validate()`.
- Return errors if an aggregated candle is invalid.

Incomplete bucket rule:

For Phase 2, drop incomplete higher-timeframe buckets by default.

Examples:

- A 5m bucket needs exactly 5 one-minute candles.
- A 15m bucket needs exactly 15 one-minute candles.

If a bucket has fewer than required candles because the file ends or data is missing, do not output that higher timeframe candle.

Record this behavior in comments and tests.

Do not forward-fill missing candles.

Do not synthesize fake candles.

Do not interpolate missing data.

Truthful data is more important than a prettier backtest.

## Research CLI behavior for Phase 2

Update `src/research/mod.rs` so this command:

```text
cargo run -- research --config config/research.toml
```

does not run a fake backtest.

It should:

1. Load config.
2. Validate explicit timeframe roles.
3. For each symbol in config, try to load:

```text
data/historical/<SYMBOL>.csv
```

4. Print a clear Phase 2 market data summary:
   - symbol
   - source path
   - 1m candle count
   - 5m candle count
   - 15m candle count
   - data quality issue count
   - duplicate timestamp count
   - missing gap count
5. If data file does not exist, print a clear message explaining where to place the CSV.
6. Do not claim profitability.
7. Do not generate fake trades.
8. Do not write reports yet.
9. Keep paper and live modes disabled.

Acceptable output example:

```text
Northflow — Phase 2: Market Data Foundation
Symbol: BTCUSDT
Source: data/historical/BTCUSDT.csv
1m candles: 1000
5m candles: 200
15m candles: 66
Data quality issues: 0
Next: Phase 3 — indicators
```

If no CSV exists, the command should not panic. It should explain:

```text
No historical CSV found for BTCUSDT.
Expected path: data/historical/BTCUSDT.csv
Place a 1m OHLCV CSV file with columns:
timestamp,open,high,low,close,volume
```

## Config validation requirement

Improve config validation if needed.

The config must reject or clearly error on ambiguous timeframe config.

Expected valid values:

```text
entry_timeframe = "1m"
screening_timeframe = "15m"
confirmation_timeframe = "5m"
```

Use the existing `Timeframe::from_str()`.

If config has invalid timeframe values, return a clear error.

If config uses the wrong roles, for example:

```toml
entry_timeframe = "15m"
screening_timeframe = "1m"
confirmation_timeframe = "5m"
```

return an error explaining that Northflow Phase 2 expects:

```text
entry=1m, screening=15m, confirmation=5m
```

## Module export requirements

Update `src/lib.rs`:

```rust
pub mod market;
```

Update `src/market/mod.rs` to export:

```rust
pub mod ohlcv_loader;
pub mod candle_store;
pub mod timeframe_builder;
pub mod data_quality;

pub use ohlcv_loader::*;
pub use candle_store::*;
pub use timeframe_builder::*;
pub use data_quality::*;
```

Use explicit exports if preferred.

Do not import from `legacy/`.

Do not create frontend modules.

Do not add service layers.

## Tests required

Add unit tests for Phase 2.

Minimum tests:

### OHLCV loader tests

- Loads valid CSV with `timestamp`.
- Loads valid CSV with `open_time`.
- Rejects missing required columns.
- Rejects invalid number.
- Rejects invalid timestamp.
- Rejects invalid candle.
- Detects duplicate timestamp.
- Detects non-monotonic input.
- Sorts output candles ascending.
- Normalizes seconds timestamp to milliseconds.
- Keeps milliseconds timestamp unchanged.

### Data quality tests

- `DataQualityReport::has_errors()` works.
- Error count works.
- Missing gap issue is recorded.
- Duplicate timestamp issue is recorded.

### Missing candle tests

- No missing gap for continuous 1m candles.
- Detects one missing candle.
- Detects multiple missing candles.

### Timeframe builder tests

- Builds one 5m candle from five 1m candles.
- Builds two 5m candles from ten 1m candles.
- Drops incomplete 5m bucket.
- Builds one 15m candle from fifteen 1m candles.
- Drops incomplete 15m bucket.
- Aggregated open equals first open.
- Aggregated high equals max high.
- Aggregated low equals min low.
- Aggregated close equals last close.
- Aggregated volume equals sum volume.
- Aggregated timestamp equals bucket start.

### CandleStore tests

- Builds store from 1m candles.
- `get(Timeframe::OneMinute)` returns 1m candles.
- `get(Timeframe::FiveMinute)` returns 5m candles.
- `get(Timeframe::FifteenMinute)` returns 15m candles.
- `len()` works.
- `is_empty()` works.

### Config/research tests if applicable

- Valid explicit timeframe config passes.
- Invalid timeframe string fails.
- Wrong timeframe role fails.

## Minimum command target

These commands must pass:

```text
cargo build
cargo test
cargo run -- research --config config/research.toml
```

If build or tests fail, fix them before finishing.

Do not leave failing tests.

Do not leave TODO stubs in active Phase 2 behavior.

## Documentation update

Update `README.md` to state:

- Current phase is Phase 2.
- Phase 1 core domain is complete.
- Phase 2 market data loader and timeframe builder are implemented.
- Paper and live modes remain disabled.
- CSV source must be 1m OHLCV.
- Required CSV columns are `timestamp/open_time,open,high,low,close,volume`.
- 5m and 15m candles are built from 1m candles.
- Invalid candles are rejected.
- Duplicate timestamps and missing candles are detected.
- No fake backtest results are generated.

Do not remove `docs/ROADMAP.md` or `AGENTS.md`.

Do not rewrite the roadmap unless only marking Phase 2 as implemented or clarifying Phase 2 usage.

## Strictly forbidden in this phase

Do not create:

- React app
- TypeScript app
- dashboard
- web UI
- Telegram integration
- LLM trading decision
- manager agent
- learning agent
- survival agent
- orchestrator
- live exchange order placement
- paper trading loop
- strategy router
- portfolio optimizer
- 100x leverage logic
- fake trades
- fake backtest report
- optimistic data fill
- synthetic candles
- interpolated candles
- exchange API integration
- websocket feed
- database requirement

Do not add frontend dependencies.

Do not add runtime services.

Do not add async networking.

Do not hide bad market data.

## Expected final result

At the end of Phase 2, the repository should have:

- Phase 1 core still intact.
- `src/market/` module implemented.
- Deterministic OHLCV CSV loader.
- Data quality report.
- Duplicate timestamp detection.
- Missing 1m candle gap detection.
- 5m timeframe builder from 1m.
- 15m timeframe builder from 1m.
- CandleStore holding 1m, 5m, and 15m candles.
- Config timeframe validation.
- Research CLI prints truthful market data summary.
- No fake backtest.
- No report generation yet.
- Paper mode disabled.
- Live mode disabled.
- Unit tests for loader, data quality, timeframe builder, and candle store.
- `cargo build` passing.
- `cargo test` passing.
- `cargo run -- research --config config/research.toml` working.

## Suggested implementation order

1. Read `AGENTS.md`, `docs/ROADMAP.md`, and Phase 1 core files.
2. Add `src/market/mod.rs`.
3. Implement `data_quality.rs`.
4. Implement `ohlcv_loader.rs`.
5. Implement duplicate timestamp and missing gap detection.
6. Implement `timeframe_builder.rs`.
7. Implement `candle_store.rs`.
8. Export `market` from `src/lib.rs`.
9. Improve config timeframe validation.
10. Update `research/mod.rs` to print Phase 2 summary.
11. Add comprehensive unit tests.
12. Update README.
13. Run `cargo fmt`.
14. Run `cargo build`.
15. Run `cargo test`.
16. Run `cargo run -- research --config config/research.toml`.

## Commit message suggestion

```text
phase2: implement market data loader and timeframe builder
```