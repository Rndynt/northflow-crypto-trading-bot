# Northflow Phase 4 Build Prompt

You are working on this repository:

https://github.com/Rndynt/northflow-crypto-trading-bot

Your task is to implement Phase 4: Strategy Engine.

Read these files first:

- AGENTS.md
- docs/ROADMAP.md
- README.md
- config/research.toml
- src/core/signal.rs
- src/core/side.rs
- src/core/symbol.rs
- src/core/timeframe.rs
- src/core/candle.rs
- src/core/error.rs
- src/market/candle_store.rs
- src/indicators/mod.rs
- src/indicators/snapshot.rs
- existing files under src/strategy/

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

The current goal is to build a deterministic strategy layer that emits explainable Signal objects.

In Phase 4, a strategy may only evaluate validated candles and indicator snapshots and produce optional signals.

A strategy must not:

- place orders
- call exchange APIs
- call LLMs
- calculate final position size
- mutate account state
- run a backtest
- write reports
- claim profitability

## Current phase

Implement:

Phase 4 - Strategy Engine

Target structure:

- src/strategy/mod.rs
- src/strategy/traits.rs
- src/strategy/regime.rs
- src/strategy/screened_vwap_scalp.rs

The first and only active strategy for this phase is:

screened_vwap_scalp

Do not implement strategy routing.

Do not implement multiple strategies.

Do not implement Kalman, HMM, VPIN, order flow, alpha gate, Kelly, or portfolio optimization.

## Mandatory strategy boundary

Strategy modules must not:

- call exchange APIs
- call LLMs
- place orders
- calculate final position size
- mutate account state
- write reports
- run backtest execution

Strategies may only output:

Result<Option<Signal>, NorthflowError>

A Signal is not an order.

A Signal is only a pure strategy decision record for later risk validation and execution simulation.

## Mandatory timeframe model

Phase 4 must keep the existing explicit timeframe roles:

entry_timeframe = "1m"
screening_timeframe = "15m"
confirmation_timeframe = "5m"

Meaning:

- 1m = entry and execution signal timeframe
- 15m = screening / market regime bias
- 5m = confirmation layer

Never infer timeframe roles from array order.

Do not loosen config validation.

Do not change the active roadmap timeframe model.

## Mandatory signal identity

Every emitted Signal must have a non-empty signal_id.

The downstream traceability chain remains:

signal_id -> order_id -> fill_id -> position_id -> exit_order_id -> trade_id

Use the existing Phase 1 Signal, SignalId, and StrategyId types from src/core/signal.rs.

The current Signal type already contains:

- signal_id
- symbol
- strategy_id
- side
- entry_timeframe
- screening_timeframe
- confirmation_timeframe
- entry_time
- entry_price
- stop_loss
- take_profit
- confidence
- regime
- entry_reason
- filters_passed
- filters_failed
- expected_reward_bps
- estimated_cost_bps
- expected_net_edge_bps

Do not create a second Signal type.

Do not create a fake order type inside strategy.

Do not bypass Signal::validate().

## Required exports

Update src/strategy/mod.rs to export:

pub mod traits;
pub mod regime;
pub mod screened_vwap_scalp;

pub use traits::*;
pub use regime::*;
pub use screened_vwap_scalp::*;

Use explicit exports if preferred.

## Strategy trait requirements

Implement src/strategy/traits.rs.

Create a small deterministic trait.

Recommended API:

use crate::core::{Candle, NorthflowError, Signal, Symbol};
use crate::indicators::IndicatorSnapshot;

pub struct StrategyContext {
    pub symbol: Symbol,
    pub signal_index: u64,
    pub estimated_cost_bps: f64,
    pub min_confidence: u8,
}

pub struct MultiTimeframeInput {
    pub entry_candle: Candle,
    pub confirmation_candle: Candle,
    pub screening_candle: Candle,
    pub entry_indicators: IndicatorSnapshot,
    pub confirmation_indicators: IndicatorSnapshot,
    pub screening_indicators: IndicatorSnapshot,
}

pub trait Strategy {
    fn strategy_id(&self) -> &'static str;

    fn evaluate(
        &self,
        ctx: &StrategyContext,
        input: &MultiTimeframeInput,
    ) -> Result<Option<Signal>, NorthflowError>;
}

You may adjust names slightly if cleaner, but the concepts must stay:

- explicit strategy id
- explicit context
- explicit 1m entry candle
- explicit 5m confirmation candle
- explicit 15m screening candle
- explicit indicator snapshots for each timeframe
- output is Result<Option<Signal>, NorthflowError>

Do not pass raw arrays without role names.

Do not infer role from Vec order.

## Regime requirements

Implement src/strategy/regime.rs.

Create a deterministic market regime enum:

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketRegime {
    Bullish,
    Bearish,
    Neutral,
    Unknown,
}

Provide stable string output:

- bullish
- bearish
- neutral
- unknown

Recommended API:

impl MarketRegime {
    pub fn as_str(self) -> &'static str;
}

Implement:

pub fn classify_screening_regime(
    candle: Candle,
    snapshot: &IndicatorSnapshot,
) -> MarketRegime

Suggested rule:

Bullish:
- ema_50 > ema_200
- close > ema_50

Bearish:
- ema_50 < ema_200
- close < ema_50

Neutral:
- required EMA values exist but bullish/bearish rules do not pass

Unknown:
- ema_50 or ema_200 is missing

Keep it simple.

No ML.

No Kalman.

No HMM.

No order flow.

## screened_vwap_scalp requirements

Implement src/strategy/screened_vwap_scalp.rs.

The strategy must implement the Strategy trait.

Strategy id:

screened_vwap_scalp

Timeframe roles:

- 15m = screening / regime
- 5m = confirmation
- 1m = entry

## Concrete deterministic rules for Phase 4

Use deterministic and conservative rules.

Do not tune profitability.

Do not add adaptive learning.

Do not add hidden optimization.

## Required indicator values

For entry timeframe, require:

- ema_8
- ema_21
- atr_14
- vwap
- volume_sma_20

For screening timeframe, require:

- ema_50
- ema_200

For confirmation timeframe, require:

- ema_50
- ema_200

If required values are missing, return Ok(None).

Do not emit a signal while indicators are warming up.

## Regime rules

Use classify_screening_regime() for 15m screening.

Use the same helper for 5m confirmation.

Long allowed when:

- screening regime == Bullish
- confirmation regime == Bullish or Neutral

Short allowed when:

- screening regime == Bearish
- confirmation regime == Bearish or Neutral

If screening is Neutral or Unknown, do not emit a signal.

If confirmation is Unknown, do not emit a signal.

## Pullback-near rule

Use entry candle and entry indicators.

Define:

near_vwap = abs(close - vwap) / close * 10000 <= 20 bps
near_ema21 = abs(close - ema_21) / close * 10000 <= 20 bps
pullback_near = near_vwap || near_ema21

For Phase 4, use close as the proxy.

Do not inspect intrabar order flow.

## Reclaim / reject rule

Long reclaim:

entry_candle.close > ema_8 || entry_candle.close > vwap

Short reject:

entry_candle.close < ema_8 || entry_candle.close < vwap

## ATR valid rule

ATR is valid when:

atr_14 > 0
atr_bps = atr_14 / close * 10000
atr_bps >= 5
atr_bps <= 300

This rejects no-volatility and extreme-volatility candles.

## Volume acceptable rule

Volume acceptable when:

entry_candle.volume >= volume_sma_20 * 0.8

Do not require volume spike in Phase 4.

## Stop-loss and take-profit geometry

The strategy must generate valid signal geometry.

For Long:

entry_price = entry_candle.close
stop_loss = entry_price - atr_14
take_profit = entry_price + (atr_14 * 1.5)

For Short:

entry_price = entry_candle.close
stop_loss = entry_price + atr_14
take_profit = entry_price - (atr_14 * 1.5)

This gives initial reward/risk 1.5.

Do not calculate quantity.

Do not calculate leverage.

Do not calculate final risk.

Risk model is Phase 5.

## Expected edge fields

Fill these Signal fields deterministically:

expected_reward_bps = abs(take_profit - entry_price) / entry_price * 10000
estimated_cost_bps = ctx.estimated_cost_bps
expected_net_edge_bps = expected_reward_bps - estimated_cost_bps

Do not fake win rate.

Do not claim profitability.

## Confidence

Use a simple deterministic confidence score.

Base:

confidence = 50

Add:

- +10 if screening regime is directional and matches side
- +10 if confirmation regime matches side
- +5 if pullback_near
- +5 if reclaim or reject condition passed
- +5 if volume acceptable
- +5 if ATR valid

Clamp to 0..100.

Do not emit signal if:

confidence < ctx.min_confidence

Use config min_confidence later via StrategyContext.

## Filters passed / failed

Populate filters_passed and filters_failed clearly.

Examples for passed filters:

- screening_bullish
- screening_bearish
- confirmation_bullish_or_neutral
- confirmation_bearish_or_neutral
- pullback_near_vwap_or_ema21
- reclaim_above_ema8_or_vwap
- reject_below_ema8_or_vwap
- atr_valid
- volume_acceptable

Examples for failed filters:

- screening_not_bullish
- screening_not_bearish
- confirmation_unknown
- not_near_vwap_or_ema21
- no_reclaim
- no_reject
- atr_invalid
- volume_below_threshold
- confidence_below_minimum

These fields are critical for Phase 7 attribution.

## Entry reason

Use stable strings.

Long example:

15m bullish, 5m bullish_or_neutral, 1m pullback near VWAP/EMA21 and reclaim above EMA8/VWAP

Short example:

15m bearish, 5m bearish_or_neutral, 1m pullback near VWAP/EMA21 and reject below EMA8/VWAP

## Signal ID generation

Use StrategyContext.signal_index.

Recommended deterministic ID format:

SIG-BT-00000001
SIG-BT-00000002

Implementation helper:

fn make_signal_id(index: u64) -> SignalId {
    SignalId::new(format!("SIG-BT-{index:08}"))
}

Do not use random IDs.

Do not use current system time.

Do not use UUID dependency.

Do not use nondeterministic generation.

## Symbol

Use ctx.symbol.clone().

## Timeframe fields

Set:

entry_timeframe: Timeframe::OneMinute
screening_timeframe: Timeframe::FifteenMinute
confirmation_timeframe: Timeframe::FiveMinute

Do not infer from input order.

## Signal validation

Before returning Some(signal), call:

signal.validate()?;

If validation fails, return Err.

## Tests required

Add comprehensive tests.

## Regime tests

- regime_unknown_when_missing_ema
- regime_bullish_when_ema50_above_ema200_and_close_above_ema50
- regime_bearish_when_ema50_below_ema200_and_close_below_ema50
- regime_neutral_when_ema_relationship_exists_but_close_filter_fails
- regime_as_str_is_stable

## Strategy trait / ID tests

- screened_vwap_scalp_strategy_id_is_stable
- signal_id_is_deterministic_from_context_index

## Long signal tests

- emits_long_signal_when_all_long_filters_pass
- long_signal_has_valid_geometry
- long_signal_has_required_timeframes
- long_signal_has_filters_passed
- long_signal_uses_expected_strategy_id
- long_signal_reward_risk_is_approximately_1_5
- long_signal_expected_net_edge_is_reward_minus_cost

## Short signal tests

- emits_short_signal_when_all_short_filters_pass
- short_signal_has_valid_geometry
- short_signal_has_required_timeframes
- short_signal_has_filters_passed
- short_signal_uses_expected_strategy_id
- short_signal_reward_risk_is_approximately_1_5

## No signal tests

- returns_none_when_indicators_missing
- returns_none_when_screening_neutral
- returns_none_when_screening_unknown
- returns_none_when_confirmation_unknown
- returns_none_when_not_near_vwap_or_ema21
- returns_none_when_no_reclaim_or_reject
- returns_none_when_atr_invalid
- returns_none_when_volume_below_threshold
- returns_none_when_confidence_below_minimum

## Boundary tests

- near_threshold_accepts_exactly_20_bps
- near_threshold_rejects_above_20_bps
- atr_bps_accepts_5_bps
- atr_bps_accepts_300_bps
- atr_bps_rejects_below_5_bps
- atr_bps_rejects_above_300_bps

## Research CLI behavior for Phase 4

Update src/research/mod.rs lightly.

The command:

cargo run -- research --config config/research.toml

should still:

- validate config
- load market data
- build candle store
- print truthful data summary
- print indicator readiness
- print Phase 4 strategy readiness

Acceptable output:

Strategy engine ready:
  screened_vwap_scalp
  Output: Signal only
  No orders, no risk sizing, no backtest execution

Next: Phase 5 - risk and cost model

Do not run full backtest.

Do not generate fake trades.

Do not write reports.

Do not claim profitability.

Do not create orders.

Do not simulate fills.

## README update

Update README.md to state:

- Current phase is Phase 4.
- Phase 1 core domain is complete.
- Phase 2 market data is complete.
- Phase 3 indicators are complete.
- Phase 4 strategy engine is implemented.
- First strategy is screened_vwap_scalp.
- Strategy emits Signal only.
- No risk sizing yet.
- No order creation yet.
- No backtest execution yet.
- Paper and live modes remain disabled.

Do not remove docs/ROADMAP.md.

Do not mark Phase 5, Phase 6, or Phase 7 as complete.

## Strictly forbidden in Phase 4

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

- risk sizing
- Kelly
- max leverage logic
- cost model enforcement
- position sizing
- order creation
- fill simulation
- backtest engine
- report writers
- equity curve
- PnL
- win rate
- profitability claims

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

Do not leave TODO stubs in active Phase 4 behavior.

## Expected final result

At the end of Phase 4, the repository should have:

- Phase 1 core still intact
- Phase 2 market data still intact
- Phase 3 indicators still intact
- src/strategy/traits.rs
- src/strategy/regime.rs
- src/strategy/screened_vwap_scalp.rs
- deterministic Strategy trait
- deterministic MarketRegime
- deterministic screened_vwap_scalp implementation
- strategy emits Option<Signal> only
- signals have mandatory signal_id
- signals use explicit timeframe roles
- signals validate via Signal::validate()
- no orders
- no fills
- no positions
- no risk sizing
- no backtest
- no reports
- README updated to Phase 4
- cargo fmt passing
- cargo build passing
- cargo test passing
- cargo run -- research --config config/research.toml working
- cargo run -- help working

## Suggested implementation order

1. Read AGENTS.md and docs/ROADMAP.md.
2. Review existing src/core/signal.rs.
3. Review existing src/indicators/snapshot.rs.
4. Replace placeholder src/strategy/mod.rs.
5. Add src/strategy/traits.rs.
6. Add src/strategy/regime.rs.
7. Add src/strategy/screened_vwap_scalp.rs.
8. Add unit tests for regime classification.
9. Add unit tests for long signal emission.
10. Add unit tests for short signal emission.
11. Add no-signal tests for missing/failed filters.
12. Add boundary tests for near threshold and ATR bps.
13. Update src/research/mod.rs readiness output.
14. Update README to Phase 4.
15. Run cargo fmt.
16. Run cargo build.
17. Run cargo test.
18. Run cargo run -- research --config config/research.toml.
19. Run cargo run -- help.

## Commit message suggestion

phase4: implement screened vwap scalp strategy signals