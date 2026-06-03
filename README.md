# Northflow — Deterministic Crypto Trading Research Core

A pure Rust CLI and library for deterministic, research-first crypto strategy backtesting.

## Current phase: Phase 5 — Risk and Cost Model ✓

| Phase | Status |
|---|---|
| Phase 1 — Core Domain (Candle, Signal, Order, Trade …) | ✅ Complete |
| Phase 2 — Market Data (OHLCV loader, timeframe builder, data quality) | ✅ Complete |
| Phase 3 — Indicators (EMA 8/21/50/200, ATR 14, VWAP, Volume SMA 20) | ✅ Complete |
| Phase 4 — Strategy Engine (screened_vwap_scalp) | ✅ Complete |
| Phase 5 — Risk & Cost model | ✅ Implemented |
| Phase 6 — Backtest engine | ⏳ Pending |
| Phase 7 — Reports & Attribution | ⏳ Pending |

See `docs/ROADMAP.md` for full roadmap and architecture decisions.

---

## Phase 5 risk and cost model

Phase 5 validates a `Signal` against risk limits and calculates a safe theoretical quantity.

**Risk model output: `RiskAssessment` only.**  
No orders. No fills. No positions. No backtest execution. No profitability claims.

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

`total_expected_cost` excludes stop slippage. `total_adverse_cost` includes it.

### Risk guards

| Guard | Reject condition |
|---|---|
| Max open positions | `open_positions >= max_open_positions` |
| Daily loss | `abs(min(daily_pnl, 0)) / equity × 100 >= max_daily_loss_pct` |
| Max drawdown | `(peak − equity) / peak × 100 >= max_drawdown_pct` |
| Min reward/risk | `signal.reward_risk() < min_reward_risk` |
| Net edge | `expected_reward_bps − total_adverse_cost_bps <= 0` |

Normal guard rejection returns `Ok(RiskAssessment { approved: false, .. })`.  
Invalid input (bad signal geometry, invalid equity) returns `Err(NorthflowError)`.

Paper and live modes remain disabled. No order creation. No fill simulation. No backtest execution.

---

## Phase 4 strategy engine

The first active strategy is `screened_vwap_scalp`.

**Strategy output: `Signal` only.**  
No orders. No risk sizing. No backtest execution. No position creation.  
Strategy input candles are defensively validated at the start of `evaluate()`.

### Timeframe roles (explicit — never inferred from order)

| Role | Timeframe | Purpose |
|---|---|---|
| `entry_timeframe` | 1m | Entry and execution signal |
| `confirmation_timeframe` | 5m | Intermediate confirmation |
| `screening_timeframe` | 15m | Market regime / bias filter |

### screened_vwap_scalp rules

**Required indicators (1m entry):** EMA 8, EMA 21, ATR 14, VWAP, Volume SMA 20  
**Required indicators (15m / 5m):** EMA 50, EMA 200

**Regime classification (15m screening and 5m confirmation):**
- Bullish: EMA 50 > EMA 200 AND close > EMA 50
- Bearish: EMA 50 < EMA 200 AND close < EMA 50
- Neutral: EMA values present but above conditions not met
- Unknown: EMA 50 or EMA 200 missing (warmup)

**Signal direction:**
- Long: screening Bullish + confirmation Bullish or Neutral
- Short: screening Bearish + confirmation Bearish or Neutral
- No signal: screening Neutral / Unknown, or confirmation Unknown

**Hard gates (any failure → no signal):**
- Pullback near: |close − VWAP| or |close − EMA 21| ≤ 20 bps
- Reclaim (Long): close > EMA 8 OR close > VWAP
- Reject (Short): close < EMA 8 OR close < VWAP
- ATR valid: 5 bps ≤ ATR₁₄ ≤ 300 bps
- Volume acceptable: volume ≥ Volume SMA 20 × 0.8
- Confidence ≥ `min_confidence`

**Geometry:**
- Long: entry = close, SL = close − ATR, TP = close + ATR × 1.5
- Short: entry = close, SL = close + ATR, TP = close − ATR × 1.5
- Target reward/risk ≈ 1.5

---

## Phase 3 indicators

| Indicator | Period | Notes |
|---|---|---|
| EMA | 8, 21, 50, 200 | First price initialises directly; alpha = 2/(period+1) |
| ATR | 14 | Wilder smoothing; initial value = mean of first 14 TRs |
| VWAP | — | Session-cumulative; typical = (H+L+C)/3; zero-volume safe |
| Volume SMA | 20 | Rolling window; `VecDeque` with O(1) update |

---

## Key rules

### Signal ID is mandatory

Every `Signal` must carry a `signal_id`. All downstream objects trace back to it:

```
signal_id → order_id → fill_id → position_id → exit_order_id → trade_id
```

Deterministic format: `SIG-BT-00000001`, `SIG-BT-00000002`, …  
No random IDs. No UUID dependency. No system time.

### Timeframe roles are explicit

Declared explicitly in config — never inferred from array order:

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

Timestamps must be **positive integers** (Unix seconds or Unix milliseconds):

- Decimal timestamps (e.g. `1700000000.5`) are **rejected**.
- `NaN`, `inf`, `-INF` and any non-integer string are **rejected**.
- Negative timestamps are **rejected**.
- Zero (`0`) is **rejected**.
- Values `< 10^12` are treated as Unix seconds and multiplied by 1000 to normalise to milliseconds.
- Values `>= 10^12` are kept as milliseconds unchanged.

### Invalid candles are rejected

Every loaded candle is validated:
- All prices must be finite and > 0
- `high >= low`
- `open` and `close` must be inside `[low, high]`
- `volume` must be finite and ≥ 0

Invalid candles are rejected and recorded in the data quality report. No silent failures.

### Interval and gap detection

- **Duplicate timestamps**: first occurrence is kept, subsequent duplicates rejected and reported.
- **Missing 1m gaps**: delta is a positive exact multiple of 60 000 ms — detected and reported with exact missing count (warning, not fatal). Clean gaps require the delta to be divisible by 60 000 ms with no remainder.
- **Irregular intervals**: any delta that is not an exact multiple of 60 000 ms — detected and reported as an **error**. This includes sub-minute deltas (e.g. 30 000 ms) and non-multiple super-minute deltas (e.g. 90 000 ms, 150 000 ms).
- **Non-monotonic input**: detected before sorting and flagged in the quality report.

### Timeframe buckets require exact candle counts

- A 5m bucket requires **exactly 5** one-minute candles — no more, no less.
- A 15m bucket requires **exactly 15** one-minute candles — no more, no less.
- Underfilled buckets (incomplete data) are dropped silently.
- Overfilled buckets (irregular data) are also dropped silently.
- No candle synthesis, interpolation, or forward-fill is ever performed.

### Paper and live modes are disabled

```
northflow paper   # exits with error — research engine not yet validated
northflow live    # exits with error — research engine not yet validated
```

These modes will be enabled only after the research engine produces validated, truthful backtest results.

### No fake backtest results

`cargo run -- research` prints a truthful market data + indicator + strategy readiness summary. It does not claim profitability or generate fake trades.

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
│   ├── indicators/         — Phase 3: deterministic streaming indicators
│   │   ├── ema.rs          — EMA (periods: 8, 21, 50, 200)
│   │   ├── atr.rs          — ATR 14 (Wilder smoothing)
│   │   ├── vwap.rs         — VWAP (session-cumulative)
│   │   ├── volume.rs       — VolumeSma 20 (rolling window)
│   │   └── snapshot.rs     — IndicatorSnapshot + IndicatorEngine
│   ├── strategy/           — Phase 4: deterministic strategy engine
│   │   ├── traits.rs       — Strategy trait, StrategyContext, MultiTimeframeInput
│   │   ├── regime.rs       — MarketRegime enum + classify_screening_regime()
│   │   └── screened_vwap_scalp.rs — ScreenedVwapScalp strategy
│   ├── config/             — ResearchConfig (parsed from TOML, no serde)
│   ├── risk/               — Phase 5 placeholder (sizing + drawdown guards)
│   ├── execution/          — Phase 6 placeholder (SimExecutor)
│   ├── research/           — Research CLI orchestrator
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

# Phase 4 research summary (needs data/historical/BTCUSDT.csv for symbol data)
cargo run -- research --config config/research.toml

# Run all unit tests (Phase 1 + Phase 2 + Phase 3 + Phase 4)
cargo test

# Print help
cargo run -- help
```

### Example output (no CSV file)

```
Northflow — Phase 4: Strategy Engine

  Timeframe model:
    entry_timeframe        = "1m"  (1m  → entry & execution)
    screening_timeframe    = "15m" (15m → regime bias)
    confirmation_timeframe = "5m"  (5m  → confirmation)

No historical CSV found for BTCUSDT.
Expected path: data/historical/BTCUSDT.csv
Place a 1m OHLCV CSV file with columns:
  timestamp,open,high,low,close,volume

Indicators ready:
  EMA 8 / 21 / 50 / 200
  ATR 14 (Wilder smoothing)
  VWAP (session-cumulative)
  Volume SMA 20

Strategy engine ready:
  screened_vwap_scalp
  Output: Signal only
  No orders, no risk sizing, no backtest execution

Next: Phase 5 — risk and cost model
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
