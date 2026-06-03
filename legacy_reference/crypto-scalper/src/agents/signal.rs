//! Signal agent — listens for `CandleClosed` events, updates per-symbol
//! state, runs the regime detector + active strategies, and emits a
//! `PreSignalEmitted` event for the best candidate.
//!
//! **P0-1/P0-2/P0-3 fix**: The agent now maintains a separate per-symbol
//! `SymbolState` for the 15m (screening) timeframe alongside the 1m entry
//! states. When a 15m candle closes, `ScreeningBias` is computed from the
//! HTF state and stored per symbol. 1m entry signals are hard-gated: a LONG
//! is only emitted when the screening bias is `Bullish` or `Unknown`; a SHORT
//! only when `Bearish` or `Unknown`. `ScreeningUpdated` is published so other
//! agents can observe the bias in real time.

use crate::agents::MessageBus;
use crate::agents::messages::{AgentEvent, AgentId, ScreeningBias, SignalEvaluationMsg};
use crate::config::{AdvancedAlphaCfg, Schedule};
use crate::data::Side;
use crate::feeds::ExternalSnapshot;
use crate::feeds::alt_data::AltDataInputs;
use crate::microstructure::{Ofi, Vpin};
use crate::quant::QuantEngine;
use crate::shared_state::SharedState;
use crate::strategy::{
    RegimeDetector, Strategy,
    alpha_gate::{
        AdvancedAlphaInputs, AlphaGateDecision, advanced_alpha_gate, alt_data_inputs_from_snapshot,
        funding_rate_from_snapshot, kalman_trend_score,
    },
    kalman_trend::KalmanTrendStrategy,
    microstructure_reversion::MicrostructureReversion,
    order_flow::OrderFlow,
    screened_vwap_scalp::ScreenedVwapScalp as ScreenedVwapScalpStrategy,
    select_strategies,
    squeeze::Squeeze,
    state::{PreSignal, StrategyName, SymbolState},
    trade_flow::TradeFlow,
};
use chrono::{DateTime, Timelike, Utc};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

pub struct SignalAgentConfig {
    pub active: Vec<StrategyName>,
    pub schedule: Schedule,
    pub advanced_alpha: AdvancedAlphaCfg,
    pub quant_engine: Option<Arc<QuantEngine>>,
    pub paper_scout_enabled: bool,
    pub entry_timeframe_secs: i64,
    /// Timeframe used for higher-timeframe screening (default: 900 = 15m).
    /// When a candle of this timeframe closes, `ScreeningBias` is recomputed.
    pub screening_timeframe_secs: i64,
    /// REST base URL used to bootstrap HTF states on startup.
    pub rest_base_url: String,
    /// Symbol list — needed for HTF bootstrap allocation.
    pub symbols: Vec<String>,
}

pub fn spawn(
    bus: MessageBus,
    states: Arc<Mutex<HashMap<String, SymbolState>>>,
    cfg: SignalAgentConfig,
    shared_state: Arc<SharedState>,
) -> JoinHandle<()> {
    let SignalAgentConfig {
        active,
        schedule,
        advanced_alpha,
        quant_engine,
        paper_scout_enabled,
        entry_timeframe_secs,
        screening_timeframe_secs,
        rest_base_url,
        symbols,
    } = cfg;

    let mut rx = bus.subscribe();

    tokio::spawn(async move {
        info!(?active, "signal agent starting");
        crate::agents::heartbeat::spawn(bus.clone(), AgentId::Signal);
        shared_state.heartbeat("signal");

        // --- HTF screening state (P0-1/P0-2/P0-3) ---
        // Separate SymbolState per symbol for the screening timeframe.
        // These are NOT shared with main.rs — they're owned solely by this agent.
        let mut htf_states: HashMap<String, SymbolState> = symbols
            .iter()
            .map(|s| (s.clone(), SymbolState::new(s)))
            .collect();
        let mut screening_bias: HashMap<String, ScreeningBias> = HashMap::new();

        // Bootstrap HTF states so the first screening bias is available immediately.
        if screening_timeframe_secs > 0 && !rest_base_url.is_empty() {
            bootstrap_htf_states(&mut htf_states, &rest_base_url, screening_timeframe_secs).await;
            // Compute initial bias from bootstrapped candles
            for (sym, state) in &htf_states {
                let bias = compute_htf_bias(state);
                screening_bias.insert(sym.clone(), bias);
                info!(symbol = %sym, bias = %bias.as_str(), "initial screening bias (bootstrapped)");
            }
        }

        // VPIN bucket size helper — target ~$50k USD per bucket
        fn vpin_bucket_size_for(symbol: &str) -> f64 {
            match symbol {
                s if s.starts_with("BTC") => 0.8,
                s if s.starts_with("ETH") => 16.0,
                s if s.starts_with("SOL") => 250.0,
                s if s.starts_with("BNB") => 120.0,
                _ => 10.0,
            }
        }

        let mut ofi_by_symbol: HashMap<String, Ofi> = HashMap::new();
        let mut vpin_by_symbol: HashMap<String, Vpin> = HashMap::new();
        let mut feeds_by_symbol: HashMap<String, TimedExternalSnapshot> = HashMap::new();
        let mut higher_timeframes: HashMap<String, BTreeMap<i64, HigherTimeframeSnapshot>> =
            HashMap::new();
        // Symbols that currently have an open position.
        let mut open_symbols: std::collections::HashSet<String> = std::collections::HashSet::new();

        loop {
            let ev = match rx.recv().await {
                Ok(ev) => ev,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    warn!(skipped = n, "broadcast lagged — skipping events");
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            };
            match ev {
                AgentEvent::FeedsSnapshot(msg) => {
                    feeds_by_symbol.insert(
                        msg.symbol,
                        TimedExternalSnapshot {
                            snapshot: msg.snapshot,
                            ts: msg.ts,
                        },
                    );
                }
                AgentEvent::Tick { symbol, trade } => {
                    let (buy_vol, sell_vol) = if trade.is_buyer_maker {
                        (0.0, trade.qty)
                    } else {
                        (trade.qty, 0.0)
                    };
                    let vpin_tracker = vpin_by_symbol
                        .entry(symbol.clone())
                        .or_insert_with(|| Vpin::new(vpin_bucket_size_for(&symbol), 50));
                    let vpin_value = vpin_tracker.update(buy_vol, sell_vol);
                    if let Some(vpin) = vpin_value {
                        let abnormal_check = vpin_tracker.is_abnormal();
                        let abnormal = abnormal_check
                            .map(|(is_ab, _raw, _thresh)| is_ab)
                            .unwrap_or(false);
                        let mut states_guard = states.lock().await;
                        if let Some(state) = states_guard.get_mut(&symbol) {
                            let was_abnormal = state.vpin_abnormal;
                            if abnormal && !was_abnormal {
                                if let Some((_, raw, thresh)) = abnormal_check {
                                    warn!(symbol=%symbol, vpin=raw, threshold=thresh, "VPIN ABNORMAL — above 95th percentile");
                                }
                            }
                            state.last_vpin = Some(vpin);
                            state.vpin_abnormal = abnormal;
                        }
                    }
                }
                AgentEvent::BookTicker {
                    symbol,
                    best_bid,
                    bid_qty,
                    best_ask,
                    ask_qty,
                } => {
                    let ofi = ofi_by_symbol
                        .entry(symbol.clone())
                        .or_insert_with(|| Ofi::new(20))
                        .update(bid_qty, ask_qty);
                    let mut states = states.lock().await;
                    if let Some(state) = states.get_mut(&symbol) {
                        state
                            .order_book
                            .set_top_with_qty(best_bid, bid_qty, best_ask, ask_qty);
                        if let Some(value) = ofi {
                            state.last_ofi = Some(value);
                        }
                    }
                }
                AgentEvent::DepthUpdate { symbol, bids, asks } => {
                    let mut states = states.lock().await;
                    if let Some(state) = states.get_mut(&symbol) {
                        state.order_book.update_depth(bids, asks);
                    }
                }
                AgentEvent::CandleClosed {
                    symbol,
                    timeframe_secs,
                    candle,
                } => {
                    if !is_valid_closed_candle(&candle) {
                        warn!(
                            symbol = %symbol,
                            timeframe_secs,
                            open = candle.open,
                            high = candle.high,
                            low = candle.low,
                            close = candle.close,
                            "signal: dropping malformed candle before indicator/strategy update"
                        );
                        bus.publish(AgentEvent::SignalEvaluation(SignalEvaluationMsg {
                            symbol,
                            timeframe_secs,
                            regime: None,
                            candles: 0,
                            strategies: Vec::new(),
                            reason: "malformed_candle_ohlc".to_string(),
                            best_strategy: None,
                            best_confidence: None,
                        }));
                        continue;
                    }

                    // ── HTF screening update (P0-2/P0-3) ──────────────────────────
                    // When a 15m (screening) candle closes, update the HTF state and
                    // recompute the ScreeningBias for this symbol. Publish the bias
                    // so other agents can observe it.
                    if screening_timeframe_secs > 0 && timeframe_secs == screening_timeframe_secs {
                        if let Some(htf) = htf_states.get_mut(&symbol) {
                            htf.on_closed(candle);
                            let bias = compute_htf_bias(htf);
                            let prev_bias = screening_bias.get(&symbol).copied();
                            screening_bias.insert(symbol.clone(), bias);
                            // Only log when bias actually changes — avoids spam every 15m
                            if prev_bias != Some(bias) {
                                info!(
                                    symbol = %symbol,
                                    bias = %bias.as_str(),
                                    prev = %prev_bias.map(|b| b.as_str()).unwrap_or("none"),
                                    candles = htf.candles.len(),
                                    "📡 15m bias changed"
                                );
                            } else {
                                debug!(
                                    symbol = %symbol,
                                    bias = %bias.as_str(),
                                    candles = htf.candles.len(),
                                    "📡 15m bias unchanged"
                                );
                            }
                            bus.publish(AgentEvent::ScreeningUpdated {
                                symbol: symbol.clone(),
                                bias,
                                ts: Utc::now(),
                            });
                        }
                        // Also store the raw open/close snapshot for legacy HTF bias context
                        higher_timeframes
                            .entry(symbol)
                            .or_default()
                            .insert(timeframe_secs, HigherTimeframeSnapshot::from_candle(candle));
                        continue;
                    }

                    // ── Non-entry, non-screening timeframe (e.g. 5m when entry=1m) ─
                    if timeframe_secs != entry_timeframe_secs {
                        higher_timeframes
                            .entry(symbol)
                            .or_default()
                            .insert(timeframe_secs, HigherTimeframeSnapshot::from_candle(candle));
                        continue;
                    }

                    // ── Entry timeframe (e.g. 1m) ─────────────────────────────────
                    if in_dead_zone(&schedule) && !paper_scout_enabled {
                        bus.publish(AgentEvent::SignalEvaluation(SignalEvaluationMsg {
                            symbol,
                            timeframe_secs,
                            regime: None,
                            candles: 0,
                            strategies: Vec::new(),
                            reason: format!(
                                "dead_zone_{}-{}_WIB",
                                schedule.dead_zone_start_hour_wib, schedule.dead_zone_end_hour_wib
                            ),
                            best_strategy: None,
                            best_confidence: None,
                        }));
                        continue;
                    }

                    if open_symbols.contains(&symbol) {
                        debug!(symbol = %symbol, "signal: holding position — skipping screening");
                        continue;
                    }

                    // P0-3: Gate the signal by the 15m screening bias.
                    // The bias is checked after strategy evaluation so we can log both
                    // the strategy outcome and the bias block separately.
                    let current_bias = screening_bias
                        .get(&symbol)
                        .copied()
                        .unwrap_or(ScreeningBias::Unknown);

                    let htf = higher_timeframes.get(&symbol).cloned().unwrap_or_default();
                    let symbol_for_state = symbol.clone();
                    let (best, regime, candles, chosen, best_seen, forced) = {
                        let mut states = states.lock().await;
                        let state = match states.get_mut(&symbol_for_state) {
                            Some(s) => s,
                            None => continue,
                        };
                        let prev_close = state.candles.back().map(|c| c.close);
                        state.on_closed(candle);

                        if let Some(ref qe) = quant_engine {
                            qe.update_kalman(&symbol, candle.close);
                            if let Some(prev) = prev_close {
                                if prev > 0.0 {
                                    let ret = (candle.close - prev) / prev;
                                    qe.record_return(&symbol, ret);
                                }
                            }
                        }

                        let regime = RegimeDetector::detect(state);
                        let chosen = select_strategies(&active, regime);

                        // Per-candle evaluation — debug only to avoid log spam
                        debug!(
                            symbol = %symbol,
                            regime = %regime.as_str(),
                            candles = state.candles.len(),
                            strategies = ?chosen,
                            screening_bias = %current_bias.as_str(),
                            ema200_ready = state.ema_200.value().is_some(),
                            "🔍 eval"
                        );

                        let mut best: Option<PreSignal> = None;
                        let mut best_seen: Option<(StrategyName, u8)> = None;
                        for &name in &chosen {
                            let strategy_name = name.as_str();
                            if !shared_state.is_strategy_enabled(strategy_name) {
                                info!(symbol = %symbol, strategy = %strategy_name, "⛔ strategy disabled by health");
                                continue;
                            }

                            let sig = match name {
                                StrategyName::EmaRibbon => OrderFlow.evaluate(state, &candle),
                                StrategyName::Momentum => TradeFlow.evaluate(state, &candle),
                                StrategyName::VwapScalp => {
                                    KalmanTrendStrategy.evaluate(state, &candle)
                                }
                                StrategyName::MeanReversion => {
                                    MicrostructureReversion.evaluate(state, &candle)
                                }
                                StrategyName::Squeeze => Squeeze.evaluate(state, &candle),
                                StrategyName::ScreenedVwapScalp => {
                                    ScreenedVwapScalpStrategy.evaluate(state, &candle)
                                }
                            };
                            if let Some(mut s) = sig {
                                if best_seen
                                    .as_ref()
                                    .map(|(_, confidence)| s.ta_confidence > *confidence)
                                    .unwrap_or(true)
                                {
                                    best_seen = Some((s.strategy, s.ta_confidence));
                                }
                                apply_mtf_context(
                                    &mut s,
                                    candle.close,
                                    state.ema_200.value(),
                                    &htf,
                                );
                                info!(
                                    symbol = %symbol,
                                    strategy = %s.strategy.as_str(),
                                    side = %s.side.as_str(),
                                    confidence = s.ta_confidence,
                                    reason = %s.reason,
                                    "📊 strategy fired"
                                );
                                if best
                                    .as_ref()
                                    .map(|b| s.ta_confidence > b.ta_confidence)
                                    .unwrap_or(true)
                                {
                                    best = Some(s);
                                }
                            }
                        }
                        let mut forced = false;
                        if best.is_none() && paper_scout_enabled {
                            best = paper_scout_signal(state, &candle, &htf);
                            if let Some(s) = &best {
                                best_seen = Some((s.strategy, s.ta_confidence));
                                forced = true;
                            }
                        }
                        let filtered = apply_advanced_alpha(
                            best,
                            state,
                            feeds_by_symbol.get(&symbol),
                            &advanced_alpha,
                        );
                        if let (Some(qe), Some(signal)) = (&quant_engine, filtered.as_ref()) {
                            if let Some(prev) = prev_close {
                                if prev > 0.0 && signal.entry > 0.0 {
                                    let direction = match signal.side {
                                        Side::Long => 1.0,
                                        Side::Short => -1.0,
                                    };
                                    let forward_return = direction * (candle.close - prev) / prev;
                                    let signal_value = direction
                                        * ((signal.entry - prev) / prev)
                                        * (signal.ta_confidence as f64 / 100.0);
                                    qe.record_ic_observation(
                                        signal.strategy.as_str(),
                                        signal_value,
                                        forward_return,
                                    );
                                }
                            }
                        }
                        (
                            filtered,
                            regime,
                            state.candles.len(),
                            chosen,
                            best_seen,
                            forced,
                        )
                    };

                    if let Some(mut signal) = best {
                        // Assign sequential signal ID for tracking across all events
                        signal.signal_id = shared_state.next_signal_id();
                        // P0-3: Hard gate — only emit PreSignalEmitted when bias allows the side.
                        if !current_bias.allows(&signal.side) {
                            info!(
                                symbol = %signal.symbol,
                                side = %signal.side.as_str(),
                                bias = %current_bias.as_str(),
                                confidence = signal.ta_confidence,
                                "🚫 signal blocked by 15m screening bias"
                            );
                            bus.publish(AgentEvent::SignalEvaluation(SignalEvaluationMsg {
                                symbol: signal.symbol.clone(),
                                timeframe_secs,
                                regime: Some(regime),
                                candles,
                                strategies: chosen,
                                reason: format!(
                                    "screening_bias_block: {} signal blocked by {} htf bias",
                                    signal.side.as_str(),
                                    current_bias.as_str()
                                ),
                                best_strategy: Some(signal.strategy),
                                best_confidence: Some(signal.ta_confidence),
                            }));
                            continue;
                        }

                        if forced {
                            info!(
                                symbol = %signal.symbol,
                                side = %signal.side.as_str(),
                                entry = signal.entry,
                                sl = signal.stop_loss,
                                tp = signal.take_profit,
                                confidence = signal.ta_confidence,
                                htf = %htf_summary(&htf),
                                bias = %current_bias.as_str(),
                                "paper scout htf-aware scalp signal"
                            );
                        }
                        bus.publish(AgentEvent::PreSignalEmitted {
                            signal: Box::new(signal),
                            regime,
                        });
                    } else {
                        let (best_strategy, best_confidence) = best_seen
                            .map(|(strategy, confidence)| (Some(strategy), Some(confidence)))
                            .unwrap_or((None, None));
                        bus.publish(AgentEvent::SignalEvaluation(SignalEvaluationMsg {
                            symbol,
                            timeframe_secs,
                            regime: Some(regime),
                            candles,
                            strategies: chosen,
                            reason: no_signal_reason(candles, best_strategy, best_confidence),
                            best_strategy,
                            best_confidence,
                        }));
                    }
                }
                AgentEvent::ControlCommand(crate::agents::messages::ControlCommand::ResetDaily) => {
                    let mut states = states.lock().await;
                    for state in states.values_mut() {
                        state.vwap.reset();
                        state.last_vwap = None;
                        state.last_vwap_slope = None;
                    }
                    tracing::info!("signal: VWAP reset for new session");
                }
                AgentEvent::OrderFilled { symbol, side, .. } => {
                    info!(symbol = %symbol, side = ?side, "signal: position opened — pausing screening for {}", symbol);
                    open_symbols.insert(symbol);
                }
                AgentEvent::PositionRecovered { symbol, .. } => {
                    open_symbols.insert(symbol);
                }
                AgentEvent::PositionClosed { symbol, .. } => {
                    info!(symbol = %symbol, "signal: position closed — resuming screening for {}", symbol);
                    open_symbols.remove(&symbol);
                }
                // A partial close does NOT resume screening — the position is still open.
                AgentEvent::PositionReduced { .. } => {}
                AgentEvent::Shutdown => break,
                _ => {}
            }
        }
    })
}

/// Compute the 15m screening bias from the HTF SymbolState.
///
/// Uses a 5-candle majority-vote slope over recent closes, requiring
/// at least 5 candles. Returns `Unknown` when there isn't enough data.
/// The vote logic: 3+ of 4 consecutive moves in the same direction →
/// Bullish or Bearish; otherwise NoTrade.
fn compute_htf_bias(state: &SymbolState) -> ScreeningBias {
    let candles = &state.candles;
    if candles.len() < 5 {
        return ScreeningBias::Unknown;
    }

    // Take the last 8 candles (7 close-to-close moves) for more signal
    let recent: Vec<f64> = candles.iter().rev().take(8).map(|c| c.close).collect();
    let up_moves = recent.windows(2).filter(|w| w[0] > w[1]).count();
    let dn_moves = recent.windows(2).filter(|w| w[0] < w[1]).count();
    let total = recent.len().saturating_sub(1).max(1);

    // Require 70% strong directional agreement for Bullish/Bearish
    // Below threshold → Unknown (allows both sides, strategies decide)
    // NoTrade is only used when explicitly ranging — here we use Unknown as default
    let bull_threshold = (total as f64 * 0.70).ceil() as usize;
    let bear_threshold = (total as f64 * 0.70).ceil() as usize;

    if up_moves >= bull_threshold {
        ScreeningBias::Bullish
    } else if dn_moves >= bear_threshold {
        ScreeningBias::Bearish
    } else {
        // Insufficient directional clarity → Unknown (not NoTrade)
        // Unknown allows both directions; let strategies decide based on regime
        ScreeningBias::Unknown
    }
}

/// Bootstrap HTF states for all symbols before the live event loop.
/// Fetches historical candles from Binance REST and feeds them into the states.
async fn bootstrap_htf_states(
    htf_states: &mut HashMap<String, SymbolState>,
    rest_base_url: &str,
    timeframe_secs: i64,
) {
    use crate::data::Timeframe;

    let tf = Timeframe {
        seconds: timeframe_secs,
    };
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "htf bootstrap: failed to build http client");
            return;
        }
    };

    for (symbol, state) in htf_states.iter_mut() {
        match crate::data::kline_bootstrap::fetch_klines(&client, rest_base_url, symbol, &tf, 220)
            .await
        {
            Ok(candles) => {
                let n = candles.len();
                for c in candles {
                    state.on_closed(c);
                }
                info!(symbol = %symbol, timeframe_secs, seeded = n, "htf bootstrap ok");
            }
            Err(e) => {
                warn!(symbol = %symbol, timeframe_secs, error = %e, "htf bootstrap failed — bias will be Unknown until live candles arrive");
            }
        }
    }
}

fn paper_scout_signal(
    state: &SymbolState,
    candle: &crate::data::Candle,
    higher_timeframes: &BTreeMap<i64, HigherTimeframeSnapshot>,
) -> Option<PreSignal> {
    if state.candles.len() < 3 {
        return None;
    }
    if candle.close <= 0.0 {
        return None;
    }

    let price = candle.close;
    let vwap = state.last_vwap.unwrap_or(price);
    let bias = higher_timeframe_bias(higher_timeframes);
    let side = if bias > 0.0 || (bias == 0.0 && price >= vwap) {
        Side::Long
    } else {
        Side::Short
    };

    let raw_atr = state.last_atr.unwrap_or(0.0);
    let atr_pct = if raw_atr > 0.0 { raw_atr / price } else { 0.0 };
    let stop_pct = if atr_pct > 0.001 && atr_pct < 0.008 {
        (0.6 * raw_atr / price).min(0.005)
    } else {
        0.003
    };

    let stop_distance = price * stop_pct;
    let take_distance = stop_distance * 2.0;

    let (stop_loss, take_profit) = match side {
        Side::Long => (price - stop_distance, price + take_distance),
        Side::Short => (price + stop_distance, price - take_distance),
    };

    Some(PreSignal {
        signal_id: String::new(), // assigned later when emitted
        symbol: state.symbol.clone(),
        strategy: StrategyName::VwapScalp,
        side,
        entry: price,
        stop_loss,
        take_profit,
        ta_confidence: 60,
        reason: format!(
            "paper_scout htf_bias={:.2} close={:.4} vwap={:.4} stop_pct={:.3}% atr_raw={:.4}",
            bias,
            price,
            vwap,
            stop_pct * 100.0,
            raw_atr
        ),
        atr: Some(raw_atr),
    })
}

#[derive(Debug, Clone, Copy)]
struct HigherTimeframeSnapshot {
    open: f64,
    close: f64,
}

impl HigherTimeframeSnapshot {
    fn from_candle(candle: crate::data::Candle) -> Self {
        Self {
            open: candle.open,
            close: candle.close,
        }
    }
}

fn higher_timeframe_bias(higher_timeframes: &BTreeMap<i64, HigherTimeframeSnapshot>) -> f64 {
    let total_weight: f64 = higher_timeframes.keys().map(|tf| *tf as f64).sum();
    if total_weight <= 0.0 {
        return 0.0;
    }
    let weighted = higher_timeframes
        .iter()
        .map(|(tf, snapshot)| {
            let direction = if snapshot.close > snapshot.open {
                1.0
            } else if snapshot.close < snapshot.open {
                -1.0
            } else {
                0.0
            };
            direction * *tf as f64
        })
        .sum::<f64>();
    (weighted / total_weight).clamp(-1.0, 1.0)
}

fn apply_mtf_context(
    signal: &mut PreSignal,
    price: f64,
    ema200: Option<f64>,
    higher_timeframes: &BTreeMap<i64, HigherTimeframeSnapshot>,
) {
    let mut score = 0_i16;
    if let Some(ema200) = ema200 {
        let aligned = match signal.side {
            Side::Long => price > ema200,
            Side::Short => price < ema200,
        };
        score += if aligned { 1 } else { -2 };
    }
    let htf_bias = higher_timeframe_bias(higher_timeframes);
    if htf_bias.abs() > f64::EPSILON {
        let aligned = match signal.side {
            Side::Long => htf_bias > 0.0,
            Side::Short => htf_bias < 0.0,
        };
        score += if aligned { 2 } else { -3 };
    }
    if score < 0 {
        // The 15m screening bias is already a hard side gate above.  Keep this
        // MTF adjustment as a nudge, not a second hard block that drags otherwise
        // valid scalps below the risk TA floor for several candles.
        signal.ta_confidence = signal.ta_confidence.saturating_sub((-score * 2) as u8);
        signal.reason = format!(
            "{} | MTF-contradict(score={}, htf={})",
            signal.reason,
            score,
            htf_summary(higher_timeframes)
        );
    } else if score > 0 {
        signal.ta_confidence = signal
            .ta_confidence
            .saturating_add((score * 2) as u8)
            .min(100);
        signal.reason = format!(
            "{} | MTF-confirm(score={}, htf={})",
            signal.reason,
            score,
            htf_summary(higher_timeframes)
        );
    }
}

fn htf_summary(higher_timeframes: &BTreeMap<i64, HigherTimeframeSnapshot>) -> String {
    if higher_timeframes.is_empty() {
        return "none".into();
    }
    higher_timeframes
        .iter()
        .map(|(tf, snap)| {
            let dir = if snap.close > snap.open {
                "↑"
            } else if snap.close < snap.open {
                "↓"
            } else {
                "→"
            };
            format!("{}s:{}", tf, dir)
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn no_signal_reason(
    candles: usize,
    best_strategy: Option<StrategyName>,
    best_confidence: Option<u8>,
) -> String {
    if candles < 20 {
        return format!("warming_up({candles}/20)");
    }
    match (best_strategy, best_confidence) {
        (Some(s), Some(c)) => format!("below_threshold({} conf={})", s.as_str(), c),
        (Some(s), None) => format!("no_signal({})", s.as_str()),
        _ => "no_candidate".into(),
    }
}

fn is_valid_closed_candle(candle: &crate::data::Candle) -> bool {
    let prices = [candle.open, candle.high, candle.low, candle.close];
    if prices.iter().any(|p| !p.is_finite() || *p <= 0.0) {
        return false;
    }

    candle.high >= candle.low
        && candle.open >= candle.low
        && candle.open <= candle.high
        && candle.close >= candle.low
        && candle.close <= candle.high
}

fn in_dead_zone(schedule: &Schedule) -> bool {
    let start = schedule.dead_zone_start_hour_wib;
    let end = schedule.dead_zone_end_hour_wib;
    if start == end {
        return false; // disabled
    }
    // WIB = UTC+7
    let hour_wib = ((Utc::now().hour() as i32 + 7) % 24) as u8;
    if start < end {
        hour_wib >= start && hour_wib < end
    } else {
        // Wraps midnight: e.g. start=22, end=6
        hour_wib >= start || hour_wib < end
    }
}

struct TimedExternalSnapshot {
    snapshot: ExternalSnapshot,
    ts: DateTime<Utc>,
}

fn apply_advanced_alpha(
    signal: Option<PreSignal>,
    state: &SymbolState,
    feeds: Option<&TimedExternalSnapshot>,
    cfg: &AdvancedAlphaCfg,
) -> Option<PreSignal> {
    if !cfg.enabled {
        return signal;
    }
    let mut sig = signal?;

    let (alt_data, funding_rate) = if let Some(f) = feeds {
        let age_secs = (Utc::now() - f.ts).num_seconds();
        if age_secs <= cfg.feed_max_age_secs as i64 {
            (
                alt_data_inputs_from_snapshot(&f.snapshot),
                funding_rate_from_snapshot(&f.snapshot),
            )
        } else {
            (AltDataInputs::default(), 0.0)
        }
    } else {
        (AltDataInputs::default(), 0.0)
    };

    let prices: Vec<f64> = state.candles.iter().map(|c| c.close).collect();
    let trend_score = if prices.len() >= 2 {
        kalman_trend_score(
            &prices,
            cfg.kalman_process_noise,
            cfg.kalman_measurement_noise,
        )
    } else {
        0.0
    };

    let alpha_inputs = AdvancedAlphaInputs {
        alt_data,
        funding_rate,
        trend_score,
        min_abs_score: cfg.min_abs_score,
    };
    let signal_is_long = matches!(sig.side, Side::Long);
    match advanced_alpha_gate(alpha_inputs, signal_is_long) {
        AlphaGateDecision::Allow => Some(sig),
        AlphaGateDecision::Reduce => {
            sig.ta_confidence = sig
                .ta_confidence
                .saturating_sub(cfg.reduce_confidence_delta);
            sig.reason = format!(
                "{} | alpha_caution(-{})",
                sig.reason, cfg.reduce_confidence_delta
            );
            Some(sig)
        }
        AlphaGateDecision::Block => {
            debug!(symbol = %sig.symbol, "advanced alpha gate blocked signal");
            None
        }
    }
}
