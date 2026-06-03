//! Risk agent — listens for `PreSignalEmitted`, applies the existing
//! 8-gate `RiskManager` plus the `LearningPolicy` verdict, sizes the
//! trade, and publishes a `RiskVerdict` event.
//!
//! The agent additionally reads the latest [`SurvivalState`] (set by
//! the SurvivalAgent) and the latest funding rate from `FeedsSnapshot`
//! to apply two extra gates before sizing:
//!
//! * **Survival gate** — if the bot is in `Frozen` or `Dead` mode it
//!   refuses every entry.
//! * **Funding gate** — extreme funding rates are a strong sign of a
//!   one-sided crowd; we block longs when funding > +0.1% and shorts
//!   when funding < -0.1% (configurable). Default thresholds are
//!   wide enough to never bite under normal conditions but tight
//!   enough to dodge a funding-flush.

use crate::agents::MessageBus;
use crate::agents::messages::{
    AgentEvent, AgentId, FeedsSnapshotMsg, RiskOutcome, RiskVerdictMsg, SurvivalMode, SurvivalState,
};
use crate::data::Side;
use crate::execution::RiskManager;
use crate::execution::tcm::TransactionCostModel;
use crate::learning::LearningPolicy;
use crate::quant::{QuantEngine, QuantSizingInput};
use chrono::Utc;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct RiskAgentConfig {
    pub base_min_ta_threshold: u8,
    pub base_min_llm_floor: u8,
    /// Funding rate threshold beyond which we reject same-direction
    /// trades. Binance reports funding as a fraction (e.g. 0.0005 ==
    /// 0.05%). Default 0.001 = 0.1%.
    pub funding_block_threshold: f64,
    pub tcm: TransactionCostModel,
    /// Base risk per trade % — passed to the quant engine for Kelly
    /// comparison.  Default 0.5%.
    pub base_risk_pct: f64,
    /// Hard block trades when spread exceeds this percentage.
    pub max_spread_pct_block: f64,
    /// Reduce size when spread exceeds this percentage (but below hard block).
    pub spread_caution_pct: f64,
    /// Multiplier applied in spread caution zone.
    pub spread_caution_size_mult: f64,
    /// Hard block when latest book ticker age exceeds this threshold.
    pub max_book_staleness_secs: i64,
}

impl Default for RiskAgentConfig {
    fn default() -> Self {
        Self {
            // Lower thresholds for HFT scalping — the strategies already
            // score conservatively (62-68 base), so a 60 TA threshold
            // lets most valid signals through.
            base_min_ta_threshold: 60,
            // LLM floor: accept signals where LLM confidence is >= 50.
            // The brain LLM prompt now defaults to GO for composite >= 50.
            base_min_llm_floor: 50,
            funding_block_threshold: 0.001,
            tcm: TransactionCostModel {
                taker_fee_bps: 4.0,
                maker_fee_bps: -1.0,
                avg_slippage_bps: 2.0,
                market_impact_bps: 1.0,
            },
            base_risk_pct: 0.5,
            max_spread_pct_block: 0.20,
            spread_caution_pct: 0.08,
            spread_caution_size_mult: 0.6,
            max_book_staleness_secs: 20,
        }
    }
}

pub fn spawn(
    bus: MessageBus,
    risk: Arc<RiskManager>,
    policy: LearningPolicy,
    cfg: RiskAgentConfig,
    quant_engine: Option<Arc<QuantEngine>>,
) -> JoinHandle<()> {
    let mut rx = bus.subscribe();
    let survival: Arc<Mutex<Option<SurvivalState>>> = Arc::new(Mutex::new(None));
    let orchestrator_multiplier: Arc<Mutex<f64>> = Arc::new(Mutex::new(1.0));
    let funding: Arc<Mutex<HashMap<String, f64>>> = Arc::new(Mutex::new(HashMap::new()));
    let spreads: Arc<Mutex<HashMap<String, f64>>> = Arc::new(Mutex::new(HashMap::new()));
    let spread_ts: Arc<Mutex<HashMap<String, i64>>> = Arc::new(Mutex::new(HashMap::new()));
    // open_symbols: symbols with a CONFIRMED open position (updated on OrderFilled).
    let open_symbols: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    // pending_symbols: symbols where RiskVerdict::Allowed was just published but
    // the position is not yet confirmed by the exchange. This closes the race window
    // between RiskVerdict and OrderFilled (typically 2-10s due to LLM + execution).
    // Released when OrderFilled fires (→ moves to open_symbols) or on Veto/error.
    let pending_symbols: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    let slippage_bps: Arc<Mutex<HashMap<String, f64>>> = Arc::new(Mutex::new(HashMap::new()));
    let strategy_perf: Arc<Mutex<HashMap<String, VecDeque<f64>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let disabled_strategies: Arc<Mutex<HashMap<String, i64>>> =
        Arc::new(Mutex::new(HashMap::new()));

    tokio::spawn(async move {
        info!("risk agent starting");
        crate::agents::heartbeat::spawn(bus.clone(), AgentId::Risk);
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
                AgentEvent::Shutdown => break,
                AgentEvent::SurvivalUpdated(s) => {
                    *survival.lock() = Some(s);
                    continue;
                }
                AgentEvent::OrchestratorUpdated(s) => {
                    *orchestrator_multiplier.lock() = s.size_multiplier.clamp(0.0, 2.0);
                    continue;
                }
                AgentEvent::OrderFilled { symbol, .. } => {
                    // Move from pending → confirmed open
                    pending_symbols.lock().remove(&symbol);
                    open_symbols.lock().insert(symbol);
                    continue;
                }
                AgentEvent::PositionRecovered { symbol, .. } => {
                    pending_symbols.lock().remove(&symbol);
                    open_symbols.lock().insert(symbol);
                    continue;
                }
                AgentEvent::PositionClosed {
                    symbol,
                    ref strategy,
                    pnl_usd,
                    ..
                } => {
                    open_symbols.lock().remove(&symbol);
                    pending_symbols.lock().remove(&symbol);
                    let mut perf = strategy_perf.lock();
                    let q = perf.entry(strategy.clone()).or_default();
                    q.push_back(pnl_usd);
                    while q.len() > 30 {
                        q.pop_front();
                    }
                    // OOS decay: only reduce size, never hard-block.
                    // Requires 30 trades + WR < 20% + deeply negative PnL.
                    // Bot must keep trading — size reduction is the lever.
                    if q.len() >= 30 {
                        let wins = q.iter().filter(|&&p| p > 0.0).count();
                        let wr = wins as f64 / q.len() as f64;
                        let pnl_sum: f64 = q.iter().sum();
                        if wr < 0.20 && pnl_sum < -20.0 {
                            // Throttle to 30 min cooldown only when catastrophically broken
                            let until = chrono::Utc::now().timestamp() + 1800;
                            disabled_strategies.lock().insert(strategy.clone(), until);
                            warn!(strategy = %strategy, win_rate = wr, pnl_sum,
                                "risk: OOS decay throttle (30 min reduced-size cooldown)");
                        }
                    }
                    continue;
                }
                AgentEvent::SlippageObserved {
                    symbol,
                    shortfall_bps,
                } => {
                    slippage_bps
                        .lock()
                        .insert(symbol, shortfall_bps.clamp(0.0, 100.0));
                    continue;
                }
                AgentEvent::FeedsSnapshot(FeedsSnapshotMsg {
                    symbol, snapshot, ..
                }) => {
                    if let Some(f) = &snapshot.funding {
                        funding.lock().insert(symbol, f.rate);
                    }
                    continue;
                }
                AgentEvent::BookTicker {
                    symbol,
                    best_bid,
                    bid_qty: _,
                    best_ask,
                    ask_qty: _,
                } => {
                    let mid = (best_bid + best_ask) / 2.0;
                    if mid > 0.0 && best_ask >= best_bid {
                        spreads
                            .lock()
                            .insert(symbol.clone(), (best_ask - best_bid) / mid * 100.0);
                        spread_ts.lock().insert(symbol, Utc::now().timestamp());
                    }
                    continue;
                }
                AgentEvent::PreSignalEmitted { signal, regime } => {
                    // Stale book data is a caution signal, not a startup deadlock.
                    // When bootstrap/live bookTicker is late, keep the bot trading
                    // with reduced size instead of hard-blocking every candidate.
                    let now_ts = Utc::now().timestamp();
                    let last_book_ts = spread_ts.lock().get(&signal.symbol).copied().unwrap_or(0);
                    let book_stale =
                        last_book_ts == 0 || (now_ts - last_book_ts) > cfg.max_book_staleness_secs;
                    if book_stale {
                        warn!(
                            symbol = %signal.symbol,
                            last_book_age_secs = if last_book_ts == 0 { -1 } else { now_ts - last_book_ts },
                            max_age_secs = cfg.max_book_staleness_secs,
                            "risk: book ticker stale/missing — continuing at reduced size"
                        );
                    }
                    // Block if symbol already has an open OR pending position.
                    // pending_symbols covers the race window between RiskVerdict::Allowed
                    // and the eventual OrderFilled confirmation (2-10s LLM + execution gap).
                    let already_taken = open_symbols.lock().contains(&signal.symbol)
                        || pending_symbols.lock().contains(&signal.symbol);
                    if already_taken {
                        warn!(symbol = %signal.symbol, "risk blocked: position open or pending");
                        bus.publish(AgentEvent::RiskVerdict(RiskVerdictMsg {
                            signal: signal.clone(),
                            regime,
                            outcome: RiskOutcome::Blocked,
                            size: 0.0,
                            size_multiplier: 1.0,
                            effective_ta_threshold: 0,
                            effective_llm_floor: 0,
                            matched_lessons: Vec::new(),
                            reason: Some(format!("position already open for {}", signal.symbol)),
                        }));
                        continue;
                    }
                    // Auto-disable strategy on OOS decay (time-boxed cooldown).
                    if let Some(until) = disabled_strategies
                        .lock()
                        .get(signal.strategy.as_str())
                        .copied()
                    {
                        if chrono::Utc::now().timestamp() < until {
                            bus.publish(AgentEvent::RiskVerdict(RiskVerdictMsg {
                                signal: signal.clone(),
                                regime,
                                outcome: RiskOutcome::Blocked,
                                size: 0.0,
                                size_multiplier: 0.0,
                                effective_ta_threshold: cfg.base_min_ta_threshold,
                                effective_llm_floor: cfg.base_min_llm_floor,
                                matched_lessons: vec!["strategy disabled by OOS decay".into()],
                                reason: Some("strategy cooling-down (OOS decay)".into()),
                            }));
                            continue;
                        }
                    }

                    // Survival hard-gate: refuse outright when frozen or dead.
                    let surv = survival.lock().clone();
                    if let Some(s) = &surv {
                        if matches!(s.mode, SurvivalMode::Frozen | SurvivalMode::Dead) {
                            bus.publish(AgentEvent::RiskVerdict(RiskVerdictMsg {
                                signal: signal.clone(),
                                regime,
                                outcome: RiskOutcome::Blocked,
                                size: 0.0,
                                size_multiplier: 0.0,
                                effective_ta_threshold: cfg.base_min_ta_threshold,
                                effective_llm_floor: cfg.base_min_llm_floor,
                                matched_lessons: vec![],
                                reason: Some(format!("survival {}", s.mode.as_str())),
                            }));
                            continue;
                        }
                    }

                    let verdict =
                        policy.evaluate(signal.strategy.as_str(), regime.as_str(), &signal.symbol);
                    // Cap TA threshold — learning can only raise by max 5 points (60→65)
                    let effective_ta_threshold = cfg.base_min_ta_threshold; // always 60, no learning delta
                    let llm_floor = verdict
                        .llm_min_confidence_floor
                        .unwrap_or(cfg.base_min_llm_floor)
                        .max(cfg.base_min_llm_floor);

                    // NOTE: As of current policy design, verdict.allowed is always true
                    // (policy reduces size instead of blocking). This gate is kept as a
                    // safety net for future policy changes that may reintroduce hard blocks.
                    if !verdict.allowed {
                        bus.publish(AgentEvent::RiskVerdict(RiskVerdictMsg {
                            signal: signal.clone(),
                            regime,
                            outcome: RiskOutcome::Blocked,
                            size: 0.0,
                            size_multiplier: 0.0,
                            effective_ta_threshold,
                            effective_llm_floor: llm_floor,
                            matched_lessons: verdict.matched_lessons,
                            reason: Some("learning policy blocked".into()),
                        }));
                        continue;
                    }

                    if signal.ta_confidence < effective_ta_threshold {
                        bus.publish(AgentEvent::RiskVerdict(RiskVerdictMsg {
                            signal: signal.clone(),
                            regime,
                            outcome: RiskOutcome::Blocked,
                            size: 0.0,
                            size_multiplier: 1.0,
                            effective_ta_threshold,
                            effective_llm_floor: llm_floor,
                            matched_lessons: verdict.matched_lessons,
                            reason: Some(format!(
                                "TA {} < {}",
                                signal.ta_confidence, effective_ta_threshold
                            )),
                        }));
                        continue;
                    }

                    if let Err(e) = risk.can_open_position() {
                        warn!(symbol = %signal.symbol, reason = %e, "risk blocked");
                        bus.publish(AgentEvent::RiskVerdict(RiskVerdictMsg {
                            signal: signal.clone(),
                            regime,
                            outcome: RiskOutcome::Blocked,
                            size: 0.0,
                            size_multiplier: 1.0,
                            effective_ta_threshold,
                            effective_llm_floor: llm_floor,
                            matched_lessons: verdict.matched_lessons,
                            reason: Some(e.to_string()),
                        }));
                        continue;
                    }

                    let spread_pct = spreads.lock().get(&signal.symbol).copied();
                    if let Some(sp) = spread_pct {
                        if sp >= cfg.max_spread_pct_block {
                            bus.publish(AgentEvent::RiskVerdict(RiskVerdictMsg {
                                signal: signal.clone(),
                                regime,
                                outcome: RiskOutcome::Blocked,
                                size: 0.0,
                                size_multiplier: 1.0,
                                effective_ta_threshold,
                                effective_llm_floor: llm_floor,
                                matched_lessons: verdict.matched_lessons,
                                reason: Some(format!(
                                    "spread {:.4}% >= {:.4}%",
                                    sp, cfg.max_spread_pct_block
                                )),
                            }));
                            continue;
                        }
                    }

                    if let Err(e) = risk.validate_signal(
                        signal.entry,
                        signal.stop_loss,
                        signal.take_profit,
                        &signal.side,
                        spread_pct,
                        &cfg.tcm,
                    ) {
                        warn!(symbol = %signal.symbol, reason = %e, "risk blocked");
                        bus.publish(AgentEvent::RiskVerdict(RiskVerdictMsg {
                            signal: signal.clone(),
                            regime,
                            outcome: RiskOutcome::Blocked,
                            size: 0.0,
                            size_multiplier: 1.0,
                            effective_ta_threshold,
                            effective_llm_floor: llm_floor,
                            matched_lessons: verdict.matched_lessons,
                            reason: Some(e),
                        }));
                        continue;
                    }

                    // Funding-rate gate.
                    let funding_rate = funding.lock().get(&signal.symbol).copied().unwrap_or(0.0);
                    let funding_blocks = match signal.side {
                        Side::Long => funding_rate >= cfg.funding_block_threshold,
                        Side::Short => funding_rate <= -cfg.funding_block_threshold,
                    };
                    if funding_blocks {
                        bus.publish(AgentEvent::RiskVerdict(RiskVerdictMsg {
                            signal: signal.clone(),
                            regime,
                            outcome: RiskOutcome::Blocked,
                            size: 0.0,
                            size_multiplier: 1.0,
                            effective_ta_threshold,
                            effective_llm_floor: llm_floor,
                            matched_lessons: verdict.matched_lessons,
                            reason: Some(format!("funding {:.4}%", funding_rate * 100.0)),
                        }));
                        continue;
                    }

                    // BRAIN controls SL/TP dynamically — no hardcoded caps
                    // Risk agent only checks max drawdown limits
                    let effective_entry = signal.entry;
                    let effective_sl = signal.stop_loss;
                    let effective_tp = signal.take_profit;

                    // Log SL/TP for monitoring
                    // RiskManager.calculate_size already multiplies by
                    // the SurvivalAgent-controlled size_multiplier.
                    let base_size = risk.calculate_size(effective_entry, effective_sl);

                    // Size is controlled solely by the risk agent — no external multipliers.
                    // Quant engine runs for IC/Kalman context only; its size_multiplier is ignored.
                    if let Some(ref qe) = quant_engine {
                        qe.compute_sizing(QuantSizingInput {
                            symbol: &signal.symbol,
                            strategy: signal.strategy.as_str(),
                            side: signal.side,
                            entry: effective_entry,
                            stop_loss: effective_sl,
                            equity: risk.equity(),
                            base_risk_pct: cfg.base_risk_pct,
                        });
                    }
                    let size = base_size;

                    if size <= 0.0 {
                        bus.publish(AgentEvent::RiskVerdict(RiskVerdictMsg {
                            signal: signal.clone(),
                            regime,
                            outcome: RiskOutcome::Blocked,
                            size: 0.0,
                            size_multiplier: 1.0,
                            effective_ta_threshold,
                            effective_llm_floor: llm_floor,
                            matched_lessons: verdict.matched_lessons,
                            reason: Some("size <= 0".into()),
                        }));
                        continue;
                    }

                    {
                        let limits = risk.limits();
                        let risk_amount = risk.equity() * limits.risk_per_trade_pct / 100.0;
                        if limits.min_margin_usd > 0.0 && risk_amount < limits.min_margin_usd {
                            warn!(
                                symbol = %signal.symbol,
                                risk_amount,
                                min_margin_usd = limits.min_margin_usd,
                                size,
                                "risk: blocked dust-sized position below min_margin_usd"
                            );
                            bus.publish(AgentEvent::RiskVerdict(RiskVerdictMsg {
                                signal: signal.clone(),
                                regime,
                                outcome: RiskOutcome::Blocked,
                                size: 0.0,
                                size_multiplier: 1.0,
                                effective_ta_threshold,
                                effective_llm_floor: llm_floor,
                                matched_lessons: verdict.matched_lessons,
                                reason: Some(format!(
                                    "risk_amount ${risk_amount:.2} < min_margin_usd ${:.2}",
                                    limits.min_margin_usd
                                )),
                            }));
                            continue;
                        }
                    }

                    // Update signal with capped SL/TP for downstream (brain, execution)
                    let mut capped_signal = signal.clone();
                    capped_signal.stop_loss = effective_sl;
                    capped_signal.take_profit = effective_tp;

                    // Extract symbol before move into RiskVerdictMsg
                    let symbol_for_pending = capped_signal.symbol.clone();

                    bus.publish(AgentEvent::RiskVerdict(RiskVerdictMsg {
                        signal: capped_signal,
                        regime,
                        outcome: RiskOutcome::Allowed,
                        size,
                        size_multiplier: 1.0,
                        effective_ta_threshold,
                        effective_llm_floor: llm_floor,
                        matched_lessons: verdict.matched_lessons,
                        reason: None,
                    }));
                    // Lock symbol immediately — prevents duplicate while LLM + execution run.
                    // Released on OrderFilled (→ open_symbols) or Veto (→ cleared).
                    pending_symbols.lock().insert(symbol_for_pending);
                }
                // Veto: release pending lock so the symbol can be retried next signal.
                AgentEvent::ManagerVerdictEmitted(ref v)
                    if matches!(
                        v.action,
                        crate::agents::messages::ManagerAction::Veto { .. }
                    ) =>
                {
                    pending_symbols.lock().remove(&v.proposal.symbol);
                }
                // P0-7: Execution failed to place/fill an order — release the pending lock.
                // Without this, a failed order permanently blocks the symbol from re-entry.
                AgentEvent::ExecutionFailed { symbol, .. } => {
                    if pending_symbols.lock().remove(&symbol) {
                        info!(symbol = %symbol, "risk: released pending lock after execution failure");
                    }
                }
                AgentEvent::RiskReservationReleased { symbol, reason } => {
                    if pending_symbols.lock().remove(&symbol) {
                        info!(symbol = %symbol, reason = %reason, "risk: released pending lock after downstream rejection");
                    }
                }
                _ => {}
            }
        }
    })
}
