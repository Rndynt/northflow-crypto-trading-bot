# Northflow Research Dashboard

A research-first crypto trading bot platform. The Replit project hosts the research dashboard web app; the Rust CLI lives in `northflow/`.

## Run & Operate

- `pnpm --filter @workspace/api-server run dev` — run the API server (port 8080)
- `pnpm run typecheck` — full typecheck across all packages
- `pnpm run build` — typecheck + build all packages
- `pnpm --filter @workspace/api-spec run codegen` — regenerate API hooks and Zod schemas from the OpenAPI spec
- `pnpm --filter @workspace/db run push` — push DB schema changes (dev only)
- Required env: `DATABASE_URL` — Postgres connection string (auto-provisioned)

## Stack

- pnpm workspaces, Node.js 24, TypeScript 5.9
- API: Express 5
- DB: PostgreSQL + Drizzle ORM
- Validation: Zod (`zod/v4`), `drizzle-zod`
- API codegen: Orval (from OpenAPI spec)
- Build: esbuild (CJS bundle)
- Frontend: React + Vite + shadcn/ui + TanStack Query

## Where things live

- `lib/api-spec/openapi.yaml` — OpenAPI contract (source of truth)
- `lib/db/src/schema/index.ts` — DB schema (backtest_runs, trades)
- `artifacts/api-server/src/routes/research.ts` — backtest/research API routes
- `artifacts/northflow-dashboard/` — React research dashboard (dark terminal UI)
- `northflow/` — Rust CLI + library (`northflow` binary + `northflow_crypto_trading_bot` lib)

## Architecture decisions

- Research-first: paper and live modes are disabled until backtest engine is validated
- LLM, Telegram, manager agents, learning, and multi-agent orchestration kept out of entry path
- Legacy ARIA/scalper codebase preserved in `northflow/legacy/aria/` — reference only
- OpenAPI-first: all API types generated from `openapi.yaml` — no hand-written types
- Dashboard syncs real backtest results from the Rust CLI via JSON/CSV report files

## Product

- Research dashboard: view and compare backtest runs, track metrics (PnL, win rate, Sharpe, drawdown)
- Run detail: per-trade ledger with entry/exit prices and P&L
- Log new runs manually or import from Rust CLI JSON output
- Rust CLI: `northflow backtest --config config/research.toml` runs deterministic EMA crossover + VWAP strategy

## Northflow Rust CLI (northflow/)

```
northflow/
├── Cargo.toml                         # workspace root
├── northflow_crypto_trading_bot/      # library crate
│   └── src/
│       ├── core/      — Candle, Trade, Signal, Side types
│       ├── config/    — ResearchConfig from TOML
│       ├── data/      — CSV OHLCV loader
│       ├── indicators/ — EMA, ATR, VWAP
│       ├── strategy/  — EMA crossover + VWAP filter
│       ├── risk/      — position sizing, stop/TP, fee model
│       ├── sim/       — simulation execution
│       ├── research/  — backtest runner
│       ├── report/    — CSV + JSON writers
│       ├── journal/   — placeholder (disabled)
│       └── advisor/   — placeholder (disabled)
├── northflow-cli/     # binary crate
│   └── src/main.rs
├── config/research.toml
└── legacy/aria/       — old ARIA codebase (reference only)
```

Build: `cd northflow && cargo build --release`
Run:   `cargo run -- backtest --config config/research.toml`

## GitHub Push (northflow/ → Rndynt/northflow-crypto-trading-bot)

To push the Rust project to your GitHub repo:

```bash
cd northflow
git init
git add .
git commit -m "feat: clean research-first Rust core + legacy/aria preservation"
git remote add origin https://github.com/Rndynt/northflow-crypto-trading-bot.git
git push -u origin main
```

You'll need a GitHub Personal Access Token (PAT) with `repo` scope.
Set it as an environment secret `GITHUB_TOKEN` and use:
  `https://<username>:<token>@github.com/Rndynt/northflow-crypto-trading-bot.git`

## Gotchas

- `@import url(...)` must be the FIRST line in `index.css` — before tailwindcss imports
- After any OpenAPI spec change: run codegen before using updated types
- Rust `northflow/` is a separate workspace from the pnpm monorepo — build with `cargo`, not `pnpm`
- Paper and live trading commands intentionally exit with error (disabled by design)

## User preferences

- Research-first approach: validate strategies before live trading
- Legacy ARIA code in legacy/aria for reference only
- No LLM/Telegram/multi-agent in entry path

## Pointers

- See the `pnpm-workspace` skill for workspace structure, TypeScript setup, and package details
