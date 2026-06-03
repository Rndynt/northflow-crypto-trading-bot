//! Layer 5 — trade journal, Telegram alerts, HTTP metrics dashboard, REST API.

pub mod api;
pub mod chart;
pub mod logger;
pub mod metrics;
pub mod telegram;

pub use api::{
    ApiState, ConfigSummary, PositionEntry, ScreeningBiasEntry, SignalEntry, TradeEntry,
    broadcast_event, build_config_summary, record_signal, spawn_api_server, spawn_event_bridge,
    update_screening_bias,
};
pub use logger::{LearningStateSnapshot, TradeJournal, TradeRecord};
pub use metrics::{
    DashboardState, MetricsSnapshot, MetricsState, spawn_dashboard_server, spawn_metrics_server,
};
pub use telegram::{InlineButton, TelegramNotifier};
