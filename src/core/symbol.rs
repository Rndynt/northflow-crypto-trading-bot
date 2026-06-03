//! Symbol — validated ticker symbol (e.g. BTCUSDT).

use std::fmt;

use crate::core::error::NorthflowError;

const MAX_SYMBOL_LEN: usize = 24;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Symbol(String);

impl Symbol {
    pub fn new(s: &str) -> Result<Self, NorthflowError> {
        if s.is_empty() {
            return Err(NorthflowError::InvalidSignal(
                "symbol must not be empty".to_string(),
            ));
        }
        if s.len() > MAX_SYMBOL_LEN {
            return Err(NorthflowError::InvalidSignal(format!(
                "symbol '{s}' exceeds max length of {MAX_SYMBOL_LEN}"
            )));
        }
        if s.chars().any(|c| c.is_whitespace()) {
            return Err(NorthflowError::InvalidSignal(format!(
                "symbol '{s}' must not contain whitespace"
            )));
        }
        Ok(Self(s.to_uppercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_symbol_uppercased() {
        let s = Symbol::new("btcusdt").unwrap();
        assert_eq!(s.as_str(), "BTCUSDT");
    }

    #[test]
    fn already_uppercase_ok() {
        let s = Symbol::new("ETHUSDT").unwrap();
        assert_eq!(s.as_str(), "ETHUSDT");
    }

    #[test]
    fn empty_symbol_fails() {
        assert!(Symbol::new("").is_err());
    }

    #[test]
    fn whitespace_symbol_fails() {
        assert!(Symbol::new("BTC USDT").is_err());
    }

    #[test]
    fn too_long_symbol_fails() {
        let long = "A".repeat(MAX_SYMBOL_LEN + 1);
        assert!(Symbol::new(&long).is_err());
    }

    #[test]
    fn display_matches_as_str() {
        let s = Symbol::new("SOLUSDT").unwrap();
        assert_eq!(s.to_string(), s.as_str());
    }
}
