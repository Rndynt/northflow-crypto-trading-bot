//! MEXC Futures REST client — OPEN-API HMAC-SHA256 signed requests.

use crate::data::Side;
use crate::errors::{Result, ScalperError};
use crate::execution::exchange::{Exchange, OpenOrderSnapshot, OrderAck, PositionSnapshot};
use crate::execution::orders::{OrderRequest, OrderType};
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde_json::{Value, json};
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;

pub struct MexcFutures {
    client: Client,
    base_url: String,
    api_key: String,
    api_secret: String,
    recv_window_ms: u64,
    open_type: u8,
    leverage: u8,
}

impl MexcFutures {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        api_secret: impl Into<String>,
        recv_window_ms: u64,
        open_type: impl AsRef<str>,
        leverage: u8,
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
            open_type: mexc_open_type(open_type.as_ref()),
            leverage: leverage.max(1),
        }
    }

    fn timestamp_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }

    fn sign(&self, timestamp: i64, parameter_string: &str) -> Result<String> {
        sign_mexc(&self.api_key, &self.api_secret, timestamp, parameter_string)
    }

    fn auth_headers(
        &self,
        rb: reqwest::RequestBuilder,
        ts: i64,
        sig: String,
    ) -> reqwest::RequestBuilder {
        rb.header("ApiKey", &self.api_key)
            .header("Request-Time", ts.to_string())
            .header("Signature", sig)
            .header("Recv-Window", self.recv_window_ms.to_string())
    }

    async fn ensure_success(resp: reqwest::Response, op: &str) -> Result<Value> {
        let status = resp.status();
        let body: Value = resp.json().await?;
        if !status.is_success() || body.get("success").and_then(|v| v.as_bool()) == Some(false) {
            return Err(ScalperError::Exchange(format!(
                "{op} http {status}: {}",
                body.get("message")
                    .and_then(|v| v.as_str())
                    .map(ToString::to_string)
                    .unwrap_or_else(|| body.to_string())
            )));
        }
        Ok(body)
    }
}

impl Exchange for MexcFutures {
    fn name(&self) -> &'static str {
        "mexc-futures"
    }

    fn place_order<'a>(
        &'a self,
        req: &'a OrderRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<OrderAck>> + Send + 'a>> {
        Box::pin(async move {
            let ts = self.timestamp_ms();
            let is_close = req.reduce_only
                || matches!(req.order_type, OrderType::StopLoss | OrderType::TakeProfit);
            let payload = json!({
                "symbol": mexc_symbol(&req.symbol),
                "price": req.price.unwrap_or(0.0),
                "vol": req.size,
                "leverage": self.leverage,
                "side": mexc_side(req.side, is_close),
                "type": mexc_order_type(req.order_type),
                "openType": self.open_type,
                "externalOid": req.client_id,
                "positionMode": 1,
                "reduceOnly": req.reduce_only,
                "stopLossPrice": stop_loss_for(req),
                "takeProfitPrice": take_profit_for(req),
                "lossTrend": 2,
                "profitTrend": 2,
                "priceProtect": 1
            });
            let body = canonical_json(&payload)?;
            let sig = self.sign(ts, &body)?;
            let url = format!(
                "{}/api/v1/private/order/create",
                self.base_url.trim_end_matches('/')
            );
            let resp = self
                .auth_headers(
                    self.client
                        .post(url)
                        .body(body)
                        .header("Content-Type", "application/json"),
                    ts,
                    sig,
                )
                .send()
                .await?;
            let body = Self::ensure_success(resp, "place_order").await?;
            let data = body.get("data").unwrap_or(&Value::Null);
            let exchange_order_id = data
                .get("orderId")
                .map(|v| v.to_string().trim_matches('"').to_string())
                .or_else(|| data.as_str().map(ToString::to_string))
                .unwrap_or_default();
            Ok(OrderAck {
                client_id: req.client_id.clone(),
                exchange_order_id,
                symbol: req.symbol.clone(),
                filled_qty: 0.0,
                avg_fill_price: req.price.unwrap_or(0.0),
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
            let payload = json!({ "symbol": mexc_symbol(symbol), "externalOid": client_id });
            let body = canonical_json(&payload)?;
            let sig = self.sign(ts, &body)?;
            let url = format!(
                "{}/api/v1/private/order/cancel",
                self.base_url.trim_end_matches('/')
            );
            let resp = self
                .auth_headers(
                    self.client
                        .post(url)
                        .body(body)
                        .header("Content-Type", "application/json"),
                    ts,
                    sig,
                )
                .send()
                .await?;
            Self::ensure_success(resp, "cancel_order").await?;
            Ok(())
        })
    }

    fn cancel_all<'a>(
        &'a self,
        symbol: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let orders = self.fetch_open_orders(symbol).await?;
            for order in orders {
                self.cancel_order(symbol, &order.client_id).await?;
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
            let payload = json!({
                "symbol": mexc_symbol(symbol),
                "leverage": leverage.max(1),
                "openType": self.open_type,
                "positionType": 1,
                "leverageMode": 2
            });
            let body = canonical_json(&payload)?;
            let sig = self.sign(ts, &body)?;
            let url = format!(
                "{}/api/v1/private/position/change_leverage",
                self.base_url.trim_end_matches('/')
            );
            let resp = self
                .auth_headers(
                    self.client
                        .post(url)
                        .body(body)
                        .header("Content-Type", "application/json"),
                    ts,
                    sig,
                )
                .send()
                .await?;
            Self::ensure_success(resp, "set_leverage").await?;
            Ok(())
        })
    }

    fn fetch_equity_usd<'a>(
        &'a self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<f64>> + Send + 'a>> {
        Box::pin(async move {
            let ts = self.timestamp_ms();
            let sig = self.sign(ts, "")?;
            let url = format!(
                "{}/api/v1/private/account/assets",
                self.base_url.trim_end_matches('/')
            );
            let resp = self
                .auth_headers(self.client.get(url), ts, sig)
                .send()
                .await?;
            let body = Self::ensure_success(resp, "fetch_equity_usd").await?;
            let mut total = 0.0;
            if let Some(arr) = body.get("data").and_then(|v| v.as_array()) {
                for asset in arr {
                    let currency = asset.get("currency").and_then(|v| v.as_str()).unwrap_or("");
                    if currency == "USDT" || currency == "USDC" || currency == "BUSD" {
                        total += parse_f64(
                            asset,
                            &["equity", "walletBalance", "availableBalance", "cashBalance"],
                        );
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
            let mut out = Vec::new();
            let symbol_filter: std::collections::HashSet<String> =
                symbols.iter().map(|s| mexc_symbol(s)).collect();
            let ts = self.timestamp_ms();
            let sig = self.sign(ts, "")?;
            let url = format!(
                "{}/api/v1/private/position/open_positions",
                self.base_url.trim_end_matches('/')
            );
            let resp = self
                .auth_headers(self.client.get(url), ts, sig)
                .send()
                .await?;
            let body = Self::ensure_success(resp, "fetch_open_positions").await?;
            if let Some(arr) = body.get("data").and_then(|v| v.as_array()) {
                for p in arr {
                    let symbol = p.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
                    if !symbol_filter.is_empty() && !symbol_filter.contains(symbol) {
                        continue;
                    }
                    let size = parse_f64(p, &["holdVol", "vol"]);
                    if size <= 0.0 {
                        continue;
                    }
                    let side = match p.get("positionType").and_then(|v| v.as_i64()).unwrap_or(0) {
                        1 => Side::Long,
                        2 => Side::Short,
                        _ => continue,
                    };
                    out.push(PositionSnapshot {
                        symbol: symbol.replace('_', ""),
                        side,
                        size,
                        entry_price: parse_f64(
                            p,
                            &["holdAvgPrice", "openAvgPrice", "newOpenAvgPrice"],
                        ),
                        mark_price: parse_f64(p, &["markPrice", "fairPrice", "holdAvgPrice"]),
                        unrealized_pnl: parse_f64(p, &["unrealizedPnl", "profit", "realised"]),
                        leverage: p
                            .get("leverage")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(1)
                            .min(u8::MAX as u64) as u8,
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
            let symbol = mexc_symbol(symbol);
            let qs = format!("symbol={}", urlencode(&symbol));
            let ts = self.timestamp_ms();
            let sig = self.sign(ts, &qs)?;
            let url = format!(
                "{}/api/v1/private/order/list/open_orders?{qs}",
                self.base_url.trim_end_matches('/')
            );
            let resp = self
                .auth_headers(self.client.get(url), ts, sig)
                .send()
                .await?;
            let body = Self::ensure_success(resp, "fetch_open_orders").await?;
            let mut out = Vec::new();
            if let Some(arr) = body.get("data").and_then(|v| v.as_array()) {
                for o in arr {
                    let side_code = o.get("side").and_then(|v| v.as_i64()).unwrap_or(0);
                    let side = match side_code {
                        1 | 4 => Side::Long,
                        2 | 3 => Side::Short,
                        _ => continue,
                    };
                    let order_type = match o
                        .get("orderType")
                        .or_else(|| o.get("type"))
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0)
                    {
                        1 => OrderType::Limit,
                        5 => OrderType::Market,
                        _ => OrderType::Market,
                    };
                    out.push(OpenOrderSnapshot {
                        symbol: symbol.replace('_', ""),
                        client_id: o
                            .get("externalOid")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        exchange_order_id: o
                            .get("orderId")
                            .map(|v| v.to_string().trim_matches('"').to_string())
                            .unwrap_or_default(),
                        side,
                        order_type,
                        stop_price: None,
                        reduce_only: matches!(side_code, 2 | 4),
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
            let sym = mexc_symbol(symbol);
            let qs = format!(
                "symbol={}&externalOid={}",
                urlencode(&sym),
                urlencode(client_id)
            );
            let ts = self.timestamp_ms();
            let sig = self.sign(ts, &qs)?;
            let url = format!(
                "{}/api/v1/private/order/get?{qs}",
                self.base_url.trim_end_matches('/')
            );
            let resp = self
                .auth_headers(self.client.get(url), ts, sig)
                .send()
                .await;
            match resp {
                Ok(r) if r.status().is_success() => {
                    // MEXC order states: 1=New, 2=Filled, 3=PartiallyFilled, 4=Canceled
                    let body: serde_json::Value = r.json().await.unwrap_or(serde_json::Value::Null);
                    let state = body
                        .get("data")
                        .and_then(|d| d.get("state"))
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    Ok(match state {
                        1 => OrderStatus::New,
                        2 => OrderStatus::Filled,
                        3 => OrderStatus::PartiallyFilled,
                        4 => OrderStatus::Canceled,
                        _ => OrderStatus::Unknown,
                    })
                }
                _ => Ok(OrderStatus::Unknown),
            }
        })
    }
}

fn mexc_symbol(symbol: &str) -> String {
    if symbol.contains('_') {
        symbol.to_string()
    } else if let Some(base) = symbol.strip_suffix("USDT") {
        format!("{base}_USDT")
    } else if let Some(base) = symbol.strip_suffix("USDC") {
        format!("{base}_USDC")
    } else {
        symbol.to_string()
    }
}

fn mexc_open_type(open_type: &str) -> u8 {
    if open_type.eq_ignore_ascii_case("isolated") {
        1
    } else {
        2
    }
}

fn mexc_side(side: Side, close: bool) -> u8 {
    match (side, close) {
        (Side::Long, false) => 1,
        (Side::Short, true) => 2,
        (Side::Short, false) => 3,
        (Side::Long, true) => 4,
    }
}

fn mexc_order_type(order_type: OrderType) -> u8 {
    match order_type {
        OrderType::Limit => 1,
        OrderType::Market | OrderType::StopLoss | OrderType::TakeProfit => 5,
    }
}

fn stop_loss_for(req: &OrderRequest) -> Option<f64> {
    match req.order_type {
        OrderType::StopLoss => req.stop_price,
        OrderType::Market | OrderType::Limit if req.stop_loss > 0.0 => Some(req.stop_loss),
        _ => None,
    }
}

fn take_profit_for(req: &OrderRequest) -> Option<f64> {
    match req.order_type {
        OrderType::TakeProfit => req.stop_price,
        OrderType::Market | OrderType::Limit if req.take_profit > 0.0 => Some(req.take_profit),
        _ => None,
    }
}

fn canonical_json(value: &Value) -> Result<String> {
    serde_json::to_string(value).map_err(ScalperError::Json)
}

fn sign_mexc(
    access_key: &str,
    secret: &str,
    timestamp: i64,
    parameter_string: &str,
) -> Result<String> {
    let target = format!("{access_key}{timestamp}{parameter_string}");
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|e| ScalperError::Exchange(format!("hmac: {e}")))?;
    mac.update(target.as_bytes());
    Ok(hex::encode(mac.finalize().into_bytes()))
}

fn parse_f64(value: &Value, keys: &[&str]) -> f64 {
    keys.iter()
        .find_map(|key| {
            value.get(*key).and_then(|v| {
                v.as_f64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            })
        })
        .unwrap_or(0.0)
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mexc_maps_symbols_and_sides() {
        assert_eq!(mexc_symbol("BTCUSDT"), "BTC_USDT");
        assert_eq!(mexc_symbol("ETH_USDT"), "ETH_USDT");
        assert_eq!(mexc_side(Side::Long, false), 1);
        assert_eq!(mexc_side(Side::Short, true), 2);
        assert_eq!(mexc_side(Side::Short, false), 3);
        assert_eq!(mexc_side(Side::Long, true), 4);
    }

    #[test]
    fn mexc_signing_uses_access_key_timestamp_params() {
        let sig = sign_mexc("ak", "secret", 1700000000000, "{\"symbol\":\"BTC_USDT\"}").unwrap();
        assert_eq!(
            sig,
            "83ea301f87adefa59e0cc38e76be3526000a246a521edce276fb2beeaa6110cc"
        );
    }
}
