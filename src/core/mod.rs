//! Core trading domain — Phase 1 types.
//!
//! Signal identity chain:
//!   signal_id → order_id → fill_id → position_id → exit_order_id → trade_id
//!
//! Timeframe roles (mandatory — never infer from array order):
//!   entry_timeframe        = "1m"
//!   confirmation_timeframe = "5m"
//!   screening_timeframe    = "15m"

pub mod candle;
pub mod error;
pub mod fill;
pub mod order;
pub mod position;
pub mod side;
pub mod signal;
pub mod symbol;
pub mod timeframe;
pub mod trade;

pub use candle::Candle;
pub use error::NorthflowError;
pub use fill::{Fill, FillId};
pub use order::{Order, OrderId, OrderStatus, OrderType};
pub use position::{Position, PositionId, PositionStatus};
pub use side::Side;
pub use signal::{Signal, SignalId, StrategyId};
pub use symbol::Symbol;
pub use timeframe::Timeframe;
pub use trade::{Trade, TradeExitReason, TradeId};
