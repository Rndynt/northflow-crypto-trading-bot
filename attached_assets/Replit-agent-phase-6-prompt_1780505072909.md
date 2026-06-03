# Northflow Phase 6 Build Prompt

You are working on this repository:

https://github.com/Rndynt/northflow-crypto-trading-bot

Your task is to implement Phase 6: Backtest Engine.

Read these files first:

- AGENTS.md
- docs/ROADMAP.md
- README.md
- config/research.toml
- src/config/mod.rs
- src/core/candle.rs
- src/core/signal.rs
- src/core/trade.rs
- src/core/position.rs
- src/core/side.rs
- src/core/symbol.rs
- src/core/timeframe.rs
- src/core/error.rs
- src/market/mod.rs
- src/market/candle_store.rs
- src/market/ohlcv_loader.rs
- src/indicators/mod.rs
- src/indicators/snapshot.rs
- src/strategy/mod.rs
- src/strategy/screened_vwap_scalp.rs
- src/risk/mod.rs
- src/risk/guard.rs
- src/risk/cost_model.rs
- src/risk/position_sizing.rs
- src/research/mod.rs

Do not ignore the repository documentation.

## Project mission

Northflow is a deterministic research-first crypto trading engine.

Northflow is not:

- a dashboard
- a React app
- a Telegram bot
- an AI trading agent
- a live trading system
- a paper trading loop
- a strategy optimizer
- a portfolio optimizer

The current goal is to implement a truthful historical backtest engine that replays validated candles, computes indicators, evaluates strategy signals, validates risk, simulates fills conservatively, updates equity, and writes deterministic report files.

Phase 6 is still research only.

Do not implement live trading.

Do not implement paper trading.

Do not call exchange APIs.

Do not call LLMs.

Do not create a dashboard.

## Current phase

Implement:

Phase 6 - Backtest Engine

Target structure from docs/ROADMAP.md:

- src/backtest/mod.rs
- src/backtest/engine.rs
- src/backtest/fill_model.rs
- src/backtest/metrics.rs
- src/backtest/report.rs
- src/backtest/walk_forward.rs

Update:

- src/lib.rs
- src/research/mod.rs
- src/config/mod.rs
- config/research.toml
- README.md

only as needed.

## Required Phase 6 flow

The backtest flow must follow the roadmap:

1. Load 1m CSV.
2. Build 5m and 15m candles.
3. Update indicators.
4. Compute 15m regime.
5. Compute 5m confirmation.
6. Evaluate 1m strategy.
7. Validate signal with risk model.
8. Simulate entry.
9. Simulate SL / TP / time exit.
10. Apply fees and slippage.
11. Write trade row.
12. Update equity curve.
13. Export reports.

The command:

cargo run -- research --config config/research.toml

must produce these files when valid data exists:

- reports/backtest_summary.json
- reports/trades.csv
- reports/equity_curve.csv

If no historical CSV exists, keep the existing friendly message and do not panic.

## Important boundary

Backtest simulation is allowed in Phase 6.

But Phase 6 must not:

- place real orders
- call any exchange API
- use websockets
- use live data
- use paper trading loops
- mutate external account state
- use LLMs
- optimize parameters
- claim future profitability

Backtest results are historical simulation only.

Do not add AI trading decisions.

Do not add Telegram.

Do not add dashboard.

## Required exports

Create src/backtest/mod.rs and export:

pub mod engine;
pub mod fill_model;
pub mod metrics;
pub mod report;
pub mod walk_forward;

pub use engine::*;
pub use fill_model::*;
pub use metrics::*;
pub use report::*;
pub use walk_forward::*;

Update src/lib.rs:

pub mod backtest;

## Backtest config

Add a small BacktestConfig type in src/backtest/engine.rs or src/backtest/mod.rs.

Recommended:

pub struct BacktestConfig {
    pub initial_equity: f64,
    pub reports_dir: String,
    pub conservative_intrabar: bool,
    pub max_bars_held: u32,
}

Add max_bars_held to ResearchConfig if it does not exist.

Recommended default:

max_bars_held = 60

Update config/research.toml:

[backtest]
max_bars_held = 60

Parsing rule:

- If max_bars_held is missing, default to 60.
- max_bars_held must be > 0 when used.

Do not break existing config parsing.

## No-lookahead rule

This is critical.

Do not use future candles or incomplete higher timeframe candles.

Candle timestamps represent bucket start time.

A 5m candle with timestamp T covers:

T <= time < T + 300000 ms

It is only available after:

T + 300000 ms

A 15m candle with timestamp T covers:

T <= time < T + 900000 ms

It is only available after:

T + 900000 ms

For a 1m entry candle with timestamp T, the signal decision is made at the close of that candle:

signal_time = T + 60000 ms

At signal_time, the engine may use only:

- 5m candles whose timestamp + 300000 <= signal_time
- 15m candles whose timestamp + 900000 <= signal_time

Never use a 5m or 15m candle that has not closed yet.

Never use future 1m candles to compute a signal.

Entry must occur on the next 1m candle open after the signal candle.

If there is no next candle, no entry is created.

## Indicator snapshot preparation

Use the existing IndicatorEngine from src/indicators/snapshot.rs.

For each timeframe:

- 1m: update an IndicatorEngine as the 1m stream is replayed.
- 5m: build snapshots from completed 5m candles.
- 15m: build snapshots from completed 15m candles.

Recommended implementation:

1. Precompute 5m snapshots by iterating sorted 5m candles through IndicatorEngine.
2. Store by candle timestamp in a Vec or BTreeMap.
3. Precompute 15m snapshots similarly.
4. During 1m replay, select the latest completed 5m and 15m snapshot using close-time availability.
5. If no completed 5m or 15m snapshot exists yet, skip strategy evaluation for that 1m candle.

Do not infer timeframe roles from vector order.

Use explicit roles:

- entry = 1m
- confirmation = 5m
- screening = 15m

## Strategy integration

Use existing strategy:

ScreenedVwapScalp

Use existing types:

- StrategyContext
- MultiTimeframeInput
- Strategy trait
- Signal
- RiskEngine
- RiskContext
- RiskConfig
- CostModelConfig

For each eligible 1m signal candle:

1. Build MultiTimeframeInput:
   - entry_candle = current 1m candle
   - confirmation_candle = latest completed 5m candle
   - screening_candle = latest completed 15m candle
   - entry_indicators = current 1m snapshot
   - confirmation_indicators = latest completed 5m snapshot
   - screening_indicators = latest completed 15m snapshot

2. Evaluate ScreenedVwapScalp.
3. If strategy returns Ok(None), continue.
4. If strategy returns Ok(Some(signal)):
   - increment deterministic signal counter
   - assess signal with RiskEngine
   - if rejected, do not enter a trade
   - if approved, simulate entry on next 1m candle open
5. If strategy returns Err, return the error.

Signal IDs must remain deterministic:

SIG-BT-00000001
SIG-BT-00000002

Do not use UUIDs.

Do not use system time.

## Position handling

Phase 6 initial implementation may support one open position at a time.

The default risk config uses max_open_positions = 1.

If a position is open, do not evaluate new entries until it is closed.

This keeps the backtest deterministic and aligned with initial roadmap risk settings.

Do not implement portfolio-level multi-position netting in Phase 6.

## Fill model requirements

Implement src/backtest/fill_model.rs.

Create deterministic fill simulation types.

Recommended types:

pub struct EntryFill {
    pub time: i64,
    pub price: f64,
    pub qty: f64,
    pub fee: f64,
    pub slippage: f64,
}

pub struct ExitFill {
    pub time: i64,
    pub price: f64,
    pub fee: f64,
    pub slippage: f64,
    pub reason: TradeExitReason,
    pub bars_held: u32,
}

pub struct OpenSimPosition {
    pub signal: Signal,
    pub qty: f64,
    pub entry_time: i64,
    pub entry_price: f64,
    pub entry_fee: f64,
    pub entry_slippage: f64,
    pub bars_held: u32,
}

pub struct FillModel;

Entry simulation:

- A signal generated on candle i is entered on candle i+1 open.
- If there is no next candle, skip entry.
- Use adverse slippage:
  - Long entry price = next_open * (1 + slippage_bps / 10000)
  - Short entry price = next_open * (1 - slippage_bps / 10000)
- Entry fee = entry_price * qty * taker_fee_bps / 10000.
- Entry slippage cost = abs(entry_price - next_open) * qty.
- Do not apply spread twice if you also include it later; for Phase 6, include spread/market impact as additional exit/trade cost components in final trade calculation.

Exit simulation:

For each following 1m candle after entry, test SL / TP:

Long:
- stop_touched = candle.low <= signal.stop_loss
- tp_touched = candle.high >= signal.take_profit

Short:
- stop_touched = candle.high >= signal.stop_loss
- tp_touched = candle.low <= signal.take_profit

Conservative intrabar rule:

If stop-loss and take-profit are both touched in the same candle, assume stop-loss was hit first.

This is mandatory.

Exit price base:

- StopLoss exits at signal.stop_loss
- TakeProfit exits at signal.take_profit
- TimeExit exits at candle.close
- EndOfBacktest exits at last candle.close

Apply adverse slippage:

Long:
- Exit sell price = base_exit_price * (1 - slippage_bps / 10000)

Short:
- Exit buy-to-cover price = base_exit_price * (1 + slippage_bps / 10000)

Exit fee = exit_price * qty * taker_fee_bps / 10000.

Exit slippage cost = abs(exit_price - base_exit_price) * qty.

Additional adverse costs for final trade:

- spread cost = avg_notional * spread_bps / 10000
- market impact cost = avg_notional * market_impact_bps / 10000
- stop slippage cost applies only when exit_reason == StopLoss:
  avg_notional * stop_slippage_bps / 10000

Do not call CostModel as a black box if it double counts entry/exit slippage already applied. Either use it carefully or compute these components explicitly in the fill model.

The final trade fee field should include:

entry_fee + exit_fee

The final trade slippage field should include:

entry_slippage + exit_slippage + spread_cost + market_impact_cost + optional stop_slippage_cost

## PnL calculation

For a Long:

gross_pnl = (exit_price - entry_price) * qty

For a Short:

gross_pnl = (entry_price - exit_price) * qty

net_pnl = gross_pnl - fee - slippage

actual_edge_bps = net_pnl / entry_notional * 10000

entry_notional = entry_price * qty

All values must be finite.

## Trade record

Use existing core Trade type from src/core/trade.rs.

Each closed trade must contain:

- trade_id
- signal_id
- position_id
- symbol
- strategy_id
- side
- entry_time
- exit_time
- entry_price
- exit_price
- stop_loss
- take_profit
- quantity
- gross_pnl
- fee
- slippage
- net_pnl
- reward_risk
- bars_held
- exit_reason
- entry_reason
- filters_passed
- filters_failed
- expected_edge_bps
- actual_edge_bps

Deterministic IDs:

position_id = POS-<signal_id>
trade_id = TRD-<signal_id>

Example:

signal_id = SIG-BT-00000001
position_id = POS-SIG-BT-00000001
trade_id = TRD-SIG-BT-00000001

Do not use random IDs.

Do not use system time.

## Time exit

Implement max_bars_held.

If a trade remains open for max_bars_held bars and neither SL nor TP was touched:

exit_reason = TimeExit
exit at current candle close with adverse slippage

If the backtest ends while a trade is still open:

exit_reason = EndOfBacktest
exit at the last candle close with adverse slippage

## Equity curve

Implement equity curve tracking.

Recommended type:

pub struct EquityPoint {
    pub timestamp: i64,
    pub equity: f64,
    pub drawdown_pct: f64,
}

Initial equity point may be included at first candle timestamp.

Update equity after each closed trade:

equity += trade.net_pnl

Do not update equity with unrealized PnL in Phase 6.

Peak equity:

peak = max(previous_peak, current_equity)

drawdown_pct = (peak - equity) / peak * 100

Equity must remain finite.

If equity <= 0, stop the backtest and close no more new trades.

## Metrics requirements

Implement src/backtest/metrics.rs.

Recommended type:

pub struct BacktestSummary {
    pub total_trades: usize,
    pub win_rate: f64,
    pub net_pnl: f64,
    pub gross_pnl: f64,
    pub total_fee: f64,
    pub total_slippage: f64,
    pub profit_factor: f64,
    pub expectancy: f64,
    pub avg_win: f64,
    pub avg_loss: f64,
    pub max_drawdown: f64,
    pub max_consecutive_losses: usize,
    pub avg_trade_duration: f64,
}

pub struct Metrics;

impl Metrics {
    pub fn summarize(trades: &[Trade], equity_curve: &[EquityPoint]) -> BacktestSummary;
}

Metric rules:

- total_trades = trades.len()
- win_rate = winning_trades / total_trades * 100, or 0 if no trades
- net_pnl = sum(trade.net_pnl)
- gross_pnl = sum(trade.gross_pnl)
- total_fee = sum(trade.fee)
- total_slippage = sum(trade.slippage)
- profit_factor = sum(winning net_pnl) / abs(sum(losing net_pnl))
  - if no losses and wins > 0, use f64::INFINITY
  - if no wins and no losses, use 0
- expectancy = net_pnl / total_trades, or 0
- avg_win = average positive net_pnl, or 0
- avg_loss = average negative net_pnl, or 0
- max_drawdown = max equity_curve.drawdown_pct, or 0
- max_consecutive_losses = longest streak of net_pnl <= 0
- avg_trade_duration = average trade.duration_seconds(), or 0

## Report writer requirements

Implement src/backtest/report.rs.

Write reports without external dependencies.

No serde is required.

Use std::fs only.

Create reports directory if missing.

Required output files:

- reports/backtest_summary.json
- reports/trades.csv
- reports/equity_curve.csv

Recommended API:

pub struct ReportWriter;

impl ReportWriter {
    pub fn write_all(
        reports_dir: &str,
        summary: &BacktestSummary,
        trades: &[Trade],
        equity_curve: &[EquityPoint],
    ) -> Result<(), NorthflowError>;
}

CSV requirements:

trades.csv header must include at least these fields:

trade_id
signal_id
symbol
strategy_id
regime
side
entry_time
exit_time
entry_price
exit_price
stop_loss
take_profit
qty
gross_pnl
fee
slippage
net_pnl
reward_risk
bars_held
exit_reason
entry_reason
filters_passed
filters_failed
expected_edge_bps
actual_edge_bps

Important:

The existing Trade type does not currently have a regime field.

Use one of these approaches:

Preferred:
- Add regime: String to Trade if it does not break existing tests.
- Populate it from signal.regime.

Alternative:
- When writing trades.csv, use signal regime stored separately in a BacktestTradeRow wrapper.

For simplicity and attribution, prefer adding regime to Trade and updating existing Trade tests.

CSV escaping:

- Escape commas, quotes, and newlines.
- Join filters_passed and filters_failed using pipe character: filter_a|filter_b.
- Quote fields when needed.

summary JSON must include at least:

total_trades
win_rate
net_pnl
gross_pnl
total_fee
total_slippage
profit_factor
expectancy
avg_win
avg_loss
max_drawdown
max_consecutive_losses
avg_trade_duration

Manual JSON string formatting is acceptable.

If profit_factor is infinity, write it as a large string or null.
Recommended:

"profit_factor": "inf"

or use a finite number when finite.

equity_curve.csv header:

timestamp,equity,drawdown_pct

## Backtest engine requirements

Implement src/backtest/engine.rs.

Recommended types:

pub struct BacktestResult {
    pub trades: Vec<Trade>,
    pub equity_curve: Vec<EquityPoint>,
    pub summary: BacktestSummary,
}

pub struct BacktestEngine;

impl BacktestEngine {
    pub fn run(
        cfg: &ResearchConfig,
        symbol: &str,
    ) -> Result<Option<BacktestResult>, NorthflowError>;
}

Behavior:

- If the CSV is missing, return Ok(None), not Err.
- If data quality has errors, either:
  - continue with valid candles but print/report issues in research CLI, or
  - return Err for data quality errors.
- Prefer conservative behavior:
  - if data quality has errors, return Err explaining the data must be fixed before backtest.
  - missing gaps may remain warnings unless DataQualityReport treats them as errors.
- Build CandleStore from valid 1m candles.
- Precompute indicator snapshots for 1m, 5m, and 15m.
- Replay 1m candles in chronological order.
- Build explicit MultiTimeframeInput without lookahead.
- Evaluate ScreenedVwapScalp.
- Assess signal with RiskEngine.
- If approved, simulate entry at next candle open.
- Manage one open position at a time.
- Simulate exits using conservative intrabar rule.
- Update equity after trade closes.
- Return BacktestResult with trades, equity_curve, summary.

Do not write reports inside lower-level functions unless the API is explicitly ReportWriter.
The research module can call ReportWriter after BacktestEngine returns.

## Research CLI integration

Update src/research/mod.rs.

The command:

cargo run -- research --config config/research.toml

must now run the backtest when historical CSV exists.

Expected behavior:

- Print Phase 6 title.
- Validate config.
- For each configured symbol:
  - load data
  - run backtest
  - print data quality summary
  - print total trades
  - print net PnL
  - print max drawdown
- Write:
  - reports/backtest_summary.json
  - reports/trades.csv
  - reports/equity_curve.csv
- Print paths of generated reports.

If no CSV exists, keep friendly message and do not panic.

Do not claim the strategy is profitable.

Do not give trading advice.

## Walk-forward module

Implement src/backtest/walk_forward.rs minimally.

Do not implement optimization.

Do not tune parameters.

Recommended:

pub struct WalkForwardWindow {
    pub train_start: usize,
    pub train_end: usize,
    pub test_start: usize,
    pub test_end: usize,
}

pub fn build_walk_forward_windows(
    total_len: usize,
    train_len: usize,
    test_len: usize,
    step: usize,
) -> Vec<WalkForwardWindow>;

Validation behavior:

- If any length is zero, return empty Vec.
- If total_len < train_len + test_len, return empty Vec.
- Build deterministic rolling windows.

This module is for future Phase 7+ analysis only.

Do not use it to optimize strategy in Phase 6.

## README update

Update README.md to state:

- Current phase is Phase 6.
- Phase 1 core domain is complete.
- Phase 2 market data is complete.
- Phase 3 indicators are complete.
- Phase 4 strategy engine is complete.
- Phase 5 risk and cost model is complete.
- Phase 6 backtest engine is implemented.
- Research command now writes:
  - reports/backtest_summary.json
  - reports/trades.csv
  - reports/equity_curve.csv
- Backtest uses conservative intrabar rule.
- Paper and live modes remain disabled.
- Phase 7 report attribution remains pending if not fully completed.

Do not mark Phase 7 as complete.

## Tests required

Add comprehensive tests.

### Fill model tests

- long_stop_loss_exit
- long_take_profit_exit
- long_both_sl_tp_same_candle_assumes_stop_first
- short_stop_loss_exit
- short_take_profit_exit
- short_both_sl_tp_same_candle_assumes_stop_first
- time_exit_after_max_bars
- end_of_backtest_exit
- entry_uses_next_candle_open
- long_entry_slippage_is_adverse
- short_entry_slippage_is_adverse
- fee_is_applied
- slippage_cost_is_applied

### Metrics tests

- summary_zero_trades
- summary_calculates_total_trades
- summary_calculates_win_rate
- summary_calculates_net_pnl
- summary_calculates_gross_pnl
- summary_calculates_total_fee
- summary_calculates_total_slippage
- summary_calculates_profit_factor
- summary_profit_factor_inf_when_no_losses
- summary_calculates_expectancy
- summary_calculates_avg_win_and_avg_loss
- summary_calculates_max_drawdown
- summary_calculates_max_consecutive_losses
- summary_calculates_avg_trade_duration

### Report tests

- writes_summary_json
- writes_trades_csv
- writes_equity_curve_csv
- trades_csv_header_contains_required_fields
- csv_escape_handles_commas_and_quotes

### Engine tests

- engine_returns_none_when_csv_missing
- engine_rejects_data_quality_errors
- engine_produces_result_for_valid_csv
- engine_writes_no_fake_trades_when_no_signal
- engine_generates_deterministic_signal_ids
- engine_does_not_use_incomplete_5m_or_15m_candles
- engine_enters_on_next_candle_open
- engine_applies_conservative_intrabar_rule
- engine_updates_equity_after_closed_trade
- engine_closes_open_trade_at_end_of_backtest

### Walk-forward tests

- walk_forward_returns_empty_for_zero_lengths
- walk_forward_returns_empty_when_not_enough_data
- walk_forward_builds_deterministic_windows

## Existing behavior must remain

All existing Phase 1 through Phase 5 tests must continue passing.

Strategy still emits Signal only.

Risk still emits RiskAssessment only.

Backtest may create simulated Trade records only.

Backtest must not create real orders.

Paper and live must remain disabled.

## Strictly forbidden in Phase 6

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
- strategy optimizer
- portfolio optimizer
- 100x leverage logic
- synthetic candles
- interpolated candles
- exchange API integration
- websocket feed
- database requirement

Do not implement:

- live trading
- paper trading
- exchange adapters
- parameter optimization
- AI signal generation
- adaptive strategy tuning
- external broker integration
- notification systems

## Required commands

These must pass:

cargo fmt
cargo build
cargo test
cargo run -- research --config config/research.toml
cargo run -- help

If valid CSV data exists, research must generate:

reports/backtest_summary.json
reports/trades.csv
reports/equity_curve.csv

If no CSV data exists, research must not panic and must print the existing friendly missing-data message.

Do not leave failing tests.

Do not leave TODO stubs in active Phase 6 behavior.

## Expected final result

At the end of Phase 6, the repository should have:

- src/backtest/mod.rs
- src/backtest/engine.rs
- src/backtest/fill_model.rs
- src/backtest/metrics.rs
- src/backtest/report.rs
- src/backtest/walk_forward.rs
- deterministic backtest replay
- no lookahead across 5m / 15m candles
- conservative intrabar SL/TP rule
- simulated entries on next 1m candle open
- simulated SL / TP / time / end-of-backtest exits
- fees and slippage applied
- closed Trade records produced
- equity curve updated after closed trades
- summary metrics calculated
- reports written to reports/
- paper mode disabled
- live mode disabled
- no exchange API
- no LLM trading decisions
- cargo fmt passing
- cargo build passing
- cargo test passing
- cargo run -- research --config config/research.toml working
- cargo run -- help working

## Suggested implementation order

1. Read AGENTS.md and docs/ROADMAP.md.
2. Review Trade and TradeExitReason in src/core/trade.rs.
3. Add src/backtest/mod.rs.
4. Implement fill_model.rs with conservative SL/TP logic.
5. Implement metrics.rs.
6. Implement report.rs.
7. Implement walk_forward.rs minimal deterministic windows.
8. Implement engine.rs.
9. Add max_bars_held to config if missing.
10. Update src/lib.rs with pub mod backtest.
11. Update src/research/mod.rs to run backtest and write reports.
12. Update README to Phase 6.
13. Add fill model tests.
14. Add metrics tests.
15. Add report tests.
16. Add engine tests.
17. Add walk-forward tests.
18. Run cargo fmt.
19. Run cargo build.
20. Run cargo test.
21. Run cargo run -- research --config config/research.toml.
22. Run cargo run -- help.

## Commit message suggestion

phase6: implement deterministic backtest engine
