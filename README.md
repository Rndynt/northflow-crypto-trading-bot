# Northflow — Deterministic Crypto Trading Research Core

A pure Rust CLI and library for deterministic, research-first crypto strategy backtesting.

## Current phase: Phase 2 — Market Data Foundation ✓

| Phase | Status |
|---|---|
| Phase 1 — Core Domain (Candle, Signal, Order, Trade …) | ✅ Complete |
| Phase 2 — Market Data (OHLCV loader, timeframe builder, data quality) | ✅ Complete |
| Phase 3 — Indicators (EMA, ATR, VWAP) | ⏳ Next |
| Phase 4 — Strategy (screened_vwap_scalp) | ⏳ Pending |
| Phase 5 — Risk & Cost model | ⏳ Pending |
| Phase 6 — Backtest engine | ⏳ Pending |
| Phase 7 — Reports & Attribution | ⏳ Pending |

See `docs/ROADMAP.md` for full roadmap and architecture decisions.

---

## Key rules

### Signal ID is mandatory

Every `Signal` must carry a `signal_id`. All downstream objects trace back to it:

```
signal_id → order_id → fill_id → position_id → exit_order_id → trade_id
```

Example IDs:
```
SIG-BT-00000001
ORD-SIG-BT-00000001-ENTRY
ORD-SIG-BT-00000001-SL
TRD-SIG-BT-00000001
```

### Timeframe roles are explicit

Declared explicitly in config — never inferred from array order:

```toml
entry_timeframe        = "1m"   # entry and execution signals
screening_timeframe    = "15m"  # market regime / bias filter
confirmation_timeframe = "5m"   # intermediate confirmation layer
```

The Phase 2 config validator rejects any deviation from these roles.

### CSV source must be 1m OHLCV

```
5m and 15m candles are built from 1m — not loaded from separate files.
```

Required CSV columns:

```
timestamp,open,high,low,close,volume
```

Or alternatively `open_time` instead of `timestamp` (case-insensitive).

Timestamps accepted as Unix seconds or Unix milliseconds — normalised to ms internally.

### Invalid candles are rejected

Every loaded candle is validated:
- All prices must be finite and > 0
- `high >= low`
- `open` and `close` must be inside `[low, high]`
- `volume` must be finite and ≥ 0

Invalid candles are rejected and recorded in the data quality report. No silent failures.

### Duplicate timestamps and missing gaps are detected

- **Duplicate timestamps**: first occurrence is kept, subsequent duplicates rejected and reported.
- **Missing 1m gaps**: any gap > 60 seconds between consecutive candles is detected and reported with exact count.
- **Non-monotonic input**: detected before sorting and flagged in the quality report.

### Paper and live modes are disabled

```
northflow paper   # exits with error — research engine not yet validated
northflow live    # exits with error — research engine not yet validated
```

These modes will be enabled only after the research engine produces validated, truthful backtest results.

### No fake backtest results

`cargo run -- research` prints a truthful market data summary — candle counts, data quality issues, missing gaps. It does not claim profitability or generate fake trades.

### Legacy code is reference-only

Previous code under `legacy/aria/` is preserved for reference only. The active `src/` tree never imports from `legacy/`. See `legacy/README.md`.

---

## Design principles

- Research and validation before any live or paper trading
- Zero external dependencies — pure Rust `std` only
- Deterministic: same config + same data = same result, always
- Truthful data: bad data is reported, never hidden or silently filled
- `signal_id` mandatory on every signal for full attribution chain

---

## Project structure

```
northflow-crypto-trading-bot/
├── src/
│   ├── lib.rs              — public module exports
│   ├── main.rs             — CLI entry point
│   ├── core/               — Phase 1: core trading domain types
│   │   ├── candle.rs       — Candle (OHLCV + full validation)
│   │   ├── side.rs         — Side::Long / Side::Short
│   │   ├── symbol.rs       — Symbol (validated ticker wrapper)
│   │   ├── timeframe.rs    — Timeframe (1m/5m/15m/1h + parsing)
│   │   ├── signal.rs       — Signal (mandatory signal_id, 3 TF roles)
│   │   ├── order.rs        — Order, OrderType, OrderStatus
│   │   ├── fill.rs         — Fill (executed order record)
│   │   ├── position.rs     — Position + unrealized PnL
│   │   ├── trade.rs        — Trade (final closed result)
│   │   └── error.rs        — NorthflowError
│   ├── market/             — Phase 2: OHLCV data foundation
│   │   ├── ohlcv_loader.rs — CSV loader (1m, deterministic, no network)
│   │   ├── candle_store.rs — CandleStore (1m + 5m + 15m)
│   │   ├── timeframe_builder.rs — Aggregate 1m → 5m/15m
│   │   └── data_quality.rs — DataQualityReport, issue detection
│   ├── config/             — ResearchConfig (parsed from TOML, no serde)
│   ├── data/               — DEPRECATED: use market::OhlcvLoader instead
│   ├── indicators/         — Phase 3 placeholder (EMA, ATR, VWAP)
│   ├── strategy/           — Phase 4 placeholder (screened_vwap_scalp)
│   ├── risk/               — Phase 5 placeholder (sizing + drawdown guards)
│   ├── execution/          — Phase 6 placeholder (SimExecutor)
│   ├── research/           — Phase 2 CLI orchestrator
│   ├── report/             — Phase 7 placeholder (JSON + CSV writers)
│   ├── journal/            — placeholder (not active)
│   └── advisor/            — placeholder (not active)
├── config/
│   └── research.toml       — default research config
├── data/
│   └── historical/         — place 1m OHLCV CSV files here: <SYMBOL>.csv
├── legacy/
│   ├── README.md           — legacy boundary rules
│   └── aria/               — previous code (reference only, never imported)
└── reports/                — Phase 7 output (not generated yet)
```

---

## Quick start

```bash
# Build
cargo build --release

# Phase 2 market data summary (needs data/historical/BTCUSDT.csv)
cargo run -- research --config config/research.toml

# Run all unit tests (Phase 1 + Phase 2)
cargo test

# Print help
cargo run -- help
```

### Example output (with data file present)

```
Northflow — Phase 2: Market Data Foundation

  Timeframe model:
    entry_timeframe        = "1m"  (1m  → entry & execution)
    screening_timeframe    = "15m" (15m → regime bias)
    confirmation_timeframe = "5m"  (5m  → confirmation)

Symbol:                BTCUSDT
Source:                data/historical/BTCUSDT.csv
1m candles:            1000
5m candles:            200
15m candles:           66
Data quality issues:   0
Duplicate timestamps:  0
Missing gaps:          0

Next: Phase 3 — indicators
```

### Example output (no CSV file)

```
No historical CSV found for BTCUSDT.
Expected path: data/historical/BTCUSDT.csv
Place a 1m OHLCV CSV file with columns:
  timestamp,open,high,low,close,volume
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
- LLM trading decision
- Manager agent, learning agent, survival agent, orchestrator
- Live exchange order placement
- Paper trading loop (until research validated)
- Multi-strategy router, portfolio optimizer
- 100x leverage logic
- Fake trades, fake backtest reports
- Synthetic candles, interpolated candles, optimistic data fill
- Exchange API, websocket feed, database requirement

---

## Push to GitHub

```bash
git remote set-url origin https://github.com/Rndynt/northflow-crypto-trading-bot.git
git push -u origin main
```

Use a GitHub PAT with `repo` scope when prompted for a password.
