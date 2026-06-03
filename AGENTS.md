# AGENTS.md

This file tells coding agents how to work on Northflow.

## Project mission

Northflow is a deterministic research-first crypto trading engine. The first milestone is a truthful backtest/research core. Do not build a dashboard, Telegram bot, LLM trader, browser app, or live execution system in the current phase.

## Current allowed scope

Work only on Phase 0 through Phase 7 from `docs/ROADMAP.md`.

Allowed now:

- legacy preservation under `legacy/aria/`,
- core domain types,
- explicit timeframe config,
- market data loader,
- timeframe builder,
- indicators,
- strategy trait,
- `screened_vwap_scalp`,
- risk and cost model,
- backtest engine,
- report writers,
- unit tests.

Not allowed now:

- React,
- TypeScript web app,
- dashboard,
- Telegram,
- LLM decision maker,
- manager agent,
- learning agent,
- orchestrator,
- paper trading,
- live trading,
- 100x leverage,
- multi-strategy routing,
- portfolio optimization.

## Mandatory timeframe model

The config must use explicit roles:

```toml
entry_timeframe = "1m"
screening_timeframe = "15m"
confirmation_timeframe = "5m"
```

Rules:

- `1m` is for entry.
- `15m` is for screening/regime.
- `5m` is for confirmation.
- Never infer entry timeframe from the first item in a list.
- Reject ambiguous timeframe config.

## Mandatory signal identity

Every signal must have `signal_id` before risk validation or order creation.

Required relationship:

```text
signal_id -> order_id -> fill_id -> position_id -> exit_order_id -> trade_id
```

Minimum signal fields:

```text
signal_id
symbol
strategy_id
side
entry_timeframe
screening_timeframe
confirmation_timeframe
entry_time
entry_price
stop_loss
take_profit
confidence
regime
entry_reason
filters_passed
filters_failed
expected_reward_bps
estimated_cost_bps
expected_net_edge_bps
```

ID examples:

```text
SIG-BT-00000001
ORD-SIG-BT-00000001-ENTRY
ORD-SIG-BT-00000001-SL
ORD-SIG-BT-00000001-TP
TRD-SIG-BT-00000001
```

## Runtime boundary

Strategies may only emit signals. They must not place orders, call exchanges, call LLMs, calculate final position size, or mutate account state.

Risk validates signals and calculates quantity.

Backtest simulates fills and exits.

Reports explain every trade.

## Legacy boundary

Legacy code under `legacy/aria/` is reference only.

Never import from `legacy/` into active `src/`.

If useful legacy logic is needed, migrate it manually into the active module with tests.

## First strategy

Only implement one strategy first: `screened_vwap_scalp`.

Do not activate EMA ribbon, momentum, mean reversion, Kalman, HMM, alpha gate, order flow, pairs trading, or strategy routing in the current phase.

## Required command target

The first milestone is complete only when this command works:

```text
cargo run -- research --config config/research.toml
```

It must output:

```text
reports/backtest_summary.json
reports/trades.csv
reports/equity_curve.csv
```

## Report requirements

`trades.csv` must include `signal_id` and enough attribution to diagnose why a trade happened and why it won or lost.

`summary.json` must include at least:

```text
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
```

## Development rules

- Keep modules small and deterministic.
- Prefer pure functions for indicators, strategy, risk, and backtest logic.
- Add unit tests for every core module.
- Do not add new dependencies unless needed for Phase 1 through Phase 7.
- Do not add UI or service layers.
- Do not hide bad backtest results with optimistic assumptions.
- Use conservative intrabar behavior: if stop-loss and take-profit are both touched in the same candle, assume stop-loss first.

## Commit discipline

Use small commits by phase:

```text
phase1: core domain types
phase2: market data loader
phase3: indicators
phase4: strategy engine
phase5: risk and cost model
phase6: backtest engine
phase7: reports and attribution
```

Any change outside the current phase must be justified in the commit message.
