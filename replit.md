# Northflow — Deterministic Crypto Trading Research Core

Pure Rust CLI + library. No frontend, no Node.js, no web app.

## Build & run

```bash
cargo build --release
cargo test
cargo run -- research --config config/research.toml
cargo run -- help
```

Use the **Build** workflow button to compile.

## Stack

- Rust (edition 2024, rust-version 1.85)
- Zero external dependencies — pure std only
- Single crate: binary (`northflow`) + library (`northflow_crypto_trading_bot`)

## Where things live

```
src/
├── lib.rs          — module exports
├── main.rs         — CLI: research | paper (disabled) | live (disabled)
├── core/           — Candle, Signal, Trade, Side, Position, Symbol, Timeframe
├── config/         — ResearchConfig from TOML (no external parser)
├── market/         — OhlcvLoader, CandleStore, TimeframeBuilder, DataQualityReport
├── indicators/     — EMA 8/21/50/200, ATR 14, VWAP, VolumeSMA 20; IndicatorEngine
├── strategy/       — ScreenedVwapScalp: multi-TF EMA crossover + VWAP scalp
├── risk/           — PositionSizing, CostModel, RiskEngine (guard)
├── backtest/       — BacktestEngine, FillModel, Metrics, ReportWriter, WalkForward
├── research/       — run_research: loads CSVs, runs backtest, writes reports
├── report/         — (legacy placeholder)
├── execution/      — (placeholder, disabled)
├── journal/        — (placeholder, disabled)
└── advisor/        — (placeholder, disabled)

config/research.toml   — default config
data/historical/       — put <SYMBOL>.csv files here (1m OHLCV)
reports/               — output: backtest_summary.json, trades.csv, equity_curve.csv
```

## Implementation status

| Phase | Module | Status |
|-------|--------|--------|
| 1 | Core domain types | ✓ complete |
| 2 | Market data loader + timeframe builder | ✓ complete |
| 3 | Indicators (EMA, ATR, VWAP, VolumeSMA) | ✓ complete |
| 4 | Strategy engine (ScreenedVwapScalp) | ✓ complete |
| 5 | Risk & cost model | ✓ complete |
| 6 | Backtest engine | ✓ complete — 300 tests pass |
| — | Paper mode | disabled until research validated |
| — | Live mode | disabled until paper parity proven |

## Architecture decisions

- Research-first: paper and live modes exit with error until backtest is validated
- Config parsed manually from TOML — no serde/toml crate dependency
- No-lookahead rule: 5m candle at 1m index `i` only used if `ts ≤ candle[i].ts − 240_000 ms`
- Conservative intrabar fill: worst-case price within bar; if SL and TP both hit, SL assumed first
- Deterministic signal IDs: `SIG-BT-XXXXXXXX` (8-digit zero-padded per symbol run)
- LLM, Telegram, learning, multi-agent: out of scope until research validated

## GitHub

Repository: https://github.com/Rndynt/northflow-crypto-trading-bot.git

To push:
```bash
git remote set-url origin https://github.com/Rndynt/northflow-crypto-trading-bot.git
git push -u origin main
```
Use a GitHub PAT with `repo` scope as the password.

## User preferences

- Research-first: no live/paper until backtest engine is proven
- No LLM, Telegram, multi-agent in the entry path
- Pure Rust, no web dashboard, no Node.js
