# Northflow Roadmap

Northflow is a research-first crypto trading engine. The first goal is not live trading, dashboard, Telegram, or AI. The first goal is to prove whether one deterministic strategy has positive expectancy after realistic costs.

## Required timeframe model

Timeframes must be explicit in config:

```toml
entry_timeframe = "1m"
screening_timeframe = "15m"
confirmation_timeframe = "5m"
```

Meaning:

- `1m`: entry and execution signal timeframe.
- `15m`: higher-timeframe market bias/regime filter.
- `5m`: intermediate confirmation layer.

Never infer entry timeframe from the first element of an array.

## Required ID traceability

Every signal must have `signal_id` before risk validation or order creation.

Required chain:

```text
signal_id -> order_id -> fill_id -> position_id -> exit_order_id -> trade_id
```

Why: entry orders, stop-loss orders, take-profit orders, partial exits, final closes, reports, and journals must be traceable back to the original signal.

Minimum `Signal` fields:

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

Recommended IDs:

```text
SIG-BT-00000001
ORD-SIG-BT-00000001-ENTRY
ORD-SIG-BT-00000001-SL
ORD-SIG-BT-00000001-TP
TRD-SIG-BT-00000001
```

## Phase 0 — Legacy preservation

Goal: preserve the previous ARIA/crypto-scalper project under `legacy/aria/` for reference only.

Rules:

- Active `src/` must not import from `legacy/`.
- Legacy modules can only be reused after review, simplification, tests, and migration.
- Old multi-agent, LLM, Telegram, dashboard, and orchestration logic must not enter the Phase 1 entry path.

Done when:

- `legacy/aria/` exists.
- Active root builds without legacy imports.

## Phase 1 — Core domain

Goal: create stable trading types.

Target structure:

```text
src/core/
  candle.rs
  side.rs
  symbol.rs
  timeframe.rs
  signal.rs
  order.rs
  fill.rs
  position.rs
  trade.rs
  error.rs
```

Required types:

- Candle
- Side
- Symbol
- Timeframe
- Signal
- Order
- Fill
- Position
- Trade
- TradeExitReason
- NorthflowError

Required validations:

- Candle prices are finite and positive.
- `high >= low`.
- `open` and `close` are inside candle range.
- Signal geometry is valid.
- Reward/risk is computable.
- Every signal has `signal_id`.

## Phase 2 — Market data

Goal: load, validate, and transform historical data.

Target structure:

```text
src/market/
  ohlcv_loader.rs
  candle_store.rs
  timeframe_builder.rs
  data_quality.rs
```

Required features:

- Load 1m OHLCV CSV.
- Support `timestamp,open,high,low,close,volume`.
- Support `open_time,open,high,low,close,volume`.
- Reject invalid candles.
- Detect duplicate timestamps.
- Detect missing candles.
- Build 5m and 15m candles from 1m candles.

## Phase 3 — Indicators

Goal: deterministic indicators shared by research and future execution.

Target structure:

```text
src/indicators/
  ema.rs
  atr.rs
  vwap.rs
  volume.rs
```

Required indicators:

- EMA 8, 21, 50, 200
- ATR 14
- VWAP
- Volume SMA 20

Do not add Kalman, HMM, VPIN, order flow, alpha gate, Kelly, or portfolio optimization in Phase 1.

## Phase 4 — Strategy engine

Goal: strategy modules output signals only.

Target structure:

```text
src/strategy/
  traits.rs
  regime.rs
  screened_vwap_scalp.rs
```

Strategy modules must not:

- call exchange APIs,
- call LLMs,
- place orders,
- calculate final position size,
- mutate account state.

First strategy: `screened_vwap_scalp`.

Timeframe roles:

```text
15m = screening/regime
5m = confirmation
1m = entry
```

Long setup:

- 15m bullish bias.
- 5m bullish or neutral confirmation.
- 1m pullback near VWAP or EMA21.
- 1m close reclaim above EMA8 or VWAP.
- ATR valid.
- Volume acceptable.

Short setup:

- 15m bearish bias.
- 5m bearish or neutral confirmation.
- 1m pullback near VWAP or EMA21.
- 1m close reject below EMA8 or VWAP.
- ATR valid.
- Volume acceptable.

## Phase 5 — Risk and cost

Goal: validate signals and calculate safe size.

Target structure:

```text
src/risk/
  position_sizing.rs
  cost_model.rs
  guard.rs
```

Initial risk values:

```toml
risk_per_trade_pct = 0.25
max_open_positions = 1
max_leverage = 3.0
min_reward_risk = 1.5
max_daily_loss_pct = 1.5
max_drawdown_pct = 5.0
```

Sizing rule:

```text
risk_amount = equity * risk_per_trade_pct / 100
qty_by_risk = risk_amount / abs(entry - stop_loss)
max_qty_by_leverage = equity * max_leverage / entry
qty = min(qty_by_risk, max_qty_by_leverage)
```

Cost model must include:

- entry fee,
- exit fee,
- spread,
- slippage,
- market impact,
- stop slippage.

## Phase 6 — Backtest engine

Goal: replay candles and produce realistic performance reports.

Target structure:

```text
src/backtest/
  engine.rs
  fill_model.rs
  metrics.rs
  report.rs
  walk_forward.rs
```

Required flow:

1. Load 1m CSV.
2. Build 5m and 15m candles.
3. Update indicators.
4. Compute 15m regime.
5. Compute 5m confirmation.
6. Evaluate 1m strategy.
7. Validate signal with risk model.
8. Simulate entry.
9. Simulate SL/TP/time exit.
10. Apply fees and slippage.
11. Write trade row.
12. Update equity curve.
13. Export reports.

Conservative rule: if stop-loss and take-profit are both touched in the same candle, assume stop-loss was hit first.

Done when:

```text
cargo run -- research --config config/research.toml
```

produces:

```text
reports/backtest_summary.json
reports/trades.csv
reports/equity_curve.csv
```

## Phase 7 — Reports and attribution

Every trade must be explainable.

Required `trades.csv` fields:

```text
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
```

Required `summary.json` fields:

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

## Later phases

Paper trading, live execution, dashboard, Telegram, and AI advisor are later phases only. They must not be implemented until Phase 1 through Phase 7 are working and tested.

AI may later summarize journals and review configuration. AI must not decide entries, directly change SL/TP, or directly size positions.
