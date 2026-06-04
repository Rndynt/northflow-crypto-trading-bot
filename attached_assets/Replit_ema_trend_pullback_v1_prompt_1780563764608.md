# Northflow EMA Trend Pullback V1 Strategy Patch Prompt

You are working on this repository:

https://github.com/Rndynt/northflow-crypto-trading-bot

Your task is to implement a new deterministic strategy candidate for research comparison:

```text
ema_trend_pullback_v1
```

This patch is about strategy research only.

Do not implement paper trading.
Do not implement live trading.
Do not implement exchange APIs.
Do not implement websocket feeds.
Do not implement database, dashboard, Telegram, LLM trading decisions, AI advisor, optimizer, grid search, genetic algorithm, or auto-tuning.
Do not change indicator formulas.
Do not change risk formulas.
Do not change fill model formulas.
Do not change the existing `screened_vwap_scalp` strategy.
Do not change the existing `screened_vwap_scalp_v2` strategy.
Do not claim profitability.

## Why this patch is needed

The current research infrastructure is already useful:

- deterministic data pipeline
- 1m to 5m/15m timeframe builder
- indicators
- no-lookahead backtest
- cost model
- risk model
- report attribution
- diagnostics
- strategy comparison runner

The current strategies are failing in comparison mode:

```text
screened_vwap_scalp:
  trades:        2655
  win_rate:      36.87%
  net_pnl:       -4999.04
  gross_pnl:     -699.79
  profit_factor: 0.1354
  max_drawdown:  99.98%

screened_vwap_scalp_v2:
  trades:        2337
  win_rate:      25.37%
  net_pnl:       -4999.36
  gross_pnl:     -883.93
  profit_factor: 0.1876
  max_drawdown:  99.99%
```

Both strategies lose heavily. The issue is not the backtest runner anymore. The issue is strategy quality.

Both current strategies are too noisy and cost-sensitive for BTCUSDT 1m 2024. We need a different strategy candidate that trades less often, requires stronger trend alignment, waits for pullback, and targets larger reward.

## Goal

Add a new strategy:

```text
ema_trend_pullback_v1
```

This strategy must be:

- deterministic
- rule-based
- multi-timeframe
- cost-aware
- configurable from TOML
- compatible with existing backtest comparison runner
- output `Signal` only
- no order placement
- no risk sizing inside strategy
- no exchange calls
- no LLM decisions

## Strategy concept

Use a stricter trend-following pullback model:

```text
15m = trend regime filter
5m  = trend confirmation
1m  = pullback and trigger entry
```

Long concept:

1. 15m trend is bullish.
2. 5m trend confirms bullish.
3. 1m price pulls back near EMA21 / EMA50 / VWAP.
4. 1m candle shows bullish rejection / reclaim.
5. ATR and reward bps are large enough to overcome cost.
6. Emit long signal.

Short concept:

1. 15m trend is bearish.
2. 5m trend confirms bearish.
3. 1m price pulls back near EMA21 / EMA50 / VWAP.
4. 1m candle shows bearish rejection / reclaim.
5. ATR and reward bps are large enough to overcome cost.
6. Emit short signal.

This strategy should trade less frequently than the VWAP scalp strategies.

## Files to read first

Read these files before changing anything:

- AGENTS.md
- README.md
- docs/ROADMAP.md
- docs/STRATEGY_RESEARCH.md
- config/research.toml
- src/config/mod.rs
- src/strategy/mod.rs
- src/strategy/traits.rs
- src/strategy/regime.rs
- src/strategy/screened_vwap_scalp.rs
- src/strategy/screened_vwap_scalp_v2.rs
- src/backtest/engine.rs
- src/backtest/geometry.rs
- src/backtest/risk_trace.rs
- src/report/attribution.rs
- src/report/diagnostics.rs
- src/report/manifest.rs
- src/research/mod.rs
- src/indicators/snapshot.rs
- src/core/signal.rs
- src/core/side.rs
- src/core/timeframe.rs
- src/core/candle.rs

## Required strategy ID

Add a new valid strategy ID:

```text
ema_trend_pullback_v1
```

Existing strategy IDs must remain valid:

```text
screened_vwap_scalp
screened_vwap_scalp_v2
```

Update all strategy validation and selection paths:

- `ResearchConfig::validate_strategy_config`
- `ResearchConfig::validate_strategy_runner_config`
- `BacktestEngine` strategy selector
- docs
- tests

## Required new file

Add:

```text
src/strategy/ema_trend_pullback.rs
```

Update:

```text
src/strategy/mod.rs
```

Export:

```rust
pub mod ema_trend_pullback;
pub use ema_trend_pullback::EmaTrendPullbackV1;
```

Use an explicit type name:

```rust
EmaTrendPullbackV1
```

## Required config

Update `ResearchConfig` and `config/research.toml`.

Add config fields under `[strategy]`:

```toml
# EMA Trend Pullback V1 — only used when strategy_id = "ema_trend_pullback_v1"
etp_require_strict_15m_trend = true
etp_require_strict_5m_confirmation = true
etp_require_1m_ema_alignment = true
etp_allow_long = true
etp_allow_short = true

etp_pullback_to = "ema21_or_vwap"
etp_max_pullback_distance_atr = 1.0
etp_min_pullback_distance_atr = 0.0

etp_reclaim_mode = "close_reclaim"
etp_min_body_ratio = 0.35
etp_min_wick_rejection_ratio = 0.25

etp_sl_atr_multiple = 1.0
etp_tp_atr_multiple = 3.0
etp_min_reward_risk = 2.5

etp_min_atr_bps = 10.0
etp_max_atr_bps = 180.0
etp_min_expected_reward_bps = 30.0
etp_min_expected_net_edge_bps = 12.0
etp_min_volume_ratio = 1.0
etp_cooldown_bars = 10
```

Recommended defaults:

```text
etp_require_strict_15m_trend = true
etp_require_strict_5m_confirmation = true
etp_require_1m_ema_alignment = true
etp_allow_long = true
etp_allow_short = true
etp_pullback_to = "ema21_or_vwap"
etp_max_pullback_distance_atr = 1.25
etp_min_pullback_distance_atr = 0.0
etp_reclaim_mode = "close_reclaim"
etp_min_body_ratio = 0.30
etp_min_wick_rejection_ratio = 0.20
etp_sl_atr_multiple = 1.0
etp_tp_atr_multiple = 3.0
etp_min_reward_risk = 2.5
etp_min_atr_bps = 8.0
etp_max_atr_bps = 200.0
etp_min_expected_reward_bps = 25.0
etp_min_expected_net_edge_bps = 8.0
etp_min_volume_ratio = 1.0
etp_cooldown_bars = 10
```

Config validation:

- booleans must parse cleanly.
- finite numeric values only.
- ATR min >= 0.
- ATR max > ATR min.
- pullback max >= pullback min.
- body ratio in range 0..=1.
- wick rejection ratio in range 0..=1.
- SL ATR multiple > 0.
- TP ATR multiple > 0.
- min reward risk >= 1.0.
- min expected reward bps >= 0.
- min expected net edge bps >= 0.
- min volume ratio >= 0.
- cooldown bars >= 0.
- `etp_pullback_to` valid values:
  - `ema21`
  - `ema50`
  - `vwap`
  - `ema21_or_vwap`
  - `ema21_or_ema50_or_vwap`
- `etp_reclaim_mode` valid values:
  - `close_reclaim`
  - `wick_rejection`
  - `close_reclaim_or_wick`

Unknown values must return ConfigError. Do not silently default invalid values.

## Strategy rules

### Required indicator fields

If any required indicator is missing or invalid, return `Ok(None)`.

Required 1m entry indicators:

```text
ema_8
ema_21
ema_50
ema_200
atr_14
vwap
volume_sma_20
```

Required 5m confirmation indicators:

```text
ema_21
ema_50
ema_200
close
```

Required 15m screening indicators:

```text
ema_50
ema_200
close
```

Use whatever actual field names exist in `IndicatorSnapshot`.

Do not panic if missing.

### 15m trend filter

Bullish 15m trend:

```text
ema_50 > ema_200
screening_close > ema_50
```

Bearish 15m trend:

```text
ema_50 < ema_200
screening_close < ema_50
```

If neutral, return `Ok(None)`.

### 5m confirmation filter

Bullish 5m confirmation:

```text
ema_21 > ema_50
ema_50 > ema_200
confirmation_close > ema_21
```

Bearish 5m confirmation:

```text
ema_21 < ema_50
ema_50 < ema_200
confirmation_close < ema_21
```

If confirmation does not match screening direction, return `Ok(None)`.

### 1m EMA alignment

Long:

```text
ema_8 > ema_21
ema_21 > ema_50
entry_close >= ema_21
```

Short:

```text
ema_8 < ema_21
ema_21 < ema_50
entry_close <= ema_21
```

If `etp_require_1m_ema_alignment = true`, require this.

### Pullback distance filter

Compute distance to configured pullback anchors.

Possible anchors:

```text
ema21
ema50
vwap
```

For the selected `etp_pullback_to`, compute nearest distance:

```text
distance = min(abs(close - selected_anchor))
distance_atr = distance / atr
```

Require:

```text
etp_min_pullback_distance_atr <= distance_atr <= etp_max_pullback_distance_atr
```

If ATR <= 0, return `Ok(None)`.

### Reclaim / rejection trigger

Implement two trigger modes.

#### close_reclaim

Long trigger:

```text
low <= nearest_anchor
close > nearest_anchor
close > open
```

Short trigger:

```text
high >= nearest_anchor
close < nearest_anchor
close < open
```

Where `nearest_anchor` is the anchor with the smallest absolute distance.

#### wick_rejection

Compute candle metrics:

```text
range = high - low
body = abs(close - open)
upper_wick = high - max(open, close)
lower_wick = min(open, close) - low
body_ratio = body / range
lower_wick_ratio = lower_wick / range
upper_wick_ratio = upper_wick / range
```

If range <= 0, return `Ok(None)`.

Long trigger:

```text
lower_wick_ratio >= etp_min_wick_rejection_ratio
body_ratio >= etp_min_body_ratio
close > open
```

Short trigger:

```text
upper_wick_ratio >= etp_min_wick_rejection_ratio
body_ratio >= etp_min_body_ratio
close < open
```

#### close_reclaim_or_wick

Pass if either close_reclaim or wick_rejection passes.

### ATR bps filter

```text
atr_bps = atr / close * 10000
```

Require:

```text
etp_min_atr_bps <= atr_bps <= etp_max_atr_bps
```

If close <= 0, return `Ok(None)`.

### Volume ratio filter

```text
volume_ratio = entry_candle.volume / volume_sma_20
```

Require:

```text
volume_ratio >= etp_min_volume_ratio
```

If volume_sma_20 <= 0, return `Ok(None)`.

### Direction enable/disable

If direction is long and `etp_allow_long = false`, return `Ok(None)`.

If direction is short and `etp_allow_short = false`, return `Ok(None)`.

If both false, no signals.

### Cooldown bars

Use the existing engine-level cooldown design if available.

Make cooldown strategy-specific:

- `screened_vwap_scalp_v2` uses `v2_cooldown_bars`
- `ema_trend_pullback_v1` uses `etp_cooldown_bars`
- `screened_vwap_scalp` uses 0 unless already configured otherwise

Add a helper on config:

```rust
pub fn cooldown_bars_for_strategy(&self, strategy_id: &str) -> u64
```

Do not use static mutable state.

### Signal geometry

Long:

```text
entry = close
stop_loss = close - atr * etp_sl_atr_multiple
take_profit = close + atr * etp_tp_atr_multiple
```

Short:

```text
entry = close
stop_loss = close + atr * etp_sl_atr_multiple
take_profit = close - atr * etp_tp_atr_multiple
```

Reward/risk:

```text
rr = abs(take_profit - entry) / abs(entry - stop_loss)
```

Require:

```text
rr >= etp_min_reward_risk
```

### Expected reward and edge

Long:

```text
expected_reward_bps = (take_profit - entry) / entry * 10000
```

Short:

```text
expected_reward_bps = (entry - take_profit) / entry * 10000
```

Then:

```text
expected_net_edge_bps = expected_reward_bps - estimated_cost_bps
```

Require:

```text
expected_reward_bps >= etp_min_expected_reward_bps
expected_net_edge_bps >= etp_min_expected_net_edge_bps
```

### Confidence

Use deterministic scoring.

Recommended:

Start 50.

Add:

```text
+10 strict 15m trend passed
+10 5m confirmation passed
+10 1m EMA alignment passed
+10 reclaim/rejection trigger passed
+10 expected net edge passed
+5 volume ratio >= min
+5 ATR bps in range
```

Clamp at 100.

If confidence < `min_confidence`, return `Ok(None)`.

### Signal fields

Emitted signals must include:

```text
strategy_id = "ema_trend_pullback_v1"
signal_id deterministic
side
entry_timeframe = 1m
confirmation_timeframe = 5m
screening_timeframe = 15m
entry_time
entry_price
stop_loss
take_profit
confidence
regime
entry_reason
filters_passed
filters_failed
expected_reward_bps
estimated_cost_bps
expected_net_edge_bps
```

Signal ID must remain deterministic using the existing signal index scheme.

No random IDs. No UUID. No system time.

### Filters passed

Populate rich filters for diagnostics:

Examples:

```text
15m_trend_bullish
15m_trend_bearish
5m_confirmation_bullish
5m_confirmation_bearish
1m_ema_alignment_long
1m_ema_alignment_short
pullback_near_ema21
pullback_near_ema50
pullback_near_vwap
close_reclaim_long
close_reclaim_short
wick_rejection_long
wick_rejection_short
atr_bps_in_range
volume_ratio_ok
reward_risk_ok
expected_reward_ok
expected_net_edge_ok
direction_enabled
cooldown_ok
confidence_ok
```

For emitted signals, `filters_failed` should normally be empty.

## Update strategy comparison runner

Update comparison examples to allow:

```toml
[backtest]
strategy_run_mode = "comparison"
strategies = [
  "screened_vwap_scalp",
  "screened_vwap_scalp_v2",
  "ema_trend_pullback_v1"
]
reports_dir = "reports/comparison"
entry_geometry_mode = "reanchor_to_actual_entry"
```

Expected output:

```text
reports/comparison/screened_vwap_scalp/...
reports/comparison/screened_vwap_scalp_v2/...
reports/comparison/ema_trend_pullback_v1/...
reports/comparison/comparison_summary.csv
reports/comparison/comparison_summary.json
```

## Attribution and diagnostics

Existing reports should work because trades include `strategy_id`.

Ensure:

```text
reports/.../attribution_by_strategy.csv
```

includes `ema_trend_pullback_v1`.

No new report type is required in this patch.

## CLI output

Update research CLI where selected strategy config is printed.

For `ema_trend_pullback_v1`, print concise strategy config:

```text
Strategy:
  strategy_id = ema_trend_pullback_v1

EMA Trend Pullback V1 filters:
  pullback_to: ema21_or_vwap
  reclaim_mode: close_reclaim
  TP ATR multiple: 3.00
  SL ATR multiple: 1.00
  min reward/risk: 2.50
  min expected reward bps: 30.00
  min expected net edge bps: 12.00
  min ATR bps: 10.00
  max ATR bps: 180.00
  cooldown bars: 10
```

Keep output concise.

## Documentation

Update README.md.

Add `ema_trend_pullback_v1` under strategy variants.

Add or update `docs/STRATEGY_RESEARCH.md`.

Include:

- what `ema_trend_pullback_v1` is
- how it differs from `screened_vwap_scalp`
- how to run comparison with all three strategies
- explanation that it is not a profitability claim
- recommended initial config

Do not rewrite the whole README.

## Tests required

Add focused tests.

### Config tests

- parses_strategy_id_ema_trend_pullback_v1
- rejects_unknown_strategy_id_still
- parses_etp_defaults
- parses_etp_pullback_to_ema21_or_vwap
- rejects_unknown_etp_pullback_to
- parses_etp_reclaim_mode_close_reclaim
- rejects_unknown_etp_reclaim_mode
- rejects_invalid_etp_atr_range
- rejects_invalid_etp_tp_sl_multiple
- cooldown_bars_for_strategy_returns_etp_value

### Strategy signal tests

- etp_returns_none_when_indicators_missing
- etp_returns_none_when_15m_trend_neutral
- etp_long_requires_bullish_15m_trend
- etp_short_requires_bearish_15m_trend
- etp_long_requires_5m_bullish_confirmation
- etp_short_requires_5m_bearish_confirmation
- etp_long_requires_1m_ema_alignment
- etp_short_requires_1m_ema_alignment
- etp_rejects_pullback_too_far
- etp_accepts_pullback_near_ema21
- etp_accepts_pullback_near_vwap
- etp_long_close_reclaim_trigger
- etp_short_close_reclaim_trigger
- etp_long_wick_rejection_trigger
- etp_short_wick_rejection_trigger
- etp_rejects_atr_bps_below_min
- etp_rejects_atr_bps_above_max
- etp_rejects_volume_ratio_below_min
- etp_rejects_expected_reward_below_min
- etp_rejects_expected_net_edge_below_min
- etp_uses_configurable_tp_atr_multiple
- etp_uses_configurable_sl_atr_multiple
- etp_emits_long_signal_with_valid_geometry
- etp_emits_short_signal_with_valid_geometry
- etp_strategy_id_is_correct
- etp_filters_passed_are_populated

### Backtest / comparison tests

- backtest_selects_ema_trend_pullback_v1
- comparison_accepts_ema_trend_pullback_v1
- unknown_strategy_validation_still_fails
- attribution_by_strategy_supports_ema_trend_pullback_v1

Existing tests must continue passing.

## Required commands

Run:

```bash
cargo fmt
cargo build
cargo test
cargo run -- help
```

If `data/historical/BTCUSDT.csv` exists, run comparison:

```toml
[backtest]
strategy_run_mode = "comparison"
strategies = [
  "screened_vwap_scalp",
  "screened_vwap_scalp_v2",
  "ema_trend_pullback_v1"
]
reports_dir = "reports/comparison"
entry_geometry_mode = "reanchor_to_actual_entry"

[risk]
max_drawdown_pct = 100.0
max_daily_loss_pct = 100.0
```

Then:

```bash
cargo run --release -- research --config config/research.toml
```

Expected:

- `reports/comparison/ema_trend_pullback_v1/` exists.
- `reports/comparison/comparison_summary.csv` has all three strategies.
- All reports and diagnostics are written for the new strategy.
- Audit passes.
- No paper/live/exchange/LLM behavior is added.

Do not hardcode expected profitability.

## Strictly forbidden

Do not implement:

- paper trading
- live trading
- exchange order placement
- exchange adapter
- websocket
- database
- dashboard
- Telegram
- LLM signal generation
- AI advisor
- optimizer
- auto parameter tuning
- grid search
- walk-forward optimization
- portfolio multi-strategy router
- shared-account multi-strategy
- profitability claims

## Expected final result

At the end of this patch:

- New strategy `ema_trend_pullback_v1` exists.
- Existing V1 and V2 strategies remain unchanged.
- Strategy selection supports the new strategy.
- Comparison runner can run all three strategies.
- Reports include the new strategy.
- Docs explain the strategy and comparison usage.
- All tests pass.

## Commit message suggestion

strategy: add ema trend pullback v1
