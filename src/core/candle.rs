//! Candle — OHLCV bar with deterministic validation.

use crate::core::error::NorthflowError;

#[derive(Debug, Clone, Copy, Default)]
pub struct Candle {
    pub timestamp: i64,
    pub open:      f64,
    pub high:      f64,
    pub low:       f64,
    pub close:     f64,
    pub volume:    f64,
}

impl Candle {
    pub fn validate(&self) -> Result<(), NorthflowError> {
        let price_fields = [
            ("open",  self.open),
            ("high",  self.high),
            ("low",   self.low),
            ("close", self.close),
        ];
        for (name, val) in price_fields {
            if !val.is_finite() || val <= 0.0 {
                return Err(NorthflowError::InvalidCandle(
                    format!("{name} must be finite and > 0, got {val}"),
                ));
            }
        }
        if !self.volume.is_finite() || self.volume < 0.0 {
            return Err(NorthflowError::InvalidCandle(
                format!("volume must be finite and >= 0, got {}", self.volume),
            ));
        }
        if self.high < self.low {
            return Err(NorthflowError::InvalidCandle(
                format!("high ({}) < low ({})", self.high, self.low),
            ));
        }
        if self.open < self.low || self.open > self.high {
            return Err(NorthflowError::InvalidCandle(
                format!(
                    "open ({}) outside [low={}, high={}]",
                    self.open, self.low, self.high
                ),
            ));
        }
        if self.close < self.low || self.close > self.high {
            return Err(NorthflowError::InvalidCandle(
                format!(
                    "close ({}) outside [low={}, high={}]",
                    self.close, self.low, self.high
                ),
            ));
        }
        Ok(())
    }

    /// Convenience wrapper — returns `true` if the candle passes all rules.
    pub fn is_valid(&self) -> bool {
        self.validate().is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid() -> Candle {
        Candle {
            timestamp: 0,
            open:      100.0,
            high:      105.0,
            low:       95.0,
            close:     102.0,
            volume:    10.0,
        }
    }

    #[test]
    fn valid_candle_passes() {
        assert!(valid().is_valid());
        assert!(valid().validate().is_ok());
    }

    #[test]
    fn zero_open_fails() {
        let c = Candle { open: 0.0, ..valid() };
        assert!(!c.is_valid());
    }

    #[test]
    fn negative_price_fails() {
        let c = Candle { close: -1.0, ..valid() };
        assert!(!c.is_valid());
    }

    #[test]
    fn infinite_price_fails() {
        let c = Candle { high: f64::INFINITY, ..valid() };
        assert!(!c.is_valid());
    }

    #[test]
    fn nan_price_fails() {
        let c = Candle { low: f64::NAN, ..valid() };
        assert!(!c.is_valid());
    }

    #[test]
    fn negative_volume_fails() {
        let c = Candle { volume: -1.0, ..valid() };
        assert!(!c.is_valid());
    }

    #[test]
    fn zero_volume_is_ok() {
        let c = Candle { volume: 0.0, ..valid() };
        assert!(c.is_valid());
    }

    #[test]
    fn high_lt_low_fails() {
        let c = Candle { high: 90.0, low: 95.0, ..valid() };
        assert!(!c.is_valid());
    }

    #[test]
    fn open_above_high_fails() {
        let c = Candle { open: 110.0, ..valid() };
        assert!(!c.is_valid());
    }

    #[test]
    fn open_below_low_fails() {
        let c = Candle { open: 80.0, ..valid() };
        assert!(!c.is_valid());
    }

    #[test]
    fn close_above_high_fails() {
        let c = Candle { close: 110.0, ..valid() };
        assert!(!c.is_valid());
    }

    #[test]
    fn close_below_low_fails() {
        let c = Candle { close: 80.0, ..valid() };
        assert!(!c.is_valid());
    }

    #[test]
    fn validate_returns_descriptive_error() {
        let c = Candle { high: 90.0, low: 95.0, ..valid() };
        let err = c.validate().unwrap_err();
        assert!(matches!(err, NorthflowError::InvalidCandle(_)));
        let msg = err.to_string();
        assert!(msg.contains("high"), "error should mention 'high': {msg}");
    }
}
