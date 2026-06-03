//! market — Phase 2: deterministic 1m OHLCV data foundation.
//!
//! Pipeline:
//!   OhlcvLoader → OhlcvLoadResult → CandleStore (holds 1m + 5m + 15m)
//!
//! Data quality issues (duplicates, missing gaps, invalid candles) are
//! captured in DataQualityReport without panicking.

pub mod candle_store;
pub mod data_quality;
pub mod ohlcv_loader;
pub mod timeframe_builder;

pub use candle_store::CandleStore;
pub use data_quality::{
    DataQualityIssue, DataQualityIssueKind, DataQualityReport, MissingCandleGap,
};
pub use ohlcv_loader::{OhlcvLoadResult, OhlcvLoader};
pub use timeframe_builder::TimeframeBuilder;
