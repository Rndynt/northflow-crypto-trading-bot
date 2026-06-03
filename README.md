# Northflow — Deterministic Crypto Trading Research Core

A pure Rust CLI and library for deterministic, research-first crypto strategy backtesting.

## Design principles

- Research and validation before any live or paper trading
- Zero external dependencies in the hot path — pure Rust, no runtime
- Deterministic simulation: same config + same data = same result, always
- Paper and live modes intentionally disabled until research engine is validated

## Project structure

```
northflow-crypto-trading-bot/
├── src/
│   ├── lib.rs          — public module exports
│   ├── main.rs         — CLI entry point
│   ├── core/           — Candle, Signal, SimTrade, Side types
│   ├── config/         — ResearchConfig (parsed from TOML)
│   ├── data/           — CSV OHLCV loader (flexible header detection)
│   ├── indicators/     — EMA, ATR, VWAP (streaming, no allocation)
│   ├── strategy/       — ScreenedVwapScalp (EMA crossover + VWAP filter)
│   ├── risk/           — RiskManager: position sizing, drawdown, daily loss
│   ├── execution/      — SimExecutor: intrabar SL/TP, fill model, trade log
│   ├── research/       — run_research: orchestrates full backtest per symbol
│   ├── report/         — RunReport: metrics, JSON writer, CSV trade ledger
│   ├── journal/        — placeholder (not active)
│   └── advisor/        — placeholder (not active)
├── config/
│   └── research.toml   — default research config
├── data/
│   └── historical/     — place OHLCV CSV files here: <SYMBOL>.csv
└── reports/            — output: <SYMBOL>_report.json, <SYMBOL>_trades.csv
```

## Quick start

```bash
# Build
cargo build --release

# Run backtest (needs data/historical/BTCUSDT.csv)
cargo run -- research --config config/research.toml

# Print help
cargo run -- help
```

## CSV data format

Header must include: `timestamp`, `open`, `high`, `low`, `close`, `volume`
Timestamps accepted: Unix epoch (ms or s), ISO 8601, or `YYYY-MM-DD HH:MM:SS`

```
timestamp,open,high,low,close,volume
1704067200000,42150.0,42800.0,41900.0,42600.0,1234.5
```

## Config reference (`config/research.toml`)

| Key | Section | Description |
|-----|---------|-------------|
| `symbols` | `[pairs]` | List of symbols, e.g. `["BTCUSDT"]` |
| `data_dir` | `[backtest]` | Directory containing CSV files |
| `reports_dir` | `[backtest]` | Output directory for reports |
| `initial_equity_usd` | `[risk]` | Starting capital |
| `risk_per_trade_pct` | `[risk]` | % of equity risked per trade |
| `max_open_positions` | `[risk]` | Max simultaneous positions |
| `max_leverage` | `[risk]` | Max notional leverage |
| `min_reward_risk` | `[risk]` | Minimum R:R ratio to take a trade |
| `max_daily_loss_pct` | `[risk]` | Daily loss circuit breaker |
| `max_drawdown_pct` | `[risk]` | Total drawdown circuit breaker |
| `taker_fee_bps` | `[cost]` | Taker fee in basis points |
| `slippage_bps` | `[cost]` | Slippage estimate in bps |
| `spread_bps` | `[cost]` | Spread cost in bps |
| `market_impact_bps` | `[cost]` | Market impact estimate in bps |
| `conservative_intrabar` | `[backtest]` | Use worst-case intrabar fill |
| `min_confidence` | `[strategy]` | Minimum signal confidence (0-100) |

## Disabled modes

```
northflow paper   # exits with error — not validated
northflow live    # exits with error — not validated
```

LLM, Telegram, multi-agent orchestration, and learning modules are
**not in this codebase** and must not be introduced before research validation.

## Push to GitHub

```bash
git remote add origin https://github.com/Rndynt/northflow-crypto-trading-bot.git
git push -u origin main
```

Use a GitHub PAT with `repo` scope when prompted for a password.
