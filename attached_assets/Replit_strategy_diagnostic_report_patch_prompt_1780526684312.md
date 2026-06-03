# Northflow Strategy Diagnostic Report Patch Prompt

You are working on this repository:

https://github.com/Rndynt/northflow-crypto-trading-bot

Your task is to implement a focused analytics/reporting patch after Phase 7, after risk rejection attribution, and after entry geometry modes.

Do not implement a new phase.

Do not tune the strategy.

Do not change screened_vwap_scalp rules.

Do not change indicator formulas.

Do not change risk formulas.

Do not optimize parameters.

Do not implement paper trading.

Do not implement live trading.

Do not implement exchange APIs.

Do not implement dashboard, Telegram, LLM decisions, or AI advisor.

This patch is only for research diagnostics so we can understand why the strategy fails.

## Current diagnostic result

The user ran reanchor mode without tight circuit breakers:

```toml
[backtest]
entry_geometry_mode = "reanchor_to_actual_entry"
reports_dir = "reports/reanchor"

[risk]
max_drawdown_pct = 100.0
max_daily_loss_pct = 100.0
```

BTCUSDT 1m 2024 result:

```text
Backtest complete: 4761 trades, final equity 0.00

Total trades: 4761
Win rate: 35.75%
Net PnL: -5000.00
Gross PnL: -796.84
Total fees: 1956.65
Total slippage: 2246.52
Profit factor: 0.1306
Max drawdown: 100.00%
Max consecutive losses: 17

Signal flow:
  signals generated:          64882
  signals preapproved:        4765
  rejected initial risk:      60117
  rejected actual entry:      4
  trades opened:              4761
  trades closed:              4761
  risk rejection rows:        60121
    max_drawdown:             0
    daily_loss:               0
    reward_risk:              0
    expected_net_edge:        60121
    other:                    0

Attribution:
  Avg expected edge bps: 12.17
  Avg actual edge bps:   -20.96
  Edge realization bps:  -33.14
```

Interpretation:

- Engine works.
- Data is valid.
- Reanchor geometry works.
- Risk attribution works.
- Strategy is deeply unprofitable under cost model.
- Most signals fail because expected reward is below cost.
- Trades that pass still lose due to low win rate and high fee/slippage.
- We need deeper diagnostics before tuning.

## Goal

Add diagnostic report files that answer:

1. Which months are losing or winning?
2. Which side, regime, exit reason, and filter are responsible?
3. How much of loss comes from gross PnL vs fee/slippage?
4. How does expected edge compare to actual edge?
5. How are rejected signals distributed by stage and reason?
6. How many signals/trades happen per month?
7. Are trades dying because reward bps is too small relative to cost bps?
8. Are some months, sides, or regimes worth keeping?

## Required new report files

After:

```bash
cargo run --release -- research --config config/research.toml
```

write these files in the configured reports directory:

```text
reports/signal_diagnostics.csv
reports/rejection_by_stage_reason.csv
reports/monthly_summary.csv
reports/cost_edge_distribution.csv
reports/trade_distribution_summary.json
```

These files are additive.

Do not remove or rename existing files:

```text
backtest_summary.json
trades.csv
equity_curve.csv
risk_rejections.csv
signal_flow_summary.json
attribution_summary.json
attribution_by_regime.csv
attribution_by_exit_reason.csv
attribution_by_side.csv
attribution_by_filter.csv
audit_report.json
report_manifest.json
```

Update manifest to include the new files.

## Files to read first

Read these files before changing anything:

- AGENTS.md
- docs/ROADMAP.md
- README.md
- docs/DATA_DOWNLOAD.md
- config/research.toml
- src/backtest/engine.rs
- src/backtest/report.rs
- src/backtest/risk_trace.rs
- src/backtest/geometry.rs
- src/report/mod.rs
- src/report/attribution.rs
- src/report/audit.rs
- src/report/manifest.rs
- src/report/validation.rs
- src/research/mod.rs
- src/core/trade.rs
- src/core/signal.rs

## Recommended module structure

Add a new report module:

```text
src/report/diagnostics.rs
```

Update:

```text
src/report/mod.rs
```

to export it.

Recommended public types:

```rust
pub struct DiagnosticReport {
    pub monthly: Vec<MonthlySummaryRow>,
    pub rejection_by_stage_reason: Vec<RejectionByStageReasonRow>,
    pub cost_edge_distribution: CostEdgeDistribution,
    pub trade_distribution: TradeDistributionSummary,
}

pub struct DiagnosticEngine;

impl DiagnosticEngine {
    pub fn build(
        trades: &[Trade],
        risk_rejections: &[RiskRejection],
        signal_flow: &SignalFlowSummary,
    ) -> DiagnosticReport;
}

pub struct DiagnosticWriter;

impl DiagnosticWriter {
    pub fn write_all(
        reports_dir: &str,
        report: &DiagnosticReport,
    ) -> Result<(), NorthflowError>;
}
```

Manual CSV/JSON formatting is acceptable. No serde dependency required.

Do not add external dependencies unless absolutely necessary.

## New report 1 - signal_diagnostics.csv

Purpose:

One row per closed trade with normalized diagnostic fields for strategy analysis.

This should be derived from `Trade`.

CSV header:

```csv
trade_id,signal_id,month,symbol,strategy_id,regime,side,entry_time,exit_time,duration_ms,entry_price,exit_price,stop_loss,take_profit,qty,gross_pnl,fee,slippage,total_cost,net_pnl,reward_risk,bars_held,exit_reason,expected_edge_bps,actual_edge_bps,edge_realization_bps,fee_bps,slippage_bps,total_cost_bps,net_pnl_bps,filters_passed,filters_failed,entry_reason
```

Definitions:

- `month`: UTC-like month derived from timestamp in format `YYYY-MM`.
- `duration_ms`: `exit_time - entry_time`, clamped to 0 if negative.
- `total_cost`: `fee + slippage`.
- `edge_realization_bps`: `actual_edge_bps - expected_edge_bps`.
- `entry_notional`: internal only = `entry_price * qty`.
- `fee_bps`: `fee / entry_notional * 10000`, or 0 if notional <= 0.
- `slippage_bps`: `slippage / entry_notional * 10000`, or 0 if notional <= 0.
- `total_cost_bps`: `(fee + slippage) / entry_notional * 10000`, or 0.
- `net_pnl_bps`: `net_pnl / entry_notional * 10000`, or 0.
- `filters_passed`: join with `|`
- `filters_failed`: join with `|`

Rules:

- Write header even if no trades.
- Stable row order same as trades order.
- CSV escaping must handle comma, quote, and newline.

## New report 2 - rejection_by_stage_reason.csv

Purpose:

Show why signals did not become trades.

Input:

```text
risk_rejections.csv / RiskRejection model
```

CSV header:

```csv
stage,entry_geometry_mode,reason,count,unique_signals,avg_equity,avg_drawdown_pct,avg_daily_realized_pnl,avg_expected_reward_bps,avg_expected_cost_bps,avg_expected_net_edge_bps,min_expected_net_edge_bps,max_expected_net_edge_bps
```

Grouping key:

```text
(stage, entry_geometry_mode, reason)
```

Definitions:

- `count`: number of RiskRejection rows in the group.
- `unique_signals`: unique signal_id count in the group.
- averages: arithmetic mean, 0 if group empty.
- min/max expected net edge: min/max of expected_net_edge_bps, 0 if group empty.

Sort rows by:

```text
stage ascending
entry_geometry_mode ascending
reason ascending
```

Rules:

- Write header even if no rejections.
- CSV escaping.

## New report 3 - monthly_summary.csv

Purpose:

Show whether a strategy fails everywhere or only in specific months.

CSV header:

```csv
month,trades,wins,losses,win_rate,gross_pnl,fee,slippage,total_cost,net_pnl,profit_factor,avg_win,avg_loss,expectancy,max_consecutive_losses,avg_reward_risk,avg_expected_edge_bps,avg_actual_edge_bps,avg_edge_realization_bps,avg_total_cost_bps,take_profit_count,stop_loss_count,time_exit_count,end_of_backtest_count
```

Definitions:

- `month`: `YYYY-MM` from trade entry_time.
- `wins`: net_pnl > 0.
- `losses`: net_pnl <= 0.
- `win_rate`: wins / trades * 100.
- `profit_factor`: gross winners / abs(gross losers). If no gross losers and winners exist, write `"inf"` or a large stable value. Prefer `"inf"` string if existing report style uses it.
- `avg_win`: average net_pnl of winning trades, 0 if no wins.
- `avg_loss`: average net_pnl of losing trades, 0 if no losses.
- `expectancy`: net_pnl / trades.
- `max_consecutive_losses`: within that month only, based on net_pnl <= 0.
- `avg_reward_risk`: average reward_risk.
- `avg_expected_edge_bps`: average expected_edge_bps.
- `avg_actual_edge_bps`: average actual_edge_bps.
- `avg_edge_realization_bps`: average actual_edge_bps - expected_edge_bps.
- `avg_total_cost_bps`: average `(fee + slippage) / entry_notional * 10000`.
- exit reason counts based on TradeExitReason.

Sort rows by month ascending.

Rules:

- Write header even if no trades.
- Use stable numeric formatting, preferably 6 decimals.

## New report 4 - cost_edge_distribution.csv

Purpose:

Show whether reward is too small relative to cost.

This can be a bucketed distribution.

CSV header:

```csv
bucket,trades,wins,losses,win_rate,avg_expected_edge_bps,avg_actual_edge_bps,avg_edge_realization_bps,avg_total_cost_bps,avg_net_pnl_bps,net_pnl
```

Bucket by expected_edge_bps.

Recommended buckets:

```text
edge_lt_0
edge_0_5
edge_5_10
edge_10_15
edge_15_20
edge_20_30
edge_30_50
edge_gte_50
```

Bucket rules:

- `edge_lt_0`: expected_edge_bps < 0
- `edge_0_5`: 0 <= edge < 5
- `edge_5_10`: 5 <= edge < 10
- `edge_10_15`: 10 <= edge < 15
- `edge_15_20`: 15 <= edge < 20
- `edge_20_30`: 20 <= edge < 30
- `edge_30_50`: 30 <= edge < 50
- `edge_gte_50`: edge >= 50

Include all buckets even if count = 0.

Definitions:

- wins/losses based on net_pnl.
- avg total cost bps uses `(fee + slippage) / entry_notional * 10000`.

Sort rows in the exact bucket order above.

## New report 5 - trade_distribution_summary.json

Purpose:

Single compact JSON to summarize the failure mode.

Fields in deterministic order:

```json
{
  "total_trades": 0,
  "winning_trades": 0,
  "losing_trades": 0,
  "win_rate": 0.0,
  "gross_pnl": 0.0,
  "fee": 0.0,
  "slippage": 0.0,
  "total_cost": 0.0,
  "net_pnl": 0.0,
  "avg_expected_edge_bps": 0.0,
  "avg_actual_edge_bps": 0.0,
  "avg_edge_realization_bps": 0.0,
  "avg_total_cost_bps": 0.0,
  "cost_to_gross_loss_ratio": 0.0,
  "signals_generated": 0,
  "signals_preapproved": 0,
  "signals_rejected_initial_risk": 0,
  "signals_rejected_actual_entry": 0,
  "trades_opened": 0,
  "trades_closed": 0,
  "risk_rejections": 0,
  "dominant_rejection_reason": "",
  "dominant_rejection_count": 0
}
```

Definitions:

- `cost_to_gross_loss_ratio`: `(fee + slippage) / abs(gross_pnl)`, or 0 if gross_pnl == 0.
- `dominant_rejection_reason`: highest count reason from risk rejections.
- `dominant_rejection_count`: count for that reason.
- If no rejections, reason = empty string and count = 0.

Manual JSON formatting is fine.

## Timestamp month conversion

The project currently avoids external dependencies.

Implement a deterministic UTC month conversion from millisecond timestamp to `YYYY-MM`.

Preferred: add a small helper using civil date conversion from days since Unix epoch.

You can implement Howard Hinnant's civil-from-days algorithm or an equivalent deterministic algorithm in pure Rust.

Function:

```rust
fn month_key_from_ms(timestamp_ms: i64) -> String
```

Requirements:

- Input is Unix milliseconds.
- Output format `YYYY-MM`.
- Must handle 2024 correctly.
- Must not use system timezone.
- Must not use system time.
- Do not add chrono dependency unless the repository already uses it. Prefer no dependencies.

Tests:

- 1704067200000 -> `2024-01`
- 1706745600000 -> `2024-02`
- 1711929600000 -> `2024-04`
- 1735689540000 or equivalent late 2024 timestamp -> `2024-12`

## Update manifest

Update `src/report/manifest.rs`.

Add files:

```text
reports/signal_diagnostics.csv
reports/rejection_by_stage_reason.csv
reports/monthly_summary.csv
reports/cost_edge_distribution.csv
reports/trade_distribution_summary.json
```

Row counts:

- `signal_diagnostics.csv`: trades.len()
- `rejection_by_stage_reason.csv`: grouped rejection row count
- `monthly_summary.csv`: number of month rows
- `cost_edge_distribution.csv`: always 8 rows
- `trade_distribution_summary.json`: 1

If manifest build needs the diagnostic report as input, update signature cleanly.

Keep paths relative.

Keep sorting by path ascending.

## Integration with research command

Update `src/research/mod.rs`.

After building attribution and audit, build diagnostics:

```rust
let diagnostics = DiagnosticEngine::build(
    &result.trades,
    &result.risk_rejections,
    &result.signal_flow,
);
```

Write diagnostics:

```rust
DiagnosticWriter::write_all(&cfg.reports_dir, &diagnostics)
```

Print concise output:

```text
Diagnostic reports written:
  reports/.../signal_diagnostics.csv
  reports/.../rejection_by_stage_reason.csv
  reports/.../monthly_summary.csv
  reports/.../cost_edge_distribution.csv
  reports/.../trade_distribution_summary.json
```

Also print a small diagnostic summary:

```text
Diagnostics:
  avg total cost bps:        X
  avg edge realization bps:  X
  dominant rejection:        expected_net_edge_not_positive (N)
```

Do not panic on diagnostic file write failure. Follow existing warning style.

## Documentation update

Update README.md.

Add a short section under Phase 7 or reporting:

```markdown
### Strategy diagnostics

Northflow writes extra diagnostic reports for research analysis:

- `signal_diagnostics.csv` — one row per trade with cost bps, edge realization, month, and filters.
- `rejection_by_stage_reason.csv` — grouped risk rejection reasons by stage and geometry mode.
- `monthly_summary.csv` — monthly PnL, win rate, cost, edge, and exit reason summary.
- `cost_edge_distribution.csv` — buckets trades by expected edge bps.
- `trade_distribution_summary.json` — compact summary of costs, edge realization, and dominant rejection reason.

These reports are diagnostic only. They do not tune parameters and do not imply profitability.
```

Update docs/DATA_DOWNLOAD.md expected report files list to include the new diagnostics.

Do not rewrite the whole documents.

Do not remove existing content.

## Required tests

Add focused tests.

### Month key tests

- month_key_from_ms_2024_01
- month_key_from_ms_2024_02
- month_key_from_ms_2024_12

### Signal diagnostics tests

- signal_diagnostics_empty_trades_writes_header
- signal_diagnostics_row_computes_total_cost
- signal_diagnostics_row_computes_cost_bps
- signal_diagnostics_row_computes_edge_realization
- signal_diagnostics_row_has_month_key
- signal_diagnostics_csv_escapes_filters_and_reason

### Rejection grouped tests

- rejection_by_stage_reason_groups_by_stage_mode_reason
- rejection_by_stage_reason_counts_unique_signals
- rejection_by_stage_reason_averages_cost_and_edge
- rejection_by_stage_reason_sorts_stably

### Monthly summary tests

- monthly_summary_groups_by_month
- monthly_summary_computes_win_rate
- monthly_summary_computes_profit_factor
- monthly_summary_counts_exit_reasons
- monthly_summary_computes_avg_total_cost_bps
- monthly_summary_sorts_by_month

### Cost edge distribution tests

- cost_edge_distribution_includes_all_buckets
- cost_edge_distribution_assigns_edge_10_15_bucket
- cost_edge_distribution_assigns_edge_gte_50_bucket
- cost_edge_distribution_computes_avg_net_pnl_bps

### JSON summary tests

- trade_distribution_summary_empty_is_zero
- trade_distribution_summary_calculates_total_cost
- trade_distribution_summary_finds_dominant_rejection
- trade_distribution_summary_uses_signal_flow_counts

### Writer and manifest tests

- diagnostic_writer_writes_all_files
- manifest_includes_diagnostic_files
- manifest_counts_cost_edge_distribution_as_8_rows

Existing tests must continue passing.

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

Expected after real BTCUSDT run:

- Existing reports still generated.
- New diagnostic files generated.
- Manifest includes new diagnostic files.
- Audit still passes.
- No paper/live/exchange/LLM behavior added.

## Strictly forbidden

Do not implement:

- strategy tuning
- new indicators
- new strategy rules
- parameter optimization
- automatic optimization
- paper trading
- live trading
- exchange adapter
- websocket
- database
- dashboard
- Telegram
- LLM trading decisions
- AI advisor
- any profitability claim

This is diagnostics only.

## Expected final result

At the end of this patch:

- Northflow can diagnose why a strategy loses before tuning.
- Monthly performance is visible.
- Cost vs edge distribution is visible.
- Risk rejection stages and reasons are grouped.
- Per-trade diagnostic rows show edge realization and cost bps.
- Manifest includes diagnostics.
- Docs explain diagnostics.
- All tests pass.

## Commit message suggestion

reports: add strategy diagnostics
