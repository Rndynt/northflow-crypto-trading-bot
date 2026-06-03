//! Timeframe — trading bar period with explicit role semantics.
//!
//! Three roles are mandatory:
//!   - `entry_timeframe`        = "1m"
//!   - `confirmation_timeframe` = "5m"
//!   - `screening_timeframe`    = "15m"

use std::fmt;

use crate::core::error::NorthflowError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Timeframe {
    OneMinute,
    FiveMinute,
    FifteenMinute,
    OneHour,
}

impl Timeframe {
    pub fn from_str(s: &str) -> Result<Self, NorthflowError> {
        match s.trim() {
            "1m" => Ok(Self::OneMinute),
            "5m" => Ok(Self::FiveMinute),
            "15m" => Ok(Self::FifteenMinute),
            "1h" => Ok(Self::OneHour),
            other => Err(NorthflowError::InvalidTimeframe(format!(
                "unknown timeframe '{other}'; expected one of: 1m, 5m, 15m, 1h"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::OneMinute => "1m",
            Self::FiveMinute => "5m",
            Self::FifteenMinute => "15m",
            Self::OneHour => "1h",
        }
    }

    pub fn to_seconds(self) -> u64 {
        match self {
            Self::OneMinute => 60,
            Self::FiveMinute => 300,
            Self::FifteenMinute => 900,
            Self::OneHour => 3_600,
        }
    }
}

impl fmt::Display for Timeframe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_1m() {
        let tf = Timeframe::from_str("1m").unwrap();
        assert_eq!(tf, Timeframe::OneMinute);
        assert_eq!(tf.to_seconds(), 60);
        assert_eq!(tf.as_str(), "1m");
    }

    #[test]
    fn parse_5m() {
        let tf = Timeframe::from_str("5m").unwrap();
        assert_eq!(tf, Timeframe::FiveMinute);
        assert_eq!(tf.to_seconds(), 300);
        assert_eq!(tf.as_str(), "5m");
    }

    #[test]
    fn parse_15m() {
        let tf = Timeframe::from_str("15m").unwrap();
        assert_eq!(tf, Timeframe::FifteenMinute);
        assert_eq!(tf.to_seconds(), 900);
        assert_eq!(tf.as_str(), "15m");
    }

    #[test]
    fn parse_1h() {
        let tf = Timeframe::from_str("1h").unwrap();
        assert_eq!(tf, Timeframe::OneHour);
        assert_eq!(tf.to_seconds(), 3_600);
        assert_eq!(tf.as_str(), "1h");
    }

    #[test]
    fn invalid_4h_returns_error() {
        assert!(Timeframe::from_str("4h").is_err());
    }

    #[test]
    fn invalid_empty_returns_error() {
        assert!(Timeframe::from_str("").is_err());
    }

    #[test]
    fn case_sensitive_returns_error() {
        assert!(Timeframe::from_str("1M").is_err());
        assert!(Timeframe::from_str("15M").is_err());
    }

    #[test]
    fn as_str_roundtrip() {
        for tf in [
            Timeframe::OneMinute,
            Timeframe::FiveMinute,
            Timeframe::FifteenMinute,
            Timeframe::OneHour,
        ] {
            let parsed = Timeframe::from_str(tf.as_str()).unwrap();
            assert_eq!(parsed, tf, "roundtrip failed for {}", tf.as_str());
        }
    }

    #[test]
    fn display_matches_as_str() {
        let tf = Timeframe::FifteenMinute;
        assert_eq!(tf.to_string(), "15m");
    }
}
