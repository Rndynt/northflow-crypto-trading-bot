//! Timeframe — trading bar period with explicit role semantics.
//!
//! Three roles are configurable (no longer fixed to specific values):
//!   - `entry_timeframe`        — entry and execution (e.g. "1m", "5m")
//!   - `confirmation_timeframe` — intermediate confirmation (e.g. "5m", "1h")
//!   - `screening_timeframe`    — market regime / bias (e.g. "15m", "4h")
//!
//! Supported timeframes: 1m, 5m, 15m, 30m, 1h, 4h

use std::fmt;

use crate::core::error::NorthflowError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Timeframe {
    OneMinute,
    FiveMinute,
    FifteenMinute,
    ThirtyMinute,
    OneHour,
    FourHour,
}

impl Timeframe {
    pub fn from_str(s: &str) -> Result<Self, NorthflowError> {
        match s.trim() {
            "1m"  => Ok(Self::OneMinute),
            "5m"  => Ok(Self::FiveMinute),
            "15m" => Ok(Self::FifteenMinute),
            "30m" => Ok(Self::ThirtyMinute),
            "1h"  => Ok(Self::OneHour),
            "4h"  => Ok(Self::FourHour),
            other => Err(NorthflowError::InvalidTimeframe(format!(
                "unknown timeframe '{other}'; expected one of: 1m, 5m, 15m, 30m, 1h, 4h"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::OneMinute     => "1m",
            Self::FiveMinute    => "5m",
            Self::FifteenMinute => "15m",
            Self::ThirtyMinute  => "30m",
            Self::OneHour       => "1h",
            Self::FourHour      => "4h",
        }
    }

    pub fn to_seconds(self) -> u64 {
        match self {
            Self::OneMinute     => 60,
            Self::FiveMinute    => 300,
            Self::FifteenMinute => 900,
            Self::ThirtyMinute  => 1_800,
            Self::OneHour       => 3_600,
            Self::FourHour      => 14_400,
        }
    }

    /// Timeframe duration in milliseconds.
    pub fn to_millis(self) -> i64 {
        self.to_seconds() as i64 * 1_000
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
        assert_eq!(tf.to_millis(), 60_000);
        assert_eq!(tf.as_str(), "1m");
    }

    #[test]
    fn parse_5m() {
        let tf = Timeframe::from_str("5m").unwrap();
        assert_eq!(tf, Timeframe::FiveMinute);
        assert_eq!(tf.to_seconds(), 300);
        assert_eq!(tf.to_millis(), 300_000);
    }

    #[test]
    fn parse_15m() {
        let tf = Timeframe::from_str("15m").unwrap();
        assert_eq!(tf, Timeframe::FifteenMinute);
        assert_eq!(tf.to_seconds(), 900);
        assert_eq!(tf.to_millis(), 900_000);
    }

    #[test]
    fn parse_30m() {
        let tf = Timeframe::from_str("30m").unwrap();
        assert_eq!(tf, Timeframe::ThirtyMinute);
        assert_eq!(tf.to_seconds(), 1_800);
        assert_eq!(tf.to_millis(), 1_800_000);
    }

    #[test]
    fn parse_1h() {
        let tf = Timeframe::from_str("1h").unwrap();
        assert_eq!(tf, Timeframe::OneHour);
        assert_eq!(tf.to_seconds(), 3_600);
        assert_eq!(tf.to_millis(), 3_600_000);
    }

    #[test]
    fn parse_4h() {
        let tf = Timeframe::from_str("4h").unwrap();
        assert_eq!(tf, Timeframe::FourHour);
        assert_eq!(tf.to_seconds(), 14_400);
        assert_eq!(tf.to_millis(), 14_400_000);
    }

    #[test]
    fn invalid_returns_error() {
        assert!(Timeframe::from_str("2h").is_err());
        assert!(Timeframe::from_str("").is_err());
        assert!(Timeframe::from_str("1M").is_err());
    }

    #[test]
    fn ordering() {
        assert!(Timeframe::OneMinute < Timeframe::FiveMinute);
        assert!(Timeframe::FiveMinute < Timeframe::FifteenMinute);
        assert!(Timeframe::FifteenMinute < Timeframe::ThirtyMinute);
        assert!(Timeframe::ThirtyMinute < Timeframe::OneHour);
        assert!(Timeframe::OneHour < Timeframe::FourHour);
    }

    #[test]
    fn as_str_roundtrip() {
        for tf in [
            Timeframe::OneMinute,
            Timeframe::FiveMinute,
            Timeframe::FifteenMinute,
            Timeframe::ThirtyMinute,
            Timeframe::OneHour,
            Timeframe::FourHour,
        ] {
            let parsed = Timeframe::from_str(tf.as_str()).unwrap();
            assert_eq!(parsed, tf, "roundtrip failed for {}", tf.as_str());
        }
    }

    #[test]
    fn display_matches_as_str() {
        assert_eq!(Timeframe::FifteenMinute.to_string(), "15m");
        assert_eq!(Timeframe::FourHour.to_string(), "4h");
    }
}
