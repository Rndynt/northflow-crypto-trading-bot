//! TimeframeBuilder — aggregates sorted 1m candles into higher timeframes.
//!
//! Rules (Phase 2):
//!   - Builds 5m and 15m directly from 1m candles only.
//!   - Does NOT build 15m from 5m.
//!   - Does NOT forward-fill, synthesise, or interpolate missing candles.
//!   - Drops incomplete buckets silently (no error — documented behaviour).
//!     A 5m bucket requires exactly 5 one-minute candles.
//!     A 15m bucket requires exactly 15 one-minute candles.
//!   - Validates every aggregated candle with Candle::validate().
//!   - Bucket alignment (timestamps in milliseconds):
//!       5m  → bucket_start = ts - (ts % 300_000)
//!       15m → bucket_start = ts - (ts % 900_000)

use std::collections::BTreeMap;

use crate::core::{Candle, NorthflowError, Timeframe};

pub struct TimeframeBuilder;

impl TimeframeBuilder {
    /// Build higher-timeframe candles from a slice of validated, sorted 1m candles.
    ///
    /// Supported output timeframes: `FiveMinute`, `FifteenMinute`.
    /// Returns an error if the timeframe cannot be aggregated from 1m
    /// (e.g. `OneMinute` or `OneHour`).
    pub fn build(candles_1m: &[Candle], tf: Timeframe) -> Result<Vec<Candle>, NorthflowError> {
        if candles_1m.is_empty() {
            return Ok(Vec::new());
        }

        let interval_ms = tf.to_seconds() as i64 * 1_000;
        let required    = (tf.to_seconds() / 60) as usize;

        if required <= 1 {
            return Err(NorthflowError::InvalidTimeframe(format!(
                "timeframe {tf} cannot be aggregated from 1m candles \
                 (required={required}; use FiveMinute or FifteenMinute)"
            )));
        }

        // Group candles into buckets by aligned start timestamp.
        // BTreeMap gives deterministic, ascending iteration by bucket_start.
        let mut buckets: BTreeMap<i64, Vec<Candle>> = BTreeMap::new();
        for c in candles_1m {
            let bucket_start = c.timestamp - (c.timestamp % interval_ms);
            buckets.entry(bucket_start).or_default().push(*c);
        }

        let mut result = Vec::new();
        for (bucket_start, mut candles) in buckets {
            // Drop incomplete bucket — not an error.
            if candles.len() < required {
                continue;
            }

            // Sort within bucket (defensive — input should already be sorted).
            candles.sort_by_key(|c| c.timestamp);

            let open   = candles[0].open;
            let close  = candles[candles.len() - 1].close;
            let high   = candles.iter().map(|c| c.high).fold(f64::NEG_INFINITY, f64::max);
            let low    = candles.iter().map(|c| c.low).fold(f64::INFINITY, f64::min);
            let volume: f64 = candles.iter().map(|c| c.volume).sum();

            let agg = Candle { timestamp: bucket_start, open, high, low, close, volume };

            agg.validate().map_err(|e| {
                NorthflowError::DataError(format!(
                    "aggregated {tf} candle at ts={bucket_start} failed validation: {e}"
                ))
            })?;

            result.push(agg);
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(ts_ms: i64, open: f64, high: f64, low: f64, close: f64, volume: f64) -> Candle {
        Candle { timestamp: ts_ms, open, high, low, close, volume }
    }

    /// N identical 1m candles starting at ts=0, spaced 60_000 ms apart.
    fn uniform(n: usize) -> Vec<Candle> {
        (0..n)
            .map(|i| c(i as i64 * 60_000, 100.0, 110.0, 90.0, 105.0, 10.0))
            .collect()
    }

    /// 5 distinct 1m candles all in the same 5m bucket (ts 0..240_000 ms).
    fn five_distinct() -> Vec<Candle> {
        vec![
            c(      0, 100.0, 110.0,  95.0, 105.0, 10.0),
            c( 60_000, 105.0, 115.0, 100.0, 108.0, 15.0),
            c(120_000, 108.0, 112.0, 103.0, 104.0,  8.0),
            c(180_000, 104.0, 108.0,  98.0, 106.0, 12.0),
            c(240_000, 106.0, 120.0,  92.0, 118.0, 20.0),
        ]
    }

    // ── 5m ─────────────────────────────────────────────────────────────────

    #[test]
    fn builds_one_5m_candle_from_five_1m_candles() {
        let r = TimeframeBuilder::build(&uniform(5), Timeframe::FiveMinute).unwrap();
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn builds_two_5m_candles_from_ten_1m_candles() {
        let r = TimeframeBuilder::build(&uniform(10), Timeframe::FiveMinute).unwrap();
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn drops_incomplete_5m_bucket() {
        let r = TimeframeBuilder::build(&uniform(4), Timeframe::FiveMinute).unwrap();
        assert_eq!(r.len(), 0);
    }

    #[test]
    fn drops_partial_second_5m_bucket() {
        // 7 candles → first complete bucket (5) + leftover 2 → drop second
        let r = TimeframeBuilder::build(&uniform(7), Timeframe::FiveMinute).unwrap();
        assert_eq!(r.len(), 1);
    }

    // ── 15m ────────────────────────────────────────────────────────────────

    #[test]
    fn builds_one_15m_candle_from_fifteen_1m_candles() {
        let r = TimeframeBuilder::build(&uniform(15), Timeframe::FifteenMinute).unwrap();
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn drops_incomplete_15m_bucket() {
        let r = TimeframeBuilder::build(&uniform(14), Timeframe::FifteenMinute).unwrap();
        assert_eq!(r.len(), 0);
    }

    // ── aggregation field correctness ───────────────────────────────────────

    #[test]
    fn aggregated_open_equals_first_open() {
        let r = TimeframeBuilder::build(&five_distinct(), Timeframe::FiveMinute).unwrap();
        assert_eq!(r[0].open, 100.0);
    }

    #[test]
    fn aggregated_high_equals_max_high() {
        let r = TimeframeBuilder::build(&five_distinct(), Timeframe::FiveMinute).unwrap();
        assert_eq!(r[0].high, 120.0);
    }

    #[test]
    fn aggregated_low_equals_min_low() {
        let r = TimeframeBuilder::build(&five_distinct(), Timeframe::FiveMinute).unwrap();
        assert_eq!(r[0].low, 92.0);
    }

    #[test]
    fn aggregated_close_equals_last_close() {
        let r = TimeframeBuilder::build(&five_distinct(), Timeframe::FiveMinute).unwrap();
        assert_eq!(r[0].close, 118.0);
    }

    #[test]
    fn aggregated_volume_equals_sum_volume() {
        let r = TimeframeBuilder::build(&five_distinct(), Timeframe::FiveMinute).unwrap();
        let expected = 10.0 + 15.0 + 8.0 + 12.0 + 20.0;
        assert!((r[0].volume - expected).abs() < f64::EPSILON * 100.0);
    }

    #[test]
    fn aggregated_timestamp_equals_bucket_start() {
        // ts=0..240_000 ms → bucket_start = 0 - (0 % 300_000) = 0
        let r = TimeframeBuilder::build(&uniform(5), Timeframe::FiveMinute).unwrap();
        assert_eq!(r[0].timestamp, 0);
    }

    // ── edge cases ──────────────────────────────────────────────────────────

    #[test]
    fn empty_input_returns_empty() {
        let r = TimeframeBuilder::build(&[], Timeframe::FiveMinute).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn one_minute_timeframe_returns_error() {
        let err = TimeframeBuilder::build(&uniform(5), Timeframe::OneMinute);
        assert!(err.is_err());
    }
}
