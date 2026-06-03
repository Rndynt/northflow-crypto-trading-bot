# Deep Refactor Task for ARIA Crypto Scalper

Repository: https://github.com/Rndynt/crypto-scalper

You are a senior Rust engineer, quant trading system architect, exchange execution specialist, and HFT/scalping bot reviewer.

Your job is to deeply refactor and fix this codebase so it becomes a safer, cleaner, automated AI-assisted HFT quant trading bot.

The final target architecture is:

- 15m timeframe = screening, market bias, trade permission layer
- 1m timeframe = execution, entry timing, fast scalp entry layer
- 5m timeframe = optional intermediate confirmation layer
- AI/LLM = supervisor, veto, explanation, review, learning, and post-trade analysis
- AI/LLM must not be a latency-critical blocker for fast 1m entries unless explicitly configured
- Risk and execution safety must override all strategy aggression
- Paper mode must remain the default
- Live mode must fail closed on unsafe execution conditions

Do not make cosmetic-only changes. Implement real fixes. Keep existing architecture where useful, but do not preserve broken behavior unless a migration or backward-compatible alias is necessary.

Current system context:

The repo is a Rust project with binary named aria.

Important existing modules:

    src/main.rs
    src/lib.rs
    src/agents/*
    src/data/*
    src/strategy/*
    src/execution/*
    src/quant.rs
    src/backtest/*
    src/config.rs
    config/default.toml
    config/aggressive.toml
    config/conservative.toml

The architecture already includes:

    DataAgent
    SignalAgent
    BrainAgent
    ManagerAgent
    RiskAgent
    ExecutionAgent
    MonitorAgent
    LearningAgent
    SurvivalAgent
    Watchdog
    MessageBus
    PaperExchange
    BinanceFutures
    MexcFutures
    PositionBook
    RiskManager
    QuantEngine

Keep the multi-agent architecture, but fix the incorrect trading, state, risk, and execution logic.

---

# P0-1: Fix multi-timeframe state mixing

## Problem

The current code appears to use one SymbolState per symbol. Startup bootstrap loads multiple timeframes into the same SymbolState.

This causes this dangerous behavior:

    1m candles update the same SymbolState
    5m candles update the same SymbolState
    15m candles update the same SymbolState

This corrupts indicators such as:

    EMA
    ATR
    VWAP
    RSI
    ADX
    Bollinger
    Keltner
    ROC
    Choppiness
    volume SMA

After bootstrap, a 1m entry signal can be based on indicator state polluted by 5m and 15m candles.

This is a major bug for a system that wants 15m screening and 1m execution.

## Required solution

Introduce proper per-timeframe state.

Recommended structure:

    pub struct SymbolMultiTfState {
        pub symbol: String,
        pub states: HashMap<i64, SymbolState>,
        pub last_screening: Option<ScreeningState>,
    }

Alternative acceptable structure:

    HashMap<String, HashMap<i64, SymbolState>>

Required mapping:

    BTCUSDT -> 60 seconds -> SymbolState for 1m only
    BTCUSDT -> 300 seconds -> SymbolState for 5m only
    BTCUSDT -> 900 seconds -> SymbolState for 15m only

Do not let candles from different timeframes update the same indicator state.

## Implementation requirements

Refactor the following areas:

    src/main.rs
    src/data/kline_bootstrap.rs
    src/agents/data.rs
    src/agents/signal.rs
    any code that reads SymbolState

Rules:

    Entry timeframe must be configurable.
    In aggressive scalping mode, entry timeframe should be 1m.
    Screening timeframe should be configurable and default to 15m.
    Higher timeframe states must be updated only by their own closed candles.
    1m signal evaluation must read the 1m SymbolState.
    15m screening must read the 15m SymbolState.
    Do not mutate 15m state while evaluating 1m signal.

Add tests proving:

    1m state receives only 1m candles.
    15m state receives only 15m candles.
    Bootstrap does not mix indicators across timeframes.
    A 15m candle cannot change 1m EMA, ATR, or VWAP.
    A 1m candle cannot change 15m EMA, ATR, or VWAP.

---

# P0-2: Add real 15m Screening Agent / Market Bias Layer

## Problem

Current higher timeframe logic only stores open and close, then calculates simple candle direction:

    close > open = bullish
    close < open = bearish

Then it only modifies signal confidence.

This is too weak and is not a real screening layer.

## Required solution

Create a real 15m screening and market bias module.

Add this enum:

    pub enum ScreeningBias {
        Bullish,
        Bearish,
        NoTrade,
    }

Add this state structure:

    pub struct ScreeningState {
        pub symbol: String,
        pub timeframe_secs: i64,
        pub bias: ScreeningBias,
        pub confidence: u8,
        pub reason: String,
        pub close: f64,
        pub vwap: Option<f64>,
        pub ema_fast: Option<f64>,
        pub ema_mid: Option<f64>,
        pub ema_slow: Option<f64>,
        pub kalman_direction: i8,
        pub atr_pct: Option<f64>,
        pub choppiness: Option<f64>,
        pub updated_at: DateTime<Utc>,
    }

## Screening rules

Use 15m as the default screening timeframe.

Bullish bias should require a combination such as:

    close_15m > VWAP_15m
    EMA8_15m > EMA21_15m > EMA50_15m
    Kalman direction positive or Kalman slope positive
    ATR is not too low and not too extreme
    Choppiness is not too high
    Price is not overextended too far from VWAP

Bearish bias should require a combination such as:

    close_15m < VWAP_15m
    EMA8_15m < EMA21_15m < EMA50_15m
    Kalman direction negative or Kalman slope negative
    ATR is not too low and not too extreme
    Choppiness is not too high
    Price is not overextended too far from VWAP

NoTrade should be returned when:

    EMA structure is mixed
    Price is chopping around VWAP
    Kalman is neutral
    ATR is too low
    ATR is too extreme
    Choppiness is high
    Required indicator data is missing or stale

## Mandatory behavior

15m screening must be a hard gate.

Rules:

    If 15m bias is Bullish, 1m can only search for LONG.
    If 15m bias is Bearish, 1m can only search for SHORT.
    If 15m bias is NoTrade, 1m must not open new trades.

Do not allow 1m strategy to override 15m NoTrade unless a config flag explicitly allows it in paper mode only.

## Implementation requirements

Add a dedicated module, for example:

    src/strategy/screening.rs

or:

    src/agents/screening.rs

Add a new event:

    AgentEvent::ScreeningUpdated

The SignalAgent or a new ScreeningAgent should update screening state when the 15m candle closes.

Entry signal evaluation must read the latest screening state.

If screening state is stale, block new entries.

Add config:

    [screening]
    enabled = true
    timeframe = "15m"
    max_age_secs = 1800
    hard_gate = true
    allow_countertrend_paper = false
    min_confidence = 60
    max_vwap_distance_pct = 0.8
    min_atr_pct = 0.03
    max_atr_pct = 2.5
    max_choppiness = 61.8

Add dashboard and log visibility:

    current 15m bias
    screening confidence
    screening reason
    screening age
    last screening update time

Add Telegram or monitor notification when bias changes.

---

# P0-3: Make 1m Execution Agent obey 15m bias

## Problem

Current 1m signal logic selects strategies and applies MTF context as a confidence modifier. This is not strict enough for fast scalping.

## Required solution

1m execution signal must only run after 15m screening allows it.

Required logic:

    If ScreeningBias::Bullish:
        accept only LONG pre-signals

    If ScreeningBias::Bearish:
        accept only SHORT pre-signals

    If ScreeningBias::NoTrade:
        emit SignalEvaluation with reason = screening_no_trade
        do not emit PreSignal

    If screening is stale:
        emit SignalEvaluation with reason = screening_stale
        do not emit PreSignal

## Entry model

Implement or refactor a clean 1m screened scalp strategy.

Name suggestion:

    screened_vwap_scalp

For 15m bullish:

    price pulls back near VWAP1m or EMA21
    candle closes back above EMA8 or VWAP
    OFI is neutral or bullish
    VPIN is not abnormal
    spread is acceptable
    stop loss from ATR1m or recent swing low
    take profit around 0.8R to 1.5R initially, configurable
    max holding time 3 to 8 minutes

For 15m bearish:

    inverse logic
    price pulls back near VWAP1m or EMA21
    candle closes back below EMA8 or VWAP
    OFI is neutral or bearish
    VPIN is not abnormal
    spread is acceptable
    stop loss from ATR1m or recent swing high
    take profit around 0.8R to 1.5R initially, configurable
    max holding time 3 to 8 minutes

---

# P0-4: Fix Partial TP being treated as full PositionClosed

## Problem

PositionBook can trigger PartialTP and reduce 50 percent internally, but ExecutionAgent treats every returned exit as a full close.

Dangerous effects:

    risk.on_position_closed is called
    exchange.cancel_all is called
    PositionClosed is published
    SignalAgent resumes screening for the symbol
    LearningAgent records the trade as fully closed
    Kelly receives incorrect outcome
    remaining 50 percent position can become desynced from local and exchange state

## Required solution

Use explicit position actions.

There is already a PositionAction enum concept. Make it actually drive execution.

Refactor PositionBook::check_exits to return:

    Vec<PositionAction>

Actions:

    Close(position, reason)
    Reduce(position, reduce_size, reason)
    MoveSL(position, new_stop_loss)
    None

ExecutionAgent behavior:

For Close:

    send reduce-only close order for remaining size
    cancel protective orders
    publish PositionClosed
    call risk.on_position_closed with final realized PnL

For Reduce:

    send reduce-only order for partial size
    update local remaining size
    do not publish PositionClosed
    publish PositionReduced
    do not resume SignalAgent for that symbol
    do not decrement open position count as full close

For MoveSL:

    cancel old broker-side stop loss
    place new broker-side stop loss
    update local state only after exchange confirms
    publish StopMoved

## Required new events

Add events similar to:

    AgentEvent::PositionReduced {
        client_id: String,
        symbol: String,
        side: Side,
        reduced_size: f64,
        remaining_size: f64,
        entry_price: f64,
        exit_price: f64,
        pnl_usd: f64,
        reason: PositionExitReason,
        strategy: String,
    }

    AgentEvent::StopMoved {
        client_id: String,
        symbol: String,
        old_stop: f64,
        new_stop: f64,
        reason: String,
    }

    AgentEvent::ExecutionFailed {
        symbol: String,
        client_id: Option<String>,
        reason: String,
    }

Update:

    MonitorAgent
    LearningAgent
    RiskAgent
    SignalAgent
    Telegram notifier
    Trade journal

Mandatory behavior:

    Partial TP must not unlock the symbol for new entries.
    Partial TP must not count as a full closed trade for Kelly.
    Learning may log partial PnL separately.
    Full strategy outcome should be finalized only on full close.
    Broker-side protective orders must reflect remaining position.

---

# P0-5: Fix breakeven and trailing stop being local-only

## Problem

When trailing stop or breakeven changes local stop_loss, the exchange protective order is not updated.

If the bot dies, broker-side stop loss remains old.

## Required solution

Implement real broker-side stop update.

When SL changes:

    1. cancel old SL order
    2. place new STOP_MARKET or reduce-only protective order
    3. only after exchange confirms, update local PositionBook
    4. publish StopMoved
    5. if update fails in live mode, freeze trading and alert operator

Add protective order tracking to Position:

    pub sl_client_id: Option<String>
    pub tp_client_id: Option<String>

When placing initial protective orders, store client IDs.

When moving SL:

    cancel previous SL
    submit replacement SL
    update PositionBook only after success

Paper mode should simulate this behavior.

Live mode must fail closed if protective order replacement fails.

---

# P0-6: Do not assume limit orders are filled immediately

## Problem

ExecutionAgent may convert entry orders to limit orders when spread is large. After exchange.place_order succeeds, the bot immediately treats the position as open.

This is incorrect because a limit order can be accepted but not filled.

## Required solution

Separate order acceptance from fill confirmation.

For market orders:

    require executedQty > 0
    require avg fill price > 0
    prefer exchange response type that confirms actual fill when supported

For limit orders, choose safe behavior.

Recommended for fast scalping:

    use IOC or FOK limit order
    if not filled immediately, cancel or expire
    publish ExecutionFailed

Alternative:

    submit limit order
    poll order status until FILLED, PARTIALLY_FILLED, EXPIRED, CANCELED, REJECTED, or timeout
    only open local position after actual fill
    if partial fill occurs, handle partial fill correctly or cancel remainder

## Required implementation

Extend Exchange trait if needed:

    fetch_order_status(symbol, client_id)

Add OrderStatus enum:

    New
    PartiallyFilled
    Filled
    Canceled
    Expired
    Rejected
    Unknown

Do not call risk.on_position_opened.

Do not call book.open.

Do not place protective orders.

Do not publish OrderFilled.

until actual fill is confirmed.

If fill fails or times out:

    publish ExecutionFailed
    release pending symbol lock
    alert monitor or Telegram

---

# P0-7: Prevent pending symbol lock from getting stuck

## Problem

RiskAgent inserts symbol into pending_symbols after allowing a trade.

It removes the symbol on:

    OrderFilled
    Manager veto

But if execution fails, there is no event to clear pending status.

This can cause a symbol to stop trading forever until restart.

## Required solution

Add:

    AgentEvent::ExecutionFailed { symbol, client_id, reason }

RiskAgent must handle it:

    pending_symbols.remove(symbol)

SignalAgent may resume screening if there is no open position.

Monitor and Telegram should report the failure.

ExecutionAgent must emit ExecutionFailed whenever:

    place_order fails
    fill confirmation fails
    limit order times out
    protective order placement fails
    invalid proposal is discarded after RiskVerdict allowed
    exchange duplicate check fails due to recoverable issue
    order status is unknown
    exchange rejects order
    precision validation fails

For hard safety failures, also freeze trading when appropriate.

---

# P0-8: Fix exchange precision and Binance futures position mode

## Problem

Binance order formatting is too hardcoded.

Current risk examples:

    quantity formatting by rough symbol defaults
    price formatted with 2 decimals
    stopPrice formatted with 2 decimals

This can break for many symbols because tick size, step size, minQty, minNotional, price precision, and quantity precision vary.

Config also has position_mode = dual-side, but Binance orders do not clearly handle positionSide.

## Required solution

Implement exchange symbol filters.

Add:

    ExchangeInfoCache

    SymbolFilters {
        tick_size
        step_size
        min_qty
        min_notional
        price_precision
        quantity_precision
    }

Fetch Binance Futures exchange info:

    GET /fapi/v1/exchangeInfo

Use filters to normalize:

    price
    stop_price
    quantity

before every order.

## Required behavior

Reject order if:

    size < minQty
    notional < minNotional
    price is invalid
    quantity is invalid

unless min-margin config can safely floor it.

Round:

    price to tickSize
    stop price to tickSize
    quantity to stepSize

Support Binance futures position mode:

    If position_mode = dual-side:
        send positionSide=LONG for long entries
        send positionSide=SHORT for short entries
        protective orders must use matching positionSide

    If position_mode = one-way:
        do not send positionSide

Add config:

    [exchange]
    position_mode = "one-way"

or:

    [exchange]
    position_mode = "dual-side"

Make naming consistent.

---

# P0-9: Make backtest match live strategy behavior

## Problem

Backtest currently uses legacy strategies while live mode maps strategy names to different quant implementations.

Current mismatch:

    Backtest:
        EmaRibbon
        MeanReversion
        Momentum
        VwapScalp
        Squeeze

    Live:
        EmaRibbon -> OrderFlow
        Momentum -> TradeFlow
        VwapScalp -> KalmanTrend
        MeanReversion -> MicrostructureReversion
        Squeeze -> Squeeze

This means backtest results do not represent live trading.

## Required solution

Make backtest use the same strategy pipeline as live mode.

Minimum acceptable fix:

    BacktestEngine must use the same strategy mapping as SignalAgent.

Better fix:

    Implement event-driven backtest that replays:
        CandleClosed 1m
        CandleClosed 5m
        CandleClosed 15m
        Screening updates
        Signal evaluation
        Risk evaluation
        Execution simulation
        SL, TP, partial TP, trailing, time exit

If full event-driven backtest is too large, at least:

    use the same live quant strategy mapping
    support multi-timeframe CSV input
    ensure entries use only already-closed candles
    do not enter using future high or low from the same candle
    model fee, spread, slippage, and market impact
    add tests to prevent look-ahead bias

---

# P0-10: Re-enable real cost and net edge gate for scalping

## Problem

Config uses:

    min_net_edge_bps = 0.0

RiskManager skips net-edge check when this is less than or equal to zero.

For 1m scalping, this is dangerous because fee, spread, slippage, and market impact can consume the whole target.

Reward/risk alone does not guarantee positive expected value.

## Required solution

Add real scalping cost gate.

Expected edge must exceed estimated total cost:

    expected_edge_bps > taker_fee_bps * 2 + spread_bps + slippage_bps + market_impact_bps + safety_buffer_bps

Add config:

    [risk]
    min_net_edge_bps = 2.0
    cost_safety_buffer_bps = 2.0
    require_positive_expected_value = true

If winrate estimate is unavailable, use conservative assumptions.

For paper mode, log blocked trades with:

    blocked_cost_gate
    gross_edge_bps
    estimated_cost_bps
    net_edge_bps
    spread_bps
    slippage_bps

For live mode, this must be a hard gate.

---

# P0-11: LLM should not block fast HFT entry by default

## Problem

Aggressive config enables Manager LLM and sets long timeout. This is incompatible with 1m fast scalping or HFT-like execution.

The market opportunity can disappear before LLM returns.

## Required solution

Create explicit operating modes:

    paper-fast
    paper-ai-reviewed
    live-safe
    backtest

Recommended behavior:

paper-fast:

    LLM manager disabled
    Brain optional
    deterministic TA, quant, risk, and execution run quickly

paper-ai-reviewed:

    Brain and Manager enabled
    slower
    useful for analysis and research

live-safe:

    Manager optional with short timeout
    fail closed on LLM error if LLM is required
    never wait too long for entry

backtest:

    no LLM dependency

Add config fields:

    [llm]
    entry_path_enabled = false
    max_entry_latency_ms = 1500

    [manager]
    enabled = false
    max_entry_latency_ms = 1500

If LLM times out in fast mode:

    do not block runtime
    fall back to deterministic quant and risk rules
    log LLM timeout

If fail_closed_without_llm = true:

    block safely

---

# P1: Rename misleading strategy names while keeping backward compatibility

## Problem

Current config names are misleading:

    ema_ribbon actually runs OrderFlow
    momentum actually runs TradeFlow
    vwap_scalp actually runs KalmanTrend
    mean_reversion actually runs MicrostructureReversion

## Required solution

Introduce real strategy names:

    order_flow
    trade_flow
    kalman_trend
    microstructure_reversion
    squeeze
    screened_vwap_scalp

Keep backward-compatible aliases:

    ema_ribbon -> order_flow
    momentum -> trade_flow
    vwap_scalp -> kalman_trend
    mean_reversion -> microstructure_reversion

When an old alias is used, log warning:

    deprecated strategy alias used: ema_ribbon -> order_flow

Update:

    README.md
    docs/CONFIG.md
    config/default.toml
    config/aggressive.toml
    config/conservative.toml

---

# P1: Add proper max holding time for fast scalping

## Problem

Aggressive config has max_hold_secs = 0, which disables time exit.

The target behavior is fast scalping in and out.

## Required solution

For fast scalping configs, set:

    max_hold_secs = 300

Allow configurable range:

    180 to 600 seconds

Optionally add strategy-specific holding config:

    [strategy_hold]
    screened_vwap_scalp_secs = 300
    order_flow_secs = 180
    kalman_trend_secs = 420
    squeeze_secs = 600

Do not force bad exits too aggressively, but do not let 1m scalps turn into accidental swing positions.

---

# P1: Improve journal and metrics

Add fields to trade journal:

    entry_timeframe
    screening_timeframe
    screening_bias
    screening_confidence
    screening_reason
    spread_bps
    slippage_bps
    execution_latency_ms
    order_type
    fill_status
    fee_usd
    cost_bps
    strategy_raw
    strategy_alias_resolved
    llm_used
    manager_used

These fields are required for post-trade learning and debugging.

---

# P1: Improve strategy health and Kelly

## Problem

Kelly sizing can become inaccurate if partial TP or duplicated historical ingestion is counted as a full trade.

## Required solution

Rules:

    Only full closed trade outcomes should be recorded into Kelly.
    Partial TP can be recorded separately.
    Partial TP must not count as a complete trade outcome.
    Kelly must be capped conservatively.
    Kelly should need more data before activating.

Add config:

    [quant]
    kelly_enabled = true
    kelly_fractional = 0.25
    kelly_cap = 0.10
    kelly_min_trades = 50

For live mode, default to conservative fractional Kelly.

---

# Specific implementation guide

## Message events

Update the message system with:

    ScreeningUpdated
    ExecutionFailed
    PositionReduced
    StopMoved
    OrderStatusUpdated

All agents should ignore events they do not need.

## Data and bootstrap

Refactor:

    src/data/kline_bootstrap.rs
    src/agents/data.rs
    src/agents/signal.rs

to use per-timeframe state.

## Strategy and screening

Add:

    src/strategy/screening.rs
    src/strategy/screened_vwap_scalp.rs

or equivalent.

Screening must read 15m state and publish bias.

1m execution must require fresh screening state.

## Execution

Refactor:

    src/execution/position.rs
    src/agents/execution.rs
    src/execution/binance.rs
    src/execution/mexc.rs
    src/execution/paper.rs

Add proper handling of:

    Close
    Reduce
    MoveSL

Do not treat every exit action as full close.

## Exchange trait

Extend safely with:

    fetch_order_status
    fetch_exchange_info or symbol filters
    replace_stop_order or cancel plus place

Paper exchange must implement the same trait behavior.

## Risk

Update:

    src/agents/risk.rs
    src/execution/risk.rs
    src/execution/tcm.rs

Add cost gate.

Add pending release on ExecutionFailed.

## Backtest

Update:

    src/backtest/*

Backtest must use the same strategy mapping as live mode at minimum.

Prefer event-driven parity if feasible.

## Config

Update:

    config/default.toml
    config/aggressive.toml
    config/conservative.toml
    docs/CONFIG.md
    README.md

Add new sections:

    [screening]
    [execution]
    [strategy_hold]

Recommended aggressive paper scalping config:

    [mode]
    run_mode = "paper"
    dry_run = true
    fail_closed_without_llm = false
    single_position_per_symbol = true

    [pairs]
    timeframes = ["1m", "5m", "15m"]

    [screening]
    enabled = true
    timeframe = "15m"
    hard_gate = true
    max_age_secs = 1800
    min_confidence = 60
    allow_countertrend_paper = false

    [strategy]
    mode = "adaptive"
    active = ["screened_vwap_scalp", "order_flow", "kalman_trend", "squeeze"]
    min_ta_confidence = 62

    [risk]
    risk_per_trade_pct = 0.5
    max_open_positions = 2
    max_daily_loss_pct = 3.0
    max_drawdown_pct = 6.0
    max_leverage = 10
    min_reward_risk = 1.2
    min_net_edge_bps = 2.0
    cost_safety_buffer_bps = 2.0
    require_positive_expected_value = true
    max_hold_secs = 300

    [llm]
    entry_path_enabled = false
    timeout_secs = 8

    [manager]
    enabled = false
    timeout_secs = 3

    [quant]
    kelly_enabled = true
    kelly_fractional = 0.25
    kelly_cap = 0.10
    kelly_min_trades = 50

Do not default live-safe mode to 100x leverage.

---

# Required tests

Add or update tests for the following.

## Multi-timeframe state tests

    bootstrap does not mix 1m and 15m indicators
    15m candle updates only 15m state
    1m candle updates only 1m state
    1m signal reads 15m screening but does not mutate 15m state

## Screening tests

    bullish 15m allows long and blocks short
    bearish 15m allows short and blocks long
    no-trade blocks both
    stale screening blocks entry

## Execution tests

    partial TP emits PositionReduced, not PositionClosed
    partial TP does not unlock symbol
    full TP emits PositionClosed
    MoveSL emits StopMoved and updates protective order
    failed order emits ExecutionFailed
    ExecutionFailed releases pending symbol

## Limit order tests

    accepted but unfilled limit order does not open PositionBook
    filled limit order opens position
    timed-out limit order emits ExecutionFailed

## Cost gate tests

    trade blocked when net edge is less than or equal to cost
    trade allowed when net edge is greater than cost plus buffer

## Backtest parity tests

    backtest uses same strategy mapping as live
    no entry uses future candle high or low

---

# Acceptance criteria

The final patch must satisfy:

    cargo fmt
    cargo test
    cargo build

If full build cannot pass due to environment-specific OpenSSL or native dependency issues, document exactly why and still ensure code-level fixes are complete.

Do not introduce real API keys, secrets, private values, or user credentials.

Paper mode must remain default.

Live mode must be safer than before and fail closed on:

    missing protective orders
    failed SL replacement
    unknown order fill status
    stale screening
    stale book ticker
    invalid precision/filter
    execution failure
    unknown exchange state

---

# Final deliverables

When finished, provide:

1. Summary of changed files.
2. Explanation of each fixed issue.
3. New config examples for:
   - paper-fast
   - paper-ai-reviewed
   - live-safe
4. How to run:
   - cargo fmt
   - cargo test
   - cargo build --release
   - paper mode run command
5. Any remaining limitations or TODOs.
6. Important safety notes for live trading.

Implement the fix deeply. Do not stop after superficial changes.