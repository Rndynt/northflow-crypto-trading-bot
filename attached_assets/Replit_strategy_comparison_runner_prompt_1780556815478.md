# Northflow Backtest Strategy Comparison Runner Patch Prompt

You are working on this repository:

https://github.com/Rndynt/northflow-crypto-trading-bot

Your task is to implement a focused backtest strategy comparison runner.

This patch belongs to the research/backtest layer.

Do not implement live trading, paper trading, exchange APIs, websocket feeds, database, dashboard, Telegram, LLM trading decisions, AI advisor, optimizer, or auto-tuning.

Do not change existing strategy logic, indicator formulas, risk formulas, or fill model formulas.

This patch is only for making backtest strategy execution more generic:

- single strategy backtest
- multi-strategy comparison backtest
- future-reserved multi-strategy portfolio mode, but not implemented yet

## Current state

The repository currently supports switching one active strategy from config:

```toml
[strategy]
strategy_id = "screened_vwap_scalp_v2"
```

Current valid strategy IDs:

```text
screened_vwap_scalp
screened_vwap_scalp_v2
```

The backtest engine currently runs exactly one strategy at a time. This is useful but inefficient for research because every strategy comparison requires manual config edits and report folders can be overwritten.

## Goal

Add `[backtest]` strategy runner config:

```toml
[backtest]
strategy_run_mode = "single"
strategies = ["screened_vwap_scalp_v2"]
```

Supported run modes:

```text
single
comparison
multi
```

Meaning:

### single

Run exactly one strategy.

- Uses `[backtest].strategies[0]` if present.
- Falls back to legacy `[strategy].strategy_id` if `strategies` is missing.
- Writes reports directly to `reports_dir`, preserving current behavior.

### comparison

Run multiple strategies one by one.

- Each strategy gets a fresh independent backtest.
- Each strategy gets its own starting equity.
- Each strategy has its own risk state.
- Each strategy has its own report subfolder.
- This is not portfolio simulation.
- This is not multiple strategies sharing one account.
- This is only research comparison.

Example output:

```text
reports/comparison/screened_vwap_scalp/backtest_summary.json
reports/comparison/screened_vwap_scalp/trades.csv
reports/comparison/screened_vwap_scalp_v2/backtest_summary.json
reports/comparison/screened_vwap_scalp_v2/trades.csv
reports/comparison/comparison_summary.csv
reports/comparison/comparison_summary.json
```

### multi

Reserved for future multi-strategy portfolio backtest.

For now:

- Parse the value.
- Return a clear `ConfigError` if used.
- Do not silently fall back.
- Do not implement portfolio router.

Error message should say:

```text
multi-strategy portfolio backtest is not implemented yet; use strategy_run_mode = "comparison"
```

## Why comparison mode must not be portfolio mode

Do not run strategies simultaneously in one shared account in this patch.

Portfolio mode requires extra rules that do not exist yet:

- conflict resolution if one strategy is long and another is short
- deduplication if several strategies emit the same direction
- risk allocation by strategy
- max open positions global vs per strategy
- shared drawdown guard vs per-strategy drawdown guard
- attribution when equity is shared
- strategy priority
- signal router

This patch must avoid pretending those rules exist.

## Files to read first

Read these files before changing anything:

- AGENTS.md
- docs/ROADMAP.md
- README.md
- docs/DATA_DOWNLOAD.md
- docs/STRATEGY_RESEARCH.md
- config/research.toml
- src/config/mod.rs
- src/research/mod.rs
- src/backtest/engine.rs
- src/backtest/mod.rs
- src/backtest/report.rs
- src/report/diagnostics.rs
- src/report/attribution.rs
- src/report/manifest.rs
- src/strategy/mod.rs
- src/strategy/screened_vwap_scalp.rs
- src/strategy/screened_vwap_scalp_v2.rs

## Required config changes

Update `ResearchConfig` in `src/config/mod.rs`.

Add fields:

```rust
pub strategy_run_mode: String,
pub strategies: Vec<String>,
```

Defaults:

```rust
strategy_run_mode = "single"
strategies = vec![]
```

Update parser to read:

```toml
[backtest]
strategy_run_mode = "comparison"
strategies = ["screened_vwap_scalp", "screened_vwap_scalp_v2"]
```

Important:

- Keep existing `[strategy].strategy_id` for backward compatibility.
- Existing configs must still work.
- If `strategies` is missing, `single` mode must use `strategy_id`.
- If `strategy_run_mode` is missing, default to `single`.
- If `strategy_run_mode = "single"` and `strategies` contains more than one item, return `ConfigError` to avoid surprise.
- If `strategy_run_mode = "comparison"`, `strategies` must contain at least one valid strategy.
- If `strategies` contains duplicates, return `ConfigError` with a clear message.
- Unknown strategy IDs must return `ConfigError`.
- Unknown `strategy_run_mode` must return `ConfigError`.
- `strategy_run_mode = "multi"` must return `ConfigError` because portfolio mode is reserved.

Valid run modes:

```text
single
comparison
multi
```

Valid strategy IDs currently:

```text
screened_vwap_scalp
screened_vwap_scalp_v2
```

Add helper methods:

```rust
impl ResearchConfig {
    pub fn selected_strategies(&self) -> Result<Vec<String>, NorthflowError>;
    pub fn validate_strategy_runner_config(&self) -> Result<(), NorthflowError>;
    pub fn with_strategy_for_run(&self, strategy_id: &str, reports_dir: String) -> Self;
}
```

Meaning:

### selected_strategies

Returns the list of strategies that should be run based on mode and fallback rules.

### validate_strategy_runner_config

Validates run mode, strategy list, unknown strategy IDs, duplicate strategy IDs, and multi reserved behavior.

### with_strategy_for_run

Returns a cloned config with:

```rust
strategy_id = strategy_id.to_string()
reports_dir = reports_dir
```

This allows the existing `BacktestEngine::run(&cfg, symbol)` to remain mostly unchanged.

## Required config/research.toml update

Update default `config/research.toml` to show comparison usage but keep default safe.

Recommended default:

```toml
[strategy]
strategy_id = "screened_vwap_scalp_v2"

[backtest]
strategy_run_mode = "single"
strategies = ["screened_vwap_scalp_v2"]
reports_dir = "reports"
entry_geometry_mode = "reanchor_to_actual_entry"

# For research comparison:
# strategy_run_mode = "comparison"
# strategies = ["screened_vwap_scalp", "screened_vwap_scalp_v2"]
# reports_dir = "reports/comparison"
```

Do not remove existing fields.

## Required research runner behavior

Update `src/research/mod.rs`.

Currently research likely loops:

```rust
for symbol in cfg.symbols {
    run_symbol(&cfg, symbol)
}
```

Change it so it handles strategy run modes cleanly.

Recommended structure:

```rust
pub fn run_research(config_path: &str) -> Result<(), NorthflowError> {
    let cfg = ResearchConfig::load(config_path)?;
    cfg.validate_timeframes()?;
    cfg.validate_strategy_config()?;
    cfg.validate_strategy_runner_config()?;

    match cfg.strategy_run_mode.as_str() {
        "single" => run_single_strategy(&cfg),
        "comparison" => run_strategy_comparison(&cfg),
        "multi" => Err(ConfigError(...)),
        other => Err(ConfigError(...)),
    }
}
```

If current function returns `()`, keep existing external behavior but internally print errors clearly. Prefer not to break CLI.

### single mode

Preserve current behavior:

- one strategy
- current report output style
- reports written directly to `cfg.reports_dir`

### comparison mode

For each symbol and each strategy:

- clone config
- set active strategy
- set reports dir to `<base_reports_dir>/<strategy_id>` for one symbol
- for multiple symbols, set reports dir to `<base_reports_dir>/<symbol>/<strategy_id>`

Examples:

For one symbol:

```text
reports/comparison/screened_vwap_scalp
reports/comparison/screened_vwap_scalp_v2
```

For multiple symbols:

```text
reports/comparison/BTCUSDT/screened_vwap_scalp
reports/comparison/BTCUSDT/screened_vwap_scalp_v2
reports/comparison/ETHUSDT/screened_vwap_scalp
```

Important:

- Do not overwrite one strategy's report with another.
- Run each strategy independently from initial equity.
- Do not share state between strategies.
- Each strategy should produce the full existing report set.

## Required comparison summary output

After comparison mode completes, write:

```text
<base_reports_dir>/comparison_summary.csv
<base_reports_dir>/comparison_summary.json
```

If multiple symbols are used, still write one root summary with rows for every symbol-strategy pair.

### comparison_summary.csv header

```csv
symbol,strategy_id,reports_dir,status,error,total_trades,win_rate,net_pnl,gross_pnl,total_fee,total_slippage,total_cost,profit_factor,expectancy,max_drawdown,max_consecutive_losses,avg_expected_edge_bps,avg_actual_edge_bps,avg_edge_realization_bps,avg_total_cost_bps,signals_generated,signals_preapproved,signals_rejected_initial_risk,signals_rejected_actual_entry,trades_opened,trades_closed,risk_rejections,dominant_rejection_reason,dominant_rejection_count
```

Use data from:

- BacktestResult.summary
- DiagnosticReport.trade_distribution
- SignalFlowSummary

If status is error, set numeric fields to 0 and put escaped error message in `error`.

### comparison_summary.json

Deterministic JSON:

```json
{
  "mode": "comparison",
  "runs": [
    {
      "symbol": "BTCUSDT",
      "strategy_id": "screened_vwap_scalp",
      "reports_dir": "reports/comparison/screened_vwap_scalp",
      "status": "ok",
      "error": "",
      "total_trades": 0,
      "win_rate": 0.0,
      "net_pnl": 0.0,
      "gross_pnl": 0.0,
      "total_fee": 0.0,
      "total_slippage": 0.0,
      "total_cost": 0.0,
      "profit_factor": 0.0,
      "expectancy": 0.0,
      "max_drawdown": 0.0,
      "max_consecutive_losses": 0,
      "avg_expected_edge_bps": 0.0,
      "avg_actual_edge_bps": 0.0,
      "avg_edge_realization_bps": 0.0,
      "avg_total_cost_bps": 0.0,
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
  ]
}
```

Manual JSON formatting is okay. No serde dependency required.

## Recommended module

Add:

```text
src/research/comparison.rs
```

because this is runner orchestration, not core engine.

Types:

```rust
pub struct ComparisonRunResult {
    pub symbol: String,
    pub strategy_id: String,
    pub reports_dir: String,
    pub status: String,
    pub error: String,
    ...
}

pub struct ComparisonSummary {
    pub runs: Vec<ComparisonRunResult>,
}

pub struct ComparisonWriter;

impl ComparisonWriter {
    pub fn write_all(base_reports_dir: &str, summary: &ComparisonSummary) -> Result<(), NorthflowError>;
}
```

Keep it simple.

## Required integration with existing report pipeline

Avoid duplicating the whole single-strategy report writing code.

If `run_symbol()` currently does everything including:

- BacktestEngine::run
- ReportWriter::write_all
- AttributionWriter::write_all
- DiagnosticWriter::write_all

Refactor cleanly:

```rust
fn run_symbol_strategy(cfg: &ResearchConfig, symbol: &str) -> Result<Option<CompletedResearchRun>, NorthflowError>
```

Where `CompletedResearchRun` contains:

- symbol
- strategy_id
- reports_dir
- backtest summary
- signal flow
- diagnostic summary

Then:

- single mode calls it once per symbol
- comparison mode calls it for every symbol/strategy combination
- comparison writer builds aggregate summary

Do not break current CLI output.

## Required CLI output

### single mode

Print:

```text
Backtest run mode: single
Strategy:
  strategy_id = screened_vwap_scalp_v2
Reports dir: reports
```

### comparison mode

Print:

```text
Backtest run mode: comparison
Strategies:
  - screened_vwap_scalp
  - screened_vwap_scalp_v2
Base reports dir: reports/comparison
```

For each run:

```text
Running comparison strategy:
  symbol: BTCUSDT
  strategy_id: screened_vwap_scalp
  reports_dir: reports/comparison/screened_vwap_scalp
```

At end:

```text
Comparison summary written:
  reports/comparison/comparison_summary.csv
  reports/comparison/comparison_summary.json
```

## Required documentation

Update README.md.

Add short section:

```markdown
### Backtest strategy run modes

Northflow supports:

- `single` — run one active strategy.
- `comparison` — run multiple strategies one by one with independent equity and separate report folders.
- `multi` — reserved for future shared-account multi-strategy portfolio backtesting; currently returns a config error.

Example:

```toml
[backtest]
strategy_run_mode = "comparison"
strategies = ["screened_vwap_scalp", "screened_vwap_scalp_v2"]
reports_dir = "reports/comparison"
```

Comparison mode is not portfolio simulation. Each strategy run starts from the same initial equity and does not share positions, drawdown, or risk state with other strategies.
```

Update `docs/STRATEGY_RESEARCH.md`.

Add:

- how to run single strategy
- how to run comparison mode
- how to interpret comparison_summary.csv
- why comparison is not multi-strategy portfolio
- future note for multi mode

Do not remove existing content.

## Required tests

Add focused tests.

### Config tests

- parses_strategy_run_mode_single
- parses_strategy_run_mode_comparison
- parses_backtest_strategies_array
- selected_strategies_falls_back_to_strategy_id
- selected_strategies_uses_backtest_strategies
- rejects_unknown_strategy_run_mode
- rejects_duplicate_strategies
- rejects_unknown_strategy_in_strategies
- rejects_multi_mode_as_not_implemented
- single_mode_rejects_multiple_strategies

### Comparison writer tests

- comparison_summary_csv_header_is_stable
- comparison_summary_csv_writes_ok_row
- comparison_summary_csv_writes_error_row
- comparison_summary_json_writes_runs
- comparison_summary_csv_escapes_errors
- comparison_summary_paths_are_relative

### Research runner tests

If existing structure makes integration tests possible:

- comparison_mode_runs_each_strategy_once using small fixture
- comparison_mode_writes_per_strategy_reports using small fixture
- single_mode_preserves_current_reports_dir

If integration tests are too heavy, unit-test path building and summary writing.

Existing tests must continue passing.

## Required commands

Run:

```bash
cargo fmt
cargo build
cargo test
cargo run -- help
```

If `data/historical/BTCUSDT.csv` exists, run:

### Single

```toml
[backtest]
strategy_run_mode = "single"
strategies = ["screened_vwap_scalp_v2"]
reports_dir = "reports/single_v2"
entry_geometry_mode = "reanchor_to_actual_entry"
```

```bash
cargo run --release -- research --config config/research.toml
```

### Comparison

```toml
[backtest]
strategy_run_mode = "comparison"
strategies = ["screened_vwap_scalp", "screened_vwap_scalp_v2"]
reports_dir = "reports/comparison"
entry_geometry_mode = "reanchor_to_actual_entry"
```

```bash
cargo run --release -- research --config config/research.toml
```

Expected:

- reports/comparison/screened_vwap_scalp/* generated
- reports/comparison/screened_vwap_scalp_v2/* generated
- reports/comparison/comparison_summary.csv generated
- reports/comparison/comparison_summary.json generated
- existing report files still generated per strategy
- no paper/live/exchange/LLM behavior

## Strictly forbidden

Do not implement:

- shared-account multi-strategy portfolio mode
- strategy router
- strategy priority
- signal conflict resolver
- capital allocation engine
- auto optimizer
- grid search
- genetic algorithm
- walk-forward optimization
- paper trading
- live trading
- exchange order placement
- exchange adapter
- websocket
- database
- dashboard
- Telegram
- LLM signal generation
- AI advisor
- profitability claims

This patch is a research comparison runner only.

## Expected final result

At the end of this patch:

- `[backtest].strategy_run_mode` exists.
- `[backtest].strategies` exists.
- Existing `[strategy].strategy_id` still works.
- Single mode preserves current behavior.
- Comparison mode runs many strategies independently.
- Each strategy gets separate report folder.
- Root comparison summary files are generated.
- Multi mode is explicitly reserved and returns ConfigError.
- Docs explain single vs comparison vs future multi.
- All tests pass.

## Commit message suggestion

research: add strategy comparison runner
