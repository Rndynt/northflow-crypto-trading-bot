# Northflow — Deterministic Crypto Trading Research Core

Pure Rust CLI + library. No frontend, no Node.js, no web app.

## Build & run

```bash
cargo build --release
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
├── core/           — Candle, Signal, SimTrade, Side
├── config/         — ResearchConfig from TOML (no external parser)
├── data/           — CSV OHLCV loader, flexible header detection
├── indicators/     — EMA, ATR, VWAP (streaming structs)
├── strategy/       — ScreenedVwapScalp: EMA crossover + VWAP filter
├── risk/           — RiskManager: sizing, drawdown, daily loss guards
├── execution/      — SimExecutor: intrabar SL/TP, conservative fill model
├── research/       — run_research: per-symbol backtest orchestrator
├── report/         — RunReport metrics, JSON writer, CSV trade ledger
├── journal/        — placeholder (disabled)
└── advisor/        — placeholder (disabled)

config/research.toml   — default config
data/historical/       — put <SYMBOL>.csv files here
reports/               — output: <SYMBOL>_report.json + <SYMBOL>_trades.csv
```

## Architecture decisions

- Research-first: paper and live modes exit with error until validated
- Config parsed manually from TOML — no serde/toml crate dependency
- Conservative intrabar fill: worst-case price within the bar
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
