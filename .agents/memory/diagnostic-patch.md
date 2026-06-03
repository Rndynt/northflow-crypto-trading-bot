---
name: Strategy diagnostic patch
description: Design rules for src/report/diagnostics.rs — 5 new diagnostic report files added after Phase 7.
---

## Rule

Five new files written after each backtest run:
- `signal_diagnostics.csv` — per-trade cost bps / edge realization / month / filters
- `rejection_by_stage_reason.csv` — grouped rejections by (stage, entry_geometry_mode, reason)
- `monthly_summary.csv` — monthly PnL, win rate, profit factor, exit reason counts
- `cost_edge_distribution.csv` — always exactly 8 buckets (edge_lt_0 … edge_gte_50); all included even if count=0
- `trade_distribution_summary.json` — compact JSON summary, dominant rejection reason

`DiagnosticEngine::build(trades, risk_rejections, signal_flow)` → `DiagnosticReport`
`DiagnosticWriter::write_all_with_trades(reports_dir, report, trades)` — primary entry point (research/mod.rs)

`ManifestWriter::build` gains `diagnostic: &DiagnosticReport` as 6th param. Diagnostics must be built BEFORE manifest so row counts are available. Current order: diagnostics → attribution → audit → manifest.

## month_key_from_ms

Uses `timestamp_ms.div_euclid(86_400_000)` (floor division, not truncation) then Howard Hinnant civil_from_days.
Tests: 1704067200000 → "2024-01", 1706745600000 → "2024-02", 1711929600000 → "2024-04", 1735689540000 → "2024-12".

## Profit factor bug to avoid

`gross_losers` must accumulate `t.gross_pnl` for ALL trades where `net_pnl <= 0.0`, regardless of whether `gross_pnl` is positive or negative. Fee-eaten trades can have `net_pnl < 0` but `gross_pnl > 0` — those still count as losers for profit factor.

**Why:** If you only accumulate `gross_losers` when `gross_pnl < 0`, you under-count losers and over-inflate profit factor for strategies with high cost ratios.

## How to apply

- Any new monthly metrics that need gross-split must use the same pattern: split on `net_pnl` sign, not `gross_pnl` sign.
- `cost_edge_distribution` bucket order is fixed and canonical — must not be reordered.
- All 8 cost-edge buckets always written even when count=0 (explicit spec requirement).
- `write_all_with_trades` is the research-facing API; `write_all` (stub) exists for internal use only.
- Test helpers that call `ManifestWriter::build` need `&DiagnosticEngine::build(&[], &[], &SignalFlowSummary::default())` as 6th arg.
- 441 tests pass after this patch.
