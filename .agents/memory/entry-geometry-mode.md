---
name: Entry geometry mode patch
description: Design decisions for the EntryGeometryMode patch — new configurable mode controlling SL/TP adjustment after adverse fill.
---

## Rule

`EntryGeometryMode` is a new enum in `src/backtest/geometry.rs` with two variants:
- `PreserveSignalLevels` — SL/TP stay at original absolute levels (default; current behavior).
- `ReanchorToActualEntry` — SL/TP re-anchored around actual fill using original risk distance × RR ratio.

`ResearchConfig` stores `entry_geometry_mode: String` (not the enum) to avoid circular deps between `config` and `backtest` crates.
Parsed to `EntryGeometryMode` via `EntryGeometryMode::parse()` inside `BacktestEngine::run` — returns `NorthflowError::ConfigError` on unknown value; never silently defaults.

`build_rejection()` gains `entry_geometry_mode: &str` as 3rd param (after `stage`), and `RiskRejection` and `SignalFlowSummary` each gain `entry_geometry_mode: String`.

`signal_flow.entry_geometry_mode` must be set **before** `signal_flow.finalise()` is called.

`adjusted_signal_for_actual_entry` moved entirely from `engine.rs` to `geometry.rs`; the old 2-arg private function was removed.

## Why

Preserve mode is the strict default — ensures existing backtest behavior is unchanged when upgrading. Reanchor mode allows studying "what if we had placed a bracket order at the actual fill" — different from the conservative strict mode and not comparable unless clearly labeled in reports.

Storing `entry_geometry_mode` in every `RiskRejection` row and in `signal_flow_summary.json` makes runs with different modes distinguishable without re-running.

## How to apply

- Config key: `entry_geometry_mode` under `[backtest]` in TOML.
- Default: `"preserve_signal_levels"`.
- Any new fields added to `RiskRejection` must also be added to `build_rejection()` signature (3rd param convention is signal, stage, entry_geometry_mode, timestamp, ...).
- Tests that construct `RiskRejection` directly must always include `entry_geometry_mode`.
- Tests that call `build_rejection()` must pass mode as 3rd arg.
- Tests that call `adjusted_signal_for_actual_entry()` must pass `EntryGeometryMode::PreserveSignalLevels` (or appropriate mode) as 3rd arg.
- 411 tests pass after this patch.
