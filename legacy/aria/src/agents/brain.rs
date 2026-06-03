//! Brain agent — owns the existing LLM specialist. Listens for allowed
//! `RiskVerdict` events, builds a `MarketContext` (with the historical
//! summary injected), calls the LLM, and emits `BrainOutcomeReady`.

use crate::agents::MessageBus;
use crate::agents::messages::{AgentEvent, AgentId, BrainOutcome, FeedsSnapshotMsg, RiskOutcome};
use crate::feeds::ExternalSnapshot;
use crate::learning::LearningPolicy;
use crate::llm::ContextBuilder;
use crate::llm::engine::{Decision, LlmEngine};
use crate::strategy::state::SymbolState;
use parking_lot::RwLock as PlRwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

/// Minimum seconds between LLM calls for the same symbol.
/// Prevents redundant API calls when multiple signals fire in quick succession.
const LLM_COOLDOWN_SECS: u64 = 12;

pub fn spawn(
    bus: MessageBus,
    llm: Arc<LlmEngine>,
    states: Arc<Mutex<HashMap<String, SymbolState>>>,
    policy: LearningPolicy,
    feeds_cache: Arc<PlRwLock<HashMap<String, ExternalSnapshot>>>,
    shared_state: Option<Arc<crate::shared_state::SharedState>>,
    fail_closed_without_llm: bool,
    min_confidence: u8,
) -> JoinHandle<()> {
    let mut rx = bus.subscribe();
    // Track last LLM call time per symbol for deduplication
    let last_llm_call: Arc<PlRwLock<HashMap<String, Instant>>> =
        Arc::new(PlRwLock::new(HashMap::new()));

    tokio::spawn(async move {
        info!("brain agent starting");
        crate::agents::heartbeat::spawn(bus.clone(), AgentId::Brain);
        loop {
            let ev = match rx.recv().await {
                Ok(ev) => ev,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    warn!(skipped = n, "brain: broadcast lagged — skipping events");
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            };
            match ev {
                AgentEvent::FeedsSnapshot(FeedsSnapshotMsg {
                    symbol, snapshot, ..
                }) => {
                    feeds_cache.write().insert(symbol, snapshot);
                }
                AgentEvent::RiskVerdict(risk) => {
                    if risk.outcome != RiskOutcome::Allowed {
                        continue;
                    }
                    let signal = (*risk.signal).clone();
                    let regime = risk.regime;
                    let symbol = signal.symbol.clone();

                    // Deduplication: skip if same symbol analyzed recently
                    {
                        let mut cache = last_llm_call.write();
                        if let Some(last) = cache.get(&symbol) {
                            if last.elapsed().as_secs() < LLM_COOLDOWN_SECS {
                                debug!(
                                    symbol = %symbol,
                                    elapsed_ms = last.elapsed().as_millis() as u64,
                                    cooldown_ms = LLM_COOLDOWN_SECS * 1000,
                                    "brain: LLM cooldown active — skipping"
                                );
                                release_risk_reservation(
                                    &bus,
                                    &symbol,
                                    "brain LLM cooldown active",
                                );
                                continue;
                            }
                        }
                        cache.insert(symbol.clone(), Instant::now());
                    }

                    let external = feeds_cache.read().get(&symbol).cloned().unwrap_or_default();

                    let mut ctx = {
                        let states = states.lock().await;
                        match states.get(&symbol) {
                            Some(s) => ContextBuilder::build(s, regime, &signal, external),
                            None => {
                                release_risk_reservation(
                                    &bus,
                                    &symbol,
                                    "brain missing symbol state",
                                );
                                continue;
                            }
                        }
                    };
                    ctx.historical_summary = policy.historical_summary(
                        signal.strategy.as_str(),
                        regime.as_str(),
                        &symbol,
                    );

                    // Populate strategy performance data from SharedState
                    if let Some(ref ss) = shared_state {
                        let strategy_perf = ss.get_strategy_health(signal.strategy.as_str());
                        let overall_perf = ss.get_overall_stats();

                        ctx.strategy_win_rate = strategy_perf.win_rate;
                        ctx.strategy_total_trades = strategy_perf.total_trades;
                        ctx.strategy_recent_pnl = strategy_perf.total_pnl;
                        ctx.strategy_loss_streak = strategy_perf.loss_streak;
                        ctx.overall_win_rate = overall_perf.win_rate;
                        ctx.overall_total_trades = overall_perf.total_trades;
                        ctx.recent_trade_pnl = overall_perf.last_trade_pnl;
                    }

                    info!(
                        symbol = %symbol,
                        side = %signal.side.as_str(),
                        strategy = %signal.strategy.as_str(),
                        regime = %regime.as_str(),
                        ta_confidence = signal.ta_confidence,
                        entry = signal.entry,
                        sl = signal.stop_loss,
                        tp = signal.take_profit,
                        "brain: analyzing risk-approved setup"
                    );

                    let llm_out = match llm.analyze(&ctx).await {
                        Ok(o) => o,
                        Err(e) => {
                            warn!(error = %e, fail_closed = fail_closed_without_llm, "brain agent: LLM call failed");
                            release_risk_reservation(
                                &bus,
                                &symbol,
                                format!("brain LLM call failed: {e}"),
                            );
                            continue;
                        }
                    };

                    let mut adjusted_risk = risk.clone();
                    let mut adjusted_signal = signal.clone();
                    let mut final_decision = llm_out.decision.clone();

                    info!(
                        symbol = %symbol,
                        decision = ?llm_out.decision.decision,
                        confidence = llm_out.decision.confidence,
                        offline_fallback = llm_out.offline_fallback,
                        reason = %llm_out.decision.reasoning.summary,
                        "brain: decision"
                    );

                    // BLOCK TA-only fallback if fail_closed_without_llm is set.
                    if llm_out.offline_fallback && fail_closed_without_llm {
                        warn!(symbol = %symbol, "brain: BLOCKED — LLM unavailable, fail_closed=true");
                        publish_rejected_brain(
                            &bus,
                            signal.clone(),
                            regime,
                            adjusted_risk.clone(),
                            final_decision.clone(),
                            llm_out.latency_ms,
                            llm_out.offline_fallback,
                            "LLM unavailable with fail_closed=true",
                        );
                        release_risk_reservation(
                            &bus,
                            &symbol,
                            "brain blocked: LLM unavailable with fail_closed=true",
                        );
                        continue;
                    }

                    // Convert soft LLM NoGo/Wait into a tiny exploratory trade when
                    // the deterministic TA+risk gates already approved the setup. This
                    // prevents the LLM from turning the scalper into a tuning/status bot
                    // because of WR, VPIN caution, or mixed regime narrative. Truly
                    // unsafe responses (parse failure, invalid geometry, or very low
                    // confidence) still release the reservation and do not trade.
                    if final_decision.decision != Decision::Go {
                        if should_override_soft_brain_reject(&final_decision, &signal) {
                            // Size stays unchanged — risk agent already set the correct size
                            final_decision.decision = Decision::Go;
                            final_decision.confidence = final_decision.confidence.max(45);
                            final_decision.position_size_pct =
                                final_decision.position_size_pct.max(0.25);
                            final_decision.reasoning.summary = format!(
                                "SOFT_OVERRIDE: {}; deterministic risk gate approved",
                                final_decision.reasoning.summary
                            );
                            info!(
                                symbol = %symbol,
                                decision = ?llm_out.decision.decision,
                                adjusted_size = adjusted_risk.size,
                                "brain: soft LLM reject overridden to keep scalper trading"
                            );
                        } else {
                            bus.publish(AgentEvent::BrainOutcomeReady(BrainOutcome {
                                signal: Box::new(signal),
                                regime,
                                risk: adjusted_risk,
                                decision: final_decision.clone(),
                                latency_ms: llm_out.latency_ms,
                                offline_fallback: llm_out.offline_fallback,
                            }));
                            info!(
                                symbol = %symbol,
                                decision = ?final_decision.decision,
                                "brain: REJECTED — hard unsafe reject"
                            );
                            release_risk_reservation(
                                &bus,
                                &symbol,
                                format!("brain rejected: {:?}", final_decision.decision),
                            );
                            continue;
                        }
                    }

                    // Use LLM-adjusted SL/TP — brain sets exact levels
                    let final_sl = final_decision.sl_adjustment.unwrap_or(signal.stop_loss);
                    let final_tp = final_decision.tp_adjustment.unwrap_or(signal.take_profit);
                    let final_entry = final_decision.entry_price.unwrap_or(signal.entry);

                    // Validate LLM-adjusted SL/TP geometry — reject if wrong side of entry
                    let geometry_ok = match signal.side {
                        crate::data::Side::Long => final_sl < final_entry && final_tp > final_entry,
                        crate::data::Side::Short => {
                            final_sl > final_entry && final_tp < final_entry
                        }
                    };
                    if !geometry_ok {
                        info!(
                            symbol = %symbol,
                            sl = final_sl, tp = final_tp, entry = final_entry,
                            side = %signal.side.as_str(),
                            "brain: REJECTED — LLM-adjusted SL/TP geometry invalid"
                        );
                        publish_rejected_brain(
                            &bus,
                            signal.clone(),
                            regime,
                            adjusted_risk.clone(),
                            final_decision.clone(),
                            llm_out.latency_ms,
                            llm_out.offline_fallback,
                            "invalid LLM SL/TP geometry",
                        );
                        release_risk_reservation(
                            &bus,
                            &symbol,
                            "brain rejected: invalid LLM SL/TP geometry",
                        );
                        continue;
                    }

                    // Validate minimum R:R after LLM adjustment
                    let risk_dist = (final_entry - final_sl).abs();
                    let reward_dist = (final_tp - final_entry).abs();
                    let rr = if risk_dist > 0.0 {
                        reward_dist / risk_dist
                    } else {
                        0.0
                    };
                    if rr < 0.8 {
                        info!(
                            symbol = %symbol,
                            rr = %format!("{:.2}", rr),
                            "brain: REJECTED — LLM-adjusted SL/TP gives R:R < 0.8"
                        );
                        publish_rejected_brain(
                            &bus,
                            signal.clone(),
                            regime,
                            adjusted_risk.clone(),
                            final_decision.clone(),
                            llm_out.latency_ms,
                            llm_out.offline_fallback,
                            format!("R:R {rr:.2} < 0.8"),
                        );
                        release_risk_reservation(
                            &bus,
                            &symbol,
                            format!("brain rejected: R:R {rr:.2} < 0.8"),
                        );
                        continue;
                    }

                    // Regime conflict is noted for logging only — size unchanged.
                    // Risk agent is the sole authority on position sizing.
                    {
                        let states_r = states.lock().await;
                        if let Some(st) = states_r.get(&symbol) {
                            let detected_regime = crate::strategy::RegimeDetector::detect(st);
                            let is_long = matches!(signal.side, crate::data::Side::Long);
                            let regime_str = detected_regime.as_str().to_lowercase();
                            let bearish = regime_str.contains("bear");
                            let bullish = regime_str.contains("bull");
                            if (is_long && bearish) || (!is_long && bullish) {
                                adjusted_risk
                                    .matched_lessons
                                    .push("brain regime conflict noted (size unchanged)".into());
                                info!(
                                    symbol = %symbol,
                                    regime = %detected_regime.as_str(),
                                    side = ?signal.side,
                                    "brain: regime conflict noted — size controlled by risk agent"
                                );
                            }
                        }
                    }

                    // Persist LLM-adjusted brackets into the downstream signal.
                    // Previously the brain validated final_entry/final_sl/final_tp but
                    // manager/execution still received the original strategy brackets,
                    // so live risk/reward could differ from the approved setup.
                    let _original_risk_dist = (signal.entry - signal.stop_loss).abs();
                    adjusted_signal.entry = final_entry;
                    adjusted_signal.stop_loss = final_sl;
                    adjusted_signal.take_profit = final_tp;
                    adjusted_risk.signal = Box::new(adjusted_signal.clone());

                    // LLM stop adjustment noted for logging — size unchanged.
                    // Risk agent is the sole authority on position sizing.

                    info!(
                        symbol = %symbol,
                        risk_size = risk.size,
                        adjusted_size = adjusted_risk.size,
                        final_entry,
                        final_sl,
                        final_tp,
                        rr = %format!("{:.2}", rr),
                        "brain: final executable setup"
                    );

                    // Low confidence noted — size unchanged, risk agent controls sizing.
                    let live_conf_floor = min_confidence;
                    if final_decision.confidence < live_conf_floor {
                        adjusted_risk.matched_lessons.push(format!(
                            "brain confidence {} < {} (size unchanged)",
                            final_decision.confidence, live_conf_floor
                        ));
                    }

                    bus.publish(AgentEvent::BrainOutcomeReady(BrainOutcome {
                        signal: Box::new(adjusted_signal),
                        regime,
                        risk: adjusted_risk,
                        decision: final_decision,
                        latency_ms: llm_out.latency_ms,
                        offline_fallback: llm_out.offline_fallback,
                    }));
                }
                AgentEvent::Shutdown => break,
                _ => {}
            }
        }
    })
}

fn should_override_soft_brain_reject(
    decision: &crate::llm::engine::TradeDecision,
    signal: &crate::strategy::state::PreSignal,
) -> bool {
    if decision.confidence < 35 || signal.ta_confidence < 60 || signal.rr() < 0.8 {
        return false;
    }
    let text = format!(
        "{} {} {}",
        decision.reasoning.summary,
        decision.reasoning.risk_factors,
        decision.reasoning.invalidation
    )
    .to_ascii_lowercase();
    let hard_reject_terms = [
        "parse failure",
        "malformed",
        "invalid geometry",
        "wrong side",
        "circuit",
        "frozen",
        "daily loss",
        "drawdown",
        "no liquidity",
    ];
    !hard_reject_terms.iter().any(|term| text.contains(term))
}

fn publish_rejected_brain(
    bus: &MessageBus,
    signal: crate::strategy::state::PreSignal,
    regime: crate::strategy::Regime,
    risk: crate::agents::messages::RiskVerdictMsg,
    mut decision: crate::llm::engine::TradeDecision,
    latency_ms: u64,
    offline_fallback: bool,
    reason: impl Into<String>,
) {
    let reason = reason.into();
    decision.decision = Decision::NoGo;
    decision.reasoning.summary = format!("Rejected after LLM GO: {reason}");
    decision.reasoning.risk_factors = reason;
    bus.publish(AgentEvent::BrainOutcomeReady(BrainOutcome {
        signal: Box::new(signal),
        regime,
        risk,
        decision,
        latency_ms,
        offline_fallback,
    }));
}

fn release_risk_reservation(bus: &MessageBus, symbol: &str, reason: impl Into<String>) {
    bus.publish(AgentEvent::RiskReservationReleased {
        symbol: symbol.to_string(),
        reason: reason.into(),
    });
}
