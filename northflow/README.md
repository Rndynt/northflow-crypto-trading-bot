# Northflow — Research-First Crypto Trading Bot

A deterministic, research-first crypto trading bot core.

## Goals

- Validate strategies through rigorous backtesting **before** any live or paper trading
- Keep the active codebase small, compileable, and focused on research
- Preserve legacy ARIA code in `legacy/aria/` for reference only

## Project Structure

```
northflow/
├── northflow_crypto_trading_bot/   # Core library (lib)
│   └── src/
│       ├── core/        # Trading types: Candle, Trade, Signal, Side
│       ├── config/      # ResearchConfig from TOML
│       ├── data/        # CSV OHLCV loader
│       ├── indicators/  # EMA, ATR, VWAP
│       ├── strategy/    # EMA crossover + VWAP filter (deterministic)
│       ├── risk/        # Position sizing, stop/TP, fee/slippage model
│       ├── sim/         # Simulation execution primitives
│       ├── research/    # Backtest runner
│       ├── report/      # CSV + JSON report writers
│       ├── journal/     # (placeholder — not yet active)
│       └── advisor/     # (placeholder — not yet active)
├── northflow-cli/          # CLI binary (`northflow`)
│   └── src/main.rs
├── config/
│   └── research.toml       # Default research config
├── data/                   # Put your OHLCV CSV files here
├── reports/                # Backtest report output
└── legacy/
    └── aria/               # Old ARIA/scalper codebase (reference only)
```

## Quick Start

```bash
# Install Rust if needed
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Build
cargo build --release

# Print default config
cargo run -- init

# Run a backtest
cargo run -- backtest --config config/research.toml
```

## Data Format

CSV with header: `timestamp,open,high,low,close,volume`

Timestamps can be Unix epoch (seconds or milliseconds), ISO 8601, or `YYYY-MM-DD HH:MM:SS`.

## Disabled Modes

Paper and live trading are **intentionally disabled** until the research engine is validated:

```
northflow paper  # exits with error
northflow live   # exits with error
```

LLM, manager agents, learning, Telegram, dashboard, and multi-agent orchestration
are **not in the entry path** and must not be added until research is validated.

## GitHub

https://github.com/Rndynt/northflow-crypto-trading-bot
