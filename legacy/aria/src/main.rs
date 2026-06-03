//! ARIA — Autonomous Realtime Intelligence Analyst
//!
//! Top-level binary. Loads config and starts the multi-agent runtime:
//! every layer of the stack runs as an independent tokio task that
//! communicates exclusively over a typed `MessageBus`. The
//! `TraderManagerAgent` (when enabled) sits between the brain and the
//! exchange and gives the final approve / veto / adjust verdict.

use anyhow::{Context, Result};
use crypto_scalper::{
    agents::messages::ControlCommand,
    agents::{
        bus::MessageBus, control::ControlAgentDeps, execution::ExecutionAgentDeps,
        manager::ManagerAgentConfig, messages::AgentEvent, risk::RiskAgentConfig,
        survival::SurvivalAgentDeps, watchdog::WatchdogConfig,
    },
    backtest::{BacktestEngine, load_csv},
    config::Config,
    data::Timeframe,
    execution::{
        Exchange, PaperExchange, PositionBook, RiskManager, binance::BinanceFutures,
        mexc::MexcFutures, position::Position,
    },
    execution::{risk::RiskLimits, tcm::TransactionCostModel},
    feeds::{
        DeribitOptionsClient, ExternalSnapshot, FearGreedClient, FundingClient, NewsClient,
        OnchainClient, SentimentClient,
    },
    learning::{LearningPolicy, lessons::LessonConfig},
    llm::engine::{LlmEngine, LlmEngineConfig, LlmProvider},
    monitoring::{
        DashboardState, MetricsState, TelegramNotifier, logger::TradeJournal,
        spawn_dashboard_server,
    },
    quant::{QuantConfig, QuantEngine},
    research::{ResearchReport, reports_to_json, reports_to_markdown},
    shared_state::SharedState,
    strategy::state::{StrategyName, SymbolState},
};
use parking_lot::RwLock as PlRwLock;
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    // Load `.env` (if any) before tracing so RUST_LOG works too.
    load_dotenv();
    init_tracing();

    let default_path = PathBuf::from("config/default.toml");
    let overlay_path = overlay_path_from_env();
    let cfg = Config::load(&default_path, overlay_path.as_deref())
        .context("failed to load configuration")?;

    info!("starting ARIA");

    match cfg.mode.run_mode.as_str() {
        "backtest" => run_backtest(&cfg).await,
        _ => run_agents(cfg).await,
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .without_time()
        .compact()
        .init();
}

fn overlay_path_from_env() -> Option<PathBuf> {
    std::env::var("ARIA_CONFIG_OVERLAY").ok().map(PathBuf::from)
}

/// Load environment variables from a `.env` file, if present.
///
/// Search order:
/// 1. `ARIA_DOTENV` env var (explicit path)
/// 2. `./.env` (current working directory)
/// 3. The directory containing the binary (handy for symlinked `aria`)
///
/// Lines like `KEY=VALUE` (optionally `export KEY=VALUE`) are parsed.
/// Quoted values (`"..."` or `'...'`) get the quotes stripped. Any
/// variable already present in the process environment is preserved
/// (so a real export still wins over the file).
fn load_dotenv() {
    let candidates: Vec<PathBuf> = {
        let mut v = Vec::new();
        if let Ok(p) = std::env::var("ARIA_DOTENV") {
            v.push(PathBuf::from(p));
        }
        v.push(PathBuf::from(".env"));
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                v.push(dir.join(".env"));
            }
        }
        v
    };

    for path in candidates {
        if !path.is_file() {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        for raw in content.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let line = line.strip_prefix("export ").unwrap_or(line);
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            if key.is_empty() {
                continue;
            }
            let mut val = value.trim().to_string();
            // Strip surrounding quotes if balanced.
            if (val.starts_with('"') && val.ends_with('"') && val.len() >= 2)
                || (val.starts_with('\'') && val.ends_with('\'') && val.len() >= 2)
            {
                val = val[1..val.len() - 1].to_string();
            }
            // Don't overwrite a real export already in the env.
            if std::env::var(key).is_err() {
                unsafe { std::env::set_var(key, val) };
            }
        }
        // Stop at the first .env we successfully parse.
        eprintln!("loaded env from {}", path.display());
        break;
    }
}

async fn run_backtest(cfg: &Config) -> Result<()> {
    let data_dir = PathBuf::from(&cfg.backtest.data_dir);
    if !data_dir.exists() {
        anyhow::bail!("backtest data dir not found: {}", data_dir.display());
    }

    let active: Vec<StrategyName> = cfg
        .strategy
        .active
        .iter()
        .filter_map(|s| StrategyName::parse(s))
        .collect();
    let timeframes: Vec<Timeframe> = cfg
        .pairs
        .timeframes
        .iter()
        .filter_map(|s| Timeframe::parse(s).ok())
        .collect();
    let entry_timeframe = timeframes
        .first()
        .copied()
        .unwrap_or(Timeframe { seconds: 300 });

    let mut reports = Vec::new();
    for symbol in &cfg.pairs.symbols {
        let file = data_dir.join(format!("{symbol}.csv"));
        if !file.exists() {
            warn!(csv = %file.display(), "missing backtest csv — skipping");
            continue;
        }
        let candles = load_csv(&file, entry_timeframe.seconds)?;
        let engine = BacktestEngine {
            symbol: symbol.clone(),
            active: active.clone(),
            min_ta_confidence: cfg.strategy.min_ta_confidence,
            risk_per_trade_usd: cfg.risk.equity_usd * cfg.risk.risk_per_trade_pct / 100.0,
            fee_bps: cfg.backtest.fee_bps,
            slippage_bps: cfg.backtest.slippage_bps,
            market_impact_bps: cfg.backtest.market_impact_bps,
            min_reward_risk: cfg.risk.min_reward_risk,
            max_position_notional_pct: cfg.risk.max_position_notional_pct,
            min_net_edge_bps: cfg.risk.min_net_edge_bps,
            assumed_daily_volume_usd: cfg.risk.assumed_daily_volume_usd,
            equity_usd: cfg.risk.equity_usd,
            trading_days_per_year: cfg.backtest.trading_days_per_year,
            trades_per_day: cfg.backtest.trades_per_day,
        };
        let result = engine.run(&candles)?;
        reports.push(ResearchReport::from_backtest(&result));
        info!(
            symbol = %symbol,
            trades = result.trades.len(),
            win_rate = %format!("{:.2}%", result.metrics.win_rate * 100.0),
            pf = %format!("{:.2}", result.metrics.profit_factor),
            net = %format!("{:.2}", result.metrics.net_pnl),
            "backtest symbol done"
        );
    }
    if !reports.is_empty() {
        let format =
            std::env::var("ARIA_RESEARCH_REPORT_FORMAT").unwrap_or_else(|_| "markdown".into());
        match format.as_str() {
            "json" => println!("{}", reports_to_json(&reports)),
            _ => println!("{}", reports_to_markdown(&reports)),
        }
    }
    Ok(())
}

/// Spawn the full multi-agent runtime: data, feeds, signal, risk,
/// brain, manager, execution, monitor, and the periodic learning
/// refresh agent. Exits cleanly on Ctrl-C by broadcasting `Shutdown`.
async fn run_agents(cfg: Config) -> Result<()> {
    // Capacity 65536: at 300 events/sec (3 symbols × 100 ticks/s) this gives
    // ~3.5 minutes of buffer before lagging — vs 13 seconds at 4096.
    let bus = MessageBus::new(65536);

    // --- Exchange ---
    let exchange: Arc<dyn Exchange> = if cfg.mode.run_mode == "live" && !cfg.mode.dry_run {
        match cfg.exchange.name.to_ascii_lowercase().as_str() {
            "mexc" | "mexc-futures" => {
                info!("live mode — dispatching real orders to MEXC Futures");
                Arc::new(MexcFutures::new(
                    cfg.exchange.rest_base_url.clone(),
                    cfg.exchange.api_key.clone(),
                    cfg.exchange.api_secret.clone(),
                    cfg.exchange.recv_window_ms,
                    &cfg.exchange.open_type,
                    cfg.exchange.leverage,
                ))
            }
            _ => {
                info!("live mode — dispatching real orders to Binance Futures");
                Arc::new(BinanceFutures::new(
                    cfg.exchange.rest_base_url.clone(),
                    cfg.exchange.api_key.clone(),
                    cfg.exchange.api_secret.clone(),
                    cfg.exchange.recv_window_ms,
                ))
            }
        }
    } else {
        info!("paper mode");
        Arc::new(PaperExchange::new(2.0, cfg.risk.equity_usd))
    };

    // --- Risk manager (shared between RiskAgent + ExecutionAgent) ---
    let risk = Arc::new(RiskManager::new(
        RiskLimits {
            risk_per_trade_pct: cfg.risk.risk_per_trade_pct,
            max_open_positions: cfg.risk.max_open_positions,
            max_daily_loss_pct: cfg.risk.max_daily_loss_pct,
            max_drawdown_pct: cfg.risk.max_drawdown_pct,
            max_leverage: cfg.risk.max_leverage,
            max_spread_pct: cfg.risk.max_spread_pct,
            min_reward_risk: cfg.risk.min_reward_risk,
            max_position_notional_pct: cfg.risk.max_position_notional_pct,
            min_net_edge_bps: cfg.risk.min_net_edge_bps,
            assumed_daily_volume_usd: cfg.risk.assumed_daily_volume_usd,
            min_margin_usd: cfg.risk.min_margin_usd,
        },
        cfg.risk.equity_usd,
    ));

    risk.load_equity_from_disk(); // Restore persisted equity (paper mode)
    let book = Arc::new(PositionBook::new());
    book.load_from_disk(); // Restore persisted positions (paper mode)
    let journal = Arc::new(TradeJournal::open(&cfg.monitoring.db_path)?);
    // Build optional signal topic destination from env or config.
    let signal_topic = {
        let group_id = std::env::var("TELEGRAM_GROUP_ID")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| cfg.monitoring.telegram_group_id.clone());
        let thread_id = std::env::var("TELEGRAM_SIGNAL_TOPIC_ID")
            .ok()
            .and_then(|s| s.parse::<i64>().ok())
            .or(cfg.monitoring.telegram_signal_topic_id);
        if !group_id.is_empty() {
            thread_id.map(
                |tid| crypto_scalper::monitoring::telegram::TgDestination::Topic {
                    chat_id: group_id,
                    thread_id: tid,
                },
            )
        } else {
            None
        }
    };

    let telegram = Arc::new(TelegramNotifier::new(
        cfg.monitoring.telegram_bot_token.clone(),
        std::env::var("TELEGRAM_CHAT_ID")
            .unwrap_or_else(|_| cfg.monitoring.telegram_chat_id.clone()),
        signal_topic,
    ));

    let metrics = MetricsState::new(&cfg.mode.run_mode);
    let bind = cfg
        .monitoring
        .metrics_bind
        .parse::<std::net::SocketAddr>()
        .context("invalid metrics bind")?;

    // --- Brain LLM ---
    let provider = LlmProvider::parse(&cfg.llm.provider);
    info!(
        provider = %cfg.llm.provider,
        model = %cfg.llm.model,
        api_base = %cfg.llm.api_base,
        key_set = !cfg.llm.api_key.is_empty(),
        "LLM ready"
    );
    let llm = Arc::new(LlmEngine::new(LlmEngineConfig {
        provider,
        api_key: cfg.llm.api_key.clone(),
        api_base: cfg.llm.api_base.clone(),
        model: cfg.llm.model.clone(),
        timeout_secs: cfg.llm.timeout_secs,
        max_tokens: cfg.llm.max_tokens,
        fallback_ta_threshold: cfg.llm.fallback_ta_threshold,
        http_referer: Some(cfg.llm.http_referer.clone()).filter(|s| !s.is_empty()),
        http_app_title: Some(cfg.llm.http_app_title.clone()).filter(|s| !s.is_empty()),
    }));

    // --- Feeds ---
    let fear_greed = Arc::new(FearGreedClient::new());
    let funding = Arc::new(FundingClient::new(cfg.exchange.rest_base_url.clone()));
    let news = Arc::new(NewsClient::new(
        Some(cfg.feeds.cryptopanic_api_key.clone()).filter(|s| !s.is_empty()),
        cfg.feeds.rss_feeds.clone(),
    ));
    let sentiment = Arc::new(SentimentClient::new(
        Some(cfg.feeds.lunarcrush_api_key.clone()).filter(|s| !s.is_empty()),
    ));
    let onchain = Arc::new(OnchainClient::new(
        Some(cfg.feeds.glassnode_api_key.clone()).filter(|s| !s.is_empty()),
        Some(cfg.feeds.whalealert_api_key.clone()).filter(|s| !s.is_empty()),
    ));
    let options = Arc::new(DeribitOptionsClient::new(
        cfg.feeds.deribit_base_url.clone(),
    ));

    // --- Per-symbol state (owned by SignalAgent, read by BrainAgent) ---
    let timeframes: Vec<Timeframe> = cfg
        .pairs
        .timeframes
        .iter()
        .filter_map(|s| Timeframe::parse(s).ok())
        .collect();
    let entry_timeframe = timeframes
        .first()
        .copied()
        .unwrap_or(Timeframe { seconds: 300 });
    let mut states_map: HashMap<String, SymbolState> = HashMap::new();
    for s in &cfg.pairs.symbols {
        states_map.insert(s.clone(), SymbolState::new(s));
    }
    let states = Arc::new(Mutex::new(states_map));

    // --- Bootstrap historical candles so indicators are warm on day-1 ---
    // Without this, EMA200 needs 200 live candles (= 16h+ at 5m) before
    // EmaRibbon can fire, and ADX needs ~28 candles before RegimeDetector
    // can classify anything other than Unknown.
    {
        crypto_scalper::data::bootstrap_states_for_timeframes(
            &states,
            &cfg.exchange.rest_base_url,
            &timeframes,
        )
        .await;
    }

    let active: Vec<StrategyName> = cfg
        .strategy
        .active
        .iter()
        .filter_map(|s| StrategyName::parse(s))
        .collect();

    let policy = LearningPolicy::default();
    let feeds_cache: Arc<PlRwLock<HashMap<String, ExternalSnapshot>>> =
        Arc::new(PlRwLock::new(HashMap::new()));
    let survival_state: Arc<PlRwLock<Option<crypto_scalper::agents::SurvivalState>>> =
        Arc::new(PlRwLock::new(None));

    // --- Dashboard server ---
    let _metrics_handle = spawn_dashboard_server(
        DashboardState {
            metrics: Arc::clone(&metrics),
            policy: Some(policy.clone()),
            survival: Arc::clone(&survival_state),
        },
        bind,
    );

    // Forward SurvivalUpdated events to the dashboard's snapshot.
    {
        let bus_sub = bus.clone();
        let survival_state = Arc::clone(&survival_state);
        tokio::spawn(async move {
            let mut rx = bus_sub.subscribe();
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
                    crypto_scalper::agents::messages::AgentEvent::SurvivalUpdated(s) => {
                        *survival_state.write() = Some(s);
                    }
                    crypto_scalper::agents::messages::AgentEvent::Shutdown => break,
                    _ => {}
                }
            }
        });
    }

    risk.load_equity_from_disk(); // Restore persisted equity (paper mode)
    let book = Arc::new(PositionBook::new());
    book.load_from_disk(); // Restore persisted positions (paper mode)

    // --- SharedState for cross-agent coordination ---
    let shared_state = SharedState::new(cfg.risk.equity_usd, cfg.risk.max_open_positions as u64);
    shared_state.sync_from_persisted(); // Sync SharedState with persisted equity
    // Initialize signal counter from DB so sequential IDs survive restarts
    let max_sig = journal.max_signal_id_number();
    shared_state.set_signal_counter(max_sig);
    info!(counter = max_sig, "signal_id counter initialized from DB");
    info!("SharedState initialized — all agents will coordinate through shared context");

    reconcile_startup_positions(
        &cfg,
        Arc::clone(&exchange),
        Arc::clone(&book),
        Arc::clone(&risk),
        Arc::clone(&shared_state),
        &bus,
    )
    .await;

    // --- Full REST + SSE API server ---
    // Runs on metrics_bind port + 1 to avoid conflict with old dashboard.
    {
        let api_port = bind.port() + 1;
        let api_bind: std::net::SocketAddr = format!("0.0.0.0:{}", api_port).parse().unwrap();
        let (event_tx, _) = tokio::sync::broadcast::channel(1024);
        let api_state = crypto_scalper::monitoring::api::ApiState {
            metrics: Arc::clone(&metrics),
            survival: Arc::clone(&survival_state),
            policy: Some(Arc::new(policy.clone())),
            book: Arc::clone(&book),
            risk: Arc::clone(&risk),
            journal: Some(Arc::clone(&journal)),
            shared_state: Arc::clone(&shared_state),
            config_summary: crypto_scalper::monitoring::api::build_config_summary(&cfg),
            event_tx: event_tx.clone(),
            screening_bias: Arc::new(PlRwLock::new(HashMap::new())),
            recent_signals: Arc::new(PlRwLock::new(Vec::new())),
            bus: bus.clone(),
        };
        // Spawn event bridge (MessageBus → SSE)
        crypto_scalper::monitoring::api::spawn_event_bridge(bus.clone(), api_state.clone());
        // Spawn API server
        let _api_handle = crypto_scalper::monitoring::api::spawn_api_server(api_state, api_bind);
    }

    // --- Spawn agents ---
    let _data = crypto_scalper::agents::data::spawn(
        bus.clone(),
        crypto_scalper::agents::data::DataAgentConfig {
            ws_base_url: cfg.exchange.ws_base_url.clone(),
            symbols: cfg.pairs.symbols.clone(),
            timeframes: timeframes.clone(),
        },
    );
    let _feeds = crypto_scalper::agents::feeds::spawn(
        bus.clone(),
        crypto_scalper::agents::feeds::FeedsAgentDeps {
            fear_greed,
            funding,
            news,
            sentiment,
            onchain,
            options,
        },
        cfg.pairs.symbols.clone(),
        60,
    );
    // --- Quant Engine (Kelly, vol-target, VaR, IC, Kalman) ---
    let quant_engine = Arc::new(QuantEngine::new(QuantConfig {
        enabled: cfg.quant.enabled,
        kelly_cap: cfg.quant.kelly_cap,
        kelly_min_trades: cfg.quant.kelly_min_trades,
        target_vol_annual: cfg.quant.target_vol_annual,
        max_vol_multiplier: cfg.quant.max_vol_multiplier,
        vol_window: cfg.quant.vol_window,
        var_confidence: cfg.quant.var_confidence,
        max_var_pct: cfg.quant.max_var_pct,
        ic_window: cfg.quant.ic_window,
        ic_min_abs: cfg.quant.ic_min_abs,
        ic_max_boost: cfg.quant.ic_max_boost,
        kalman_process_noise: cfg.quant.kalman_process_noise,
        kalman_measurement_noise: cfg.quant.kalman_measurement_noise,
        kalman_min_velocity_bps: cfg.quant.kalman_min_velocity_bps,
    }));

    // Determine the screening (15m) timeframe — the highest configured timeframe,
    // or 900s (15m) as a fallback. If only one timeframe is configured, the
    // screening layer is effectively disabled (bias stays Unknown).
    let screening_timeframe_secs = timeframes
        .iter()
        .filter(|tf| tf.seconds != entry_timeframe.seconds)
        .map(|tf| tf.seconds)
        .max()
        .unwrap_or(900);

    let _signal = crypto_scalper::agents::signal::spawn(
        bus.clone(),
        Arc::clone(&states),
        crypto_scalper::agents::signal::SignalAgentConfig {
            active: active.clone(),
            schedule: cfg.schedule.clone(),
            advanced_alpha: cfg.advanced_alpha.clone(),
            quant_engine: Some(Arc::clone(&quant_engine)),
            paper_scout_enabled: cfg.mode.run_mode == "paper" && cfg.strategy.paper_scout_enabled,
            entry_timeframe_secs: entry_timeframe.seconds,
            screening_timeframe_secs,
            rest_base_url: cfg.exchange.rest_base_url.clone(),
            symbols: cfg.pairs.symbols.clone(),
        },
        Arc::clone(&shared_state),
    );

    let _risk = crypto_scalper::agents::risk::spawn(
        bus.clone(),
        Arc::clone(&risk),
        policy.clone(),
        RiskAgentConfig {
            base_min_ta_threshold: cfg.strategy.min_ta_confidence,
            base_min_llm_floor: cfg.llm.min_confidence,
            tcm: TransactionCostModel {
                taker_fee_bps: cfg.backtest.fee_bps,
                maker_fee_bps: -1.0,
                avg_slippage_bps: cfg.backtest.slippage_bps,
                market_impact_bps: cfg.backtest.market_impact_bps,
            },
            // Keep the quant Kelly comparator in the same percent units
            // as RiskManager::calculate_size (e.g. 0.5 means 0.5%).
            base_risk_pct: cfg.risk.risk_per_trade_pct,
            ..RiskAgentConfig::default()
        },
        Some(Arc::clone(&quant_engine)),
    );
    let _brain = crypto_scalper::agents::brain::spawn(
        bus.clone(),
        Arc::clone(&llm),
        Arc::clone(&states),
        policy.clone(),
        Arc::clone(&feeds_cache),
        Some(Arc::clone(&shared_state)),
        cfg.mode.fail_closed_without_llm,
        cfg.llm.min_confidence,
    );
    let _manager = crypto_scalper::agents::manager::spawn(
        bus.clone(),
        ManagerAgentConfig {
            enabled: cfg.manager.enabled,
            provider: cfg.manager.provider.clone(),
            api_base: cfg.manager.api_base.clone(),
            api_key: cfg.manager.api_key.clone(),
            model: cfg.manager.model.clone(),
            timeout_secs: cfg.manager.timeout_secs,
            max_tokens: cfg.manager.max_tokens,
            http_referer: Some(cfg.manager.http_referer.clone()).filter(|s| !s.is_empty()),
            http_app_title: Some(cfg.manager.http_app_title.clone()).filter(|s| !s.is_empty()),
            fast_approve_min_conf: cfg.manager.fast_approve_min_conf,
            fail_closed_without_llm: cfg.mode.fail_closed_without_llm,
            fail_open_on_error: cfg.manager.fail_open_on_error,
        },
        policy.clone(),
        Arc::clone(&feeds_cache),
    );
    // Shared position config — can be updated dynamically via /hold command
    let pos_cfg = Arc::new(parking_lot::RwLock::new(
        crypto_scalper::execution::PositionConfig {
            max_hold_secs: cfg.risk.max_hold_secs,
            trail_atr_mult: 0.3,
            trail_activate_r: 1.0,
            // Disable pre-TP breakeven by default. Partial TP at 1R still moves
            // the remaining runner SL to entry; this avoids premature scratch
            // stop-outs before any profit is banked.
            breakeven_r: 0.0,
            partial_tp_enabled: true,
            partial_tp_r: 1.0,
        },
    ));

    let _execution = crypto_scalper::agents::execution::spawn(ExecutionAgentDeps {
        bus: bus.clone(),
        exchange: exchange.clone(),
        risk: Arc::clone(&risk),
        book: Arc::clone(&book),
        honor_survival: cfg.survival.enabled,
        protective_orders_required: cfg.mode.run_mode == "live" && !cfg.mode.dry_run,
        policy: policy.clone(),
        enforce_single_position_per_symbol: cfg.mode.single_position_per_symbol,
        pos_cfg: Arc::clone(&pos_cfg),
    });
    let _monitor = crypto_scalper::agents::monitor::spawn(
        bus.clone(),
        Arc::clone(&metrics),
        Arc::clone(&journal),
        Arc::clone(&telegram),
        cfg.risk.max_leverage as f64,
    );
    let _learning = crypto_scalper::agents::learning::spawn(
        bus.clone(),
        Arc::clone(&journal),
        policy.clone(),
        LessonConfig {
            equity_for_drawdown: cfg.risk.equity_usd,
            ..LessonConfig::default()
        },
        300,
        Some(Arc::clone(&quant_engine)),
        Arc::clone(&shared_state),
        Some(Arc::clone(&llm)),
    );

    let _survival = crypto_scalper::agents::survival::spawn(SurvivalAgentDeps {
        bus: bus.clone(),
        cfg: cfg.survival.clone(),
        exchange: exchange.clone(),
        risk: Arc::clone(&risk),
        initial_equity: cfg.risk.equity_usd,
    });

    // --- Orchestrator Agent (central coordinator) ---
    let orchestrator_state = Arc::new(PlRwLock::new(
        crypto_scalper::agents::orchestrator::OrchestratorState::default(),
    ));
    let _orchestrator = crypto_scalper::agents::orchestrator::spawn(
        bus.clone(),
        crypto_scalper::agents::orchestrator::OrchestratorConfig::default(),
        Some(policy.clone()),
        Arc::clone(&orchestrator_state),
    );

    let _control = crypto_scalper::agents::control::spawn(ControlAgentDeps {
        bus: bus.clone(),
        cfg: cfg.control.clone(),
        telegram_token: cfg.monitoring.telegram_bot_token.clone(),
        telegram_chat_id: cfg.monitoring.telegram_chat_id.clone(),
        telegram_signal_group_id: cfg.monitoring.telegram_group_id.clone(),
        telegram_signal_topic_id: cfg.monitoring.telegram_signal_topic_id,
        risk: Arc::clone(&risk),
        book: Arc::clone(&book),
        exchange: exchange.clone(),
        control_file: Some(PathBuf::from("/tmp/aria.control")),
        metrics: Arc::clone(&metrics),
        survival_state: Arc::clone(&survival_state),
        journal: Some(Arc::clone(&journal)),
        initial_equity: cfg.risk.equity_usd,
        pos_cfg: Arc::clone(&pos_cfg),
    });

    let _watchdog = crypto_scalper::agents::watchdog::spawn(bus.clone(), WatchdogConfig::default());

    let _ = telegram
        .send(&format!(
            "🤖 *ARIA started* — multi-agent mode `{}` (manager: `{}`), pairs: {}",
            cfg.mode.run_mode,
            if cfg.manager.enabled { "ON" } else { "OFF" },
            cfg.pairs.symbols.join(", ")
        ))
        .await;

    info!("runtime live");

    // --- Midnight daily-reset task ---
    // Without this, RiskManager.realized_pnl_today accumulates forever
    // and the daily loss circuit trips permanently after a bad day.
    {
        let bus_reset = bus.clone();
        let risk_reset = Arc::clone(&risk);
        tokio::spawn(async move {
            loop {
                let now = chrono::Utc::now();
                let tomorrow = (now.date_naive() + chrono::Days::new(1))
                    .and_hms_opt(0, 0, 30)
                    .expect("valid midnight")
                    .and_utc();
                let secs = (tomorrow - now).num_seconds().max(1) as u64;
                tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
                risk_reset.reset_daily();
                bus_reset.publish(AgentEvent::ControlCommand(ControlCommand::ResetDaily));
                tracing::info!("midnight UTC: daily risk counters reset");
            }
        });
    }

    // --- Periodic position/equity save (every 30s) ---
    // Ensures data survives even if container is killed without SIGTERM
    {
        let book_periodic = Arc::clone(&book);
        let risk_periodic = Arc::clone(&risk);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                book_periodic.save_to_disk_on_exit();
                risk_periodic.save_equity_to_disk();
            }
        });
    }

    // --- Wait for shutdown ---
    tokio::signal::ctrl_c()
        .await
        .context("failed to listen for ctrl-c")?;
    info!("ctrl-c received — saving positions & equity before exit");
    // Persist positions and equity to disk before agents drain
    book.save_to_disk_on_exit();
    risk.save_equity_to_disk();
    info!("broadcasting shutdown to all agents");
    bus.publish(AgentEvent::Shutdown);
    // Give agents a moment to drain.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    Ok(())
}

async fn reconcile_startup_positions(
    cfg: &Config,
    exchange: Arc<dyn Exchange>,
    book: Arc<PositionBook>,
    risk: Arc<RiskManager>,
    shared_state: Arc<SharedState>,
    bus: &MessageBus,
) {
    if cfg.mode.run_mode != "live" || cfg.mode.dry_run {
        return;
    }

    for sym in &cfg.pairs.symbols {
        if let Err(e) = exchange.set_leverage(sym, cfg.exchange.leverage).await {
            warn!(symbol = %sym, error = %e, "startup: set_leverage failed");
        }
    }
    match exchange.fetch_equity_usd().await {
        Ok(eq) if eq > 0.0 => {
            info!(equity = eq, "startup: equity reconciled");
            risk.set_equity(eq);
        }
        Ok(_) => {}
        Err(e) => warn!(error = %e, "startup: fetch_equity_usd failed"),
    }

    let positions = match exchange.fetch_open_positions(&cfg.pairs.symbols).await {
        Ok(positions) => positions,
        Err(e) => {
            warn!(error = %e, "startup: position reconciliation failed");
            return;
        }
    };
    let mut recovered = Vec::new();
    let now = chrono::Utc::now();
    for snap in positions {
        let (stop_loss, take_profit) =
            recover_protection_prices(exchange.as_ref(), &snap.symbol, snap.side, snap.entry_price)
                .await;
        if stop_loss <= 0.0 || take_profit <= 0.0 {
            let reason = format!(
                "recovered {} {:?} lacks broker SL/TP; freezing new entries",
                snap.symbol, snap.side
            );
            warn!(%reason);
            risk.freeze(reason.clone());
            bus.publish(AgentEvent::ControlCommand(ControlCommand::Freeze {
                reason,
            }));
        }
        let pos = Position {
            client_id: format!(
                "recovered-{}-{}-{}",
                snap.symbol,
                snap.side.as_str(),
                now.timestamp_millis()
            ),
            signal_id: format!(
                "recovered-{}-{}",
                snap.symbol,
                now.timestamp_millis()
            ),
            symbol: snap.symbol.clone(),
            side: snap.side,
            size: snap.size.abs(),
            entry_price: snap.entry_price,
            stop_loss,
            take_profit,
            opened_at: now,
            trailing_activated: false,
            peak_price: snap.mark_price,
            trough_price: snap.mark_price,
            atr_at_entry: 0.0,
            partial_taken: false,
            breakeven_activated: false,
            partial_realized_pnl: 0.0,
            strategy: "recovered".to_string(),
        };
        bus.publish(AgentEvent::PositionRecovered {
            symbol: pos.symbol.clone(),
            side: pos.side,
            size: pos.size,
            entry_price: pos.entry_price,
            stop_loss: pos.stop_loss,
            take_profit: pos.take_profit,
            strategy: pos.strategy.clone(),
        });
        recovered.push(pos);
    }
    let count = recovered.len();
    risk.set_open_positions(count as u32);
    shared_state.set_open_positions(count as u64);
    book.reconcile(recovered);
    if count > 0 {
        warn!(count, "startup: reconciled open positions from exchange");
    } else {
        info!("startup: no open positions to reconcile");
    }
}

async fn recover_protection_prices(
    exchange: &dyn Exchange,
    symbol: &str,
    side: crypto_scalper::data::Side,
    entry: f64,
) -> (f64, f64) {
    let Ok(open_orders) = exchange.fetch_open_orders(symbol).await else {
        return (0.0, 0.0);
    };
    let close_side = match side {
        crypto_scalper::data::Side::Long => crypto_scalper::data::Side::Short,
        crypto_scalper::data::Side::Short => crypto_scalper::data::Side::Long,
    };
    let mut stop_loss = 0.0;
    let mut take_profit = 0.0;
    for order in open_orders
        .into_iter()
        .filter(|o| o.reduce_only && o.side == close_side)
    {
        let Some(stop_price) = order.stop_price else {
            continue;
        };
        match side {
            crypto_scalper::data::Side::Long if stop_price < entry => stop_loss = stop_price,
            crypto_scalper::data::Side::Long if stop_price > entry => take_profit = stop_price,
            crypto_scalper::data::Side::Short if stop_price > entry => stop_loss = stop_price,
            crypto_scalper::data::Side::Short if stop_price < entry => take_profit = stop_price,
            _ => {}
        }
    }
    (stop_loss, take_profit)
}
