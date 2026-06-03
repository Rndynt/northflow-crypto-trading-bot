# Northflow — Deterministic Crypto Trading Research Core

A pure Rust CLI and library for deterministic, research-first crypto strategy backtesting.

## Current phase: Phase 7 — Reports and Attribution ✓

| Phase | Status |
|---|---|
| Phase 1 — Core Domain (Candle, Signal, Order, Trade …) | ✅ Complete |
| Phase 2 — Market Data (OHLCV loader, timeframe builder, data quality) | ✅ Complete |
| Phase 3 — Indicators (EMA 8/21/50/200, ATR 14, VWAP, Volume SMA 20) | ✅ Complete |
| Phase 4 — Strategy Engine (screened_vwap_scalp) | ✅ Complete |
| Phase 5 — Risk & Cost model | ✅ Complete |
| Phase 6 — Backtest engine | ✅ Complete |
| Phase 7 — Reports & Attribution | ✅ Implemented |

See `docs/ROADMAP.md` for full roadmap and architecture decisions.

---

## Phase 7 — Reports and Attribution

Phase 7 is the final research-core phase. Every trade is now explainable and auditable back to its original signal.

**Backtest output: simulated `Trade` records only.**  
No live orders. No paper trading. No exchange calls. No LLM trading decisions.

### Traceability chain

Every trade traces back through the full ID chain:

```
signal_id → order_id → fill_id → position_id → exit_order_id → trade_id
```

Deterministic format: `SIG-BT-00000001`, `TRD-SIG-BT-00000001`, `POS-SIG-BT-00000001`, …  
No random IDs. No UUID dependency. No system time.

### Report files

After `cargo run -- research --config config/research.toml` with valid historical CSV data:

| File | Description |
|---|---|
| `reports/backtest_summary.json` | Aggregate backtest metrics (win rate, net PnL, drawdown, …) |
| `reports/trades.csv` | Every simulated trade including signal_id, filters, edge bps |
| `reports/equity_curve.csv` | Equity at each closed trade |
| `reports/attribution_summary.json` | Overall attribution statistics (avg edge, realization, filter counts) |
| `reports/attribution_by_regime.csv` | Win rate, PnL, edge per market regime (bullish/bearish/neutral) |
| `reports/attribution_by_exit_reason.csv` | Win rate, PnL, edge grouped by exit reason (stop_loss/take_profit/time_exit/…) |
| `reports/attribution_by_side.csv` | Win rate, PnL, edge grouped by trade direction (long/short) |
| `reports/attribution_by_filter.csv` | Win rate, PnL, edge per passed or failed strategy filter |
| `reports/audit_report.json` | Audit validation result — errors, warnings, and per-trade issues |
| `reports/report_manifest.json` | Deterministic list of all generated files with row counts |
| `reports/risk_rejections.csv` | Every rejected signal reason with stage, equity, drawdown, and expected edge/cost context |
| `reports/signal_flow_summary.json` | Signal funnel counts: generated, preapproved, rejected, opened, and closed |

Attribution CSV files share a stable header:

```
key,trades,wins,losses,win_rate,net_pnl,gross_pnl,total_fee,total_slippage,avg_net_pnl,avg_expected_edge_bps,avg_actual_edge_bps,avg_bars_held
```

Filter bucket keys:
- `passed:<filter_name>` — strategy filters that approved the signal
- `failed:<filter_name>` — strategy filters that rejected the signal

### Risk rejection attribution

`risk_rejections.csv` explains why signals did not become trades.

The `stage` column can be:

- `initial_risk` — rejected at signal close before a pending entry is created.
- `actual_entry` — initially approved, then rejected after the engine recalculates risk using actual next-candle open entry price.

Normal RiskEngine rejections use `RiskAssessment.expected_reward_bps`, `RiskAssessment.expected_cost_bps`, and `RiskAssessment.expected_net_edge_bps` so the rejection reason and edge fields match.

`signal_flow_summary.json` summarizes the funnel:

```
signal generated → preapproved → rejected at initial risk / rejected at actual entry → trade opened → trade closed
```

`trades.csv` reward_risk is the effective reward/risk at the simulated entry fill price.

### Audit validation

Every trade is validated by `ReportAuditor`:

- **Errors** — broken attribution or invalid trade fields; `audit_report.passed = false`.  
  Examples: empty `trade_id`, `trade_id` does not embed `signal_id`, negative fee, non-finite PnL, duplicate signal IDs.

- **Warnings** — incomplete but non-fatal explainability; audit still passes.  
  Examples: `filters_passed` is empty, `expected_edge_bps <= 0`, `bars_held == 0`.

If errors are found, `audit_report.json` is still written and attribution files are still generated. Only file I/O failures stop report writing.

### Paper and live modes remain disabled

Later phases may add paper trading, live trading, and an AI advisor mode.  
**AI must not decide entries, SL/TP, or position size.**  
Those decisions must remain deterministic and rule-based.

```
northflow paper   # exits with error — research engine not yet validated for paper
northflow live    # exits with error — paper/live parity not yet proven
```

### Strategy diagnostics

Northflow writes extra diagnostic reports for research analysis:

- `signal_diagnostics.csv` — one row per trade with cost bps, edge realization, month, and filters.
- `rejection_by_stage_reason.csv` — grouped risk rejection reasons by stage and geometry mode.
- `monthly_summary.csv` — monthly PnL, win rate, cost, edge, and exit reason summary.
- `cost_edge_distribution.csv` — buckets trades by expected edge bps.
- `trade_distribution_summary.json` — compact summary of costs, edge realization, and dominant rejection reason.

These reports are diagnostic only. They do not tune parameters and do not imply profitability.

---

## Entry Geometry Mode

Configurable via `config/research.toml` under `[backtest]`:

```toml
entry_geometry_mode = "preserve_signal_levels"   # default
# entry_geometry_mode = "reanchor_to_actual_entry"
```

Controls how `stop_loss` and `take_profit` are handled after the actual adverse fill price is known at the next 1m candle open.

| Mode | SL / TP | Effective RR |
|---|---|---|
| `preserve_signal_levels` | Kept at original absolute levels from signal close | Can degrade when actual entry moves adversely |
| `reanchor_to_actual_entry` | Re-anchored around actual fill using original risk distance × original RR ratio | Preserved (simulates bracket order placed after fill) |

Both modes always update `entry_price`, `expected_reward_bps`, and `expected_net_edge_bps` to reflect the actual fill price.  
Neither mode modifies `signal_id`, `side`, `confidence`, `estimated_cost_bps`, or any identity field.

The active mode is recorded in every `RiskRejection` row (`entry_geometry_mode` column in `risk_rejections.csv`) and in `signal_flow_summary.json` so runs with different modes are always distinguishable in reports.

---

## Phase 6 — Backtest Engine

Phase 6 is a deterministic historical simulation only.

### Execution rules

- Entry is simulated at the **next 1m candle open** after signal generation.
- **No-lookahead rule**: 5m and 15m candles are only used once they are fully closed and their close time is at or before the current 1m candle's signal time.
- **Conservative intrabar rule**: if stop-loss and take-profit are both touched in the same candle, stop-loss is assumed to have been hit first.
- After entry at the next candle open, SL/TP checks run on that same entry candle.
- No new strategy signal is evaluated on the candle where an entry was just opened.

---

## Phase 5 — Risk and Cost Model

Phase 5 validates a `Signal` against risk limits and calculates a safe theoretical quantity.

**Risk model output: `RiskAssessment` only.**  
No orders. No fills. No positions. No backtest execution.

### Position sizing

```
risk_amount         = equity × risk_per_trade_pct / 100
qty_by_risk         = risk_amount / |entry − stop_loss|
max_qty_by_leverage = equity × max_leverage / entry
qty                 = min(qty_by_risk, max_qty_by_leverage)
```

### Cost model components

| Component | Formula |
|---|---|
| Entry fee | `entry_notional × taker_fee_bps / 10000` |
| Exit fee | `exit_notional × taker_fee_bps / 10000` |
| Spread | `avg_notional × spread_bps / 10000` |
| Slippage | `avg_notional × slippage_bps / 10000 × 2` |
| Market impact | `avg_notional × market_impact_bps / 10000` |
| Stop slippage | `avg_notional × stop_slippage_bps / 10000` |

### Risk guards

| Guard | Reject condition |
|---|---|
| Max open positions | `open_positions >= max_open_positions` |
| Daily loss | `abs(min(daily_pnl, 0)) / equity × 100 >= max_daily_loss_pct` |
| Max drawdown | `(peak − equity) / peak × 100 >= max_drawdown_pct` |
| Min reward/risk | `signal.reward_risk() < min_reward_risk` |
| Net edge | `expected_reward_bps − total_adverse_cost_bps <= 0` |

---

## Phase 4 — Strategy Engine

The first active strategy is `screened_vwap_scalp`.

**Strategy output: `Signal` only.**

### Timeframe roles (explicit — never inferred from order)

| Role | Timeframe | Purpose |
|---|---|---|
| `entry_timeframe` | 1m | Entry and execution signal |
| `confirmation_timeframe` | 5m | Intermediate confirmation |
| `screening_timeframe` | 15m | Market regime / bias filter |

### screened_vwap_scalp rules

**Regime classification (15m / 5m):**
- Bullish: EMA 50 > EMA 200 AND close > EMA 50
- Bearish: EMA 50 < EMA 200 AND close < EMA 50
- Neutral / Unknown: otherwise

**Signal direction:**
- Long: screening Bullish + confirmation Bullish or Neutral
- Short: screening Bearish + confirmation Bearish or Neutral

**Geometry:**
- Long: entry = close, SL = close − ATR, TP = close + ATR × 1.5
- Short: entry = close, SL = close + ATR, TP = close − ATR × 1.5

---

## Phase 3 — Indicators

| Indicator | Period | Notes |
|---|---|---|
| EMA | 8, 21, 50, 200 | First price initialises directly; alpha = 2/(period+1) |
| ATR | 14 | Wilder smoothing; initial value = mean of first 14 TRs |
| VWAP | — | Session-cumulative; typical = (H+L+C)/3; zero-volume safe |
| Volume SMA | 20 | Rolling window; `VecDeque` with O(1) update |

---

## Key rules

### Signal ID is mandatory

```
signal_id → order_id → fill_id → position_id → exit_order_id → trade_id
```

Deterministic format: `SIG-BT-00000001`, `SIG-BT-00000002`, …  
No random IDs. No UUID dependency. No system time.

### Timeframe roles are explicit

```toml
entry_timeframe        = "1m"   # entry and execution signals
screening_timeframe    = "15m"  # market regime / bias filter
confirmation_timeframe = "5m"   # intermediate confirmation layer
```

### CSV source must be 1m OHLCV

```
5m and 15m candles are built from 1m — not loaded from separate files.
```

Required CSV columns:

```
timestamp,open,high,low,close,volume
```

Or alternatively `open_time` instead of `timestamp` (case-insensitive).

### Strict timestamp rules

- Decimal, NaN, inf, negative, zero timestamps are **rejected**.
- Values `< 10^12` are treated as Unix seconds → multiplied by 1000.
- Values `>= 10^12` are kept as milliseconds unchanged.

### Timeframe buckets require exact candle counts

- A 5m bucket requires **exactly 5** one-minute candles — no more, no less.
- A 15m bucket requires **exactly 15** one-minute candles — no more, no less.
- Underfilled and overfilled buckets are dropped silently.

### Paper and live modes are disabled

```
northflow paper   # exits with error
northflow live    # exits with error
```

---

## Design principles

- Research and validation before any live or paper trading
- Zero external dependencies — pure Rust `std` only
- Deterministic: same config + same data = same result, always
- Truthful data: bad data is reported, never hidden or silently filled
- `signal_id` mandatory on every signal for full attribution chain
- Every trade auditable: errors = broken attribution, warnings = incomplete explainability

---

## Project structure

```
northflow-crypto-trading-bot/
├── src/
│   ├── lib.rs              — public module exports
│   ├── main.rs             — CLI entry point
│   ├── core/               — Phase 1: core trading domain types
│   ├── market/             — Phase 2: OHLCV data foundation
│   ├── indicators/         — Phase 3: deterministic streaming indicators
│   ├── strategy/           — Phase 4: deterministic strategy engine
│   ├── config/             — ResearchConfig (parsed from TOML, no serde)
│   ├── risk/               — Phase 5: position sizing + cost model + risk guards
│   ├── backtest/           — Phase 6: deterministic replay engine + fill model + reports
│   ├── report/             — Phase 7: attribution, audit, manifest, validation
│   │   ├── attribution.rs  — AttributionEngine (groups by regime/side/exit/filter)
│   │   ├── audit.rs        — ReportAuditor (validates every trade field + traceability)
│   │   ├── manifest.rs     — ManifestWriter (deterministic file manifest, no system time)
│   │   └── validation.rs   — TradeValidator (composable field-level checks)
│   ├── research/           — Research CLI orchestrator
│   ├── execution/          — placeholder (not active)
│   ├── journal/            — placeholder (not active)
│   └── advisor/            — placeholder (not active)
├── config/
│   └── research.toml       — default research config
├── data/
│   └── historical/         — place 1m OHLCV CSV files here: <SYMBOL>.csv
└── reports/                — all report output files
```

---

## Quick start

```bash
# Build
cargo build --release

# Run backtest (needs data/historical/BTCUSDT.csv)
cargo run -- research --config config/research.toml

# Run all unit tests
cargo test

# Print help
cargo run -- help
```

---

## CSV data format

```
timestamp,open,high,low,close,volume
1704067200000,42150.0,42800.0,41900.0,42600.0,1234.5
1704067260000,42600.0,42900.0,42550.0,42750.0,987.2
```

- Header: `timestamp` or `open_time` (case-insensitive)
- Timestamps: Unix epoch in seconds or milliseconds (normalised to ms)

---

## Config reference (`config/research.toml`)

| Key | Section | Description |
|-----|---------|-------------|
| `symbols` | `[pairs]` | List of symbols, e.g. `["BTCUSDT"]` |
| `entry_timeframe` | `[pairs]` | Must be `"1m"` |
| `screening_timeframe` | `[pairs]` | Must be `"15m"` |
| `confirmation_timeframe` | `[pairs]` | Must be `"5m"` |
| `data_dir` | `[backtest]` | Directory containing CSV files |
| `reports_dir` | `[backtest]` | Output directory for reports |
| `initial_equity_usd` | `[risk]` | Starting capital |
| `risk_per_trade_pct` | `[risk]` | % of equity risked per trade |
| `max_open_positions` | `[risk]` | Max simultaneous positions |
| `max_leverage` | `[risk]` | Max notional leverage |
| `min_reward_risk` | `[risk]` | Minimum R:R ratio |
| `max_daily_loss_pct` | `[risk]` | Daily loss circuit breaker |
| `max_drawdown_pct` | `[risk]` | Total drawdown circuit breaker |
| `taker_fee_bps` | `[cost]` | Taker fee in basis points |
| `slippage_bps` | `[cost]` | Slippage estimate in bps |
| `spread_bps` | `[cost]` | Spread cost in bps |
| `market_impact_bps` | `[cost]` | Market impact estimate in bps |
| `conservative_intrabar` | `[backtest]` | Worst-case intrabar fill |
| `min_confidence` | `[strategy]` | Minimum signal confidence (0–100) |

---

## Strictly forbidden (current phase and beyond)

- React app, TypeScript app, dashboard, web UI
- Telegram integration
- LLM trading decision or AI-decided entries/SL/TP/sizing
- Manager agent, learning agent, survival agent, orchestrator
- Live exchange order placement
- Paper trading loop (until research validated)
- Multi-strategy router, portfolio optimizer
- 100x leverage logic
- Fake trades, fake backtest reports
- Synthetic candles, interpolated candles, optimistic data fill
- Exchange API, websocket feed, database requirement
