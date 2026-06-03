---
name: Phase 1 Core Domain
description: What was built in Phase 1, key design decisions, and what comes next.
---

## What is complete

Phase 1 is implemented and fully tested (`cargo build` + `cargo test` = 61 passed, 0 failed).

### Active modules (src/core/)
Each type lives in its own file — no monolithic types.rs:
- `candle.rs` — Candle + validate() + is_valid() + 13 tests
- `side.rs` — Side::Long / Side::Short (NOT Buy/Sell — migrate on demand)
- `symbol.rs` — Symbol newtype, validated, always uppercase
- `timeframe.rs` — Timeframe enum (1m/5m/15m/1h), from_str, to_seconds, 10 tests
- `signal.rs` — Signal with signal_id mandatory, explicit 3-timeframe fields, 14 tests
- `order.rs` — Order, OrderId, OrderType, OrderStatus (no exchange logic)
- `fill.rs` — Fill, FillId (no live exchange logic)
- `position.rs` — Position, unrealized_pnl, validate, 11 tests
- `trade.rs` — Trade, TradeExitReason, is_win, computed_net_pnl, 8 tests
- `error.rs` — NorthflowError enum (InvalidCandle, InvalidTimeframe, etc.)

### Stubs (Phase 2+)
strategy, risk, execution, report are empty placeholder modules.
research/mod.rs has run_research() that prints Phase 1 status — not a real backtest.

## Key design decisions

**Why:** Mandated by AGENTS.md and the Phase 1 build prompt.

- `Side` uses `Long`/`Short` — old `Buy`/`Sell` was in the previous scaffold; do not reintroduce.
- `signal_id` is mandatory on every Signal — all downstream objects trace back to it.
- Three explicit timeframe fields on Signal: entry_timeframe="1m", screening_timeframe="15m", confirmation_timeframe="5m". Never infer from array order.
- All IDs are newtypes around String: SignalId, OrderId, FillId, PositionId, TradeId.
- `Candle::is_valid()` kept as convenience wrapper over `validate()` — data module uses it.
- `config::ResearchConfig` stores timeframes as String (entry_timeframe, screening_timeframe, confirmation_timeframe) — parsed from TOML, defaults to "1m"/"15m"/"5m".

## What comes next (Phase 2)
- Market data loader: flexible CSV OHLCV reader (data/mod.rs already has a base, needs timeframe-aware builder)
- Timeframe builder: resample 1m candles to 5m and 15m
- Phase 3: indicators (EMA, ATR, VWAP)
- Phase 4: screened_vwap_scalp strategy
- Phase 5: risk + cost model
- Phase 6: backtest engine (SimExecutor with intrabar SL/TP)
- Phase 7: report writers (summary.json, trades.csv with signal_id, equity_curve.csv)
