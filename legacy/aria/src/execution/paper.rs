//! Paper exchange — instant fills at the requested price, no network.

use crate::errors::Result;
use crate::execution::exchange::{Exchange, OpenOrderSnapshot, OrderAck, PositionSnapshot};
use crate::execution::orders::OrderRequest;
use chrono::Utc;
use parking_lot::Mutex;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct OpenPaperOrder {
    pub req: OrderRequest,
    pub filled_at: chrono::DateTime<chrono::Utc>,
}

pub struct PaperExchange {
    fee_bps: f64,
    /// Synthetic execution slippage per fill (basis points).
    slippage_bps: f64,
    /// Simulated latency before order acknowledgement.
    ack_latency_ms: u64,
    orders: Mutex<HashMap<String, OpenPaperOrder>>,
    /// Synthetic balance the paper exchange "holds". Updated by callers
    /// (RiskAgent / SurvivalAgent) so they can simulate equity drift.
    equity_usd: Mutex<f64>,
}

impl PaperExchange {
    pub fn new(fee_bps: f64, equity_usd: f64) -> Self {
        Self {
            fee_bps,
            slippage_bps: 1.5,
            ack_latency_ms: 60,
            orders: Mutex::new(HashMap::new()),
            equity_usd: Mutex::new(equity_usd),
        }
    }

    pub fn open_orders(&self) -> Vec<OpenPaperOrder> {
        self.orders.lock().values().cloned().collect()
    }

    pub fn set_equity(&self, equity: f64) {
        *self.equity_usd.lock() = equity;
    }
}

impl Exchange for PaperExchange {
    fn name(&self) -> &'static str {
        "paper"
    }

    fn place_order<'a>(
        &'a self,
        req: &'a OrderRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<OrderAck>> + Send + 'a>> {
        Box::pin(async move {
            tokio::time::sleep(std::time::Duration::from_millis(self.ack_latency_ms)).await;
            let base = req.price.unwrap_or(0.0);
            // Conservative paper model: longs pay up, shorts sell lower.
            let signed_slip = match req.side {
                crate::data::Side::Long => 1.0,
                crate::data::Side::Short => -1.0,
            };
            let price = base * (1.0 + signed_slip * self.slippage_bps / 10_000.0);
            let notional = price * req.size;
            let fee = notional * self.fee_bps / 10_000.0;
            self.orders.lock().insert(
                req.client_id.clone(),
                OpenPaperOrder {
                    req: req.clone(),
                    filled_at: Utc::now(),
                },
            );
            Ok(OrderAck {
                client_id: req.client_id.clone(),
                exchange_order_id: format!("paper-{}", req.client_id),
                symbol: req.symbol.clone(),
                filled_qty: req.size,
                avg_fill_price: price,
                fee_usd: fee,
                ts_ms: Utc::now().timestamp_millis(),
            })
        })
    }

    fn cancel_order<'a>(
        &'a self,
        _symbol: &'a str,
        client_id: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            self.orders.lock().remove(client_id);
            Ok(())
        })
    }

    fn cancel_all<'a>(
        &'a self,
        symbol: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            self.orders.lock().retain(|_, o| o.req.symbol != symbol);
            Ok(())
        })
    }

    fn set_leverage<'a>(
        &'a self,
        _symbol: &'a str,
        _leverage: u8,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move { Ok(()) })
    }

    fn fetch_equity_usd<'a>(
        &'a self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<f64>> + Send + 'a>> {
        // Return 0 so the SurvivalAgent reconciliation loop skips the
        // `risk.set_equity()` call. In paper mode the RiskManager's own
        // `on_position_closed(pnl)` is the authoritative equity source;
        // overwriting it with the exchange's stale initial balance would
        // cause a false drawdown spike after every profitable trade.
        Box::pin(async move { Ok(0.0) })
    }

    fn fetch_open_positions<'a>(
        &'a self,
        _symbols: &'a [String],
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<PositionSnapshot>>> + Send + 'a>,
    > {
        // Paper exchange has no broker-side positions — the in-memory
        // PositionBook is the source of truth.
        Box::pin(async move { Ok(Vec::new()) })
    }

    fn fetch_open_orders<'a>(
        &'a self,
        symbol: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<OpenOrderSnapshot>>> + Send + 'a>,
    > {
        Box::pin(async move {
            let out = self
                .orders
                .lock()
                .values()
                .filter(|o| o.req.symbol == symbol)
                .map(|o| OpenOrderSnapshot {
                    symbol: o.req.symbol.clone(),
                    client_id: o.req.client_id.clone(),
                    exchange_order_id: format!("paper-{}", o.req.client_id),
                    side: o.req.side,
                    order_type: o.req.order_type,
                    stop_price: o.req.stop_price,
                    reduce_only: o.req.reduce_only,
                })
                .collect();
            Ok(out)
        })
    }

    fn fetch_order_status<'a>(
        &'a self,
        _symbol: &'a str,
        _client_id: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<crate::execution::exchange::OrderStatus>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            // Paper exchange fills instantly — always Filled
            Ok(crate::execution::exchange::OrderStatus::Filled)
        })
    }
}
