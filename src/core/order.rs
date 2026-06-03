//! Order — an intended order linked to a signal.
//! No exchange API logic lives here.

use std::fmt;

use crate::core::{side::Side, signal::SignalId, symbol::Symbol};

// ── OrderId ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OrderId(pub String);

impl OrderId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OrderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── OrderType ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderType {
    MarketEntry,
    LimitEntry,
    StopLoss,
    TakeProfit,
    PartialTakeProfit,
    Close,
}

impl OrderType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MarketEntry => "market_entry",
            Self::LimitEntry => "limit_entry",
            Self::StopLoss => "stop_loss",
            Self::TakeProfit => "take_profit",
            Self::PartialTakeProfit => "partial_take_profit",
            Self::Close => "close",
        }
    }
}

impl fmt::Display for OrderType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ── OrderStatus ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderStatus {
    Pending,
    Accepted,
    Rejected,
    PartiallyFilled,
    Filled,
    Cancelled,
}

impl OrderStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::PartiallyFilled => "partially_filled",
            Self::Filled => "filled",
            Self::Cancelled => "cancelled",
        }
    }
}

impl fmt::Display for OrderStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ── Order ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Order {
    pub order_id: OrderId,
    pub signal_id: SignalId,
    pub symbol: Symbol,
    pub side: Side,
    pub order_type: OrderType,
    pub status: OrderStatus,
    pub requested_price: f64,
    pub quantity: f64,
    pub created_at: i64,
}

impl Order {
    pub fn new(
        order_id: OrderId,
        signal_id: SignalId,
        symbol: Symbol,
        side: Side,
        order_type: OrderType,
        requested_price: f64,
        quantity: f64,
        created_at: i64,
    ) -> Self {
        Self {
            order_id,
            signal_id,
            symbol,
            side,
            order_type,
            status: OrderStatus::Pending,
            requested_price,
            quantity,
            created_at,
        }
    }
}
