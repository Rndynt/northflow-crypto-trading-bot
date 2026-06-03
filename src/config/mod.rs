//! ResearchConfig — parsed from config/research.toml.
//!
//! Timeframe roles are explicit; never inferred from array order:
//!   entry_timeframe        = "1m"   (entry & execution)
//!   screening_timeframe    = "15m"  (regime bias)
//!   confirmation_timeframe = "5m"   (confirmation)

use std::{fs, path::Path};

use crate::core::{NorthflowError, Timeframe};

#[derive(Debug, Clone)]
pub struct ResearchConfig {
    // pairs
    pub symbols:                Vec<String>,
    /// entry_timeframe = "1m"
    pub entry_timeframe:        String,
    /// screening_timeframe = "15m"
    pub screening_timeframe:    String,
    /// confirmation_timeframe = "5m"
    pub confirmation_timeframe: String,
    // data / output
    pub data_dir:               String,
    pub reports_dir:            String,
    // risk
    pub initial_equity:         f64,
    pub risk_per_trade_pct:     f64,
    pub max_open_positions:     usize,
    pub max_leverage:           f64,
    pub min_reward_risk:        f64,
    pub max_daily_loss_pct:     f64,
    pub max_drawdown_pct:       f64,
    // cost
    pub taker_fee_bps:          f64,
    pub slippage_bps:           f64,
    pub spread_bps:             f64,
    pub market_impact_bps:      f64,
    // backtest
    pub conservative_intrabar:  bool,
    pub min_confidence:         u8,
}

impl Default for ResearchConfig {
    fn default() -> Self {
        Self {
            symbols:                vec!["BTCUSDT".to_string()],
            entry_timeframe:        "1m".to_string(),
            screening_timeframe:    "15m".to_string(),
            confirmation_timeframe: "5m".to_string(),
            data_dir:               "data/historical".to_string(),
            reports_dir:            "reports".to_string(),
            initial_equity:         5000.0,
            risk_per_trade_pct:     0.25,
            max_open_positions:     1,
            max_leverage:           3.0,
            min_reward_risk:        1.5,
            max_daily_loss_pct:     1.5,
            max_drawdown_pct:       5.0,
            taker_fee_bps:          4.0,
            slippage_bps:           2.0,
            spread_bps:             1.0,
            market_impact_bps:      1.0,
            conservative_intrabar:  true,
            min_confidence:         65,
        }
    }
}

impl ResearchConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let raw = fs::read_to_string(path.as_ref())
            .map_err(|e| format!("failed to read config {}: {e}", path.as_ref().display()))?;
        Ok(Self::parse(&raw))
    }

    pub fn parse(raw: &str) -> Self {
        let mut cfg = Self::default();
        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let key   = key.trim();
            let value = value.trim().trim_matches('"');
            match key {
                "symbols"                => cfg.symbols = parse_string_array(value),
                "entry_timeframe"        => cfg.entry_timeframe = value.to_string(),
                "screening_timeframe"    => cfg.screening_timeframe = value.to_string(),
                "confirmation_timeframe" => cfg.confirmation_timeframe = value.to_string(),
                "data_dir"               => cfg.data_dir = value.to_string(),
                "reports_dir"            => cfg.reports_dir = value.to_string(),
                "initial_equity_usd"     => cfg.initial_equity = parse_f64(value, cfg.initial_equity),
                "risk_per_trade_pct"     => cfg.risk_per_trade_pct = parse_f64(value, cfg.risk_per_trade_pct),
                "max_open_positions"     => cfg.max_open_positions = value.parse().unwrap_or(cfg.max_open_positions),
                "max_leverage"           => cfg.max_leverage = parse_f64(value, cfg.max_leverage),
                "min_reward_risk"        => cfg.min_reward_risk = parse_f64(value, cfg.min_reward_risk),
                "max_daily_loss_pct"     => cfg.max_daily_loss_pct = parse_f64(value, cfg.max_daily_loss_pct),
                "max_drawdown_pct"       => cfg.max_drawdown_pct = parse_f64(value, cfg.max_drawdown_pct),
                "taker_fee_bps"          => cfg.taker_fee_bps = parse_f64(value, cfg.taker_fee_bps),
                "slippage_bps"           => cfg.slippage_bps = parse_f64(value, cfg.slippage_bps),
                "spread_bps"             => cfg.spread_bps = parse_f64(value, cfg.spread_bps),
                "market_impact_bps"      => cfg.market_impact_bps = parse_f64(value, cfg.market_impact_bps),
                "conservative_intrabar"  => cfg.conservative_intrabar = value == "true",
                "min_confidence"         => cfg.min_confidence = value.parse().unwrap_or(cfg.min_confidence),
                _ => {}
            }
        }
        cfg
    }

    /// Validate that the three explicit timeframe roles match Phase 2 requirements:
    ///   entry_timeframe        = "1m"
    ///   screening_timeframe    = "15m"
    ///   confirmation_timeframe = "5m"
    ///
    /// Returns `Err` if any value is unparseable or assigned to the wrong role.
    pub fn validate_timeframes(&self) -> Result<(), NorthflowError> {
        let entry = Timeframe::from_str(&self.entry_timeframe).map_err(|e| {
            NorthflowError::ConfigError(format!("entry_timeframe invalid: {e}"))
        })?;
        let screening = Timeframe::from_str(&self.screening_timeframe).map_err(|e| {
            NorthflowError::ConfigError(format!("screening_timeframe invalid: {e}"))
        })?;
        let confirmation = Timeframe::from_str(&self.confirmation_timeframe).map_err(|e| {
            NorthflowError::ConfigError(format!("confirmation_timeframe invalid: {e}"))
        })?;

        if entry != Timeframe::OneMinute {
            return Err(NorthflowError::ConfigError(format!(
                "entry_timeframe must be '1m', got '{}'. \
                 Northflow Phase 2 expects: entry=1m, screening=15m, confirmation=5m",
                self.entry_timeframe
            )));
        }
        if screening != Timeframe::FifteenMinute {
            return Err(NorthflowError::ConfigError(format!(
                "screening_timeframe must be '15m', got '{}'. \
                 Northflow Phase 2 expects: entry=1m, screening=15m, confirmation=5m",
                self.screening_timeframe
            )));
        }
        if confirmation != Timeframe::FiveMinute {
            return Err(NorthflowError::ConfigError(format!(
                "confirmation_timeframe must be '5m', got '{}'. \
                 Northflow Phase 2 expects: entry=1m, screening=15m, confirmation=5m",
                self.confirmation_timeframe
            )));
        }

        Ok(())
    }
}

fn parse_string_array(value: &str) -> Vec<String> {
    let trimmed = value.trim().trim_start_matches('[').trim_end_matches(']');
    let items: Vec<String> = trimmed
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if items.is_empty() { vec!["BTCUSDT".to_string()] } else { items }
}

fn parse_f64(value: &str, default: f64) -> f64 {
    value.trim().parse::<f64>().unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_cfg() -> ResearchConfig {
        ResearchConfig::default()
    }

    #[test]
    fn valid_explicit_timeframe_config_passes() {
        let cfg = default_cfg();
        assert!(cfg.validate_timeframes().is_ok());
    }

    #[test]
    fn invalid_entry_timeframe_string_fails() {
        let mut cfg = default_cfg();
        cfg.entry_timeframe = "4h".to_string();
        let err = cfg.validate_timeframes().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("entry_timeframe"), "expected mention of field: {msg}");
    }

    #[test]
    fn invalid_screening_timeframe_string_fails() {
        let mut cfg = default_cfg();
        cfg.screening_timeframe = "badval".to_string();
        assert!(cfg.validate_timeframes().is_err());
    }

    #[test]
    fn wrong_entry_timeframe_role_fails() {
        let mut cfg = default_cfg();
        cfg.entry_timeframe = "15m".to_string(); // wrong: entry must be 1m
        let err = cfg.validate_timeframes().unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("entry=1m"),
            "error should mention expected roles: {msg}"
        );
    }

    #[test]
    fn wrong_screening_timeframe_role_fails() {
        let mut cfg = default_cfg();
        cfg.screening_timeframe = "1m".to_string(); // wrong: screening must be 15m
        let err = cfg.validate_timeframes().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("entry=1m"), "error should list expected roles: {msg}");
    }

    #[test]
    fn wrong_confirmation_timeframe_role_fails() {
        let mut cfg = default_cfg();
        cfg.confirmation_timeframe = "1h".to_string(); // wrong: confirmation must be 5m
        assert!(cfg.validate_timeframes().is_err());
    }
}
