# Northflow Phase 5 Build Prompt

You are working on this repository:

https://github.com/Rndynt/northflow-crypto-trading-bot

Your task is to implement Phase 5: Risk and Cost Model.

Read these files first:

- AGENTS.md
- docs/ROADMAP.md
- README.md
- config/research.toml
- src/config/mod.rs
- src/core/signal.rs
- src/core/side.rs
- src/core/symbol.rs
- src/core/timeframe.rs
- src/core/error.rs
- src/risk/mod.rs
- existing files under src/risk/
- src/strategy/screened_vwap_scalp.rs

Do not ignore the repository documentation.

## Project mission

Northflow is a deterministic research-first crypto trading engine.

Northflow is not:

- a dashboard
- a React app
- a Telegram bot
- an AI trading agent
- a live trading system
- a paper trading loop
- a strategy router

The current goal is to build a deterministic risk and cost layer that validates strategy Signals and calculates safe theoretical position size.

In Phase 5, risk modules may validate Signal objects and calculate sizing/cost estimates.

Risk modules must not:

- place orders
- create fills
- mutate account state
- run a backtest
- write reports
- call exchange APIs
- call LLMs
- claim profitability

## Current phase

Implement:

Phase 5 - Risk and Cost Model

Target structure:

- src/risk/mod.rs
- src/risk/position_sizing.rs
- src/risk/cost_model.rs
- src/risk/guard.rs

Do not implement Phase 6 backtest.

Do not implement order execution.

Do not implement fill simulation.

Do not implement report writers.

## Phase 5 goal

Phase 5 must answer this question:

Given a validated Signal and current account/risk context, is this signal allowed, and if allowed, what is the safe theoretical quantity?

It must produce a deterministic risk assessment.

It must not create an order.

It must not simulate a fill.

It must not update equity.

It must not open a position.

## Required risk config values

Use the existing config values from config/research.toml and src/config/mod.rs:

- risk_per_trade_pct
- max_open_positions
- max_leverage
- min_reward_risk
- max_daily_loss_pct
- max_drawdown_pct
- taker_fee_bps
- slippage_bps
- spread_bps
- market_impact_bps

Add this new cost config value if it does not already exist:

- stop_slippage_bps

Default recommendation:

stop_slippage_bps = 5.0

Update:

- config/research.toml
- src/config/mod.rs
- README.md

only as needed.

Do not break existing config validation.

## Required modules

Implement these files:

- src/risk/position_sizing.rs
- src/risk/cost_model.rs
- src/risk/guard.rs

Update:

- src/risk/mod.rs
- src/research/mod.rs
- README.md

only as needed.

## Required exports

Update src/risk/mod.rs to export:

pub mod position_sizing;
pub mod cost_model;
pub mod guard;

pub use position_sizing::*;
pub use cost_model::*;
pub use guard::*;

Use explicit exports if preferred.

## Position sizing requirements

Implement src/risk/position_sizing.rs.

Use the roadmap sizing rule:

risk_amount = equity * risk_per_trade_pct / 100
qty_by_risk = risk_amount / abs(entry - stop_loss)
max_qty_by_leverage = equity * max_leverage / entry
qty = min(qty_by_risk, max_qty_by_leverage)

Create deterministic types.

Recommended types:

pub struct PositionSizingConfig {
    pub risk_per_trade_pct: f64,
    pub max_leverage: f64,
}

pub struct PositionSizingInput {
    pub equity: f64,
    pub entry_price: f64,
    pub stop_loss: f64,
}

pub struct PositionSize {
    pub qty: f64,
    pub qty_by_risk: f64,
    pub max_qty_by_leverage: f64,
    pub risk_amount: f64,
    pub risk_per_unit: f64,
    pub notional: f64,
    pub leverage_used: f64,
}

pub struct PositionSizer;

impl PositionSizer {
    pub fn calculate(
        config: &PositionSizingConfig,
        input: &PositionSizingInput,
    ) -> Result<PositionSize, NorthflowError>;
}

Validation:

- equity must be finite and > 0
- risk_per_trade_pct must be finite and > 0
- max_leverage must be finite and > 0
- entry_price must be finite and > 0
- stop_loss must be finite and > 0
- risk_per_unit = abs(entry_price - stop_loss) must be > 0
- qty must be finite and > 0
- notional must be finite and > 0
- leverage_used must be <= max_leverage plus a tiny epsilon

Do not round quantity in Phase 5.

Do not use exchange lot size.

Do not use min notional.

Exchange constraints belong to a later adapter/execution phase.

## Cost model requirements

Implement src/risk/cost_model.rs.

The cost model must include:

- entry fee
- exit fee
- spread
- slippage
- market impact
- stop slippage

Create deterministic types.

Recommended types:

pub struct CostModelConfig {
    pub taker_fee_bps: f64,
    pub slippage_bps: f64,
    pub spread_bps: f64,
    pub market_impact_bps: f64,
    pub stop_slippage_bps: f64,
}

pub struct CostModelInput {
    pub entry_price: f64,
    pub exit_price: f64,
    pub qty: f64,
}

pub struct CostBreakdown {
    pub entry_notional: f64,
    pub exit_notional: f64,
    pub entry_fee: f64,
    pub exit_fee: f64,
    pub spread_cost: f64,
    pub slippage_cost: f64,
    pub market_impact_cost: f64,
    pub stop_slippage_cost: f64,
    pub total_expected_cost: f64,
    pub total_adverse_cost: f64,
    pub total_expected_cost_bps: f64,
    pub total_adverse_cost_bps: f64,
}

pub struct CostModel;

impl CostModel {
    pub fn calculate(
        config: &CostModelConfig,
        input: &CostModelInput,
    ) -> Result<CostBreakdown, NorthflowError>;
}

Suggested calculations:

entry_notional = entry_price * qty
exit_notional = exit_price * qty
avg_notional = (entry_notional + exit_notional) / 2

entry_fee = entry_notional * taker_fee_bps / 10000
exit_fee = exit_notional * taker_fee_bps / 10000
spread_cost = avg_notional * spread_bps / 10000
slippage_cost = avg_notional * slippage_bps / 10000 * 2
market_impact_cost = avg_notional * market_impact_bps / 10000
stop_slippage_cost = avg_notional * stop_slippage_bps / 10000

total_expected_cost = entry_fee + exit_fee + spread_cost + slippage_cost + market_impact_cost
total_adverse_cost = total_expected_cost + stop_slippage_cost

total_expected_cost_bps = total_expected_cost / avg_notional * 10000
total_adverse_cost_bps = total_adverse_cost / avg_notional * 10000

Validation:

- all bps values must be finite and >= 0
- entry_price must be finite and > 0
- exit_price must be finite and > 0
- qty must be finite and > 0
- all calculated costs must be finite and >= 0

Do not use live exchange fees.

Do not fetch fee schedule.

Do not call network.

## Guard requirements

Implement src/risk/guard.rs.

The guard validates a Signal against risk context.

Recommended types:

pub struct RiskConfig {
    pub risk_per_trade_pct: f64,
    pub max_open_positions: usize,
    pub max_leverage: f64,
    pub min_reward_risk: f64,
    pub max_daily_loss_pct: f64,
    pub max_drawdown_pct: f64,
}

pub struct RiskContext {
    pub equity: f64,
    pub peak_equity: f64,
    pub daily_realized_pnl: f64,
    pub open_positions: usize,
}

pub struct RiskAssessment {
    pub approved: bool,
    pub signal_id: String,
    pub qty: Option<f64>,
    pub notional: Option<f64>,
    pub leverage_used: Option<f64>,
    pub risk_amount: Option<f64>,
    pub risk_per_unit: Option<f64>,
    pub reward_risk: f64,
    pub expected_reward_bps: f64,
    pub expected_cost_bps: f64,
    pub expected_net_edge_bps: f64,
    pub passed: Vec<String>,
    pub failed: Vec<String>,
}

pub struct RiskEngine;

impl RiskEngine {
    pub fn assess(
        risk_config: &RiskConfig,
        cost_config: &CostModelConfig,
        context: &RiskContext,
        signal: &Signal,
    ) -> Result<RiskAssessment, NorthflowError>;
}

RiskEngine behavior:

1. Validate signal using signal.validate().
2. Validate risk config.
3. Validate risk context.
4. Check max open positions.
5. Check daily loss limit.
6. Check max drawdown limit.
7. Check min reward/risk.
8. Calculate position size.
9. Calculate cost estimate.
10. Check expected net edge after recalculated cost.
11. Return approved assessment if all checks pass.
12. Return rejected assessment with failed reasons if any check fails.

## Guard rules

### Max open positions

Reject when:

open_positions >= max_open_positions

Failed reason:

max_open_positions_reached

Passed reason:

max_open_positions_ok

### Daily loss guard

daily_loss_pct = abs(min(daily_realized_pnl, 0)) / equity * 100

Reject when:

daily_loss_pct >= max_daily_loss_pct

Failed reason:

daily_loss_limit_reached

Passed reason:

daily_loss_ok

### Drawdown guard

drawdown_pct = (peak_equity - equity) / peak_equity * 100

Reject when:

drawdown_pct >= max_drawdown_pct

Failed reason:

max_drawdown_reached

Passed reason:

drawdown_ok

Validation:

- peak_equity must be finite and > 0
- equity must be finite and > 0
- peak_equity should be >= equity
- if peak_equity < equity, treat drawdown as 0 or return config/context error
- prefer treating drawdown as 0 for robustness

### Minimum reward/risk

Use existing Signal::reward_risk().

Reject when:

signal.reward_risk() + epsilon < min_reward_risk

Recommended epsilon:

1e-9

Failed reason:

reward_risk_below_minimum

Passed reason:

reward_risk_ok

### Expected net edge

Use recalculated cost model, not only signal.estimated_cost_bps.

For expected reward:

expected_reward_bps = signal.expected_reward_bps

For expected cost:

expected_cost_bps = cost_breakdown.total_adverse_cost_bps

Expected net edge:

expected_net_edge_bps = expected_reward_bps - expected_cost_bps

Reject when:

expected_net_edge_bps <= 0

Failed reason:

expected_net_edge_not_positive

Passed reason:

expected_net_edge_positive

Important:

Do not claim this means the strategy is profitable.

This is only a conservative filter.

### Approved assessment

If all checks pass:

approved = true
qty = Some(position_size.qty)
notional = Some(position_size.notional)
leverage_used = Some(position_size.leverage_used)
risk_amount = Some(position_size.risk_amount)
risk_per_unit = Some(position_size.risk_per_unit)

### Rejected assessment

If any risk guard fails:

approved = false

Still return a RiskAssessment.

For rejected assessments:

- qty can be None
- notional can be None
- leverage_used can be None
- risk_amount can be None
- risk_per_unit can be None

Do not return Err for normal risk rejection.

Return Err only for invalid input/config, such as:

- invalid Signal geometry
- invalid equity
- invalid price
- invalid config values
- invalid cost calculation

## Integration with ResearchConfig

Add helper conversion methods if useful.

Recommended:

impl ResearchConfig {
    pub fn risk_config(&self) -> RiskConfig { ... }
    pub fn cost_model_config(&self) -> CostModelConfig { ... }
}

Only add these if it keeps code clean.

Do not break existing ResearchConfig parsing.

If adding stop_slippage_bps:

- add field to ResearchConfig
- add default
- parse from TOML
- document it in README
- add it to config/research.toml

## Research CLI behavior for Phase 5

Update src/research/mod.rs lightly.

The command:

cargo run -- research --config config/research.toml

should still:

- validate config
- load market data
- build candle store
- print truthful data summary
- print indicator readiness
- print strategy readiness
- print risk model readiness

Acceptable output:

Risk model ready:
  position sizing
  cost model
  risk guards
  Output: RiskAssessment only
  No orders, no fills, no backtest execution

Next: Phase 6 - backtest engine

Do not run full backtest.

Do not generate fake trades.

Do not create orders.

Do not simulate fills.

Do not write reports.

Do not claim profitability.

## README update

Update README.md to state:

- Current phase is Phase 5.
- Phase 1 core domain is complete.
- Phase 2 market data is complete.
- Phase 3 indicators are complete.
- Phase 4 strategy engine is complete.
- Phase 5 risk and cost model is implemented.
- Risk model validates Signal and calculates theoretical safe quantity.
- Output is RiskAssessment only.
- No order creation yet.
- No fill simulation yet.
- No backtest execution yet.
- Paper and live modes remain disabled.

Do not mark Phase 6 or Phase 7 as complete.

## Tests required

Add comprehensive tests.

### Position sizing tests

- position_sizing_rejects_invalid_equity
- position_sizing_rejects_zero_risk_per_trade
- position_sizing_rejects_zero_max_leverage
- position_sizing_rejects_zero_entry_price
- position_sizing_rejects_zero_risk_per_unit
- position_sizing_calculates_risk_amount
- position_sizing_calculates_qty_by_risk
- position_sizing_applies_leverage_cap
- position_sizing_uses_risk_qty_when_below_leverage_cap
- position_sizing_outputs_notional_and_leverage

### Cost model tests

- cost_model_rejects_negative_bps
- cost_model_rejects_invalid_price
- cost_model_rejects_invalid_qty
- cost_model_calculates_entry_fee
- cost_model_calculates_exit_fee
- cost_model_calculates_spread_cost
- cost_model_calculates_slippage_cost
- cost_model_calculates_market_impact_cost
- cost_model_calculates_stop_slippage_cost
- cost_model_total_expected_cost_excludes_stop_slippage
- cost_model_total_adverse_cost_includes_stop_slippage
- cost_model_outputs_cost_bps

### Risk guard tests

- risk_engine_rejects_invalid_signal
- risk_engine_rejects_max_open_positions
- risk_engine_rejects_daily_loss_limit
- risk_engine_rejects_max_drawdown
- risk_engine_rejects_low_reward_risk
- risk_engine_rejects_non_positive_net_edge
- risk_engine_approves_valid_signal
- approved_assessment_contains_qty
- rejected_assessment_has_no_qty
- risk_engine_passed_and_failed_reasons_are_stable

### Config tests

If you add stop_slippage_bps:

- config_parses_stop_slippage_bps
- default_stop_slippage_bps_is_positive
- cost_model_config_from_research_config_contains_stop_slippage

## Existing behavior must remain

All existing Phase 1, Phase 2, Phase 3, and Phase 4 tests must continue passing.

Strategy must still emit Signal only.

Risk must not create Order.

Execution must not run.

Backtest must not run.

Paper and live must remain disabled.

## Strictly forbidden in Phase 5

Do not create:

- React app
- TypeScript app
- dashboard
- web UI
- Telegram integration
- LLM trading decision
- manager agent
- learning agent
- survival agent
- orchestrator
- live exchange order placement
- paper trading loop
- strategy router
- portfolio optimizer
- 100x leverage logic
- fake trades
- fake backtest report
- synthetic candles
- interpolated candles
- exchange API integration
- websocket feed
- database requirement

Do not implement:

- order creation
- fill simulation
- backtest engine
- report writers
- equity curve
- realized PnL update
- win rate
- profitability claims
- live trading
- paper trading

Those belong to later phases.

## Required commands

These must pass:

cargo fmt
cargo build
cargo test
cargo run -- research --config config/research.toml
cargo run -- help

If any command fails, fix it before finishing.

Do not leave failing tests.

Do not leave TODO stubs in active Phase 5 behavior.

## Expected final result

At the end of Phase 5, the repository should have:

- Phase 1 core still intact
- Phase 2 market data still intact
- Phase 3 indicators still intact
- Phase 4 strategy engine still intact
- src/risk/position_sizing.rs implemented
- src/risk/cost_model.rs implemented
- src/risk/guard.rs implemented
- deterministic position sizing
- deterministic cost model
- deterministic risk guards
- RiskEngine::assess() returns RiskAssessment
- valid Signal can be approved
- invalid or unsafe Signal can be rejected
- no orders
- no fills
- no positions
- no backtest
- no reports
- README updated to Phase 5
- cargo fmt passing
- cargo build passing
- cargo test passing
- cargo run -- research --config config/research.toml working
- cargo run -- help working

## Suggested implementation order

1. Read AGENTS.md and docs/ROADMAP.md.
2. Review src/core/signal.rs and Signal::validate().
3. Review src/config/mod.rs and config/research.toml.
4. Implement src/risk/position_sizing.rs.
5. Implement src/risk/cost_model.rs.
6. Implement src/risk/guard.rs.
7. Update src/risk/mod.rs exports.
8. Add stop_slippage_bps to config if missing.
9. Add unit tests for position sizing.
10. Add unit tests for cost model.
11. Add unit tests for risk guards.
12. Add config tests if config changed.
13. Update src/research/mod.rs readiness output.
14. Update README to Phase 5.
15. Run cargo fmt.
16. Run cargo build.
17. Run cargo test.
18. Run cargo run -- research --config config/research.toml.
19. Run cargo run -- help.

## Commit message suggestion

phase5: implement risk and cost model
