//! DataQualityReport — tracks every issue found while loading OHLCV data.
//!
//! Issue severity:
//!   Errors   — MissingRequiredColumn, MalformedRow, InvalidNumber,
//!              InvalidTimestamp, InvalidCandle, DuplicateTimestamp,
//!              NonMonotonicTimestamp, IrregularInterval, EmptyFile
//!   Warnings — MissingCandleGap (detected and visible, not fatal in Phase 2)

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataQualityIssueKind {
    MissingRequiredColumn,
    MalformedRow,
    InvalidNumber,
    InvalidTimestamp,
    InvalidCandle,
    DuplicateTimestamp,
    NonMonotonicTimestamp,
    MissingCandleGap,
    IrregularInterval,
    EmptyFile,
}

impl DataQualityIssueKind {
    /// Returns true if this kind is treated as an error (not just a warning).
    pub fn is_error(self) -> bool {
        !matches!(self, Self::MissingCandleGap)
    }
}

impl fmt::Display for DataQualityIssueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::MissingRequiredColumn => "missing_required_column",
            Self::MalformedRow => "malformed_row",
            Self::InvalidNumber => "invalid_number",
            Self::InvalidTimestamp => "invalid_timestamp",
            Self::InvalidCandle => "invalid_candle",
            Self::DuplicateTimestamp => "duplicate_timestamp",
            Self::NonMonotonicTimestamp => "non_monotonic_timestamp",
            Self::MissingCandleGap => "missing_candle_gap",
            Self::IrregularInterval => "irregular_interval",
            Self::EmptyFile => "empty_file",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone)]
pub struct DataQualityIssue {
    pub kind: DataQualityIssueKind,
    pub row: Option<usize>,
    pub timestamp: Option<i64>,
    pub message: String,
}

/// A contiguous gap in the sorted 1m candle sequence.
#[derive(Debug, Clone)]
pub struct MissingCandleGap {
    /// Timestamp of the last good candle before the gap.
    pub from_timestamp: i64,
    /// Timestamp of the first good candle after the gap.
    pub to_timestamp: i64,
    /// What the next timestamp should have been (from_timestamp + 60_000 ms).
    pub expected_next_timestamp: i64,
    /// Number of 1m candles absent between from and to.
    pub missing_count: u64,
}

#[derive(Debug, Clone)]
pub struct DataQualityReport {
    pub source: String,
    pub total_rows: usize,
    pub valid_candles: usize,
    pub rejected_rows: usize,
    pub issues: Vec<DataQualityIssue>,
    pub missing_gaps: Vec<MissingCandleGap>,
}

impl DataQualityReport {
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            total_rows: 0,
            valid_candles: 0,
            rejected_rows: 0,
            issues: Vec::new(),
            missing_gaps: Vec::new(),
        }
    }

    /// True if any issue is classified as an error (not just a warning).
    pub fn has_errors(&self) -> bool {
        self.issues.iter().any(|i| i.kind.is_error())
    }

    /// Count of error-level issues.
    pub fn error_count(&self) -> usize {
        self.issues.iter().filter(|i| i.kind.is_error()).count()
    }

    /// Count of warning-level issues (currently: MissingCandleGap only).
    pub fn warning_count(&self) -> usize {
        self.issues.iter().filter(|i| !i.kind.is_error()).count()
    }

    pub fn push_issue(
        &mut self,
        kind: DataQualityIssueKind,
        row: Option<usize>,
        timestamp: Option<i64>,
        message: impl Into<String>,
    ) {
        self.issues.push(DataQualityIssue {
            kind,
            row,
            timestamp,
            message: message.into(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_report() -> DataQualityReport {
        DataQualityReport::new("test_source")
    }

    #[test]
    fn new_report_has_no_errors() {
        let r = make_report();
        assert!(!r.has_errors());
        assert_eq!(r.error_count(), 0);
        assert_eq!(r.warning_count(), 0);
    }

    #[test]
    fn has_errors_true_after_error_issue() {
        let mut r = make_report();
        r.push_issue(
            DataQualityIssueKind::InvalidCandle,
            Some(2),
            None,
            "bad candle",
        );
        assert!(r.has_errors());
        assert_eq!(r.error_count(), 1);
        assert_eq!(r.warning_count(), 0);
    }

    #[test]
    fn missing_gap_is_warning_not_error() {
        let mut r = make_report();
        r.push_issue(
            DataQualityIssueKind::MissingCandleGap,
            None,
            Some(60_000),
            "gap",
        );
        assert!(!r.has_errors());
        assert_eq!(r.error_count(), 0);
        assert_eq!(r.warning_count(), 1);
    }

    #[test]
    fn error_count_counts_only_errors() {
        let mut r = make_report();
        r.push_issue(
            DataQualityIssueKind::DuplicateTimestamp,
            None,
            Some(0),
            "dup",
        );
        r.push_issue(
            DataQualityIssueKind::MissingCandleGap,
            None,
            Some(60_000),
            "gap",
        );
        r.push_issue(
            DataQualityIssueKind::InvalidNumber,
            Some(3),
            None,
            "bad num",
        );
        assert_eq!(r.error_count(), 2);
        assert_eq!(r.warning_count(), 1);
    }

    #[test]
    fn duplicate_timestamp_issue_is_recorded() {
        let mut r = make_report();
        r.push_issue(
            DataQualityIssueKind::DuplicateTimestamp,
            None,
            Some(1_700_000_000_000),
            "duplicate",
        );
        let found = r
            .issues
            .iter()
            .find(|i| i.kind == DataQualityIssueKind::DuplicateTimestamp);
        assert!(found.is_some());
        assert_eq!(found.unwrap().timestamp, Some(1_700_000_000_000));
    }

    #[test]
    fn missing_gap_record_is_stored() {
        let mut r = make_report();
        r.missing_gaps.push(MissingCandleGap {
            from_timestamp: 0,
            to_timestamp: 180_000,
            expected_next_timestamp: 60_000,
            missing_count: 2,
        });
        r.push_issue(
            DataQualityIssueKind::MissingCandleGap,
            None,
            Some(60_000),
            "2 missing",
        );
        assert_eq!(r.missing_gaps.len(), 1);
        assert_eq!(r.missing_gaps[0].missing_count, 2);
        assert_eq!(r.warning_count(), 1);
    }

    #[test]
    fn irregular_interval_is_error() {
        let mut r = make_report();
        r.push_issue(
            DataQualityIssueKind::IrregularInterval,
            None,
            Some(1_700_000_030_000),
            "irregular 1m interval: prev=1700000000000 current=1700000030000 delta=30000 expected=60000",
        );
        assert!(r.has_errors());
        assert_eq!(r.error_count(), 1);
        assert_eq!(r.warning_count(), 0);
    }

    #[test]
    fn irregular_interval_display_string_is_stable() {
        assert_eq!(
            DataQualityIssueKind::IrregularInterval.to_string(),
            "irregular_interval"
        );
    }
}
