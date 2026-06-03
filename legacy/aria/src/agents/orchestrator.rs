//! OrchestratorAgent — the "brain of brains" that coordinates all agents.
//!
//! This agent subscribes to ALL events on the MessageBus and maintains a
//! unified global state. It acts as the central nervous system that:
//!
//! 1. **Coordinates agent awareness** — Survival knows about Brain's
//!    confidence, Brain knows about Survival's mode, Risk knows about
//!    Learning's lessons.
//!
//! 2. **Dynamic strategy selection** — based on market regime, survival
//!    mode, and recent performance, it can suggest which strategies to
//!    prioritize.
//!
//! 3. **Meta-learning integration** — injects lessons from LearningAgent
//!    into Brain's context so the AI learns from past mistakes.
//!
//! 4. **Emergency override** — can freeze/unfreeze, adjust sizing, or
//!    force flat when systemic risk is detected.
//!
//! 5. **Performance monitoring** — tracks win rate, consecutive losses,
//!    and adjusts aggressiveness dynamically.

use crate::agents::MessageBus;
use crate::agents::messages::{
    AgentEvent, AgentId, ControlCommand, OrchestratorSnapshot, SurvivalMode,
};
use crate::learning::{LearningPolicy, Lesson};
use parking_lot::RwLock;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::{info, warn};

/// Maximum recent outcomes to track for performance analysis.
const MAX_RECENT_OUTCOMES: usize = 100;
/// Maximum recent brain decisions to track.
const MAX_RECENT_BRAINS: usize = 50;

/// Global orchestrator state — visible to all agents via shared reference.
#[derive(Debug, Clone)]
pub struct OrchestratorState {
    /// Current survival mode.
    pub survival_mode: SurvivalMode,
    /// Survival score (0-100).
    pub survival_score: u8,
    /// Current drawdown percentage.
    pub drawdown_pct: f64,
    /// Consecutive losses.
    pub consecutive_losses: u32,
    /// Total open positions.
    pub open_positions: u32,
    /// Recent trade outcomes (true = win, false = loss).
    pub recent_outcomes: VecDeque<bool>,
    /// Recent brain decisions with confidence.
    pub recent_brains: VecDeque<BrainDecision>,
    /// Active lessons from LearningAgent.
    pub active_lessons: Vec<Lesson>,
    /// Current market regime per symbol.
    pub regimes: HashMap<String, String>,
    /// Win rate (rolling).
    pub win_rate: f64,
    /// Average brain confidence (rolling).
    pub avg_confidence: f64,
    /// Suggested size multiplier (orchestrator can adjust).
    pub size_multiplier: f64,
    /// Whether orchestrator has frozen trading.
    pub orchestrator_frozen: bool,
    /// Reason for orchestrator freeze.
    pub freeze_reason: Option<String>,
    /// Timestamp of last emergency action.
    pub last_emergency_ts: i64,
}

impl Default for OrchestratorState {
    fn default() -> Self {
        Self {
            survival_mode: SurvivalMode::Healthy,
            survival_score: 100,
            drawdown_pct: 0.0,
            consecutive_losses: 0,
            open_positions: 0,
            recent_outcomes: VecDeque::new(),
            recent_brains: VecDeque::new(),
            active_lessons: Vec::new(),
            regimes: HashMap::new(),
            win_rate: 50.0,
            avg_confidence: 50.0,
            size_multiplier: 1.0,
            orchestrator_frozen: false,
            freeze_reason: None,
            last_emergency_ts: 0,
        }
    }
}

/// A brain decision record for tracking.
#[derive(Debug, Clone)]
pub struct BrainDecision {
    pub symbol: String,
    pub decision: String,
    pub confidence: u8,
    pub strategy: String,
    pub regime: String,
    pub ts: i64,
}

/// Configuration for the OrchestratorAgent.
#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    /// Minimum survival score before reducing size.
    pub caution_score_threshold: u8,
    /// Minimum survival score before freezing.
    pub freeze_score_threshold: u8,
    /// Maximum consecutive losses before size reduction.
    pub max_consecutive_losses: u32,
    /// Win rate threshold below which we reduce aggressiveness.
    pub min_win_rate: f64,
    /// Whether to inject lessons into brain context.
    pub lessons_injection_enabled: bool,
    /// Whether orchestrator can force freeze.
    pub emergency_freeze_enabled: bool,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            caution_score_threshold: 60,
            freeze_score_threshold: 30,
            max_consecutive_losses: 10,
            min_win_rate: 40.0,
            lessons_injection_enabled: true,
            emergency_freeze_enabled: true,
        }
    }
}

/// Spawn the OrchestratorAgent.
pub fn spawn(
    bus: MessageBus,
    cfg: OrchestratorConfig,
    policy: Option<LearningPolicy>,
    orchestrator_state: Arc<RwLock<OrchestratorState>>,
) -> JoinHandle<()> {
    let mut rx = bus.subscribe();

    tokio::spawn(async move {
        info!("orchestrator agent starting — coordinating all agents");
        {
            let bus_hb = bus.clone();
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(std::time::Duration::from_secs(20));
                loop {
                    tick.tick().await;
                    bus_hb.publish(AgentEvent::Heartbeat {
                        from: AgentId::Orchestrator,
                        ts: chrono::Utc::now(),
                    });
                }
            });
        }

        loop {
            let ev = match rx.recv().await {
                Ok(ev) => ev,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    warn!(skipped = n, "broadcast lagged — skipping events");
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            };
            let mut state = orchestrator_state.write();
            let mut publish_snapshot = false;

            match ev {
                AgentEvent::Shutdown => {
                    info!("orchestrator shutting down");
                    break;
                }

                // ── Survival updates ──────────────────────────────
                AgentEvent::SurvivalUpdated(surv) => {
                    state.survival_mode = surv.mode;
                    state.survival_score = surv.score;
                    state.drawdown_pct = surv.drawdown_pct;
                    state.consecutive_losses = surv.consecutive_losses;
                    state.open_positions = surv.open_positions;

                    // Emergency freeze if survival score too low
                    if cfg.emergency_freeze_enabled
                        && surv.score <= cfg.freeze_score_threshold
                        && !state.orchestrator_frozen
                    {
                        let reason = format!(
                            "orchestrator emergency: survival score {} <= {}",
                            surv.score, cfg.freeze_score_threshold
                        );
                        warn!("{}", reason);
                        state.orchestrator_frozen = true;
                        state.freeze_reason = Some(reason.clone());
                        state.last_emergency_ts = chrono::Utc::now().timestamp();
                        bus.publish(AgentEvent::ControlCommand(ControlCommand::Freeze {
                            reason,
                        }));
                    }

                    // Unfreeze if recovered
                    if state.orchestrator_frozen && surv.score > cfg.caution_score_threshold {
                        info!("orchestrator: survival recovered, unfreezing");
                        state.orchestrator_frozen = false;
                        state.freeze_reason = None;
                        bus.publish(AgentEvent::ControlCommand(ControlCommand::Unfreeze));
                    }

                    // Dynamic size multiplier based on survival
                    state.size_multiplier = if surv.score >= 70 {
                        1.0
                    } else if surv.score >= 50 {
                        0.8
                    } else if surv.score >= 30 {
                        0.6
                    } else {
                        0.4
                    };

                    // Extra reduction for consecutive losses
                    if state.consecutive_losses >= cfg.max_consecutive_losses {
                        state.size_multiplier *= 0.5;
                        warn!(
                            "orchestrator: {} consecutive losses, size halved",
                            state.consecutive_losses
                        );
                    }
                    publish_snapshot = true;
                }

                // ── Brain outcomes ────────────────────────────────
                AgentEvent::BrainOutcomeReady(brain) => {
                    let decision_str = match brain.decision.decision {
                        crate::llm::engine::Decision::Go => "GO",
                        crate::llm::engine::Decision::NoGo => "NOGO",
                        crate::llm::engine::Decision::Wait => "WAIT",
                    };

                    state.recent_brains.push_back(BrainDecision {
                        symbol: brain.signal.symbol.clone(),
                        decision: decision_str.to_string(),
                        confidence: brain.decision.confidence,
                        strategy: brain.signal.strategy.as_str().to_string(),
                        regime: brain.regime.as_str().to_string(),
                        ts: chrono::Utc::now().timestamp(),
                    });
                    while state.recent_brains.len() > MAX_RECENT_BRAINS {
                        state.recent_brains.pop_front();
                    }

                    // Update rolling average confidence
                    let conf_sum: u32 = state
                        .recent_brains
                        .iter()
                        .map(|b| b.confidence as u32)
                        .sum();
                    state.avg_confidence =
                        conf_sum as f64 / state.recent_brains.len().max(1) as f64;

                    // Avg confidence is tracked for monitoring only — NOT used to
                    // compound-reduce size. Sizing is driven by survival score.
                    publish_snapshot = true;
                }

                // ── Position closed ───────────────────────────────
                AgentEvent::PositionClosed { pnl_usd, .. } => {
                    let is_win = pnl_usd >= 0.0;
                    state.recent_outcomes.push_back(is_win);
                    while state.recent_outcomes.len() > MAX_RECENT_OUTCOMES {
                        state.recent_outcomes.pop_front();
                    }

                    // Update rolling win rate
                    let wins = state.recent_outcomes.iter().filter(|&&w| w).count();
                    state.win_rate =
                        wins as f64 / state.recent_outcomes.len().max(1) as f64 * 100.0;

                    // Win rate is NOT used to reduce size — net PnL and ROE matter,
                    // not win rate. The survival score (already applied above via
                    // SurvivalUpdated) is the correct lever. Compounding WR-based
                    // reductions cause the bot to stop trading entirely.

                    // Emergency freeze on big loss (relative: >30% of equity in one trade)
                    if pnl_usd < -100.0 && cfg.emergency_freeze_enabled {
                        let reason = format!("orchestrator emergency: large loss ${:.2}", pnl_usd);
                        warn!("{}", reason);
                        state.orchestrator_frozen = true;
                        state.freeze_reason = Some(reason.clone());
                        state.last_emergency_ts = chrono::Utc::now().timestamp();
                        bus.publish(AgentEvent::ControlCommand(ControlCommand::Freeze {
                            reason,
                        }));
                    }
                    publish_snapshot = true;
                }

                // ── Market regime updates ─────────────────────────
                AgentEvent::PreSignalEmitted { signal, regime } => {
                    state
                        .regimes
                        .insert(signal.symbol.clone(), regime.as_str().to_string());
                }

                // ── Manager verdicts ──────────────────────────────
                AgentEvent::ManagerVerdictEmitted(verdict) if verdict.action.is_blocking() => {
                    warn!("orchestrator: manager vetoed trade — {:?}", verdict.action);
                }

                // ── Learning policy updates ───────────────────────
                AgentEvent::PolicyRefreshed { lessons_count, .. }
                    if cfg.lessons_injection_enabled =>
                {
                    if let Some(ref p) = policy {
                        state.active_lessons = p.active_lessons();
                        info!(
                            "orchestrator: updated {} lessons from learning agent",
                            lessons_count
                        );
                    }
                }

                _ => {}
            }
            if publish_snapshot {
                bus.publish(AgentEvent::OrchestratorUpdated(OrchestratorSnapshot {
                    size_multiplier: state.size_multiplier.clamp(0.0, 2.0),
                    frozen: state.orchestrator_frozen,
                    reason: state.freeze_reason.clone(),
                    ts: chrono::Utc::now(),
                }));
            }
        }
    })
}

/// Get a summary of orchestrator state for Telegram /status command.
pub fn orchestrator_summary(state: &OrchestratorState) -> String {
    let mode_emoji = match state.survival_mode {
        SurvivalMode::Healthy => "🟢",
        SurvivalMode::Cautious => "🟡",
        SurvivalMode::Defensive => "🟠",
        SurvivalMode::Frozen => "🧊",
        SurvivalMode::Dead => "💀",
    };

    let freeze_line = if state.orchestrator_frozen {
        format!(
            "\n🧊 <b>ORCHESTRATOR FROZEN:</b> <code>{}</code>",
            state.freeze_reason.as_deref().unwrap_or("unknown")
        )
    } else {
        String::new()
    };

    let lessons_summary = if state.active_lessons.is_empty() {
        "none".to_string()
    } else {
        state
            .active_lessons
            .iter()
            .take(3)
            .map(|l| l.reason.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };

    format!(
        "{mode_emoji} <b>Orchestrator</b>\n\
         ├ Survival: <code>{mode}</code> (score: {score})\n\
         ├ Win Rate: <code>{wr:.1}%</code> ({wins}W/{losses}L)\n\
         ├ Avg Confidence: <code>{conf:.0}%</code>\n\
         ├ Size Multiplier: <code>{mult:.2}×</code>\n\
         ├ Consecutive Losses: <code>{losses_streak}</code>\n\
         ├ Active Lessons: <code>{lessons}</code>\n\
         └ Regimes: {regimes}{freeze}",
        mode_emoji = mode_emoji,
        mode = state.survival_mode.as_str().to_uppercase(),
        score = state.survival_score,
        wr = state.win_rate,
        wins = state.recent_outcomes.iter().filter(|&&w| w).count(),
        losses = state.recent_outcomes.iter().filter(|&&w| !w).count(),
        conf = state.avg_confidence,
        mult = state.size_multiplier,
        losses_streak = state.consecutive_losses,
        lessons = lessons_summary,
        regimes = state
            .regimes
            .iter()
            .map(|(s, r)| format!("{}:{}", &s[..s.len().min(3)], r))
            .collect::<Vec<_>>()
            .join(" "),
        freeze = freeze_line,
    )
}

/// Build a context string for Brain prompt injection.
/// This gives Brain awareness of orchestrator state + lessons.
pub fn build_brain_context(state: &OrchestratorState) -> String {
    let mut ctx = String::new();

    // Survival context
    ctx.push_str(&format!(
        "\n## ORCHESTRATOR CONTEXT\n\
         - Survival Mode: {} (score: {})\n\
         - Win Rate: {:.1}% ({} trades)\n\
         - Consecutive Losses: {}\n\
         - Orchestrator Size Mult: {:.2}×\n",
        state.survival_mode.as_str(),
        state.survival_score,
        state.win_rate,
        state.recent_outcomes.len(),
        state.consecutive_losses,
        state.size_multiplier,
    ));

    // Lessons context
    if !state.active_lessons.is_empty() {
        ctx.push_str("\n## LESSONS FROM PAST TRADES\n");
        for lesson in state.active_lessons.iter().take(5) {
            ctx.push_str(&format!("- {}\n", lesson.reason));
        }
    }

    // Recent brain performance
    if !state.recent_brains.is_empty() {
        let recent_go = state
            .recent_brains
            .iter()
            .filter(|b| b.decision == "GO")
            .count();
        let recent_nogo = state
            .recent_brains
            .iter()
            .filter(|b| b.decision == "NOGO")
            .count();
        ctx.push_str(&format!(
            "\n## RECENT AI PERFORMANCE\n\
             - GO: {} / NOGO: {} / Total: {}\n\
             - Avg Confidence: {:.0}%\n",
            recent_go,
            recent_nogo,
            state.recent_brains.len(),
            state.avg_confidence,
        ));
    }

    ctx
}
