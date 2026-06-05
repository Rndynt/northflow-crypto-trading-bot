//! CandleStore — holds candles for three configurable timeframe roles.
//!
//! Built from validated, sorted 1m candles using TimeframeBuilder.
//! Timeframe roles are configured at build time (entry, confirmation, screening).
//! Immutable once built. No global state. No exchange calls.
//!
//! Always retains raw 1m candles for indicator pre-computation.

use crate::core::{Candle, NorthflowError, Timeframe};
use crate::market::timeframe_builder::TimeframeBuilder;

#[derive(Debug)]
pub struct CandleStore {
    /// Raw 1m candles — always present, used for indicator pre-computation.
    pub raw_1m: Vec<Candle>,

    /// The entry timeframe configured for this run.
    pub entry_tf: Timeframe,
    /// The confirmation timeframe configured for this run.
    pub confirmation_tf: Timeframe,
    /// The screening timeframe configured for this run.
    pub screening_tf: Timeframe,

    /// Candles for the entry timeframe (may be same as raw_1m if entry_tf == OneMinute).
    pub entry_candles: Vec<Candle>,
    /// Candles for the confirmation timeframe.
    pub confirmation_candles: Vec<Candle>,
    /// Candles for the screening timeframe.
    pub screening_candles: Vec<Candle>,
}

impl CandleStore {
    /// Build a CandleStore from sorted, validated 1m candles with configurable TF roles.
    ///
    /// Entry candles are built from 1m if entry_tf != OneMinute, else use raw 1m directly.
    /// Confirmation and screening candles are always built from 1m via TimeframeBuilder.
    /// Incomplete higher-timeframe buckets are silently dropped.
    pub fn build(
        candles_1m: Vec<Candle>,
        entry_tf: Timeframe,
        confirmation_tf: Timeframe,
        screening_tf: Timeframe,
    ) -> Result<Self, NorthflowError> {
        // Validate role ordering: entry < confirmation < screening.
        if entry_tf >= confirmation_tf {
            return Err(NorthflowError::ConfigError(format!(
                "entry_timeframe ({entry_tf}) must be shorter than confirmation_timeframe ({confirmation_tf})"
            )));
        }
        if confirmation_tf >= screening_tf {
            return Err(NorthflowError::ConfigError(format!(
                "confirmation_timeframe ({confirmation_tf}) must be shorter than screening_timeframe ({screening_tf})"
            )));
        }

        // Build entry candles.
        let entry_candles = if entry_tf == Timeframe::OneMinute {
            candles_1m.clone()
        } else {
            TimeframeBuilder::build(&candles_1m, entry_tf)?
        };

        let confirmation_candles = TimeframeBuilder::build(&candles_1m, confirmation_tf)?;
        let screening_candles   = TimeframeBuilder::build(&candles_1m, screening_tf)?;

        Ok(Self {
            raw_1m: candles_1m,
            entry_tf,
            confirmation_tf,
            screening_tf,
            entry_candles,
            confirmation_candles,
            screening_candles,
        })
    }

    /// Legacy constructor — builds with the original 1m/5m/15m roles.
    ///
    /// Preserved for backward compatibility with tests.
    pub fn build_from_1m(candles_1m: Vec<Candle>) -> Result<Self, NorthflowError> {
        Self::build(
            candles_1m,
            Timeframe::OneMinute,
            Timeframe::FiveMinute,
            Timeframe::FifteenMinute,
        )
    }

    /// Number of candles for the given timeframe role.
    pub fn entry_len(&self) -> usize         { self.entry_candles.len() }
    pub fn confirmation_len(&self) -> usize  { self.confirmation_candles.len() }
    pub fn screening_len(&self) -> usize     { self.screening_candles.len() }

    /// Number of candles for any supported timeframe (by value).
    /// Returns 0 for unsupported timeframes.
    pub fn len(&self, tf: Timeframe) -> usize {
        if tf == self.entry_tf        { return self.entry_candles.len(); }
        if tf == self.confirmation_tf { return self.confirmation_candles.len(); }
        if tf == self.screening_tf    { return self.screening_candles.len(); }
        if tf == Timeframe::OneMinute { return self.raw_1m.len(); }
        0
    }

    /// Whether the candle list for the given timeframe is empty (or unsupported).
    pub fn is_empty(&self, tf: Timeframe) -> bool {
        self.len(tf) == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_candle(ts_ms: i64) -> Candle {
        Candle { timestamp: ts_ms, open: 100.0, high: 110.0, low: 90.0, close: 105.0, volume: 10.0 }
    }

    fn fifteen_1m() -> Vec<Candle> {
        (0..15).map(|i| make_candle(i as i64 * 60_000)).collect()
    }

    #[test]
    fn builds_default_1m_5m_15m() {
        let s = CandleStore::build_from_1m(fifteen_1m()).unwrap();
        assert_eq!(s.entry_len(), 15);
        assert_eq!(s.confirmation_len(), 3);
        assert_eq!(s.screening_len(), 1);
        assert_eq!(s.entry_tf, Timeframe::OneMinute);
        assert_eq!(s.confirmation_tf, Timeframe::FiveMinute);
        assert_eq!(s.screening_tf, Timeframe::FifteenMinute);
    }

    #[test]
    fn builds_5m_15m_1h() {
        // Need at least 60 1m candles for 1h
        let candles: Vec<Candle> = (0..60).map(|i| make_candle(i * 60_000)).collect();
        let s = CandleStore::build(
            candles,
            Timeframe::FiveMinute,
            Timeframe::FifteenMinute,
            Timeframe::OneHour,
        ).unwrap();
        assert_eq!(s.entry_tf, Timeframe::FiveMinute);
        assert_eq!(s.confirmation_tf, Timeframe::FifteenMinute);
        assert_eq!(s.screening_tf, Timeframe::OneHour);
        assert_eq!(s.entry_len(), 12);     // 60/5
        assert_eq!(s.confirmation_len(), 4); // 60/15
        assert_eq!(s.screening_len(), 1);   // 60/60
    }

    #[test]
    fn len_lookup_by_tf_value() {
        let s = CandleStore::build_from_1m(fifteen_1m()).unwrap();
        assert_eq!(s.len(Timeframe::OneMinute), 15);
        assert_eq!(s.len(Timeframe::FiveMinute), 3);
        assert_eq!(s.len(Timeframe::FifteenMinute), 1);
        assert_eq!(s.len(Timeframe::OneHour), 0); // not in this store
    }

    #[test]
    fn rejects_entry_ge_confirmation() {
        let err = CandleStore::build(
            fifteen_1m(),
            Timeframe::FiveMinute,
            Timeframe::FiveMinute,
            Timeframe::FifteenMinute,
        ).unwrap_err();
        assert!(err.to_string().contains("entry_timeframe"));
    }

    #[test]
    fn rejects_confirmation_ge_screening() {
        let err = CandleStore::build(
            fifteen_1m(),
            Timeframe::OneMinute,
            Timeframe::FifteenMinute,
            Timeframe::FiveMinute,
        ).unwrap_err();
        assert!(err.to_string().contains("confirmation_timeframe"));
    }
}
