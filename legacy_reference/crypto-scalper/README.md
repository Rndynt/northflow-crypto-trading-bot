# 🤖 ARIA — Autonomous Realtime Intelligence Analyst

**LLM-Powered Autonomous Crypto Futures Scalper Bot**

ARIA is a multi-agent autonomous trading bot for Binance Futures that uses Large Language Models (LLM) for trade decisions. Every layer of the stack — from signal detection to risk management to execution — runs as an independent async agent communicating over a typed message bus. An AI "Trader Manager" oversees all decisions and can veto or adjust trades before they reach the exchange.

> **Status:** Paper mode active · 100x leverage HFT · NeonDB persistent journal · Telegram monitoring live · AI self-learning

---

## 📋 Table of Contents

- [Features](#-features)
- [Architecture](#-architecture)
- [Prerequisites](#-prerequisites)
- [Installation](#-installation)
- [Configuration](#-configuration)
- [Running the Bot](#-running-the-bot)
- [Telegram Commands](#-telegram-commands)
- [Config Profiles](#-config-profiles)
- [LLM Providers](#-llm-providers)
- [Database (NeonDB)](#-database-neondb)
- [Project Structure](#-project-structure)
- [Risk Management](#-risk-management)
- [License](#-license)

---

## ✨ Features

### 🧠 AI-Powered Decision Making
- **Brain Agent** — LLM analyzes technical indicators, sentiment, and market context to produce trade signals (GO / NO-GO / WAIT)
- **Trader Manager Agent** — Second LLM acts as "head of desk", reviewing and vetoing/approving every trade before execution
- **Orchestrator Agent** — Central coordinator managing all agents, learning policies, and trade flow
- **Learning Agent** — Self-improving system that tracks win/loss patterns per strategy/regime and generates lessons
- **Supports any OpenAI-compatible LLM** — Xiaomi MiMo, OpenRouter, OpenAI, DeepSeek, Groq, Together, Anthropic

### 📊 Trading Strategies (Adaptive)
- **EMA Ribbon** — Trend-following with exponential moving average crossovers
- **Mean Reversion** — Counter-trend entries at statistical extremes
- **Momentum** — Velocity-based entries on strong directional moves
- **VWAP Scalp** — Volume-weighted average price deviation trades
- **Squeeze** — Bollinger Band inside Keltner Channel breakout detection
- **Kalman Filter** — Noise-smoothed price velocity estimation
- **Multi-Timeframe** — Confluence across 1m, 5m, 15m timeframes
- **HMM (Hidden Markov Model)** — Regime detection for strategy selection
- **Alpha Gate** — External signal gating with configurable thresholds

### 🛡️ Risk & Survival
- **100x leverage support** — High-leverage futures trading with tight SL/TP
- **Per-trade risk sizing** — 0.5% equity per trade (Kelly + vol-target adjusted)
- **Maximum drawdown protection** — Auto-flat at 8% drawdown
- **Loss streak cooldowns** — 8 losses → 30min freeze, 8 in 1hr → 2hr freeze
- **Daily loss limits** — 12 losses/day → 24hr freeze + P&L ratchet at +3%
- **Volatility spike detection** — Auto-freeze on 2.5× abnormal volume
- **News panic filter** — Blocks trades during extreme sentiment events
- **Duplicate position prevention** — Blocks multiple entries on same symbol
- **Death line** — 70% equity floor triggers emergency shutdown (manual unfreeze required)

### 💰 Partial Take Profit System
- **50% close at 1R profit** — Lock in partial gains early
- **Breakeven stop** — SL moves to entry price after partial TP
- **Trailing stop** — Remaining 50% trails at 0.3× ATR (tighter for HFT)
- **Time-based exit** — Auto-close after 15 minutes max hold

### 📱 Telegram Monitoring
- **Signal notifications** — AI analysis, confidence scores, entry/SL/TP to DM and group topic
- **Position opened** — Full details including partial TP plan, R:R ratio, AI reasoning
- **Position closed** — PnL, duration, win/loss result, daily stats
- **Command panel** — 13 slash commands for real-time monitoring
- **Group topic support** — Signals posted to dedicated forum topic
- **Trade history** — Query NeonDB/SQLite for past trades via `/history`

### 📈 Quantitative Engine
- **Kelly Criterion** — Optimal position sizing based on historical win rate (capped at 20%)
- **Volatility targeting** — Dynamic sizing based on realized volatility (50% annual target)
- **VaR (Value at Risk)** — 95% confidence, max 3% portfolio risk
- **Information Coefficient** — Signal quality tracking with decay detection
- **Kalman smoothing** — Noise-reduced price estimation for entry timing

### 🧠 AI Self-Learning System
- **Per-strategy stats** — Win rate tracked per strategy (EMA, Momentum, VWAP, etc.)
- **Per-regime stats** — Strategy performance in Trending/Ranging/Volatile/Squeeze markets
- **Auto-lesson extraction** — After 10+ trades, bot identifies losing patterns and blacklists them
- **Policy updates** — Confidence thresholds raised for bad strategy+regime combos
- **Persistent memory** — Learning state saved to `data/learning_state.json` (survives rebuild)
- **NeonDB journal** — Full trade history with AI reasoning for post-analysis
- **LLM dedup cache** — 45s cooldown per symbol prevents redundant API calls

### 💾 Persistent Storage
- **NeonDB (PostgreSQL)** — All trades, positions, and LLM decisions persist across rebuilds
- **SQLite fallback** — Local backup when DATABASE_URL is not configured
- **JSON learning state** — Bot learning data saved to `data/learning_state.json`
- **Trade journal** — Full history with entry, exit, PnL, AI reasoning per trade

### 🔍 Research & Backtesting
- **Walk-forward analysis** — Out-of-sample strategy validation
- **Information Coefficient** — Signal quality decay tracking
- **Sensitivity analysis** — Parameter robustness testing
- **Significance testing** — Statistical validation of returns
- **CSV backtest engine** — Historical simulation with slippage model

---

## 🏗️ Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     ARIA Multi-Agent Runtime                │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌──────────┐   ┌──────────┐   ┌──────────┐               │
│  │ Data     │──▶│ Signal   │──▶│ Brain    │  (LLM #1)     │
│  │ Agent    │   │ Agent    │   │ Agent    │               │
│  └──────────┘   └──────────┘   └────┬─────┘               │
│                                      │                      │
│                                      ▼                      │
│                               ┌──────────┐                  │
│                               │ Manager  │  (LLM #2)       │
│                               │ Agent    │                  │
│                               └────┬─────┘                  │
│                                      │                      │
│  ┌──────────┐                        ▼                      │
│  │Survival  │◀─────── ┌──────────────────┐                 │
│  │ Agent    │         │   Orchestrator   │                 │
│  └──────────┘         │     Agent        │                 │
│                       └────────┬─────────┘                 │
│  ┌──────────┐                  │                            │
│  │ Risk     │◀─────────────────┘                            │
│  │ Agent    │                                               │
│  └────┬─────┘                                               │
│       │                                                      │
│       ▼                                                      │
│  ┌──────────┐   ┌──────────┐   ┌──────────┐               │
│  │Execution │──▶│ Monitor  │──▶│ Learning │               │
│  │ Agent    │   │ Agent    │   │ Agent    │               │
│  └──────────┘   └──────────┘   └──────────┘               │
│                                                             │
│  ┌──────────┐   ┌──────────┐                               │
│  │ Control  │   │ Watchdog │                               │
│  │ (TG Bot) │   │          │                               │
│  └──────────┘   └──────────┘                               │
└─────────────────────────────────────────────────────────────┘
```

**Event-driven:** All agents communicate via a typed `MessageBus` (broadcast channel). No direct agent-to-agent calls.

---

## 📦 Prerequisites

- **Rust** ≥ 1.85 (edition 2021)
- **OpenSSL** development headers (for native-tls)
- **Binance Futures API key** (for live/paper trading)
- **LLM API key** (any OpenAI-compatible provider)
- **Telegram Bot Token** (for notifications)
- **NeonDB / PostgreSQL** (optional, for persistent journal)

### Termux (Android) Specific

```bash
pkg install rust openssl
export OPENSSL_INCLUDE_DIR=$PREFIX/include
export OPENSSL_LIB_DIR=$PREFIX/lib
```

---

## 🚀 Installation

### 1. Clone the Repository

```bash
git clone https://github.com/Rndynt/crypto-scalper.git
cd crypto-scalper
```

### 2. Configure Environment

```bash
cp .env.example .env
# Edit .env with your API keys (see Configuration section)
nano .env
```

### 3. Build

```bash
# Development build (faster compile, slower runtime)
cargo build

# Release build (optimized, recommended)
cargo build --release
```

### 4. Run

```bash
# Paper mode (default — no real money)
./target/release/aria

# With aggressive config overlay
ARIA_CONFIG_OVERLAY=config/aggressive.toml ./target/release/aria

# With custom log level
RUST_LOG=debug ./target/release/aria
```

---

## ⚙️ Configuration

ARIA uses a layered configuration system:
1. **`config/default.toml`** — Base configuration (always loaded)
2. **Config overlay** — Environment-specific overrides (set via `ARIA_CONFIG_OVERLAY`)
3. **`.env`** — API keys and secrets (loaded at startup)

### Environment Variables (`.env`)

| Variable | Required | Description |
|----------|----------|-------------|
| `BINANCE_API_KEY` | Live only | Binance Futures API key |
| `BINANCE_API_SECRET` | Live only | Binance Futures API secret |
| `LLM_API_KEY` | Yes | Primary LLM API key |
| `MANAGER_API_KEY` | No | Manager LLM key (falls back to LLM_API_KEY) |
| `TELEGRAM_BOT_TOKEN` | Yes | Telegram bot token from @BotFather |
| `TELEGRAM_CHAT_ID` | Yes | Your Telegram user ID (for DM notifications) |
| `TELEGRAM_GROUP_ID` | No | Telegram group chat ID (for signal topic) |
| `TELEGRAM_SIGNAL_TOPIC_ID` | No | Forum topic thread ID for signals |
| `DATABASE_URL` | No | PostgreSQL/NeonDB connection string |
| `ARIA_CONFIG_OVERLAY` | No | Path to config overlay TOML file |
| `ARIA_LLM_PROVIDER` | Yes | LLM provider: `openai`, `openrouter`, `groq`, `together` |
| `ARIA_LLM_MODEL` | Yes | Model name (e.g., `mimo-v2-omni`, `gpt-4o-mini`) |
| `ARIA_LLM_API_BASE` | Yes | LLM API endpoint URL |
| `ARIA_MANAGER_ENABLED` | No | Enable Trader Manager agent (`true`/`false`) |
| `RUST_LOG` | No | Log level: `info`, `debug`, `warn`, `error` |

### Key Config Parameters (`config/*.toml`)

```toml
[mode]
run_mode = "paper"          # "paper", "live", or "backtest"
dry_run = true              # true = no real orders
fail_closed_without_llm = true  # block trading if brain LLM is unavailable

[pairs]
symbols = ["BTCUSDT", "ETHUSDT", "SOLUSDT"]
timeframes = ["1m", "5m", "15m"]

[risk]
risk_per_trade_pct = 0.5    # % of equity per trade (low for high leverage)
max_open_positions = 6       # Max concurrent positions
max_daily_loss_pct = 4.0     # Daily loss limit
max_drawdown_pct = 10.0      # Max drawdown before freeze
max_leverage = 100           # Maximum leverage multiplier
equity_usd = 5000.0          # Starting paper equity

[survival]
death_line_pct = 0.70        # Emergency stop at 70% equity
loss_streak_short = 8        # Losses before short cooldown
auto_flat_drawdown_pct = 8.0 # Auto-close all at 8% DD
```
[quant]
enabled = true               # Enable quant engine
kelly_cap = 0.40             # Max Kelly fraction
target_vol_annual = 0.30     # Target annualized volatility
```

---

## 🏃 Running the Bot

### Paper Mode (Recommended for Testing)

```bash
# Basic paper mode
./target/release/aria

# With aggressive scalping config
ARIA_CONFIG_OVERLAY=config/aggressive.toml ./target/release/aria
```

### Live Mode

```bash
# 1. Set your Binance API keys in .env
# 2. Use the HFT live config overlay
# 3. Test thoroughly in paper mode first!
ARIA_CONFIG_OVERLAY=config/hft-live.toml ./target/release/aria
```

> **⚠️ WARNING:** Always test in paper mode for at least 1-2 weeks before going live. Start with small equity ($500-1000) and monitor closely via Telegram.

### Backtest Mode

```bash
# Set run_mode = "backtest" in config
# Provide CSV data file
./target/release/aria
```

### Background Process

```bash
# Run with nohup
nohup ./target/release/aria > aria.log 2>&1 &

# Or use screen/tmux
screen -S aria
./target/release/aria
# Ctrl+A, D to detach
```

---

## 📱 Telegram Commands

Send these commands to your bot in DM or group:

| Command | Description |
|---------|-------------|
| `/help` | Show all available commands |
| `/status` | Bot status, equity, P&L, signal counts |
| `/positions` | List open positions with current P&L |
| `/signals` | Recent AI signals and decisions |
| `/brain` | Last AI brain decision details |
| `/performance` | Win rate, daily P&L, trade statistics |
| `/survival` | Survival mode status and cooldowns |
| `/risk` | Risk limits, current exposure, VaR |
| `/history` | Recent trade history from NeonDB/SQLite |
| `/health` | System health check (agents, latency) |
| `/freeze` | Manually freeze trading |
| `/unfreeze` | Resume trading after freeze |
| `/flat` | Close all positions immediately |

### Notification Types

- **🔔 AI Signal Detected** — New signal with confidence, strategy, scores
- **🟢/🔴 POSITION OPENED** — Entry, SL, TP, partial TP plan, AI reasoning
- **🎯 TAKE PROFIT HIT** — Full TP exit with PnL
- **🎯 PARTIAL TAKE PROFIT** — 50% close at 1R with realized PnL
- **🛑 STOP LOSS HIT** — SL exit with loss details
- **🔄 TRAILING STOP** — Trailing stop exit
- **⏰ TIME EXIT** — Max hold time reached

---

## 📁 Config Profiles

| Profile | File | Description |
|---------|------|-------------|
| Default | `config/default.toml` | Base config, always loaded |
| Aggressive | `config/aggressive.toml` | Paper mode, 100x leverage, tight SL/TP, 24/7 |
| HFT Live | `config/hft-live.toml` | **LIVE mode**, 100x leverage, conservative risk, fail-closed |
| Paper | `config/paper.toml` | Paper trading with conservative settings |
| Production | `config/production.toml` | Live trading with balanced safety limits |
| LLM Anthropic | `config/llm-anthropic.toml` | Anthropic Claude as LLM provider |
| LLM OpenRouter | `config/llm-openrouter-cheap.toml` | OpenRouter cheap models |

### Using a Config Overlay

```bash
# Via environment variable
ARIA_CONFIG_OVERLAY=config/aggressive.toml ./target/release/aria

# Or in .env file
ARIA_CONFIG_OVERLAY=config/aggressive.toml
```

---

## 🤖 LLM Providers

ARIA supports any OpenAI-compatible API. Configure in `.env`:

### Xiaomi MiMo (Recommended — Fast & Free)

```env
ARIA_LLM_PROVIDER=openai
ARIA_LLM_MODEL=mimo-v2-omni
ARIA_LLM_API_BASE=https://token-plan-sgp.xiaomimimo.com/v1/chat/completions
LLM_API_KEY=your_mimo_key
```

### OpenRouter

```env
ARIA_LLM_PROVIDER=openrouter
ARIA_LLM_MODEL=anthropic/claude-3.5-haiku
ARIA_LLM_API_BASE=https://openrouter.ai/api/v1/chat/completions
LLM_API_KEY=your_openrouter_key
```

### OpenAI

```env
ARIA_LLM_PROVIDER=openai
ARIA_LLM_MODEL=gpt-4o-mini
ARIA_LLM_API_BASE=https://api.openai.com/v1/chat/completions
LLM_API_KEY=your_openai_key
```

### DeepSeek

```env
ARIA_LLM_PROVIDER=openai
ARIA_LLM_MODEL=deepseek-chat
ARIA_LLM_API_BASE=https://api.deepseek.com/v1/chat/completions
LLM_API_KEY=your_deepseek_key
```

### Groq

```env
ARIA_LLM_PROVIDER=groq
ARIA_LLM_MODEL=llama-3.3-70b-versatile
ARIA_LLM_API_BASE=https://api.groq.com/openai/v1/chat/completions
LLM_API_KEY=your_groq_key
```

> **Note:** The Manager Agent can use a different LLM than the Brain Agent. Set `ARIA_MANAGER_*` variables separately.

---

## 🗄️ Database (NeonDB)

ARIA persists all trade data to PostgreSQL (NeonDB). Data survives rebuilds and restarts.

### Setup

1. Create a free NeonDB account at [neon.tech](https://neon.tech)
2. Create a database and copy the connection string
3. Add to `.env`:

```env
DATABASE_URL=postgresql://user:pass@ep-xxx.pooler.us-east-1.aws.neon.tech/dbname?sslmode=require
```

### Schema (Auto-Created)

```sql
-- Trades table
CREATE TABLE IF NOT EXISTS trades (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    symbol TEXT NOT NULL,
    side TEXT NOT NULL,
    size DOUBLE PRECISION NOT NULL,
    entry_price DOUBLE PRECISION NOT NULL,
    exit_price DOUBLE PRECISION,
    pnl_usd DOUBLE PRECISION,
    pnl_pct DOUBLE PRECISION,
    reason TEXT,
    strategy TEXT,
    regime TEXT,
    ai_confidence INTEGER,
    ai_reasoning TEXT,
    opened_at TIMESTAMPTZ NOT NULL,
    closed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- LLM decisions table
CREATE TABLE IF NOT EXISTS llm_decisions (
    id SERIAL PRIMARY KEY,
    user_id TEXT NOT NULL,
    symbol TEXT NOT NULL,
    decision TEXT NOT NULL,
    confidence INTEGER,
    reasoning TEXT,
    created_at TIMESTAMPTZ DEFAULT NOW()
);
```

### Fallback

If `DATABASE_URL` is not set, ARIA falls back to local SQLite at `data/aria.db`.

---

## 📂 Project Structure

```
crypto-scalper/
├── src/
│   ├── main.rs                    # Entry point, agent orchestration
│   ├── config.rs                  # Config loading (TOML + overlay)
│   ├── errors.rs                  # Error types
│   ├── quant.rs                   # Quantitative engine (Kelly, VaR, vol-target)
│   ├── agents/
│   │   ├── mod.rs                 # Module declarations
│   │   ├── bus.rs                 # MessageBus (typed broadcast channel)
│   │   ├── messages.rs            # AgentEvent enum, all message types
│   │   ├── brain.rs               # Brain Agent (LLM trade analysis)
│   │   ├── control.rs             # Control Agent (Telegram commands, signal notif)
│   │   ├── data.rs                # Data Agent (market data processing)
│   │   ├── execution.rs           # Execution Agent (order management, partial TP)
│   │   ├── feeds.rs               # Feeds Agent (external data aggregation)
│   │   ├── learning.rs            # Learning Agent (self-improvement)
│   │   ├── manager.rs             # Trader Manager Agent (LLM oversight)
│   │   ├── monitor.rs             # Monitor Agent (metrics, journal, notifs)
│   │   ├── orchestrator.rs        # Orchestrator Agent (central coordinator)
│   │   ├── risk.rs                # Risk Agent (position validation)
│   │   ├── signal.rs              # Signal Agent (strategy signal generation)
│   │   ├── survival.rs            # Survival Agent (capital protection)
│   │   └── watchdog.rs            # Watchdog Agent (health monitoring)
│   ├── execution/
│   │   ├── mod.rs                 # Exchange trait, PaperExchange
│   │   ├── binance.rs             # Binance Futures API client
│   │   ├── position.rs            # Position tracking, partial TP, trailing stop
│   │   ├── risk.rs                # RiskManager (limits, sizing)
│   │   └── tcm.rs                 # Transaction Cost Model
│   ├── strategy/
│   │   ├── mod.rs                 # Strategy selection, regime routing
│   │   ├── regime.rs              # Market regime detection
│   │   ├── ema_ribbon.rs          # EMA Ribbon strategy
│   │   ├── mean_reversion.rs      # Mean Reversion strategy
│   │   ├── momentum.rs            # Momentum strategy
│   │   ├── vwap_scalp.rs          # VWAP Scalp strategy
│   │   ├── squeeze.rs             # Squeeze (BB inside Keltner) strategy
│   │   ├── kalman.rs              # Kalman filter price estimator
│   │   ├── hmm.rs                 # Hidden Markov Model regime detection
│   │   ├── multi_timeframe.rs     # Multi-timeframe confluence
│   │   ├── alpha_gate.rs          # External signal gating
│   │   ├── ab_test.rs             # Strategy A/B testing
│   │   ├── pairs.rs               # Pairs trading
│   │   ├── retirement.rs          # Strategy retirement logic
│   │   └── state.rs               # SymbolState, strategy names
│   ├── llm/
│   │   ├── engine.rs              # LLM API client (multi-provider)
│   │   ├── prompts.rs             # System prompts for Brain & Manager
│   │   └── response_parser.rs     # JSON response parsing
│   ├── monitoring/
│   │   ├── mod.rs                 # Module declarations
│   │   ├── logger.rs              # TradeJournal (SQLite + NeonDB)
│   │   ├── metrics.rs             # MetricsState, HTTP dashboard
│   │   └── telegram.rs            # TelegramNotifier (DM + group topic)
│   ├── feeds/                     # External data feeds
│   ├── portfolio/                 # Portfolio management (Kelly, VaR, correlation)
│   ├── microstructure/            # Market microstructure (VPIN)
│   ├── research/                  # Research tools (backtest, IC, walk-forward)
│   └── learning/                  # Learning system (lessons, policy, memory)
├── config/
│   ├── default.toml               # Base configuration
│   ├── aggressive.toml            # Paper mode, 100x leverage HFT overlay
│   ├── hft-live.toml              # LIVE mode, 100x leverage, conservative risk
│   ├── paper.toml                 # Paper trading overlay
│   ├── production.toml            # Live trading overlay
│   ├── llm-anthropic.toml         # Anthropic LLM config
│   └── llm-openrouter-cheap.toml  # OpenRouter cheap models
├── data/                          # Runtime data (learning state, SQLite DB)
├── .env                           # API keys and secrets
├── Cargo.toml                     # Rust dependencies
└── README.md                      # This file
```

---

## 🛡️ Risk Management

### Position Sizing
- **Per-trade risk:** 0.5% of equity (Kelly + vol-target adjusted, capped at 20%)
- **Kelly Criterion:** Optimal fraction based on historical win rate
- **Volatility targeting:** Size adjusts inversely to realized volatility (50% annual target)
- **Max position:** 100% of equity notional
- **Leverage:** Up to 100x on Binance Futures

### Stop Loss / Take Profit (High Leverage)
- **SL:** Max 0.3% from entry (at 100x = 30% position loss)
- **TP:** Max 0.6% from entry (at 100x = 60% position gain)
- **R:R Ratio:** Minimum 1:2 (reward:risk)
- **Partial TP:** 50% close at 1R profit
- **Breakeven:** SL moves to entry after partial TP
- **Trailing:** 0.3× ATR trailing on remaining position
- **Max hold:** 15 minutes (time-based exit)

### Circuit Breakers
- **Death line:** 70% of starting equity → emergency shutdown (manual unfreeze)
- **Auto-flat:** 8% drawdown → close all positions
- **Loss streak (short):** 8 consecutive losses → 30min cooldown
- **Loss streak (long):** 8 losses in 1 hour → 2hr cooldown
- **Daily loss count:** 12 losses today → 24hr cooldown
- **Daily PnL ratchet:** +3% daily gain → freeze (lock profits)
- **Volatility spike:** 2.5× normal volume → freeze trading

### AI Learning Protections
- **Strategy blacklisting:** Win rate < 30% after 12 trades → strategy disabled for that regime
- **Symbol blacklisting:** Consistent losses on specific symbol → reduce confidence
- **Regime awareness:** Bot learns which strategies work in which market conditions
- **Confidence decay:** Stale signals (>45s) automatically skipped (LLM dedup cache)

---

## 🧠 How the Bot Learns

ARIA continuously learns from every trade and adapts its behavior:

### Learning Cycle

```
Trade 1:  LOSS → Record: EMA ribbon + RANGING = loss
Trade 2:  LOSS → Record: SOL + volatile = loss
Trade 3:  WIN  → Record: Momentum + TRENDING_BULLISH = win
...
Trade 10: → Extract lesson: "EMA ribbon in RANGING has 30% WR"
          → Update policy: raise TA threshold for EMA+RANGING combo
Trade 12: → Blacklist: "EMA ribbon disabled in RANGING regime"
Trade 20: → Extract lesson: "SOL mean_reversion has 25% WR"
          → Blacklist: "Skip SOL for mean_reversion strategy"
```

### What Gets Learned

| Data | Persisted To | Survives Rebuild |
|------|-------------|-----------------|
| Per-strategy win rate | `data/learning_state.json` + NeonDB | ✅ |
| Per-regime performance | `data/learning_state.json` + NeonDB | ✅ |
| Lessons (blacklist rules) | `data/learning_state.json` | ✅ |
| Trade history (full) | NeonDB (PostgreSQL) | ✅ |
| LLM decisions | NeonDB (PostgreSQL) | ✅ |
| Orchestrator state | `data/orchestrator_state.json` | ✅ |

### How Lessons Affect Trading

1. **TA threshold raised** — Bad strategy+regime combos need higher TA confidence to pass
2. **Strategy disabled** — Blacklisted strategies are skipped entirely for specific regimes
3. **Symbol reduced** — Consistent losers get lower confidence scores
4. **Manager informed** — LLM Manager gets historical summary to make better veto decisions

---

## 📜 License

MIT License — see [LICENSE](LICENSE) for details.

---

## 🙏 Credits

- Built with [Rust](https://www.rust-lang.org/) and [Tokio](https://tokio.rs/)
- LLM integration via [Xiaomi MiMo](https://xiaomimimo.com/) / OpenAI-compatible APIs
- Exchange connectivity via [Binance Futures API](https://binance-docs.github.io/apidocs/futures/en/)
- Database via [NeonDB (PostgreSQL)](https://neon.tech/)

---

**🤖 ARIA v1.0** — Autonomous Realtime Intelligence Analyst
*Multi-agent HFT quant trading bot with AI self-learning*
