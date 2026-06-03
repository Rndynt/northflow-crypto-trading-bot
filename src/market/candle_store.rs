//! CandleStore — holds 1m, 5m, and 15m candles for one symbol.
//!
//! Built from validated, sorted 1m candles using TimeframeBuilder.
//! Immutable once built. No global state. No exchange calls.

use crate::core::{Candle, NorthflowError, Timeframe};
use crate::market::timeframe_builder::TimeframeBuilder;

pub struct CandleStore {
    pub one_minute:     Vec<Candle>,
    pub five_minute:    Vec<Candle>,
    pub fifteen_minute: Vec<Candle>,
}

impl CandleStore {
    /// Build a CandleStore from sorted, validated 1m candles.
    ///
    /// 5m and 15m candles are derived from 1m using TimeframeBuilder.
    /// Incomplete higher-timeframe buckets are silently dropped (see
    /// TimeframeBuilder for the exact rule).
    pub fn build_from_1m(candles_1m: Vec<Candle>) -> Result<Self, NorthflowError> {
        let five_minute    = TimeframeBuilder::build(&candles_1m, Timeframe::FiveMinute)?;
        let fifteen_minute = TimeframeBuilder::build(&candles_1m, Timeframe::FifteenMinute)?;
        Ok(Self { one_minute: candles_1m, five_minute, fifteen_minute })
    }

    /// Return a slice of candles for the given timeframe.
    ///
    /// Returns `None` for `Timeframe::OneHour` (not stored in Phase 2).
    pub fn get(&self, tf: Timeframe) -> Option<&[Candle]> {
        match tf {
            Timeframe::OneMinute     => Some(&self.one_minute),
            Timeframe::FiveMinute    => Some(&self.five_minute),
            Timeframe::FifteenMinute => Some(&self.fifteen_minute),
            Timeframe::OneHour       => None,
        }
    }

    /// Number of candles for the given timeframe (0 if unsupported).
    pub fn len(&self, tf: Timeframe) -> usize {
        self.get(tf).map(|s| s.len()).unwrap_or(0)
    }

    /// Whether the candle list for the given timeframe is empty.
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

    /// 15 consecutive 1m candles → exactly 3 × 5m + 1 × 15m.
    fn fifteen_1m() -> Vec<Candle> {
        (0..15).map(|i| make_candle(i as i64 * 60_000)).collect()
    }

    #[test]
    fn builds_store_from_1m_candles() {
        let s = CandleStore::build_from_1m(fifteen_1m()).unwrap();
        assert_eq!(s.len(Timeframe::OneMinute), 15);
        assert_eq!(s.len(Timeframe::FiveMinute), 3);
        assert_eq!(s.len(Timeframe::FifteenMinute), 1);
    }

    #[test]
    fn get_one_minute_returns_1m_candles() {
        let s = CandleStore::build_from_1m(fifteen_1m()).unwrap();
        let slice = s.get(Timeframe::OneMinute).unwrap();
        assert_eq!(slice.len(), 15);
        assert_eq!(slice[0].timestamp, 0);
    }

    #[test]
    fn get_five_minute_returns_5m_candles() {
        let s = CandleStore::build_from_1m(fifteen_1m()).unwrap();
        let slice = s.get(Timeframe::FiveMinute).unwrap();
        assert_eq!(slice.len(), 3);
        assert_eq!(slice[0].timestamp, 0);
    }

    #[test]
    fn get_fifteen_minute_returns_15m_candles() {
        let s = CandleStore::build_from_1m(fifteen_1m()).unwrap();
        let slice = s.get(Timeframe::FifteenMinute).unwrap();
        assert_eq!(slice.len(), 1);
        assert_eq!(slice[0].timestamp, 0);
    }

    #[test]
    fn get_one_hour_returns_none() {
        let s = CandleStore::build_from_1m(fifteen_1m()).unwrap();
        assert!(s.get(Timeframe::OneHour).is_none());
    }

    #[test]
    fn len_works() {
        let s = CandleStore::build_from_1m(fifteen_1m()).unwrap();
        assert_eq!(s.len(Timeframe::OneMinute), 15);
        assert_eq!(s.len(Timeframe::FiveMinute), 3);
        assert_eq!(s.len(Timeframe::FifteenMinute), 1);
        assert_eq!(s.len(Timeframe::OneHour), 0);
    }

    #[test]
    fn is_empty_works() {
        let s = CandleStore::build_from_1m(Vec::new()).unwrap();
        assert!(s.is_empty(Timeframe::OneMinute));
        assert!(s.is_empty(Timeframe::FiveMinute));
        assert!(s.is_empty(Timeframe::FifteenMinute));
    }

    #[test]
    fn incomplete_1m_candles_produce_no_higher_tf() {
        // 4 candles → no complete 5m or 15m buckets
        let candles: Vec<Candle> = (0..4).map(|i| make_candle(i as i64 * 60_000)).collect();
        let s = CandleStore::build_from_1m(candles).unwrap();
        assert!(s.is_empty(Timeframe::FiveMinute));
        assert!(s.is_empty(Timeframe::FifteenMinute));
    }
}
