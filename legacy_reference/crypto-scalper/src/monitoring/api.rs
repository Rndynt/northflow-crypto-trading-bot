//! Full REST + SSE API layer for the ARIA dashboard.
//!
//! Endpoints:
//!   GET /api/status       — Full bot status (equity, survival, positions, metrics)
//!   GET /api/metrics      — MetricsSnapshot (JSON)
//!   GET /api/positions    — Open positions with signal_id, PnL, duration
//!   GET /api/trades       — Closed trade history (paginated)
//!   GET /api/signals      — Recent signal evaluations
//!   GET /api/screening    — Current screening bias per symbol
//!   GET /api/survival     — Survival state
//!   GET /api/lessons      — Active learning lessons
//!   GET /api/config       — Current runtime config (safe subset)
//!   GET /api/events       — SSE real-time event stream
//!   GET /healthz          — Health check

use crate::agents::messages::{
    AgentEvent, ControlCommand, ScreeningBias, SurvivalState,
};
use crate::agents::MessageBus;
use crate::config::Config;
use crate::execution::{PositionBook, RiskManager};
use crate::learning::LearningPolicy;
use crate::monitoring::logger::TradeJournal;
use crate::monitoring::metrics::{MetricsSnapshot, MetricsState};
use crate::shared_state::SharedState;

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use futures_util::stream::Stream;
use parking_lot::RwLock as PlRwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio::sync::broadcast;
use tracing::{info, warn};

// ── Shared API State ──────────────────────────────────────────────────

/// Central state shared across all API handlers.
#[derive(Clone)]
pub struct ApiState {
    pub metrics: Arc<MetricsState>,
    pub survival: Arc<PlRwLock<Option<SurvivalState>>>,
    pub policy: Option<Arc<LearningPolicy>>,
    pub book: Arc<PositionBook>,
    pub risk: Arc<RiskManager>,
    pub journal: Option<Arc<TradeJournal>>,
    pub shared_state: Arc<SharedState>,
    pub config_summary: ConfigSummary,
    /// SSE broadcast channel for real-time events.
    /// Capacity 1024 — old events are dropped if consumers lag.
    pub event_tx: broadcast::Sender<ApiEvent>,
    /// Per-symbol screening bias (updated by signal agent).
    pub screening_bias: Arc<PlRwLock<HashMap<String, ScreeningBiasEntry>>>,
    /// Recent signals (ring buffer of last 50).
    pub recent_signals: Arc<PlRwLock<Vec<SignalEntry>>>,
    /// Message bus — used by control POST endpoints.
    pub bus: MessageBus,
}

// ── API Response Types ────────────────────────────────────────────────

/// Safe subset of config exposed via API (no secrets).
#[derive(Debug, Clone, Serialize)]
pub struct ConfigSummary {
    pub mode: String,
    pub exchange: String,
    pub symbol_count: usize,
    pub max_leverage: u32,
    pub risk_per_trade_pct: f64,
    pub max_drawdown_pct: f64,
    pub max_open_positions: u32,
    pub partial_tp_enabled: bool,
    pub max_hold_secs: i64,
    pub metrics_bind: String,
    pub active_strategies: Vec<String>,
}

/// Full status response combining all data sources.
#[derive(Debug, Clone, Serialize)]
pub struct StatusResponse {
    pub metrics: MetricsSnapshot,
    pub survival: Option<SurvivalState>,
    pub positions: Vec<PositionEntry>,
    pub config: ConfigSummary,
    pub shared: SharedSnapshot,
    pub ts: i64,
}

/// Shared state snapshot for API.
#[derive(Debug, Clone, Serialize)]
pub struct SharedSnapshot {
    pub equity: f64,
    pub initial_equity: f64,
    pub peak_equity: f64,
    pub realized_pnl_today: f64,
    pub unrealized_pnl: f64,
    pub total_equity: f64,
    pub open_positions: u64,
    pub survival_mode: String,
    pub survival_score: f64,
    pub drawdown_pct: f64,
    pub current_regime: String,
    pub strategy_health: HashMap<String, StrategyHealthEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StrategyHealthEntry {
    pub name: String,
    pub total_trades: u64,
    pub wins: u64,
    pub losses: u64,
    pub win_rate: f64,
    pub total_pnl: f64,
    pub avg_pnl: f64,
    pub loss_streak: u64,
    pub enabled: bool,
    pub size_multiplier: f64,
}

/// Open position with full details.
#[derive(Debug, Clone, Serialize)]
pub struct PositionEntry {
    pub signal_id: String,
    pub client_id: String,
    pub symbol: String,
    pub side: String,
    pub size: f64,
    pub entry_price: f64,
    pub stop_loss: f64,
    pub take_profit: f64,
    pub strategy: String,
    pub opened_at: String,
    pub duration_mins: i64,
    pub trailing_activated: bool,
    pub breakeven_activated: bool,
    pub partial_taken: bool,
    pub partial_realized_pnl: f64,
    pub current_price: Option<f64>,
    pub unrealized_pnl: Option<f64>,
    pub unrealized_pnl_pct: Option<f64>,
}

/// Closed trade entry for /api/trades.
#[derive(Debug, Clone, Serialize)]
pub struct TradeEntry {
    pub signal_id: String,
    pub symbol: String,
    pub direction: String,
    pub strategy: String,
    pub regime: String,
    pub entry_time: String,
    pub exit_time: String,
    pub pnl_usd: f64,
    pub pnl_pct: f64,
    pub is_win: bool,
    pub ta_confidence: Option<u8>,
    pub llm_confidence: Option<u8>,
    pub entry_price: f64,
    pub exit_price: f64,
    pub stop_loss: f64,
    pub take_profit: f64,
    pub size: f64,
    pub partial_taken: bool,
    pub partial_realized_pnl: f64,
}

/// Signal entry for /api/signals.
#[derive(Debug, Clone, Serialize)]
pub struct SignalEntry {
    pub signal_id: String,
    pub symbol: String,
    pub side: String,
    pub strategy: String,
    pub ta_confidence: u8,
    pub entry: f64,
    pub stop_loss: f64,
    pub take_profit: f64,
    pub regime: String,
    pub reason: String,
    pub ts: i64,
}

/// Screening bias entry per symbol.
#[derive(Debug, Clone, Serialize)]
pub struct ScreeningBiasEntry {
    pub symbol: String,
    pub bias: String,
    pub allows_long: bool,
    pub allows_short: bool,
    pub ts: i64,
}

/// SSE event types for real-time streaming.
#[derive(Debug, Clone, Serialize)]
pub struct ApiEvent {
    pub event_type: String,
    pub data: serde_json::Value,
    pub ts: i64,
}

/// Traded response wrapper with pagination.
#[derive(Debug, Clone, Serialize)]
pub struct PaginatedResponse<T: Serialize> {
    pub items: Vec<T>,
    pub total: usize,
    pub page: usize,
    pub per_page: usize,
}

/// Query params for paginated endpoints.
#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    pub page: Option<usize>,
    pub per_page: Option<usize>,
}

// ── API Server ────────────────────────────────────────────────────────

/// Spawn the full API server with all endpoints.
pub fn spawn_api_server(state: ApiState, bind: SocketAddr) -> JoinHandle<()> {
    let cors = tower_http::cors::CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    let app = Router::new()
        // Health
        .route("/healthz", get(|| async { "ok" }))
        // Core endpoints
        .route("/api/status", get(status_handler))
        .route("/api/metrics", get(metrics_handler))
        .route("/api/positions", get(positions_handler))
        .route("/api/trades", get(trades_handler))
        .route("/api/signals", get(signals_handler))
        .route("/api/screening", get(screening_handler))
        .route("/api/survival", get(survival_handler))
        .route("/api/lessons", get(lessons_handler))
        .route("/api/config", get(config_handler))
        .route("/api/events", get(sse_handler))
        // Control POST endpoints
        .route("/api/control/freeze", post(control_freeze_handler))
        .route("/api/control/unfreeze", post(control_unfreeze_handler))
        .route("/api/control/flat", post(control_flat_handler))
        .route("/api/control/close", post(control_close_handler))
        .route("/api/control/config", post(control_config_handler))
        // Root info
        .route("/", get(|| async {
            "ARIA Dashboard API — see /api/status, /api/metrics, /api/positions, /api/trades, /api/signals, /api/screening, /api/events"
        }))
        .with_state(state)
        .layer(cors);

    tokio::spawn(async move {
        match tokio::net::TcpListener::bind(bind).await {
            Ok(listener) => {
                info!(%bind, "📡 ARIA API server listening");
                if let Err(e) = axum::serve(listener, app).await {
                    tracing::error!(error = %e, "api server error");
                }
            }
            Err(e) => {
                tracing::error!(%bind, error = %e, "cannot bind api server");
            }
        }
    })
}

// ── Broadcast helper ──────────────────────────────────────────────────

/// Broadcast an event to all SSE subscribers.
pub fn broadcast_event(state: &ApiState, event_type: &str, data: serde_json::Value) {
    let event = ApiEvent {
        event_type: event_type.to_string(),
        data,
        ts: chrono::Utc::now().timestamp(),
    };
    // Ignore error if no subscribers
    let _ = state.event_tx.send(event);
}

/// Record a signal in the recent signals ring buffer.
pub fn record_signal(state: &ApiState, signal: &SignalEntry) {
    let mut signals = state.recent_signals.write();
    signals.push(signal.clone());
    if signals.len() > 50 {
        signals.remove(0);
    }
}

/// Update screening bias for a symbol.
pub fn update_screening_bias(state: &ApiState, symbol: &str, bias: &ScreeningBias) {
    let entry = ScreeningBiasEntry {
        symbol: symbol.to_string(),
        bias: bias.as_str().to_string(),
        allows_long: bias.allows(&crate::data::Side::Long),
        allows_short: bias.allows(&crate::data::Side::Short),
        ts: chrono::Utc::now().timestamp(),
    };
    state.screening_bias.write().insert(symbol.to_string(), entry);
}

// ── Handlers ──────────────────────────────────────────────────────────

/// Full status endpoint — combines metrics, survival, positions, config, shared state.
async fn status_handler(State(state): State<ApiState>) -> Json<StatusResponse> {
    let metrics = state.metrics.snapshot();
    let survival = state.survival.read().clone();
    let positions = build_position_entries(&state);
    let config = state.config_summary.clone();
    let shared = build_shared_snapshot(&state);
    let ts = chrono::Utc::now().timestamp();

    Json(StatusResponse {
        metrics,
        survival,
        positions,
        config,
        shared,
        ts,
    })
}

/// Metrics endpoint.
async fn metrics_handler(State(state): State<ApiState>) -> Json<MetricsSnapshot> {
    Json(state.metrics.snapshot())
}

/// Open positions endpoint.
async fn positions_handler(State(state): State<ApiState>) -> Json<Vec<PositionEntry>> {
    Json(build_position_entries(&state))
}

/// Trade history endpoint (paginated).
async fn trades_handler(
    State(state): State<ApiState>,
    axum::extract::Query(params): axum::extract::Query<PaginationParams>,
) -> Result<Json<PaginatedResponse<TradeEntry>>, StatusCode> {
    let journal = match &state.journal {
        Some(j) => j,
        None => return Err(StatusCode::SERVICE_UNAVAILABLE),
    };

    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(50).min(200);
    let limit = (page * per_page) as i64;

    let closed = match journal.closed_trades(limit) {
        Ok(t) => t,
        Err(e) => {
            warn!(error = %e, "api: closed_trades query failed");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let total = closed.len();
    let start = ((page - 1) * per_page).min(total);
    let end = (page * per_page).min(total);
    let items: Vec<TradeEntry> = closed[start..end]
        .iter()
        .map(|t| TradeEntry {
            signal_id: t.signal_id.clone(),
            symbol: t.symbol.clone(),
            direction: t.direction.clone(),
            strategy: t.strategy.clone(),
            regime: t.regime.clone(),
            entry_time: t.entry_time.to_rfc3339(),
            exit_time: t.exit_time.to_rfc3339(),
            pnl_usd: t.pnl_usd,
            pnl_pct: t.pnl_pct,
            is_win: t.is_win(),
            ta_confidence: t.ta_confidence,
            llm_confidence: t.llm_confidence,
            entry_price: t.entry_price,
            exit_price: t.exit_price,
            stop_loss: t.stop_loss,
            take_profit: t.take_profit,
            size: t.size,
            partial_taken: t.partial_taken,
            partial_realized_pnl: t.partial_realized_pnl,
        })
        .collect();

    Ok(Json(PaginatedResponse {
        items,
        total,
        page,
        per_page,
    }))
}

/// Recent signals endpoint.
async fn signals_handler(State(state): State<ApiState>) -> Json<Vec<SignalEntry>> {
    let signals = state.recent_signals.read().clone();
    Json(signals)
}

/// Screening bias endpoint.
async fn screening_handler(
    State(state): State<ApiState>,
) -> Json<Vec<ScreeningBiasEntry>> {
    let biases: Vec<ScreeningBiasEntry> =
        state.screening_bias.read().values().cloned().collect();
    Json(biases)
}

/// Survival endpoint.
async fn survival_handler(State(state): State<ApiState>) -> Response {
    match state.survival.read().clone() {
        Some(s) => Json(s).into_response(),
        None => (StatusCode::NOT_FOUND, "survival state not yet computed").into_response(),
    }
}

/// Lessons endpoint.
async fn lessons_handler(State(state): State<ApiState>) -> Json<Vec<crate::learning::Lesson>> {
    Json(
        state
            .policy
            .as_ref()
            .map(|p| p.active_lessons())
            .unwrap_or_default(),
    )
}

/// Config endpoint (safe subset, no secrets).
async fn config_handler(State(state): State<ApiState>) -> Json<ConfigSummary> {
    Json(state.config_summary.clone())
}

/// SSE real-time event stream.
///
/// Clients connect to GET /api/events and receive Server-Sent Events.
/// Each event has an `event` field (type) and `data` field (JSON).
///
/// Event types:
///   - `signal`     — New signal detected
///   - `fill`       — Order filled (position opened)
///   - `close`      — Position closed (SL/TP/trailing/manual)
///   - `partial`    — Partial TP taken
///   - `sl_moved`   — Stop loss moved (breakeven/trailing)
///   - `survival`   — Survival state updated
///   - `equity`     — Equity reconciled
///   - `screening`  — Screening bias updated
///   - `error`      — Execution error
async fn sse_handler(
    State(state): State<ApiState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.event_tx.subscribe();

    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(api_event) => {
                    let event_type = api_event.event_type.clone();
                    let data = serde_json::to_string(&api_event).unwrap_or_default();
                    let event = Event::default().event(&event_type).data(&data);
                    yield Ok(event);
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!(skipped = n, "SSE subscriber lagged");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

// ── Control POST Handlers ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ControlReasonBody {
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ControlCloseBody {
    symbol: String,
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ControlConfigBody {
    max_leverage: Option<u32>,
    risk_per_trade_pct: Option<f64>,
    max_open_positions: Option<u32>,
    max_daily_loss_pct: Option<f64>,
    max_hold_secs: Option<i64>,
    breakeven_r: Option<f64>,
}

#[derive(Debug, Serialize)]
struct ControlResponse {
    ok: bool,
    message: String,
}

async fn control_freeze_handler(
    State(state): State<ApiState>,
    body: Option<Json<ControlReasonBody>>,
) -> Json<ControlResponse> {
    let reason = body
        .and_then(|b| b.reason.clone())
        .unwrap_or_else(|| "manual via dashboard".into());
    state.bus.publish(AgentEvent::ControlCommand(ControlCommand::Freeze { reason: reason.clone() }));
    info!(reason = %reason, "control: freeze published via API");
    Json(ControlResponse { ok: true, message: format!("Freeze published: {reason}") })
}

async fn control_unfreeze_handler(
    State(state): State<ApiState>,
) -> Json<ControlResponse> {
    state.bus.publish(AgentEvent::ControlCommand(ControlCommand::Unfreeze));
    info!("control: unfreeze published via API");
    Json(ControlResponse { ok: true, message: "Unfreeze published".into() })
}

async fn control_flat_handler(
    State(state): State<ApiState>,
    body: Option<Json<ControlReasonBody>>,
) -> Json<ControlResponse> {
    let reason = body
        .and_then(|b| b.reason.clone())
        .unwrap_or_else(|| "manual flat via dashboard".into());
    state.bus.publish(AgentEvent::ControlCommand(ControlCommand::FlatAll { reason: reason.clone() }));
    info!(reason = %reason, "control: flat_all published via API");
    Json(ControlResponse { ok: true, message: format!("FlatAll published: {reason}") })
}

async fn control_close_handler(
    State(state): State<ApiState>,
    Json(body): Json<ControlCloseBody>,
) -> impl IntoResponse {
    if body.symbol.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ControlResponse { ok: false, message: "symbol required".into() }),
        ).into_response();
    }
    let symbol = body.symbol.trim().to_uppercase();
    state.bus.publish(AgentEvent::ControlCommand(ControlCommand::ClosePosition { symbol: symbol.clone() }));
    info!(symbol = %symbol, "control: close_position published via API");
    (
        StatusCode::OK,
        Json(ControlResponse { ok: true, message: format!("ClosePosition published for {symbol}") }),
    ).into_response()
}

async fn control_config_handler(
    State(state): State<ApiState>,
    Json(body): Json<ControlConfigBody>,
) -> Json<ControlResponse> {
    let mut changes: Vec<String> = Vec::new();

    if let Some(lev) = body.max_leverage {
        state.risk.set_max_leverage(lev);
        changes.push(format!("max_leverage={lev}"));
    }
    if let Some(pct) = body.risk_per_trade_pct {
        state.risk.set_risk_per_trade_pct(pct);
        changes.push(format!("risk_per_trade_pct={pct:.2}%"));
    }
    if let Some(n) = body.max_open_positions {
        state.risk.set_max_open_positions(n);
        changes.push(format!("max_open_positions={n}"));
    }
    if let Some(pct) = body.max_daily_loss_pct {
        state.risk.set_max_daily_loss_pct(pct);
        changes.push(format!("max_daily_loss_pct={pct:.2}%"));
    }
    if let Some(secs) = body.max_hold_secs {
        state.bus.publish(AgentEvent::ControlCommand(ControlCommand::SetMaxHold { secs }));
        changes.push(format!("max_hold_secs={secs}"));
    }
    if let Some(r) = body.breakeven_r {
        state.bus.publish(AgentEvent::ControlCommand(ControlCommand::SetBreakevenR { r }));
        changes.push(format!("breakeven_r={r:.2}"));
    }

    if changes.is_empty() {
        return Json(ControlResponse { ok: false, message: "No valid fields provided".into() });
    }

    let msg = format!("Updated: {}", changes.join(", "));
    info!(changes = %msg, "control: config updated via API");
    Json(ControlResponse { ok: true, message: msg })
}

// ── Helpers ───────────────────────────────────────────────────────────

fn build_position_entries(state: &ApiState) -> Vec<PositionEntry> {
    let positions = state.book.snapshot();
    let now = chrono::Utc::now();

    positions
        .iter()
        .map(|p| {
            let dur = now - p.opened_at;
            let side_str = match p.side {
                crate::data::Side::Long => "LONG",
                crate::data::Side::Short => "SHORT",
            };

            // Current price from last marks (if available via risk snapshot)
            let current_price = None; // Will be populated by SSE updates
            let unrealized_pnl = None;
            let unrealized_pnl_pct = None;

            PositionEntry {
                signal_id: if p.signal_id.is_empty() {
                    "—".to_string()
                } else {
                    p.signal_id.clone()
                },
                client_id: p.client_id.clone(),
                symbol: p.symbol.clone(),
                side: side_str.to_string(),
                size: p.size,
                entry_price: p.entry_price,
                stop_loss: p.stop_loss,
                take_profit: p.take_profit,
                strategy: p.strategy.clone(),
                opened_at: p.opened_at.to_rfc3339(),
                duration_mins: dur.num_minutes(),
                trailing_activated: p.trailing_activated,
                breakeven_activated: p.breakeven_activated,
                partial_taken: p.partial_taken,
                partial_realized_pnl: p.partial_realized_pnl,
                current_price,
                unrealized_pnl,
                unrealized_pnl_pct,
            }
        })
        .collect()
}

fn build_shared_snapshot(state: &ApiState) -> SharedSnapshot {
    let ss = &state.shared_state;

    let strategy_health = {
        let health = ss.strategy_health.read();
        health
            .iter()
            .map(|(name, h)| {
                (
                    name.clone(),
                    StrategyHealthEntry {
                        name: h.name.clone(),
                        total_trades: h.total_trades,
                        wins: h.wins,
                        losses: h.losses,
                        win_rate: h.win_rate,
                        total_pnl: h.total_pnl,
                        avg_pnl: h.avg_pnl,
                        loss_streak: h.loss_streak,
                        enabled: h.enabled,
                        size_multiplier: h.size_multiplier(),
                    },
                )
            })
            .collect()
    };

    SharedSnapshot {
        equity: *ss.equity.read(),
        initial_equity: *ss.initial_equity.read(),
        peak_equity: *ss.peak_equity.read(),
        realized_pnl_today: *ss.realized_pnl_today.read(),
        unrealized_pnl: *ss.unrealized_pnl.read(),
        total_equity: ss.total_equity(),
        open_positions: *ss.open_positions.read(),
        survival_mode: ss.survival_mode.read().as_str().to_string(),
        survival_score: *ss.survival_score.read(),
        drawdown_pct: *ss.drawdown_pct.read(),
        current_regime: ss.current_regime.read().clone(),
        strategy_health,
    }
}

/// Build a ConfigSummary from the full Config (safe subset, no secrets).
pub fn build_config_summary(cfg: &Config) -> ConfigSummary {
    let active_strategies: Vec<String> = cfg
        .strategy
        .active
        .iter()
        .map(|s| s.to_string())
        .collect();

    ConfigSummary {
        mode: cfg.mode.run_mode.clone(),
        exchange: cfg.exchange.name.clone(),
        symbol_count: cfg.pairs.symbols.len(),
        max_leverage: cfg.risk.max_leverage,
        risk_per_trade_pct: cfg.risk.risk_per_trade_pct,
        max_drawdown_pct: cfg.risk.max_drawdown_pct,
        max_open_positions: cfg.risk.max_open_positions,
        partial_tp_enabled: true, // Default from PositionConfig
        max_hold_secs: cfg.risk.max_hold_secs,
        metrics_bind: cfg.monitoring.metrics_bind.clone(),
        active_strategies,
    }
}

// ── Event Bridge ──────────────────────────────────────────────────────

/// Spawn a task that listens to the MessageBus and converts relevant events
/// into API events for SSE streaming.
pub fn spawn_event_bridge(bus: MessageBus, api_state: ApiState) -> JoinHandle<()> {
    let mut rx = bus.subscribe();

    tokio::spawn(async move {
        loop {
            let ev = match rx.recv().await {
                Ok(ev) => ev,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    warn!(skipped = n, "api event bridge lagged");
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            };

            match ev {
                AgentEvent::PreSignalEmitted { signal, regime } => {
                    let entry = SignalEntry {
                        signal_id: signal.signal_id.clone(),
                        symbol: signal.symbol.clone(),
                        side: signal.side.as_str().to_string(),
                        strategy: signal.strategy.as_str().to_string(),
                        ta_confidence: signal.ta_confidence,
                        entry: signal.entry,
                        stop_loss: signal.stop_loss,
                        take_profit: signal.take_profit,
                        regime: regime.as_str().to_string(),
                        reason: signal.reason.clone(),
                        ts: chrono::Utc::now().timestamp(),
                    };
                    record_signal(&api_state, &entry);
                    broadcast_event(
                        &api_state,
                        "signal",
                        serde_json::to_value(&entry).unwrap_or_default(),
                    );
                }
                AgentEvent::OrderFilled {
                    signal_id,
                    symbol,
                    side,
                    size,
                    ack,
                    ..
                } => {
                    broadcast_event(
                        &api_state,
                        "fill",
                        serde_json::json!({
                            "signal_id": signal_id,
                            "symbol": symbol,
                            "side": side.as_str(),
                            "size": size,
                            "fill_price": ack.avg_fill_price,
                            "ts": chrono::Utc::now().timestamp(),
                        }),
                    );
                }
                AgentEvent::PositionClosed {
                    signal_id,
                    symbol,
                    side,
                    entry_price,
                    exit_price,
                    pnl_usd,
                    reason,
                    ..
                } => {
                    broadcast_event(
                        &api_state,
                        "close",
                        serde_json::json!({
                            "signal_id": signal_id,
                            "symbol": symbol,
                            "side": side.as_str(),
                            "entry_price": entry_price,
                            "exit_price": exit_price,
                            "pnl_usd": pnl_usd,
                            "reason": reason.as_str(),
                            "ts": chrono::Utc::now().timestamp(),
                        }),
                    );
                }
                AgentEvent::PositionReduced {
                    signal_id,
                    symbol,
                    side,
                    reduced_size,
                    remaining_size,
                    entry_price,
                    exit_price,
                    pnl_usd,
                    reason,
                    ..
                } => {
                    broadcast_event(
                        &api_state,
                        "partial",
                        serde_json::json!({
                            "signal_id": signal_id,
                            "symbol": symbol,
                            "side": side.as_str(),
                            "reduced_size": reduced_size,
                            "remaining_size": remaining_size,
                            "entry_price": entry_price,
                            "exit_price": exit_price,
                            "pnl_usd": pnl_usd,
                            "reason": reason.as_str(),
                            "ts": chrono::Utc::now().timestamp(),
                        }),
                    );
                }
                AgentEvent::StopMoved {
                    signal_id,
                    symbol,
                    old_stop,
                    new_stop,
                    reason,
                    ..
                } => {
                    broadcast_event(
                        &api_state,
                        "sl_moved",
                        serde_json::json!({
                            "signal_id": signal_id,
                            "symbol": symbol,
                            "old_stop": old_stop,
                            "new_stop": new_stop,
                            "reason": reason,
                            "ts": chrono::Utc::now().timestamp(),
                        }),
                    );
                }
                AgentEvent::ScreeningUpdated { symbol, bias, ts } => {
                    update_screening_bias(&api_state, &symbol, &bias);
                    broadcast_event(
                        &api_state,
                        "screening",
                        serde_json::json!({
                            "symbol": symbol,
                            "bias": bias.as_str(),
                            "allows_long": bias.allows(&crate::data::Side::Long),
                            "allows_short": bias.allows(&crate::data::Side::Short),
                            "ts": ts.timestamp(),
                        }),
                    );
                }
                AgentEvent::SurvivalUpdated(survival) => {
                    *api_state.survival.write() = Some(survival.clone());
                    broadcast_event(
                        &api_state,
                        "survival",
                        serde_json::to_value(&survival).unwrap_or_default(),
                    );
                }
                AgentEvent::EquityReconciled { equity_usd, ts } => {
                    broadcast_event(
                        &api_state,
                        "equity",
                        serde_json::json!({
                            "equity_usd": equity_usd,
                            "ts": ts.timestamp(),
                        }),
                    );
                }
                AgentEvent::ExecutionFailed { symbol, reason } => {
                    broadcast_event(
                        &api_state,
                        "error",
                        serde_json::json!({
                            "symbol": symbol,
                            "reason": reason,
                            "ts": chrono::Utc::now().timestamp(),
                        }),
                    );
                }
                _ => {}
            }
        }
    })
}
