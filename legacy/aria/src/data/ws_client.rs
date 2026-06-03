//! Binance WebSocket client with auto-reconnect.
//!
//! Subscribes to combined streams for the configured symbols and emits
//! strongly-typed events to an async channel. When disconnected, retries with
//! exponential backoff up to a bounded ceiling.

use crate::data::types::Trade;
use anyhow::{Context, anyhow};
use chrono::{TimeZone, Utc};
use futures_util::{SinkExt, StreamExt};
use native_tls::TlsConnector;
use serde::Deserialize;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio_tungstenite::Connector;
use tokio_tungstenite::{connect_async_tls_with_config, tungstenite::Message};
use tracing::{debug, error, info, warn};
use url::Url;

#[derive(Debug)]
pub enum WsEvent {
    Trade {
        symbol: String,
        trade: Trade,
    },
    BookTicker {
        symbol: String,
        best_bid: f64,
        bid_qty: f64,
        best_ask: f64,
        ask_qty: f64,
    },
    DepthUpdate {
        symbol: String,
        bids: Vec<(f64, f64)>,
        asks: Vec<(f64, f64)>,
    },
    Heartbeat,
    Disconnected(String),
}

pub struct WsClient {
    base_url: String,
    symbols: Vec<String>,
}

impl WsClient {
    pub fn new(base_url: impl Into<String>, symbols: Vec<String>) -> Self {
        Self {
            base_url: base_url.into(),
            symbols,
        }
    }

    /// Build the combined-stream URL for this client.
    fn build_url(&self) -> anyhow::Result<Url> {
        let streams: Vec<String> = self
            .symbols
            .iter()
            .flat_map(|s| {
                let lower = s.to_lowercase();
                vec![
                    format!("{lower}@trade"),
                    format!("{lower}@bookTicker"),
                    format!("{lower}@depth20@100ms"),
                ]
            })
            .collect();
        let joined = streams.join("/");
        let url = format!("{}?streams={joined}", self.base_url.trim_end_matches('/'));
        Url::parse(&url).context("invalid ws url")
    }

    /// Run the client indefinitely, reconnecting on failure. Events are sent on
    /// `tx`. Errors are logged; the loop exits only when `tx` is dropped.
    pub async fn run(self, tx: mpsc::Sender<WsEvent>) {
        let mut backoff_ms: u64 = 500;
        loop {
            if tx.is_closed() {
                break;
            }
            let url = match self.build_url() {
                Ok(u) => u,
                Err(e) => {
                    error!(error = %e, "failed to build ws url, aborting");
                    return;
                }
            };

            info!(url = %url, "ws connect");
            // Accept invalid certs — Termux has broken CA chain for Binance
            let connector = TlsConnector::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .expect("Failed to create TLS connector");
            let connector = Some(Connector::NativeTls(connector));
            match connect_async_tls_with_config(url.as_str(), None, false, connector).await {
                Ok((mut stream, _)) => {
                    backoff_ms = 500;
                    loop {
                        tokio::select! {
                            msg = stream.next() => {
                                match msg {
                                    Some(Ok(Message::Text(txt))) => {
                                        if let Err(e) = handle_text(&txt, &tx).await {
                                            debug!(error = %e, "parse error");
                                        }
                                    }
                                    Some(Ok(Message::Ping(p))) => {
                                        let _ = stream.send(Message::Pong(p)).await;
                                    }
                                    Some(Ok(Message::Close(f))) => {
                                        warn!(frame = ?f, "ws closed by peer");
                                        break;
                                    }
                                    Some(Ok(_)) => {}
                                    Some(Err(e)) => {
                                        warn!(error = %e, "ws read error");
                                        break;
                                    }
                                    None => {
                                        warn!("ws stream ended");
                                        break;
                                    }
                                }
                            }
                            _ = sleep(Duration::from_secs(30)) => {
                                // heartbeat ping
                                let _ = stream.send(Message::Ping(vec![])).await;
                                let _ = tx.send(WsEvent::Heartbeat).await;
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "ws connect failed");
                }
            }

            let _ = tx.send(WsEvent::Disconnected("reconnecting".into())).await;
            sleep(Duration::from_millis(backoff_ms)).await;
            backoff_ms = (backoff_ms * 2).min(30_000);
        }
    }
}

#[derive(Debug, Deserialize)]
struct CombinedMsg<T> {
    #[allow(dead_code)]
    stream: String,
    data: T,
}

#[derive(Debug, Deserialize)]
struct BinanceTrade {
    #[serde(rename = "E")]
    event_time_ms: i64,
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "p")]
    price: String,
    #[serde(rename = "q")]
    qty: String,
    #[serde(rename = "m")]
    is_buyer_maker: bool,
}

#[derive(Debug, Deserialize)]
struct BinanceBookTicker {
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "b")]
    best_bid: String,
    #[serde(rename = "B")]
    bid_qty: String,
    #[serde(rename = "a")]
    best_ask: String,
    #[serde(rename = "A")]
    ask_qty: String,
}

#[derive(Debug, Deserialize)]
struct BinanceDepth {
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "b")]
    bids: Vec<[String; 2]>,
    #[serde(rename = "a")]
    asks: Vec<[String; 2]>,
}

async fn handle_text(txt: &str, tx: &mpsc::Sender<WsEvent>) -> anyhow::Result<()> {
    let value: serde_json::Value = serde_json::from_str(txt).context("ws: text is not json")?;
    let stream = value
        .get("stream")
        .and_then(|s| s.as_str())
        .ok_or_else(|| anyhow!("no stream field"))?;

    if stream.ends_with("@trade") {
        let parsed: CombinedMsg<BinanceTrade> =
            serde_json::from_value(value).context("parse trade")?;
        let trade = Trade {
            ts: Utc
                .timestamp_millis_opt(parsed.data.event_time_ms)
                .single()
                .ok_or_else(|| anyhow!("bad ts"))?,
            price: parsed.data.price.parse()?,
            qty: parsed.data.qty.parse()?,
            is_buyer_maker: parsed.data.is_buyer_maker,
        };
        let _ = tx
            .send(WsEvent::Trade {
                symbol: parsed.data.symbol,
                trade,
            })
            .await;
    } else if stream.ends_with("@bookTicker") {
        let parsed: CombinedMsg<BinanceBookTicker> =
            serde_json::from_value(value).context("parse book ticker")?;
        let _ = tx
            .send(WsEvent::BookTicker {
                symbol: parsed.data.symbol,
                best_bid: parsed.data.best_bid.parse()?,
                bid_qty: parsed.data.bid_qty.parse()?,
                best_ask: parsed.data.best_ask.parse()?,
                ask_qty: parsed.data.ask_qty.parse()?,
            })
            .await;
    } else if stream.contains("@depth") {
        let parsed: CombinedMsg<BinanceDepth> =
            serde_json::from_value(value).context("parse depth")?;
        let bids: Vec<(f64, f64)> = parsed
            .data
            .bids
            .iter()
            .filter_map(|b| {
                let price = b[0].parse::<f64>().ok()?;
                let qty = b[1].parse::<f64>().ok()?;
                Some((price, qty))
            })
            .collect();
        let asks: Vec<(f64, f64)> = parsed
            .data
            .asks
            .iter()
            .filter_map(|a| {
                let price = a[0].parse::<f64>().ok()?;
                let qty = a[1].parse::<f64>().ok()?;
                Some((price, qty))
            })
            .collect();
        let _ = tx
            .send(WsEvent::DepthUpdate {
                symbol: parsed.data.symbol,
                bids,
                asks,
            })
            .await;
    }
    Ok(())
}
