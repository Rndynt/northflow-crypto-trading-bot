//! Fill — the record of an executed order fill.
//! No live exchange handling in this phase.

use std::fmt;

use crate::core::{
    order::OrderId,
    side::Side,
    signal::SignalId,
    symbol::Symbol,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FillId(pub String);

impl FillId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FillId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone)]
pub struct Fill {
    pub fill_id:   FillId,
    pub order_id:  OrderId,
    pub signal_id: SignalId,
    pub symbol:    Symbol,
    pub side:      Side,
    pub price:     f64,
    pub quantity:  f64,
    pub fee:       f64,
    pub timestamp: i64,
}
