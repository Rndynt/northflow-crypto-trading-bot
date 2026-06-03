//! Trade journal — SQLite or PostgreSQL (NeonDB) backed.
//!
//! When `DATABASE_URL` env is set, connects to PostgreSQL (NeonDB).
//! Otherwise falls back to SQLite.
//!
//! PostgreSQL operations run on a dedicated background thread to avoid
//! "Cannot start a runtime from within a runtime" panics (the `postgres`
//! crate internally uses tokio-postgres which conflicts with our tokio).

use crate::errors::Result;
use crate::learning::Lesson;
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc;

// ── SQLite Schema ──────────────────────────────────────────────────────

pub const SQLITE_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS trades (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    client_order_id TEXT UNIQUE NOT NULL,
    signal_id TEXT DEFAULT '',
    user_id INTEGER NOT NULL DEFAULT 7773988648,
    symbol TEXT NOT NULL,
    direction TEXT NOT NULL,
    strategy TEXT NOT NULL,
    market_regime TEXT NOT NULL,
    entry_time DATETIME NOT NULL,
    entry_price REAL NOT NULL,
    size REAL NOT NULL,
    stop_loss REAL NOT NULL,
    take_profit REAL NOT NULL,
    exit_time DATETIME,
    exit_price REAL,
    exit_reason TEXT,
    pnl_usd REAL,
    pnl_pct REAL,
    fees_paid REAL,
    ta_confidence INTEGER,
    rsi REAL,
    adx REAL,
    vwap_delta_pct REAL,
    ema_alignment TEXT,
    llm_model TEXT,
    llm_decision TEXT,
    llm_confidence INTEGER,
    llm_ta_score INTEGER,
    llm_sentiment_score INTEGER,
    llm_fundamental_score INTEGER,
    llm_composite INTEGER,
    llm_summary TEXT,
    llm_ta_analysis TEXT,
    llm_sentiment TEXT,
    llm_fundamental TEXT,
    llm_risks TEXT,
    llm_invalidation TEXT,
    llm_latency_ms INTEGER,
    fear_greed INTEGER,
    social_sentiment REAL,
    news_score REAL,
    funding_rate REAL,
    exchange_flow_btc REAL,
    top_news_titles TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_trades_symbol     ON trades(symbol);
CREATE INDEX IF NOT EXISTS idx_trades_entry_time ON trades(entry_time);
CREATE INDEX IF NOT EXISTS idx_trades_strategy   ON trades(strategy);
CREATE INDEX IF NOT EXISTS idx_trades_llm_dec    ON trades(llm_decision);

CREATE TABLE IF NOT EXISTS llm_decisions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts DATETIME NOT NULL,
    symbol TEXT NOT NULL,
    strategy TEXT NOT NULL,
    regime TEXT NOT NULL,
    direction TEXT NOT NULL,
    ta_confidence INTEGER,
    llm_decision TEXT,
    llm_confidence INTEGER,
    composite_score INTEGER,
    summary TEXT,
    raw_json TEXT,
    latency_ms INTEGER,
    offline_fallback INTEGER DEFAULT 0
);
"#;

// ── PostgreSQL Schema ──────────────────────────────────────────────────

const PG_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS trades (
    id BIGSERIAL PRIMARY KEY,
    client_order_id TEXT UNIQUE NOT NULL,
    signal_id TEXT DEFAULT '',
    user_id BIGINT NOT NULL DEFAULT 7773988648,
    symbol TEXT NOT NULL,
    direction TEXT NOT NULL,
    strategy TEXT NOT NULL,
    market_regime TEXT NOT NULL,
    entry_time TIMESTAMPTZ NOT NULL,
    entry_price DOUBLE PRECISION NOT NULL,
    size DOUBLE PRECISION NOT NULL,
    stop_loss DOUBLE PRECISION NOT NULL,
    take_profit DOUBLE PRECISION NOT NULL,
    exit_time TIMESTAMPTZ,
    exit_price DOUBLE PRECISION,
    exit_reason TEXT,
    pnl_usd DOUBLE PRECISION,
    pnl_pct DOUBLE PRECISION,
    fees_paid DOUBLE PRECISION,
    ta_confidence SMALLINT,
    rsi DOUBLE PRECISION,
    adx DOUBLE PRECISION,
    vwap_delta_pct DOUBLE PRECISION,
    ema_alignment TEXT,
    llm_model TEXT,
    llm_decision TEXT,
    llm_confidence SMALLINT,
    llm_ta_score SMALLINT,
    llm_sentiment_score SMALLINT,
    llm_fundamental_score SMALLINT,
    llm_composite SMALLINT,
    llm_summary TEXT,
    llm_ta_analysis TEXT,
    llm_sentiment TEXT,
    llm_fundamental TEXT,
    llm_risks TEXT,
    llm_invalidation TEXT,
    llm_latency_ms BIGINT,
    fear_greed SMALLINT,
    social_sentiment DOUBLE PRECISION,
    news_score DOUBLE PRECISION,
    funding_rate DOUBLE PRECISION,
    exchange_flow_btc DOUBLE PRECISION,
    top_news_titles TEXT,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_trades_symbol     ON trades(symbol);
CREATE INDEX IF NOT EXISTS idx_trades_entry_time ON trades(entry_time);
CREATE INDEX IF NOT EXISTS idx_trades_strategy   ON trades(strategy);
CREATE INDEX IF NOT EXISTS idx_trades_llm_dec    ON trades(llm_decision);
CREATE INDEX IF NOT EXISTS idx_trades_user_id    ON trades(user_id);

CREATE TABLE IF NOT EXISTS llm_decisions (
    id BIGSERIAL PRIMARY KEY,
    ts TIMESTAMPTZ NOT NULL,
    symbol TEXT NOT NULL,
    strategy TEXT NOT NULL,
    regime TEXT NOT NULL,
    direction TEXT NOT NULL,
    ta_confidence SMALLINT,
    llm_decision TEXT,
    llm_confidence SMALLINT,
    composite_score SMALLINT,
    summary TEXT,
    raw_json TEXT,
    latency_ms BIGINT,
    offline_fallback BOOLEAN DEFAULT FALSE
);
"#;

// ── TradeRecord ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRecord {
    pub client_order_id: String,
    /// Signal ID that originated this trade (e.g. "S-00001").
    #[serde(default)]
    pub signal_id: String,
    pub symbol: String,
    pub direction: String,
    pub strategy: String,
    pub market_regime: String,
    pub entry_time: DateTime<Utc>,
    pub entry_price: f64,
    pub size: f64,
    pub stop_loss: f64,
    pub take_profit: f64,
    pub exit_time: Option<DateTime<Utc>>,
    pub exit_price: Option<f64>,
    pub exit_reason: Option<String>,
    pub pnl_usd: Option<f64>,
    pub pnl_pct: Option<f64>,
    pub fees_paid: Option<f64>,

    pub ta_confidence: Option<u8>,
    pub rsi: Option<f64>,
    pub adx: Option<f64>,
    pub vwap_delta_pct: Option<f64>,
    pub ema_alignment: Option<String>,

    pub llm_model: Option<String>,
    pub llm_decision: Option<String>,
    pub llm_confidence: Option<u8>,
    pub llm_ta_score: Option<u8>,
    pub llm_sentiment_score: Option<u8>,
    pub llm_fundamental_score: Option<u8>,
    pub llm_composite: Option<u8>,
    pub llm_summary: Option<String>,
    pub llm_ta_analysis: Option<String>,
    pub llm_sentiment: Option<String>,
    pub llm_fundamental: Option<String>,
    pub llm_risks: Option<String>,
    pub llm_invalidation: Option<String>,
    pub llm_latency_ms: Option<u64>,

    pub fear_greed: Option<u8>,
    pub social_sentiment: Option<f64>,
    pub news_score: Option<f64>,
    pub funding_rate: Option<f64>,
    pub top_news_titles: Option<String>,

    /// Telegram user ID for per-user journaling.
    #[serde(default = "default_user_id")]
    pub user_id: i64,
}

fn default_user_id() -> i64 {
    7773988648
}

// ── ClosedTrade (compact view for learning) ────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClosedTrade {
    pub signal_id: String,
    pub symbol: String,
    pub direction: String,
    pub strategy: String,
    pub regime: String,
    pub entry_time: DateTime<Utc>,
    pub exit_time: DateTime<Utc>,
    pub pnl_usd: f64,
    pub pnl_pct: f64,
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

impl ClosedTrade {
    pub fn is_win(&self) -> bool {
        self.pnl_usd > 0.0
    }
}

struct PgCloseUpdate<'a> {
    client_id: &'a str,
    exit_time: DateTime<Utc>,
    exit_price: f64,
    exit_reason: &'a str,
    pnl_usd: f64,
    pnl_pct: f64,
    fees: f64,
}

// ── PostgreSQL channel worker ──────────────────────────────────────────

/// Requests sent to the dedicated PG worker thread.
enum PgReq {
    Insert(Box<TradeRecord>),
    Close {
        client_id: String,
        exit_time: DateTime<Utc>,
        exit_price: f64,
        exit_reason: String,
        pnl_usd: f64,
        pnl_pct: f64,
        fees: f64,
    },
    LogLlm {
        symbol: String,
        strategy: String,
        regime: String,
        direction: String,
        ta_confidence: u8,
        llm_decision: String,
        llm_confidence: u8,
        composite_score: u8,
        summary: String,
        raw_json: String,
        latency_ms: u64,
        offline_fallback: bool,
    },
    RecentPnl(mpsc::SyncSender<f64>),
    TradeCount(mpsc::SyncSender<i64>),
    ClosedTrades(i64, mpsc::SyncSender<Vec<ClosedTrade>>),
    MaxSignalId(mpsc::SyncSender<u64>),
}

/// Spawn a background thread owning a `postgres::Client`.
/// Returns a channel sender for dispatching requests.
fn spawn_pg_worker(url: String) -> Result<mpsc::SyncSender<PgReq>> {
    let (init_tx, init_rx) = mpsc::sync_channel::<Result<()>>(1);
    let (req_tx, req_rx) = mpsc::sync_channel::<PgReq>(128);

    std::thread::Builder::new()
        .name("pg-worker".into())
        .spawn(move || {
            use native_tls::TlsConnector;
            use postgres_native_tls::MakeTlsConnector;

            // Connect outside tokio
            let tls = match TlsConnector::builder().build() {
                Ok(t) => t,
                Err(e) => {
                    let _ = init_tx.send(Err(crate::errors::ScalperError::Postgres(e.to_string())));
                    return;
                }
            };
            let connector = MakeTlsConnector::new(tls);
            let mut client = match postgres::Client::connect(&url, connector) {
                Ok(c) => c,
                Err(e) => {
                    let _ = init_tx.send(Err(crate::errors::ScalperError::Postgres(e.to_string())));
                    return;
                }
            };
            if let Err(e) = client.batch_execute(PG_SCHEMA) {
                let _ = init_tx.send(Err(crate::errors::ScalperError::Postgres(e.to_string())));
                return;
            }
            let _ = init_tx.send(Ok(()));

            // Process loop
            while let Ok(req) = req_rx.recv() {
                match req {
                    PgReq::Insert(t) => {
                        let _ = pg_insert(&mut client, &t);
                    }
                    PgReq::Close {
                        client_id,
                        exit_time,
                        exit_price,
                        exit_reason,
                        pnl_usd,
                        pnl_pct,
                        fees,
                    } => {
                        let _ = pg_close(
                            &mut client,
                            PgCloseUpdate {
                                client_id: &client_id,
                                exit_time,
                                exit_price,
                                exit_reason: &exit_reason,
                                pnl_usd,
                                pnl_pct,
                                fees,
                            },
                        );
                    }
                    PgReq::LogLlm {
                        symbol,
                        strategy,
                        regime,
                        direction,
                        ta_confidence,
                        llm_decision,
                        llm_confidence,
                        composite_score,
                        summary,
                        raw_json,
                        latency_ms,
                        offline_fallback,
                    } => {
                        let _ = pg_log_llm(
                            &mut client,
                            &symbol,
                            &strategy,
                            &regime,
                            &direction,
                            ta_confidence,
                            &llm_decision,
                            llm_confidence,
                            composite_score,
                            &summary,
                            &raw_json,
                            latency_ms,
                            offline_fallback,
                        );
                    }
                    PgReq::RecentPnl(tx) => {
                        let v = pg_recent_pnl(&mut client).unwrap_or(0.0);
                        let _ = tx.send(v);
                    }
                    PgReq::TradeCount(tx) => {
                        let v = pg_trade_count(&mut client).unwrap_or(0);
                        let _ = tx.send(v);
                    }
                    PgReq::ClosedTrades(limit, tx) => {
                        let v = pg_closed_trades(&mut client, limit).unwrap_or_default();
                        let _ = tx.send(v);
                    }
                    PgReq::MaxSignalId(tx) => {
                        let v = pg_max_signal_id(&mut client).unwrap_or(0);
                        let _ = tx.send(v);
                    }
                }
            }
        })
        .map_err(|e| crate::errors::ScalperError::Postgres(e.to_string()))?;

    // Wait for init result
    init_rx
        .recv()
        .map_err(|_| crate::errors::ScalperError::Postgres("pg worker thread died".into()))??;

    Ok(req_tx)
}

// ── Raw postgres operations (run on worker thread) ─────────────────────

fn pg_insert(client: &mut postgres::Client, t: &TradeRecord) -> std::result::Result<(), String> {
    client
        .execute(
            "INSERT INTO trades (
                client_order_id, signal_id, user_id, symbol, direction, strategy, market_regime,
                entry_time, entry_price, size, stop_loss, take_profit,
                exit_time, exit_price, exit_reason, pnl_usd, pnl_pct, fees_paid,
                ta_confidence, rsi, adx, vwap_delta_pct, ema_alignment,
                llm_model, llm_decision, llm_confidence,
                llm_ta_score, llm_sentiment_score, llm_fundamental_score,
                llm_composite, llm_summary, llm_ta_analysis, llm_sentiment,
                llm_fundamental, llm_risks, llm_invalidation, llm_latency_ms,
                fear_greed, social_sentiment, news_score, funding_rate, top_news_titles
            ) VALUES (
                $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,
                $21,$22,$23,$24,$25,$26,$27,$28,$29,$30,$31,$32,$33,$34,$35,$36,$37,$38,
                $39,$40,$41,$42
            )",
            &[
                &t.client_order_id,
                &t.signal_id,
                &t.user_id,
                &t.symbol,
                &t.direction,
                &t.strategy,
                &t.market_regime,
                &t.entry_time,
                &t.entry_price,
                &t.size,
                &t.stop_loss,
                &t.take_profit,
                &t.exit_time as &Option<DateTime<Utc>>,
                &t.exit_price as &Option<f64>,
                &t.exit_reason as &Option<String>,
                &t.pnl_usd as &Option<f64>,
                &t.pnl_pct as &Option<f64>,
                &t.fees_paid as &Option<f64>,
                &t.ta_confidence.map(|v| v as i16) as &Option<i16>,
                &t.rsi as &Option<f64>,
                &t.adx as &Option<f64>,
                &t.vwap_delta_pct as &Option<f64>,
                &t.ema_alignment as &Option<String>,
                &t.llm_model as &Option<String>,
                &t.llm_decision as &Option<String>,
                &t.llm_confidence.map(|v| v as i16) as &Option<i16>,
                &t.llm_ta_score.map(|v| v as i16) as &Option<i16>,
                &t.llm_sentiment_score.map(|v| v as i16) as &Option<i16>,
                &t.llm_fundamental_score.map(|v| v as i16) as &Option<i16>,
                &t.llm_composite.map(|v| v as i16) as &Option<i16>,
                &t.llm_summary as &Option<String>,
                &t.llm_ta_analysis as &Option<String>,
                &t.llm_sentiment as &Option<String>,
                &t.llm_fundamental as &Option<String>,
                &t.llm_risks as &Option<String>,
                &t.llm_invalidation as &Option<String>,
                &t.llm_latency_ms.map(|v| v as i64) as &Option<i64>,
                &t.fear_greed.map(|v| v as i16) as &Option<i16>,
                &t.social_sentiment as &Option<f64>,
                &t.news_score as &Option<f64>,
                &t.funding_rate as &Option<f64>,
                &t.top_news_titles as &Option<String>,
            ],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn pg_close(
    client: &mut postgres::Client,
    update: PgCloseUpdate<'_>,
) -> std::result::Result<(), String> {
    client
        .execute(
            "UPDATE trades SET exit_time=$2, exit_price=$3, exit_reason=$4,
                pnl_usd=$5, pnl_pct=$6, fees_paid=$7
             WHERE client_order_id=$1",
            &[
                &update.client_id,
                &update.exit_time,
                &update.exit_price,
                &update.exit_reason,
                &update.pnl_usd,
                &update.pnl_pct,
                &update.fees,
            ],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn pg_log_llm(
    client: &mut postgres::Client,
    symbol: &str,
    strategy: &str,
    regime: &str,
    direction: &str,
    ta_confidence: u8,
    llm_decision: &str,
    llm_confidence: u8,
    composite_score: u8,
    summary: &str,
    raw_json: &str,
    latency_ms: u64,
    offline_fallback: bool,
) -> std::result::Result<(), String> {
    let now = Utc::now();
    client
        .execute(
            "INSERT INTO llm_decisions (
                ts, symbol, strategy, regime, direction, ta_confidence,
                llm_decision, llm_confidence, composite_score, summary,
                raw_json, latency_ms, offline_fallback
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
            &[
                &now,
                &symbol,
                &strategy,
                &regime,
                &direction,
                &(ta_confidence as i16),
                &llm_decision,
                &(llm_confidence as i16),
                &(composite_score as i16),
                &summary,
                &raw_json,
                &(latency_ms as i64),
                &offline_fallback,
            ],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn pg_recent_pnl(client: &mut postgres::Client) -> std::result::Result<f64, String> {
    let row = client
        .query_opt(
            "SELECT COALESCE(SUM(pnl_usd), 0.0) FROM trades
             WHERE exit_time IS NOT NULL AND exit_time::date = CURRENT_DATE",
            &[],
        )
        .map_err(|e| e.to_string())?;
    Ok(row.map(|r| r.get(0)).unwrap_or(0.0))
}

fn pg_trade_count(client: &mut postgres::Client) -> std::result::Result<i64, String> {
    let row = client
        .query_one("SELECT COUNT(*)::bigint FROM trades", &[])
        .map_err(|e| e.to_string())?;
    Ok(row.get(0))
}

fn pg_closed_trades(
    client: &mut postgres::Client,
    limit: i64,
) -> std::result::Result<Vec<ClosedTrade>, String> {
    let rows = client
        .query(
            "SELECT signal_id, symbol, direction, strategy, market_regime, entry_time, exit_time,
                    pnl_usd, pnl_pct, ta_confidence, llm_confidence,
                    entry_price, exit_price, stop_loss, take_profit, size
             FROM trades
             WHERE exit_time IS NOT NULL AND pnl_usd IS NOT NULL
             ORDER BY exit_time DESC
             LIMIT $1",
            &[&limit],
        )
        .map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        out.push(ClosedTrade {
            signal_id: r.get::<_, Option<String>>(0).unwrap_or_default(),
            symbol: r.get(1),
            direction: r.get(2),
            strategy: r.get(3),
            regime: r.get(4),
            entry_time: r.get(5),
            exit_time: r.get(6),
            pnl_usd: r.get(7),
            pnl_pct: r.get::<_, Option<f64>>(8).unwrap_or(0.0),
            ta_confidence: r.get::<_, Option<i16>>(9).map(|v| v as u8),
            llm_confidence: r.get::<_, Option<i16>>(10).map(|v| v as u8),
            entry_price: r.get::<_, Option<f64>>(11).unwrap_or(0.0),
            exit_price: r.get::<_, Option<f64>>(12).unwrap_or(0.0),
            stop_loss: r.get::<_, Option<f64>>(13).unwrap_or(0.0),
            take_profit: r.get::<_, Option<f64>>(14).unwrap_or(0.0),
            size: r.get::<_, Option<f64>>(15).unwrap_or(0.0),
            partial_taken: false,
            partial_realized_pnl: 0.0,
        });
    }
    Ok(out)
}

fn pg_max_signal_id(client: &mut postgres::Client) -> std::result::Result<u64, String> {
    let row = client
        .query_opt(
            "SELECT signal_id FROM trades WHERE signal_id != '' ORDER BY CAST(SUBSTRING(signal_id FROM 3) AS BIGINT) DESC LIMIT 1",
            &[],
        )
        .map_err(|e| e.to_string())?;
    Ok(row
        .and_then(|r| {
            let s: Option<String> = r.get(0);
            s.and_then(|s| s.strip_prefix("S-").and_then(|n| n.parse::<u64>().ok()))
        })
        .unwrap_or(0))
}

// ── TradeJournal ───────────────────────────────────────────────────────

enum JournalBackend {
    Sqlite(Arc<Mutex<Connection>>),
    Postgres { tx: mpsc::SyncSender<PgReq> },
}

pub struct TradeJournal {
    backend: JournalBackend,
}

impl TradeJournal {
    /// Open a journal — prefers PostgreSQL if DATABASE_URL is set.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        if let Ok(url) = std::env::var("DATABASE_URL") {
            if !url.is_empty() {
                match Self::open_pg(&url) {
                    Ok(j) => return Ok(j),
                    Err(e) => {
                        tracing::warn!(error = %e, "PostgreSQL failed, falling back to SQLite");
                    }
                }
            }
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(SQLITE_SCHEMA)?;
        tracing::info!("TradeJournal opened SQLite");
        Ok(Self {
            backend: JournalBackend::Sqlite(Arc::new(Mutex::new(conn))),
        })
    }

    /// Open an in-memory SQLite journal (for tests).
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SQLITE_SCHEMA)?;
        Ok(Self {
            backend: JournalBackend::Sqlite(Arc::new(Mutex::new(conn))),
        })
    }

    /// Connect to PostgreSQL via a dedicated background thread.
    fn open_pg(url: &str) -> Result<Self> {
        let tx = spawn_pg_worker(url.to_owned())?;
        tracing::info!("TradeJournal connected to PostgreSQL (NeonDB) via worker thread");
        Ok(Self {
            backend: JournalBackend::Postgres { tx },
        })
    }

    /// Open directly with a PostgreSQL URL (no fallback).
    pub fn open_with_url(url: &str) -> Result<Self> {
        Self::open_pg(url)
    }

    // ── insert_trade ───────────────────────────────────────────────────

    pub fn insert_trade(&self, t: &TradeRecord) -> Result<()> {
        match &self.backend {
            JournalBackend::Sqlite(conn) => {
                let conn = conn.lock();
                conn.execute(
                    "INSERT INTO trades (
                        client_order_id, signal_id, user_id, symbol, direction, strategy, market_regime,
                        entry_time, entry_price, size, stop_loss, take_profit,
                        exit_time, exit_price, exit_reason, pnl_usd, pnl_pct, fees_paid,
                        ta_confidence, rsi, adx, vwap_delta_pct, ema_alignment,
                        llm_model, llm_decision, llm_confidence,
                        llm_ta_score, llm_sentiment_score, llm_fundamental_score,
                        llm_composite, llm_summary, llm_ta_analysis, llm_sentiment,
                        llm_fundamental, llm_risks, llm_invalidation, llm_latency_ms,
                        fear_greed, social_sentiment, news_score, funding_rate, top_news_titles
                    ) VALUES (
                        ?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,
                        ?21,?22,?23,?24,?25,?26,?27,?28,?29,?30,?31,?32,?33,?34,?35,?36,?37,?38,
                        ?39,?40,?41,?42
                    )",
                    params![
                        t.client_order_id,
                        t.signal_id,
                        t.user_id,
                        t.symbol,
                        t.direction,
                        t.strategy,
                        t.market_regime,
                        t.entry_time,
                        t.entry_price,
                        t.size,
                        t.stop_loss,
                        t.take_profit,
                        t.exit_time,
                        t.exit_price,
                        t.exit_reason,
                        t.pnl_usd,
                        t.pnl_pct,
                        t.fees_paid,
                        t.ta_confidence,
                        t.rsi,
                        t.adx,
                        t.vwap_delta_pct,
                        t.ema_alignment,
                        t.llm_model,
                        t.llm_decision,
                        t.llm_confidence,
                        t.llm_ta_score,
                        t.llm_sentiment_score,
                        t.llm_fundamental_score,
                        t.llm_composite,
                        t.llm_summary,
                        t.llm_ta_analysis,
                        t.llm_sentiment,
                        t.llm_fundamental,
                        t.llm_risks,
                        t.llm_invalidation,
                        t.llm_latency_ms.map(|x| x as i64),
                        t.fear_greed,
                        t.social_sentiment,
                        t.news_score,
                        t.funding_rate,
                        t.top_news_titles,
                    ],
                )?;
                Ok(())
            }
            JournalBackend::Postgres { tx } => {
                tx.send(PgReq::Insert(Box::new(t.clone())))
                    .map_err(|_| crate::errors::ScalperError::Postgres("pg worker died".into()))?;
                Ok(())
            }
        }
    }

    // ── close_trade ────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    pub fn close_trade(
        &self,
        client_id: &str,
        exit_time: DateTime<Utc>,
        exit_price: f64,
        exit_reason: &str,
        pnl_usd: f64,
        pnl_pct: f64,
        fees: f64,
    ) -> Result<()> {
        match &self.backend {
            JournalBackend::Sqlite(conn) => {
                let conn = conn.lock();
                conn.execute(
                    "UPDATE trades SET exit_time=?2, exit_price=?3, exit_reason=?4,
                        pnl_usd=?5, pnl_pct=?6, fees_paid=?7
                     WHERE client_order_id=?1",
                    params![
                        client_id,
                        exit_time,
                        exit_price,
                        exit_reason,
                        pnl_usd,
                        pnl_pct,
                        fees
                    ],
                )?;
                Ok(())
            }
            JournalBackend::Postgres { tx } => {
                tx.send(PgReq::Close {
                    client_id: client_id.to_string(),
                    exit_time,
                    exit_price,
                    exit_reason: exit_reason.to_string(),
                    pnl_usd,
                    pnl_pct,
                    fees,
                })
                .map_err(|_| crate::errors::ScalperError::Postgres("pg worker died".into()))?;
                Ok(())
            }
        }
    }

    // ── log_llm_decision ───────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    pub fn log_llm_decision(
        &self,
        symbol: &str,
        strategy: &str,
        regime: &str,
        direction: &str,
        ta_confidence: u8,
        llm_decision: &str,
        llm_confidence: u8,
        composite_score: u8,
        summary: &str,
        raw_json: &str,
        latency_ms: u64,
        offline_fallback: bool,
    ) -> Result<()> {
        match &self.backend {
            JournalBackend::Sqlite(conn) => {
                let conn = conn.lock();
                conn.execute(
                    "INSERT INTO llm_decisions (
                        ts, symbol, strategy, regime, direction, ta_confidence,
                        llm_decision, llm_confidence, composite_score, summary,
                        raw_json, latency_ms, offline_fallback
                    ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                    params![
                        Utc::now(),
                        symbol,
                        strategy,
                        regime,
                        direction,
                        ta_confidence,
                        llm_decision,
                        llm_confidence,
                        composite_score,
                        summary,
                        raw_json,
                        latency_ms as i64,
                        offline_fallback as i64,
                    ],
                )?;
                Ok(())
            }
            JournalBackend::Postgres { tx } => {
                tx.send(PgReq::LogLlm {
                    symbol: symbol.to_string(),
                    strategy: strategy.to_string(),
                    regime: regime.to_string(),
                    direction: direction.to_string(),
                    ta_confidence,
                    llm_decision: llm_decision.to_string(),
                    llm_confidence,
                    composite_score,
                    summary: summary.to_string(),
                    raw_json: raw_json.to_string(),
                    latency_ms,
                    offline_fallback,
                })
                .map_err(|_| crate::errors::ScalperError::Postgres("pg worker died".into()))?;
                Ok(())
            }
        }
    }

    // ── recent_pnl ─────────────────────────────────────────────────────

    pub fn recent_pnl(&self) -> Result<f64> {
        match &self.backend {
            JournalBackend::Sqlite(conn) => {
                let conn = conn.lock();
                let v: Option<f64> = conn
                    .query_row(
                        "SELECT SUM(pnl_usd) FROM trades WHERE exit_time IS NOT NULL AND date(exit_time) = date('now')",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap_or(None);
                Ok(v.unwrap_or(0.0))
            }
            JournalBackend::Postgres { tx } => {
                let (resp_tx, resp_rx) = mpsc::sync_channel(1);
                tx.send(PgReq::RecentPnl(resp_tx))
                    .map_err(|_| crate::errors::ScalperError::Postgres("pg worker died".into()))?;
                resp_rx
                    .recv()
                    .map_err(|_| crate::errors::ScalperError::Postgres("pg worker died".into()))
            }
        }
    }

    // ── trade_count ────────────────────────────────────────────────────

    pub fn trade_count(&self) -> Result<i64> {
        match &self.backend {
            JournalBackend::Sqlite(conn) => {
                let conn = conn.lock();
                let v: i64 = conn.query_row("SELECT COUNT(*) FROM trades", [], |r| r.get(0))?;
                Ok(v)
            }
            JournalBackend::Postgres { tx } => {
                let (resp_tx, resp_rx) = mpsc::sync_channel(1);
                tx.send(PgReq::TradeCount(resp_tx))
                    .map_err(|_| crate::errors::ScalperError::Postgres("pg worker died".into()))?;
                resp_rx
                    .recv()
                    .map_err(|_| crate::errors::ScalperError::Postgres("pg worker died".into()))
            }
        }
    }

    // ── closed_trades ──────────────────────────────────────────────────

    /// Compact view of closed trades — used by the learning agent.
    pub fn closed_trades(&self, limit: i64) -> Result<Vec<ClosedTrade>> {
        match &self.backend {
            JournalBackend::Sqlite(conn) => {
                let conn = conn.lock();
                let mut stmt = conn.prepare(
                    "SELECT signal_id, symbol, direction, strategy, market_regime, entry_time, exit_time,
                            pnl_usd, pnl_pct, ta_confidence, llm_confidence,
                            entry_price, exit_price, stop_loss, take_profit, size
                     FROM trades
                     WHERE exit_time IS NOT NULL AND pnl_usd IS NOT NULL
                     ORDER BY exit_time DESC
                     LIMIT ?1",
                )?;
                let rows = stmt.query_map(params![limit], |r| {
                    Ok(ClosedTrade {
                        signal_id: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                        symbol: r.get(1)?,
                        direction: r.get(2)?,
                        strategy: r.get(3)?,
                        regime: r.get(4)?,
                        entry_time: r.get(5)?,
                        exit_time: r.get(6)?,
                        pnl_usd: r.get(7)?,
                        pnl_pct: r.get(8).unwrap_or(0.0),
                        ta_confidence: r.get::<_, Option<i64>>(9)?.map(|v| v as u8),
                        llm_confidence: r.get::<_, Option<i64>>(10)?.map(|v| v as u8),
                        entry_price: r.get::<_, Option<f64>>(11)?.unwrap_or(0.0),
                        exit_price: r.get::<_, Option<f64>>(12)?.unwrap_or(0.0),
                        stop_loss: r.get::<_, Option<f64>>(13)?.unwrap_or(0.0),
                        take_profit: r.get::<_, Option<f64>>(14)?.unwrap_or(0.0),
                        size: r.get::<_, Option<f64>>(15)?.unwrap_or(0.0),
                        partial_taken: false,
                        partial_realized_pnl: 0.0,
                    })
                })?;
                let mut out = Vec::new();
                for r in rows {
                    out.push(r?);
                }
                Ok(out)
            }
            JournalBackend::Postgres { tx } => {
                let (resp_tx, resp_rx) = mpsc::sync_channel(1);
                tx.send(PgReq::ClosedTrades(limit, resp_tx))
                    .map_err(|_| crate::errors::ScalperError::Postgres("pg worker died".into()))?;
                resp_rx
                    .recv()
                    .map_err(|_| crate::errors::ScalperError::Postgres("pg worker died".into()))
            }
        }
    }

    /// Whether this journal is backed by PostgreSQL.
    pub fn is_pg(&self) -> bool {
        matches!(self.backend, JournalBackend::Postgres { .. })
    }

    /// Get the maximum signal ID number from the database for counter initialization.
    /// Returns 0 if no signals exist yet.
    pub fn max_signal_id_number(&self) -> u64 {
        match &self.backend {
            JournalBackend::Sqlite(conn) => {
                let conn = conn.lock();
                let v: Option<String> = conn
                    .query_row(
                        "SELECT signal_id FROM trades WHERE signal_id != '' ORDER BY CAST(SUBSTR(signal_id, 3) AS INTEGER) DESC LIMIT 1",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap_or(None);
                v.and_then(|s| s.strip_prefix("S-").and_then(|n| n.parse::<u64>().ok()))
                    .unwrap_or(0)
            }
            JournalBackend::Postgres { tx } => {
                // For PG, we'll use a sync query via the worker
                let (resp_tx, resp_rx) = mpsc::sync_channel(1);
                let _ = tx.send(PgReq::MaxSignalId(resp_tx));
                resp_rx.recv().unwrap_or(0)
            }
        }
    }

    /// Log a partial close event as a separate trade record.
    /// This ensures partial TP appears in /history with the same signal_id.
    pub fn log_partial_close(
        &self,
        signal_id: &str,
        parent_client_id: &str,
        symbol: &str,
        side: &str,
        strategy: &str,
        regime: &str,
        entry_price: f64,
        exit_price: f64,
        reduced_size: f64,
        pnl_usd: f64,
    ) -> Result<()> {
        let partial_client_id = format!("{}-partial-{}", parent_client_id, Utc::now().timestamp_millis());
        let notional = entry_price * reduced_size;
        let pnl_pct = if notional > 0.0 { pnl_usd / notional * 100.0 } else { 0.0 };
        let record = TradeRecord {
            client_order_id: partial_client_id,
            signal_id: signal_id.to_string(),
            symbol: symbol.to_string(),
            direction: side.to_string(),
            strategy: strategy.to_string(),
            market_regime: regime.to_string(),
            entry_time: Utc::now(),
            entry_price,
            size: reduced_size,
            stop_loss: 0.0,
            take_profit: 0.0,
            exit_time: Some(Utc::now()),
            exit_price: Some(exit_price),
            exit_reason: Some("PARTIAL_TP".to_string()),
            pnl_usd: Some(pnl_usd),
            pnl_pct: Some(pnl_pct),
            fees_paid: Some(0.0),
            ta_confidence: None,
            rsi: None,
            adx: None,
            vwap_delta_pct: None,
            ema_alignment: None,
            llm_model: None,
            llm_decision: None,
            llm_confidence: None,
            llm_ta_score: None,
            llm_sentiment_score: None,
            llm_fundamental_score: None,
            llm_composite: None,
            llm_summary: None,
            llm_ta_analysis: None,
            llm_sentiment: None,
            llm_fundamental: None,
            llm_risks: None,
            llm_invalidation: None,
            llm_latency_ms: None,
            fear_greed: None,
            social_sentiment: None,
            news_score: None,
            funding_rate: None,
            top_news_titles: None,
            user_id: 7773988648,
        };
        self.insert_trade(&record)
    }
}

// ── Learning state persistence (JSON) ──────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LearningStateSnapshot {
    pub lessons_count: usize,
    pub last_refresh_ts: Option<String>,
    pub overall_trades: u32,
    pub overall_wins: u32,
    pub overall_losses: u32,
    pub overall_net_pnl: f64,
    #[serde(default)]
    pub lessons: Vec<Lesson>,
}

const LEARNING_STATE_PATH: &str = "data/learning_state.json";

impl LearningStateSnapshot {
    /// Save current state to JSON.
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = Path::new(LEARNING_STATE_PATH).parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(LEARNING_STATE_PATH, json)?;
        Ok(())
    }

    /// Load state from JSON, returning default if file missing.
    pub fn load() -> Self {
        std::fs::read_to_string(LEARNING_STATE_PATH)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_record() -> TradeRecord {
        TradeRecord {
            client_order_id: "abc".into(),
            symbol: "BTCUSDT".into(),
            direction: "LONG".into(),
            strategy: "ema_ribbon".into(),
            market_regime: "TRENDING_BULLISH".into(),
            entry_time: Utc::now(),
            entry_price: 67240.0,
            size: 0.01,
            stop_loss: 66980.0,
            take_profit: 67510.0,
            exit_time: None,
            exit_price: None,
            exit_reason: None,
            pnl_usd: None,
            pnl_pct: None,
            fees_paid: None,
            ta_confidence: Some(74),
            rsi: Some(61.4),
            adx: Some(28.4),
            vwap_delta_pct: Some(0.42),
            ema_alignment: Some("bull".into()),
            llm_model: Some("claude-3-5-haiku".into()),
            llm_decision: Some("GO".into()),
            llm_confidence: Some(78),
            llm_ta_score: Some(74),
            llm_sentiment_score: Some(72),
            llm_fundamental_score: Some(80),
            llm_composite: Some(74),
            llm_summary: Some("summary".into()),
            llm_ta_analysis: None,
            llm_sentiment: None,
            llm_fundamental: None,
            llm_risks: None,
            llm_invalidation: None,
            llm_latency_ms: Some(820),
            fear_greed: Some(71),
            social_sentiment: Some(0.68),
            news_score: Some(0.72),
            funding_rate: Some(0.0082),
            top_news_titles: Some(r#"["ETF inflow"]"#.into()),
            user_id: 7773988648,
        }
    }

    #[test]
    fn schema_and_insert() {
        let j = TradeJournal::open_memory().unwrap();
        j.insert_trade(&sample_record()).unwrap();
        assert_eq!(j.trade_count().unwrap(), 1);
    }

    #[test]
    fn close_and_query() {
        let j = TradeJournal::open_memory().unwrap();
        j.insert_trade(&sample_record()).unwrap();
        j.close_trade("abc", Utc::now(), 67400.0, "tp", 16.0, 0.24, 0.5)
            .unwrap();
        let closed = j.closed_trades(10).unwrap();
        assert_eq!(closed.len(), 1);
        assert!(closed[0].is_win());
    }
}
