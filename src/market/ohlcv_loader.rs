//! OhlcvLoader — deterministic 1m OHLCV CSV loader.
//!
//! Rules:
//!   - No async, no network, no exchange API calls.
//!   - Accepts headers: timestamp or open_time (case-insensitive, whitespace-tolerant).
//!   - Timestamps must be positive integers (Unix seconds or milliseconds).
//!     Decimal, NaN, inf, negative, and zero timestamps are rejected.
//!   - Normalises timestamps to milliseconds: < 10^12 → seconds × 1000.
//!   - Reports every rejected row in DataQualityReport; never panics on bad data.
//!   - Sorts output candles ascending by timestamp.
//!   - Detects non-monotonic input, duplicate timestamps (keep first),
//!     missing 1m gaps (delta > 60_000 ms — warning), and
//!     irregular sub-minute intervals (delta < 60_000 ms — error).

use std::path::Path;

use crate::core::{Candle, NorthflowError};
use crate::market::data_quality::{DataQualityIssueKind, DataQualityReport, MissingCandleGap};

/// Timestamps below this value are treated as Unix seconds and multiplied
/// by 1_000 to convert to milliseconds.  (~year 2001 in ms = 10^12)
const SECONDS_THRESHOLD: i64 = 1_000_000_000_000;

/// Expected gap between consecutive 1m candles (milliseconds).
const ONE_MINUTE_MS: i64 = 60_000;

pub struct OhlcvLoadResult {
    /// Sorted, deduplicated, validated 1m candles.
    pub candles: Vec<Candle>,
    /// Full data quality report including all issues and missing gaps.
    pub quality: DataQualityReport,
}

pub struct OhlcvLoader;

/// Parse a raw timestamp string into a millisecond Unix timestamp.
///
/// Rules:
///   - Must be a valid integer (no decimals, no NaN, no inf).
///   - Must be strictly positive (> 0).
///   - Values < SECONDS_THRESHOLD are treated as Unix seconds and multiplied by 1_000.
///   - Values >= SECONDS_THRESHOLD are kept as milliseconds.
fn parse_timestamp_ms(raw: &str) -> Result<i64, String> {
    let ts = raw
        .trim()
        .parse::<i64>()
        .map_err(|_| format!("timestamp must be a positive integer, got '{raw}'"))?;

    if ts <= 0 {
        return Err(format!("timestamp must be > 0, got {ts}"));
    }

    if ts < SECONDS_THRESHOLD {
        Ok(ts * 1_000)
    } else {
        Ok(ts)
    }
}

impl OhlcvLoader {
    /// Load a 1m OHLCV CSV file from disk.
    ///
    /// Returns `Err` only on OS-level failure (file not found, permission denied).
    /// All CSV parsing and candle validation issues are captured in the
    /// returned `OhlcvLoadResult.quality` report.
    pub fn load_file(path: &Path) -> Result<OhlcvLoadResult, NorthflowError> {
        let source = path.display().to_string();
        let raw = std::fs::read_to_string(path)
            .map_err(|e| NorthflowError::DataError(format!("failed to read '{source}': {e}")))?;
        Ok(Self::load_csv(&source, &raw))
    }

    /// Parse raw CSV text into validated candles plus a data quality report.
    ///
    /// This function never panics — all errors are recorded in the report.
    pub fn load_csv(source: &str, raw: &str) -> OhlcvLoadResult {
        let mut quality = DataQualityReport::new(source);
        let mut lines = raw.lines().enumerate();

        // ── locate header ────────────────────────────────────────────────────
        let header_line = loop {
            match lines.next() {
                None => {
                    quality.push_issue(
                        DataQualityIssueKind::EmptyFile,
                        None,
                        None,
                        "file is empty",
                    );
                    return OhlcvLoadResult {
                        candles: Vec::new(),
                        quality,
                    };
                }
                Some((_, line)) if line.trim().is_empty() => continue,
                Some((_, line)) => break line,
            }
        };

        // ── parse column indices (case-insensitive, whitespace-tolerant) ─────
        let cols: Vec<String> = header_line
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .collect();

        let find = |names: &[&str]| -> Option<usize> {
            cols.iter().position(|c| names.contains(&c.as_str()))
        };

        let ts_i = find(&["timestamp", "open_time"]);
        let open_i = find(&["open"]);
        let high_i = find(&["high"]);
        let low_i = find(&["low"]);
        let close_i = find(&["close"]);
        let vol_i = find(&["volume"]);

        let mut missing: Vec<&str> = Vec::new();
        if ts_i.is_none() {
            missing.push("timestamp/open_time");
        }
        if open_i.is_none() {
            missing.push("open");
        }
        if high_i.is_none() {
            missing.push("high");
        }
        if low_i.is_none() {
            missing.push("low");
        }
        if close_i.is_none() {
            missing.push("close");
        }
        if vol_i.is_none() {
            missing.push("volume");
        }

        if !missing.is_empty() {
            quality.push_issue(
                DataQualityIssueKind::MissingRequiredColumn,
                None,
                None,
                format!("missing required columns: {}", missing.join(", ")),
            );
            return OhlcvLoadResult {
                candles: Vec::new(),
                quality,
            };
        }

        let (ts_i, open_i, high_i, low_i, close_i, vol_i) = (
            ts_i.unwrap(),
            open_i.unwrap(),
            high_i.unwrap(),
            low_i.unwrap(),
            close_i.unwrap(),
            vol_i.unwrap(),
        );
        let min_fields = [ts_i, open_i, high_i, low_i, close_i, vol_i]
            .iter()
            .copied()
            .max()
            .unwrap_or(0)
            + 1;

        // ── parse data rows ──────────────────────────────────────────────────
        let mut candidates: Vec<Candle> = Vec::new();

        for (line_no, line) in lines {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            quality.total_rows += 1;

            let fields: Vec<&str> = line.split(',').collect();

            if fields.len() < min_fields {
                quality.push_issue(
                    DataQualityIssueKind::MalformedRow,
                    Some(line_no + 1),
                    None,
                    format!(
                        "expected ≥{min_fields} fields, got {} in row '{line}'",
                        fields.len()
                    ),
                );
                quality.rejected_rows += 1;
                continue;
            }

            // Parse timestamp — strictly as positive integer only.
            let ts_str = fields[ts_i].trim();
            let ts_ms = match parse_timestamp_ms(ts_str) {
                Ok(v) => v,
                Err(msg) => {
                    quality.push_issue(
                        DataQualityIssueKind::InvalidTimestamp,
                        Some(line_no + 1),
                        None,
                        format!("cannot parse timestamp '{ts_str}': {msg}"),
                    );
                    quality.rejected_rows += 1;
                    continue;
                }
            };

            // Parse OHLCV — macro avoids repeating error-handling boilerplate
            macro_rules! parse_f {
                ($idx:expr, $label:expr) => {{
                    match fields[$idx].trim().parse::<f64>() {
                        Ok(v) => v,
                        Err(_) => {
                            quality.push_issue(
                                DataQualityIssueKind::InvalidNumber,
                                Some(line_no + 1),
                                Some(ts_ms),
                                format!("cannot parse {} '{}'", $label, fields[$idx].trim()),
                            );
                            quality.rejected_rows += 1;
                            continue;
                        }
                    }
                }};
            }

            let open = parse_f!(open_i, "open");
            let high = parse_f!(high_i, "high");
            let low = parse_f!(low_i, "low");
            let close = parse_f!(close_i, "close");
            let volume = parse_f!(vol_i, "volume");

            // Validate candle geometry and value ranges
            let candle = Candle {
                timestamp: ts_ms,
                open,
                high,
                low,
                close,
                volume,
            };
            if let Err(e) = candle.validate() {
                quality.push_issue(
                    DataQualityIssueKind::InvalidCandle,
                    Some(line_no + 1),
                    Some(ts_ms),
                    e.to_string(),
                );
                quality.rejected_rows += 1;
                continue;
            }

            candidates.push(candle);
        }

        // Header-only file (no data rows at all)
        if quality.total_rows == 0 {
            quality.push_issue(
                DataQualityIssueKind::EmptyFile,
                None,
                None,
                "no data rows found (header only)",
            );
            return OhlcvLoadResult {
                candles: Vec::new(),
                quality,
            };
        }

        // ── detect non-monotonic input (report before sorting) ───────────────
        let already_sorted = candidates
            .windows(2)
            .all(|w| w[0].timestamp <= w[1].timestamp);
        if !already_sorted {
            quality.push_issue(
                DataQualityIssueKind::NonMonotonicTimestamp,
                None,
                None,
                "input rows are not ordered by timestamp; sorted automatically",
            );
        }

        // ── sort ascending ───────────────────────────────────────────────────
        candidates.sort_by_key(|c| c.timestamp);

        // ── dedup: keep first occurrence, reject subsequent duplicates ────────
        let mut deduped: Vec<Candle> = Vec::with_capacity(candidates.len());
        for candle in candidates {
            if let Some(last) = deduped.last() {
                if last.timestamp == candle.timestamp {
                    quality.push_issue(
                        DataQualityIssueKind::DuplicateTimestamp,
                        None,
                        Some(candle.timestamp),
                        format!(
                            "duplicate timestamp {}; first occurrence kept",
                            candle.timestamp
                        ),
                    );
                    quality.rejected_rows += 1;
                    continue;
                }
            }
            deduped.push(candle);
        }

        // ── detect interval issues: missing gaps and irregular intervals ──────
        for i in 1..deduped.len() {
            let prev_ts = deduped[i - 1].timestamp;
            let curr_ts = deduped[i].timestamp;
            let delta = curr_ts - prev_ts;

            if delta == ONE_MINUTE_MS {
                // Exact 1m interval — correct, nothing to report.
            } else if delta > ONE_MINUTE_MS {
                // Gap: one or more 1m candles are absent.
                let missing_count = (delta / ONE_MINUTE_MS) as u64 - 1;
                let expected_next = prev_ts + ONE_MINUTE_MS;

                quality.missing_gaps.push(MissingCandleGap {
                    from_timestamp: prev_ts,
                    to_timestamp: curr_ts,
                    expected_next_timestamp: expected_next,
                    missing_count,
                });
                quality.push_issue(
                    DataQualityIssueKind::MissingCandleGap,
                    None,
                    Some(expected_next),
                    format!(
                        "missing {missing_count} candle(s) between ts={prev_ts} and ts={curr_ts}"
                    ),
                );
            } else {
                // delta < ONE_MINUTE_MS (and > 0, since duplicates were removed).
                // This is a sub-minute interval — data source is not 1m OHLCV.
                quality.push_issue(
                    DataQualityIssueKind::IrregularInterval,
                    None,
                    Some(curr_ts),
                    format!(
                        "irregular 1m interval: prev={prev_ts} current={curr_ts} \
                         delta={delta} expected={ONE_MINUTE_MS}"
                    ),
                );
            }
        }

        quality.valid_candles = deduped.len();
        OhlcvLoadResult {
            candles: deduped,
            quality,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HDR: &str = "timestamp,open,high,low,close,volume";
    const HDR_OT: &str = "open_time,open,high,low,close,volume";

    fn row_ms(ts: i64) -> String {
        format!("{ts},100.0,110.0,90.0,105.0,10.0")
    }
    fn row_s(ts_s: i64) -> String {
        format!("{ts_s},100.0,110.0,90.0,105.0,10.0")
    }

    // ── basic load ───────────────────────────────────────────────────────────

    #[test]
    fn loads_valid_csv_with_timestamp_column() {
        let csv = format!("{HDR}\n{}\n", row_ms(1_700_000_000_000));
        let r = OhlcvLoader::load_csv("test", &csv);
        assert_eq!(r.candles.len(), 1);
        assert!(!r.quality.has_errors());
    }

    #[test]
    fn loads_valid_csv_with_open_time_column() {
        let csv = format!("{HDR_OT}\n{}\n", row_ms(1_700_000_000_000));
        let r = OhlcvLoader::load_csv("test", &csv);
        assert_eq!(r.candles.len(), 1);
        assert!(!r.quality.has_errors());
    }

    // ── timestamp normalisation ───────────────────────────────────────────────

    #[test]
    fn normalises_seconds_timestamp_to_milliseconds() {
        let csv = format!("{HDR}\n{}\n", row_s(1_700_000_000));
        let r = OhlcvLoader::load_csv("test", &csv);
        assert_eq!(r.candles.len(), 1);
        assert_eq!(r.candles[0].timestamp, 1_700_000_000_000);
    }

    #[test]
    fn keeps_milliseconds_timestamp_unchanged() {
        let ts = 1_700_000_060_000_i64;
        let csv = format!("{HDR}\n{}\n", row_ms(ts));
        let r = OhlcvLoader::load_csv("test", &csv);
        assert_eq!(r.candles[0].timestamp, ts);
    }

    #[test]
    fn normalises_positive_seconds_timestamp_to_milliseconds() {
        let csv = format!("{HDR}\n{}\n", row_s(1_700_000_000));
        let r = OhlcvLoader::load_csv("test", &csv);
        assert_eq!(r.candles.len(), 1);
        assert_eq!(r.candles[0].timestamp, 1_700_000_000_000);
        assert!(!r.quality.has_errors());
    }

    #[test]
    fn keeps_positive_milliseconds_timestamp_unchanged() {
        let ts = 1_700_000_060_000_i64;
        let csv = format!("{HDR}\n{}\n", row_ms(ts));
        let r = OhlcvLoader::load_csv("test", &csv);
        assert_eq!(r.candles.len(), 1);
        assert_eq!(r.candles[0].timestamp, ts);
        assert!(!r.quality.has_errors());
    }

    // ── strict timestamp rejection ────────────────────────────────────────────

    #[test]
    fn rejects_decimal_timestamp() {
        let csv = format!("{HDR}\n1700000000.5,100.0,110.0,90.0,105.0,10.0\n");
        let r = OhlcvLoader::load_csv("test", &csv);
        assert!(r.candles.is_empty());
        assert_eq!(r.quality.rejected_rows, 1);
        assert!(
            r.quality
                .issues
                .iter()
                .any(|i| i.kind == DataQualityIssueKind::InvalidTimestamp)
        );
    }

    #[test]
    fn rejects_nan_timestamp() {
        let csv = format!("{HDR}\nNaN,100.0,110.0,90.0,105.0,10.0\n");
        let r = OhlcvLoader::load_csv("test", &csv);
        assert!(r.candles.is_empty());
        assert_eq!(r.quality.rejected_rows, 1);
        assert!(
            r.quality
                .issues
                .iter()
                .any(|i| i.kind == DataQualityIssueKind::InvalidTimestamp)
        );
    }

    #[test]
    fn rejects_infinite_timestamp() {
        for bad in &["inf", "-INF", "Inf", "infinity"] {
            let csv = format!("{HDR}\n{bad},100.0,110.0,90.0,105.0,10.0\n");
            let r = OhlcvLoader::load_csv("test", &csv);
            assert!(r.candles.is_empty(), "expected empty candles for '{bad}'");
            assert!(
                r.quality
                    .issues
                    .iter()
                    .any(|i| i.kind == DataQualityIssueKind::InvalidTimestamp),
                "expected InvalidTimestamp for '{bad}'"
            );
        }
    }

    #[test]
    fn rejects_negative_timestamp() {
        let csv = format!("{HDR}\n-1700000000,100.0,110.0,90.0,105.0,10.0\n");
        let r = OhlcvLoader::load_csv("test", &csv);
        assert!(r.candles.is_empty());
        assert_eq!(r.quality.rejected_rows, 1);
        assert!(
            r.quality
                .issues
                .iter()
                .any(|i| i.kind == DataQualityIssueKind::InvalidTimestamp)
        );
    }

    #[test]
    fn rejects_zero_timestamp() {
        let csv = format!("{HDR}\n0,100.0,110.0,90.0,105.0,10.0\n");
        let r = OhlcvLoader::load_csv("test", &csv);
        assert!(r.candles.is_empty());
        assert_eq!(r.quality.rejected_rows, 1);
        assert!(
            r.quality
                .issues
                .iter()
                .any(|i| i.kind == DataQualityIssueKind::InvalidTimestamp)
        );
    }

    // ── rejection cases ───────────────────────────────────────────────────────

    #[test]
    fn rejects_missing_required_columns() {
        let csv = "open,high,low,close,volume\n100.0,110.0,90.0,105.0,10.0\n";
        let r = OhlcvLoader::load_csv("test", csv);
        assert!(r.candles.is_empty());
        assert!(
            r.quality
                .issues
                .iter()
                .any(|i| i.kind == DataQualityIssueKind::MissingRequiredColumn)
        );
    }

    #[test]
    fn rejects_invalid_number() {
        let csv = format!("{HDR}\n1700000000000,notanumber,110.0,90.0,105.0,10.0\n");
        let r = OhlcvLoader::load_csv("test", &csv);
        assert!(r.candles.is_empty());
        assert!(
            r.quality
                .issues
                .iter()
                .any(|i| i.kind == DataQualityIssueKind::InvalidNumber)
        );
    }

    #[test]
    fn rejects_invalid_timestamp() {
        let csv = format!("{HDR}\nabc,100.0,110.0,90.0,105.0,10.0\n");
        let r = OhlcvLoader::load_csv("test", &csv);
        assert!(r.candles.is_empty());
        assert!(
            r.quality
                .issues
                .iter()
                .any(|i| i.kind == DataQualityIssueKind::InvalidTimestamp)
        );
    }

    #[test]
    fn rejects_invalid_candle_geometry() {
        // high(85) < low(90) → invalid
        let csv = format!("{HDR}\n1700000000000,100.0,85.0,90.0,100.0,10.0\n");
        let r = OhlcvLoader::load_csv("test", &csv);
        assert!(r.candles.is_empty());
        assert!(
            r.quality
                .issues
                .iter()
                .any(|i| i.kind == DataQualityIssueKind::InvalidCandle)
        );
    }

    // ── sorting & dedup ───────────────────────────────────────────────────────

    #[test]
    fn sorts_output_candles_ascending() {
        let csv = format!(
            "{HDR}\n{}\n{}\n",
            row_ms(1_700_000_060_000),
            row_ms(1_700_000_000_000),
        );
        let r = OhlcvLoader::load_csv("test", &csv);
        assert_eq!(r.candles.len(), 2);
        assert!(r.candles[0].timestamp < r.candles[1].timestamp);
    }

    #[test]
    fn detects_non_monotonic_input() {
        let csv = format!(
            "{HDR}\n{}\n{}\n",
            row_ms(1_700_000_060_000),
            row_ms(1_700_000_000_000),
        );
        let r = OhlcvLoader::load_csv("test", &csv);
        assert!(
            r.quality
                .issues
                .iter()
                .any(|i| i.kind == DataQualityIssueKind::NonMonotonicTimestamp)
        );
    }

    #[test]
    fn detects_duplicate_timestamp() {
        let ts = 1_700_000_000_000_i64;
        let csv = format!("{HDR}\n{}\n{}\n", row_ms(ts), row_ms(ts));
        let r = OhlcvLoader::load_csv("test", &csv);
        assert_eq!(r.candles.len(), 1);
        assert!(
            r.quality
                .issues
                .iter()
                .any(|i| i.kind == DataQualityIssueKind::DuplicateTimestamp)
        );
    }

    // ── missing candle gap detection ─────────────────────────────────────────

    #[test]
    fn no_missing_gap_for_continuous_1m_candles() {
        let base = 1_700_000_000_000_i64;
        let rows: String = (0..5)
            .map(|i| row_ms(base + i * ONE_MINUTE_MS))
            .collect::<Vec<_>>()
            .join("\n");
        let csv = format!("{HDR}\n{rows}\n");
        let r = OhlcvLoader::load_csv("test", &csv);
        assert!(r.quality.missing_gaps.is_empty());
        assert_eq!(r.quality.warning_count(), 0);
    }

    #[test]
    fn detects_one_missing_candle() {
        let base = 1_700_000_000_000_i64;
        // jump 2 minutes → 1 candle missing
        let csv = format!(
            "{HDR}\n{}\n{}\n",
            row_ms(base),
            row_ms(base + 2 * ONE_MINUTE_MS),
        );
        let r = OhlcvLoader::load_csv("test", &csv);
        assert_eq!(r.quality.missing_gaps.len(), 1);
        assert_eq!(r.quality.missing_gaps[0].missing_count, 1);
    }

    #[test]
    fn detects_multiple_missing_candles() {
        let base = 1_700_000_000_000_i64;
        // jump 5 minutes → 4 candles missing
        let csv = format!(
            "{HDR}\n{}\n{}\n",
            row_ms(base),
            row_ms(base + 5 * ONE_MINUTE_MS),
        );
        let r = OhlcvLoader::load_csv("test", &csv);
        assert_eq!(r.quality.missing_gaps.len(), 1);
        assert_eq!(r.quality.missing_gaps[0].missing_count, 4);
        assert_eq!(
            r.quality.missing_gaps[0].expected_next_timestamp,
            base + ONE_MINUTE_MS
        );
    }

    // ── irregular interval detection ─────────────────────────────────────────

    #[test]
    fn detects_irregular_sub_minute_interval() {
        // Three candles: t, t+30s, t+60s — delta between first two is 30_000 ms < 60_000.
        let base = 1_700_000_000_000_i64;
        let csv = format!(
            "{HDR}\n{}\n{}\n{}\n",
            row_ms(base),
            row_ms(base + 30_000),            // +30 seconds — irregular
            row_ms(base + 2 * ONE_MINUTE_MS), // +2 min — regular gap from original base
        );
        let r = OhlcvLoader::load_csv("test", &csv);
        assert!(
            r.quality
                .issues
                .iter()
                .any(|i| i.kind == DataQualityIssueKind::IrregularInterval)
        );
        assert!(r.quality.has_errors());
    }

    #[test]
    fn does_not_flag_irregular_interval_for_valid_1m_sequence() {
        let base = 1_700_000_000_000_i64;
        let rows: String = (0..5)
            .map(|i| row_ms(base + i * ONE_MINUTE_MS))
            .collect::<Vec<_>>()
            .join("\n");
        let csv = format!("{HDR}\n{rows}\n");
        let r = OhlcvLoader::load_csv("test", &csv);
        assert!(
            !r.quality
                .issues
                .iter()
                .any(|i| i.kind == DataQualityIssueKind::IrregularInterval)
        );
    }

    #[test]
    fn still_detects_missing_gap_for_delta_above_60000() {
        let base = 1_700_000_000_000_i64;
        let csv = format!(
            "{HDR}\n{}\n{}\n",
            row_ms(base),
            row_ms(base + 3 * ONE_MINUTE_MS), // 3m gap → 2 candles missing
        );
        let r = OhlcvLoader::load_csv("test", &csv);
        assert_eq!(r.quality.missing_gaps.len(), 1);
        assert_eq!(r.quality.missing_gaps[0].missing_count, 2);
        assert!(
            r.quality
                .issues
                .iter()
                .any(|i| i.kind == DataQualityIssueKind::MissingCandleGap)
        );
        assert!(
            !r.quality
                .issues
                .iter()
                .any(|i| i.kind == DataQualityIssueKind::IrregularInterval)
        );
    }
}
