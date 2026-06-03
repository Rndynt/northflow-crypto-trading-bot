//! NorthflowError — unified error type for the core trading domain.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum NorthflowError {
    InvalidCandle(String),
    InvalidTimeframe(String),
    InvalidSignal(String),
    InvalidOrder(String),
    InvalidPosition(String),
    InvalidTrade(String),
    ConfigError(String),
    DataError(String),
}

impl fmt::Display for NorthflowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NorthflowError::InvalidCandle(m) => write!(f, "invalid candle: {m}"),
            NorthflowError::InvalidTimeframe(m) => write!(f, "invalid timeframe: {m}"),
            NorthflowError::InvalidSignal(m) => write!(f, "invalid signal: {m}"),
            NorthflowError::InvalidOrder(m) => write!(f, "invalid order: {m}"),
            NorthflowError::InvalidPosition(m) => write!(f, "invalid position: {m}"),
            NorthflowError::InvalidTrade(m) => write!(f, "invalid trade: {m}"),
            NorthflowError::ConfigError(m) => write!(f, "config error: {m}"),
            NorthflowError::DataError(m) => write!(f, "data error: {m}"),
        }
    }
}

impl std::error::Error for NorthflowError {}
