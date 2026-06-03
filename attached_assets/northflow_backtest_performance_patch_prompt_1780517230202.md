# Northflow Performance Patch Prompt

You are working on this repository:

https://github.com/Rndynt/northflow-crypto-trading-bot

Your task is to perform a small performance and observability patch for the Phase 6/7 research backtest flow.

The user has successfully downloaded BTCUSDT 1m data for 2024:

- data/historical/BTCUSDT.csv
- 1m candles: about 527,040
- 5m candles: about 105,408
- 15m candles: about 35,136
- data quality errors: 0
- duplicate timestamps: 0
- missing gaps: 0

The current problem:

When running:

cargo run -- research --config config/research.toml

the CLI prints the data summary and then appears to hang with no progress output.

This is likely because the backtest loop is processing a large dataset with no progress logging, and the higher-timeframe snapshot lookup currently uses a reverse linear scan.

Do not implement new phases.

Do not change strategy rules.

Do not change risk rules.

Do not change indicator formulas.

Do not change report schemas.

Do not implement paper/live/exchange/LLM.

This is only a performance and progress visibility patch.

## Files to read first

Read these files before changing anything:

- AGENTS.md
- docs/ROADMAP.md
- README.md
- docs/DATA_DOWNLOAD.md
- src/backtest/engine.rs
- src/research/mod.rs
- src/main.rs

## Required fix 1 - Replace linear snapshot lookup with binary search

In src/backtest/engine.rs, find this helper:

```rust
fn latest_snap<'a>(
    snaps: &'a [(i64, IndicatorSnapshot, Candle)],
    max_ts: i64,
) -> Option<&'a (i64, IndicatorSnapshot, Candle)> {
    snaps.iter().rev().find(|(ts, _, _)| *ts <= max_ts)
}
```

Replace it with a binary-search based implementation:

```rust
fn latest_snap<'a>(
    snaps: &'a [(i64, IndicatorSnapshot, Candle)],
    max_ts: i64,
) -> Option<&'a (i64, IndicatorSnapshot, Candle)> {
    let idx = snaps.partition_point(|(ts, _, _)| *ts <= max_ts);

    if idx == 0 {
        None
    } else {
        Some(&snaps[idx - 1])
    }
}
```

Rationale:

- Current reverse linear scan is O(n) per 1m candle.
- With 527k 1m candles and 105k 5m snapshots, this can become very slow.
- partition_point changes lookup to O(log n).
- The vector is already sorted chronologically because snapshots are precomputed from sorted candles.

Do not change the no-lookahead rule.

The same condition must remain:

latest higher timeframe snapshot where higher_tf_ts <= max_ts

## Required fix 2 - Add backtest progress logging

In src/backtest/engine.rs, inside the main loop:

```rust
for i in 0..n {
    let candle = one_minute[i];
```

Add progress logging after let candle = one_minute[i];.

Recommended:

```rust
if i > 0 && i % 50_000 == 0 {
    println!(
        "  Backtest progress: {}/{} 1m candles ({:.1}%)",
        i,
        n,
        i as f64 / n as f64 * 100.0
    );
}
```

Also print a final completion line after the loop and end-of-backtest close logic, before returning result:

```rust
println!(
    "  Backtest complete: {} trades, final equity {:.2}",
    trades.len(),
    equity
);
```

Keep logging simple and deterministic.

Do not add progress bars.

Do not add external dependencies.

Do not use async.

Do not use threads.

## Required fix 3 - Add phase-level research progress messages

In src/research/mod.rs, make the CLI clearly show what it is doing after data quality summary.

Before running the backtest for each symbol, print something like:

Running backtest replay...

After base reports are written:

Base backtest reports written.

After Phase 7 attribution reports are written:

Phase 7 attribution reports written.

Keep existing output. Only add helpful progress lines.

## Required fix 4 - Add tests for binary lookup

Update existing tests in src/backtest/engine.rs or add new tests for latest_snap.

Required tests:

- latest_snap_uses_exact_match
- latest_snap_returns_previous_when_between_timestamps
- latest_snap_returns_none_before_first_timestamp
- latest_snap_returns_last_when_after_last_timestamp

Existing latest_snap_returns_none_when_all_too_recent and latest_snap_returns_most_recent_eligible may already cover some cases. Add the missing cases.

The tests must still verify no-lookahead eligibility:

- return latest snapshot with ts <= max_ts
- never return snapshot with ts > max_ts

## Required fix 5 - Document release mode for large data

Update docs/DATA_DOWNLOAD.md and/or README with a short note:

For large datasets such as 12 months of BTCUSDT 1m candles, use release mode:

```bash
cargo run --release -- research --config config/research.toml
```

Explain:

- debug mode can be slow on 500k+ candles
- release mode is recommended for real backtests
- if CLI appears idle, progress logs should now show every 50,000 candles

Do not over-edit documentation.

## Required commands

Run:

```bash
cargo fmt
cargo build
cargo test
cargo run -- help
```

Then, if data/historical/BTCUSDT.csv exists, run:

```bash
cargo run --release -- research --config config/research.toml
```

If the data file exists, the output should show progress lines like:

```text
Running backtest replay...
  Backtest progress: 50000/527040 1m candles (9.5%)
  Backtest progress: 100000/527040 1m candles (19.0%)
...
  Backtest complete: X trades, final equity Y
```

If the CSV does not exist, the friendly missing-data behavior must remain.

## Strictly forbidden

Do not implement:

- paper trading
- live trading
- exchange adapter
- websocket feed
- order placement
- fill API integration
- database
- dashboard
- Telegram
- LLM trading decision
- AI advisor
- strategy optimization
- parameter tuning
- new indicators
- new strategy rules
- changed risk formulas
- changed report schemas

## Expected final result

At the end of this patch:

- latest_snap uses binary-search style lookup with partition_point.
- No-lookahead semantics are unchanged.
- Large 1-year datasets no longer appear frozen due to O(n) snapshot scans.
- Research CLI prints progress during backtest replay.
- Documentation recommends cargo run --release for large datasets.
- All tests pass.
- No phase scope creep is introduced.

## Commit message suggestion

perf: speed up backtest snapshot lookup
