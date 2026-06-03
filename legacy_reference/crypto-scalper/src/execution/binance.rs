//! Binance Futures REST client — HMAC-SHA256 signed requests.
//!
//! This is a minimal, defensive implementation: it supports `POST /fapi/v1/order`
//! for market + limit and `DELETE /fapi/v1/order` for cancellation.

use crate::data::Side;
use crate::errors::{Result, ScalperError};
use crate::execution::exchange::{Exchange, OpenOrderSnapshot, OrderAck, PositionSnapshot};
use crate::execution::orders::{OrderRequest, OrderType};
use hmac::{Hmac, Mac};
use reqwest::Client;
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::debug;

type HmacSha256 = Hmac<Sha256>;

pub struct BinanceFutures {
    client: Client,
    base_url: String,
    api_key: String,
    api_secret: String,
    recv_window_ms: u64,
}

impl BinanceFutures {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        api_secret: impl Into<String>,
        recv_window_ms: u64,
    ) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap_or_default(),
            base_url: base_url.into(),
            api_key: api_key.into(),
            api_secret: api_secret.into(),
            recv_window_ms,
        }
    }

    fn timestamp_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }

    fn sign(&self, qs: &str) -> Result<String> {
        let mut mac = HmacSha256::new_from_slice(self.api_secret.as_bytes())
            .map_err(|e| ScalperError::Exchange(format!("hmac: {e}")))?;
        mac.update(qs.as_bytes());
        Ok(hex::encode(mac.finalize().into_bytes()))
    }
}

impl Exchange for BinanceFutures {
    fn name(&self) -> &'static str {
        "binance-futures"
    }

    fn place_order<'a>(
        &'a self,
        req: &'a OrderRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<OrderAck>> + Send + 'a>> {
        Box::pin(async move {
            let side = match req.side {
                crate::data::Side::Long => "BUY",
                crate::data::Side::Short => "SELL",
            };
            let order_type = match req.order_type {
                OrderType::Market => "MARKET",
                OrderType::Limit => "LIMIT",
                OrderType::StopLoss => "STOP_MARKET",
                OrderType::TakeProfit => "TAKE_PROFIT_MARKET",
            };

            let ts = self.timestamp_ms();
            let mut params = vec![
                ("symbol".to_string(), req.symbol.clone()),
                ("side".to_string(), side.to_string()),
                ("type".to_string(), order_type.to_string()),
                ("newClientOrderId".to_string(), req.client_id.clone()),
                ("recvWindow".to_string(), self.recv_window_ms.to_string()),
                ("timestamp".to_string(), ts.to_string()),
            ];
            // For protective orders we use closePosition=true so we
            // don't have to repeat the size — Binance will close the
            // entire open position on trigger. For everything else we
            // pass quantity normally.
            let is_protective =
                matches!(req.order_type, OrderType::StopLoss | OrderType::TakeProfit);
            if is_protective {
                params.push(("closePosition".to_string(), "true".to_string()));
            } else {
                // Round quantity to the correct step size for the symbol.
                // Binance Futures lot sizes (defaults if not fetched from exchange_info):
                //   BTC=0.001, ETH=0.001, SOL=0.1, BNB=0.01, others=0.001
                let qty_str = format_quantity(&req.symbol, req.size);
                params.push(("quantity".to_string(), qty_str));
            }
            if req.reduce_only && !is_protective {
                params.push(("reduceOnly".to_string(), "true".to_string()));
            }
            if let Some(stop_price) = req.stop_price {
                if is_protective {
                    params.push(("stopPrice".to_string(), format!("{:.2}", stop_price)));
                    params.push(("workingType".to_string(), "MARK_PRICE".to_string()));
                    params.push(("priceProtect".to_string(), "true".to_string()));
                }
            }
            if let Some(p) = req.price {
                if matches!(req.order_type, OrderType::Limit) {
                    params.push(("price".to_string(), format!("{:.2}", p)));
                    params.push(("timeInForce".to_string(), "GTC".to_string()));
                }
            }

            let qs = encode_query(&params);
            let sig = self.sign(&qs)?;
            let url = format!(
                "{}/fapi/v1/order?{qs}&signature={sig}",
                self.base_url.trim_end_matches('/')
            );
            debug!(url = %url, "binance place_order");

            let resp = self
                .client
                .post(&url)
                .header("X-MBX-APIKEY", &self.api_key)
                .send()
                .await?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(ScalperError::Exchange(format!(
                    "place_order http {status}: {body}"
                )));
            }
            let body: serde_json::Value = resp.json().await?;
            let exchange_order_id = body
                .get("orderId")
                .map(|v| v.to_string())
                .unwrap_or_default();
            let filled_qty: f64 = body
                .get("executedQty")
                .and_then(|v| v.as_str())
                .unwrap_or("0")
                .parse()
                .unwrap_or(0.0);
            let avg_price: f64 = body
                .get("avgPrice")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(req.price.unwrap_or(0.0));

            Ok(OrderAck {
                client_id: req.client_id.clone(),
                exchange_order_id,
                symbol: req.symbol.clone(),
                filled_qty,
                avg_fill_price: avg_price,
                fee_usd: 0.0,
                ts_ms: ts,
            })
        })
    }

    fn cancel_order<'a>(
        &'a self,
        symbol: &'a str,
        client_id: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let ts = self.timestamp_ms();
            let params = vec![
                ("symbol".to_string(), symbol.to_string()),
                ("origClientOrderId".to_string(), client_id.to_string()),
                ("timestamp".to_string(), ts.to_string()),
                ("recvWindow".to_string(), self.recv_window_ms.to_string()),
            ];
            let qs = encode_query(&params);
            let sig = self.sign(&qs)?;
            let url = format!(
                "{}/fapi/v1/order?{qs}&signature={sig}",
                self.base_url.trim_end_matches('/')
            );
            let resp = self
                .client
                .delete(&url)
                .header("X-MBX-APIKEY", &self.api_key)
                .send()
                .await?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(ScalperError::Exchange(format!(
                    "cancel_order http {status}: {body}"
                )));
            }
            Ok(())
        })
    }

    fn cancel_all<'a>(
        &'a self,
        symbol: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let ts = self.timestamp_ms();
            let params = vec![
                ("symbol".to_string(), symbol.to_string()),
                ("timestamp".to_string(), ts.to_string()),
                ("recvWindow".to_string(), self.recv_window_ms.to_string()),
            ];
            let qs = encode_query(&params);
            let sig = self.sign(&qs)?;
            let url = format!(
                "{}/fapi/v1/allOpenOrders?{qs}&signature={sig}",
                self.base_url.trim_end_matches('/')
            );
            let resp = self
                .client
                .delete(&url)
                .header("X-MBX-APIKEY", &self.api_key)
                .send()
                .await?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(ScalperError::Exchange(format!(
                    "cancel_all http {status}: {body}"
                )));
            }
            Ok(())
        })
    }

    fn set_leverage<'a>(
        &'a self,
        symbol: &'a str,
        leverage: u8,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let ts = self.timestamp_ms();
            let params = vec![
                ("symbol".to_string(), symbol.to_string()),
                ("leverage".to_string(), leverage.to_string()),
                ("timestamp".to_string(), ts.to_string()),
                ("recvWindow".to_string(), self.recv_window_ms.to_string()),
            ];
            let qs = encode_query(&params);
            let sig = self.sign(&qs)?;
            let url = format!(
                "{}/fapi/v1/leverage?{qs}&signature={sig}",
                self.base_url.trim_end_matches('/')
            );
            let resp = self
                .client
                .post(&url)
                .header("X-MBX-APIKEY", &self.api_key)
                .send()
                .await?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(ScalperError::Exchange(format!(
                    "set_leverage http {status}: {body}"
                )));
            }
            Ok(())
        })
    }

    fn fetch_equity_usd<'a>(
        &'a self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<f64>> + Send + 'a>> {
        Box::pin(async move {
            let ts = self.timestamp_ms();
            let params = vec![
                ("timestamp".to_string(), ts.to_string()),
                ("recvWindow".to_string(), self.recv_window_ms.to_string()),
            ];
            let qs = encode_query(&params);
            let sig = self.sign(&qs)?;
            let url = format!(
                "{}/fapi/v2/balance?{qs}&signature={sig}",
                self.base_url.trim_end_matches('/')
            );
            let resp = self
                .client
                .get(&url)
                .header("X-MBX-APIKEY", &self.api_key)
                .send()
                .await?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(ScalperError::Exchange(format!(
                    "fetch_equity_usd http {status}: {body}"
                )));
            }
            let body: serde_json::Value = resp.json().await?;
            // Sum margin balance across USDT/BUSD/USDC entries.
            let mut total = 0.0_f64;
            if let Some(arr) = body.as_array() {
                for entry in arr {
                    let asset = entry.get("asset").and_then(|v| v.as_str()).unwrap_or("");
                    if asset == "USDT" || asset == "BUSD" || asset == "USDC" {
                        let bal: f64 = entry
                            .get("balance")
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0.0);
                        let upnl: f64 = entry
                            .get("crossUnPnl")
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0.0);
                        total += bal + upnl;
                    }
                }
            }
            Ok(total)
        })
    }

    fn fetch_open_positions<'a>(
        &'a self,
        symbols: &'a [String],
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<PositionSnapshot>>> + Send + 'a>,
    > {
        Box::pin(async move {
            let ts = self.timestamp_ms();
            let params = vec![
                ("timestamp".to_string(), ts.to_string()),
                ("recvWindow".to_string(), self.recv_window_ms.to_string()),
            ];
            let qs = encode_query(&params);
            let sig = self.sign(&qs)?;
            let url = format!(
                "{}/fapi/v2/positionRisk?{qs}&signature={sig}",
                self.base_url.trim_end_matches('/')
            );
            let resp = self
                .client
                .get(&url)
                .header("X-MBX-APIKEY", &self.api_key)
                .send()
                .await?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(ScalperError::Exchange(format!(
                    "fetch_open_positions http {status}: {body}"
                )));
            }
            let body: serde_json::Value = resp.json().await?;
            let mut out = Vec::new();
            let symbol_filter: std::collections::HashSet<&str> =
                symbols.iter().map(|s| s.as_str()).collect();
            if let Some(arr) = body.as_array() {
                for p in arr {
                    let symbol = p.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
                    if !symbols.is_empty() && !symbol_filter.contains(symbol) {
                        continue;
                    }
                    let amt: f64 = p
                        .get("positionAmt")
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0.0);
                    if amt.abs() < 1e-9 {
                        continue;
                    }
                    let entry: f64 = p
                        .get("entryPrice")
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0.0);
                    let mark: f64 = p
                        .get("markPrice")
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0.0);
                    let upnl: f64 = p
                        .get("unRealizedProfit")
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0.0);
                    let leverage: u8 = p
                        .get("leverage")
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(1);
                    let side = if amt > 0.0 { Side::Long } else { Side::Short };
                    out.push(PositionSnapshot {
                        symbol: symbol.to_string(),
                        side,
                        size: amt.abs(),
                        entry_price: entry,
                        mark_price: mark,
                        unrealized_pnl: upnl,
                        leverage,
                    });
                }
            }
            Ok(out)
        })
    }

    fn fetch_open_orders<'a>(
        &'a self,
        symbol: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<OpenOrderSnapshot>>> + Send + 'a>,
    > {
        Box::pin(async move {
            let ts = self.timestamp_ms();
            let params = vec![
                ("symbol".to_string(), symbol.to_string()),
                ("timestamp".to_string(), ts.to_string()),
                ("recvWindow".to_string(), self.recv_window_ms.to_string()),
            ];
            let qs = encode_query(&params);
            let sig = self.sign(&qs)?;
            let url = format!(
                "{}/fapi/v1/openOrders?{qs}&signature={sig}",
                self.base_url.trim_end_matches('/')
            );
            let resp = self
                .client
                .get(&url)
                .header("X-MBX-APIKEY", &self.api_key)
                .send()
                .await?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(ScalperError::Exchange(format!(
                    "fetch_open_orders http {status}: {body}"
                )));
            }
            let body: serde_json::Value = resp.json().await?;
            let mut out = Vec::new();
            if let Some(arr) = body.as_array() {
                for o in arr {
                    let side = match o.get("side").and_then(|v| v.as_str()).unwrap_or("") {
                        "BUY" => Side::Long,
                        "SELL" => Side::Short,
                        _ => continue,
                    };
                    let order_type = match o.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                        "MARKET" => OrderType::Market,
                        "LIMIT" => OrderType::Limit,
                        "STOP_MARKET" => OrderType::StopLoss,
                        "TAKE_PROFIT_MARKET" => OrderType::TakeProfit,
                        _ => continue,
                    };
                    let client_id = o
                        .get("clientOrderId")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let exchange_order_id =
                        o.get("orderId").map(|v| v.to_string()).unwrap_or_default();
                    let stop_price = o
                        .get("stopPrice")
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse::<f64>().ok())
                        .filter(|p| *p > 0.0);
                    let reduce_only = o
                        .get("reduceOnly")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                        || matches!(order_type, OrderType::StopLoss | OrderType::TakeProfit);
                    out.push(OpenOrderSnapshot {
                        symbol: symbol.to_string(),
                        client_id,
                        exchange_order_id,
                        side,
                        order_type,
                        stop_price,
                        reduce_only,
                    });
                }
            }
            Ok(out)
        })
    }

    fn fetch_order_status<'a>(
        &'a self,
        symbol: &'a str,
        client_id: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<crate::execution::exchange::OrderStatus>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            use crate::execution::exchange::OrderStatus;
            let ts = self.timestamp_ms();
            let params = vec![
                ("symbol".to_string(), symbol.to_string()),
                ("origClientOrderId".to_string(), client_id.to_string()),
                ("timestamp".to_string(), ts.to_string()),
                ("recvWindow".to_string(), self.recv_window_ms.to_string()),
            ];
            let qs = encode_query(&params);
            let sig = self.sign(&qs)?;
            let url = format!(
                "{}/fapi/v1/order?{qs}&signature={sig}",
                self.base_url.trim_end_matches('/')
            );
            let resp = self
                .client
                .get(&url)
                .header("X-MBX-APIKEY", &self.api_key)
                .send()
                .await?;
            if !resp.status().is_success() {
                return Ok(OrderStatus::Unknown);
            }
            let body: serde_json::Value = resp.json().await?;
            let status = match body
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("UNKNOWN")
            {
                "NEW" => OrderStatus::New,
                "PARTIALLY_FILLED" => OrderStatus::PartiallyFilled,
                "FILLED" => OrderStatus::Filled,
                "CANCELED" => OrderStatus::Canceled,
                "EXPIRED" => OrderStatus::Expired,
                "REJECTED" => OrderStatus::Rejected,
                _ => OrderStatus::Unknown,
            };
            Ok(status)
        })
    }
}

/// Round a quantity to the correct step size for the given futures symbol.
///
/// Binance rejects orders whose quantity doesn't match the symbol's LOT_SIZE
/// filter. Fetching exchange_info on every order is too slow for scalping, so
/// we use well-known defaults and let a startup reconciliation (future work)
/// override them. Using too few decimal places is always safe; using too many
/// causes a -1111 "Parameter 'quantity' has too much precision" error.
fn format_quantity(symbol: &str, qty: f64) -> String {
    // step_size decimals by symbol (Binance USDT-M Futures defaults, 2025)
    let decimals: u32 = match symbol {
        s if s.starts_with("BTC") => 3,                          // step 0.001
        s if s.starts_with("ETH") => 3,                          // step 0.001
        s if s.starts_with("BNB") => 2,                          // step 0.01
        s if s.starts_with("SOL") => 1,                          // step 0.1
        s if s.starts_with("DOGE") => 0,                         // step 1
        s if s.starts_with("XRP") => 1,                          // step 0.1
        s if s.starts_with("ADA") => 0,                          // step 1
        s if s.starts_with("AVAX") => 2,                         // step 0.01
        s if s.starts_with("MATIC") | s.starts_with("POL") => 0, // step 1
        _ => 3,                                                  // conservative default — 0.001
    };
    let factor = 10_f64.powi(decimals as i32);
    let rounded = (qty * factor).floor() / factor;
    format!("{:.prec$}", rounded, prec = decimals as usize)
}

fn encode_query(params: &[(String, String)]) -> String {
    params
        .iter()
        .map(|(k, v)| format!("{k}={}", urlencode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn urlencode(s: &str) -> String {
    // Minimal URL encoder — only the characters Binance cares about.
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}
