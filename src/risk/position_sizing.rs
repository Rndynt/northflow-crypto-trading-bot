//! Position sizing — deterministic, equity-based risk calculation.
//!
//! Sizing rules:
//!   risk_amount          = equity * risk_per_trade_pct / 100
//!   qty_by_risk          = risk_amount / |entry - stop_loss|
//!   max_qty_by_leverage  = equity * max_leverage / entry
//!   qty                  = min(qty_by_risk, max_qty_by_leverage)
//!
//! Does not place orders. Does not simulate fills. Does not update equity.

use crate::core::NorthflowError;

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PositionSizingConfig {
    pub risk_per_trade_pct: f64,
    pub max_leverage: f64,
}

// ── Input ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PositionSizingInput {
    pub equity: f64,
    pub entry_price: f64,
    pub stop_loss: f64,
}

// ── Output ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PositionSize {
    pub qty: f64,
    pub qty_by_risk: f64,
    pub max_qty_by_leverage: f64,
    pub risk_amount: f64,
    pub risk_per_unit: f64,
    pub notional: f64,
    pub leverage_used: f64,
}

// ── Sizer ─────────────────────────────────────────────────────────────────────

pub struct PositionSizer;

impl PositionSizer {
    pub fn calculate(
        config: &PositionSizingConfig,
        input: &PositionSizingInput,
    ) -> Result<PositionSize, NorthflowError> {
        // Validate config.
        if !config.risk_per_trade_pct.is_finite() || config.risk_per_trade_pct <= 0.0 {
            return Err(NorthflowError::ConfigError(format!(
                "risk_per_trade_pct must be finite and > 0, got {}",
                config.risk_per_trade_pct
            )));
        }
        if !config.max_leverage.is_finite() || config.max_leverage <= 0.0 {
            return Err(NorthflowError::ConfigError(format!(
                "max_leverage must be finite and > 0, got {}",
                config.max_leverage
            )));
        }

        // Validate input.
        if !input.equity.is_finite() || input.equity <= 0.0 {
            return Err(NorthflowError::ConfigError(format!(
                "equity must be finite and > 0, got {}",
                input.equity
            )));
        }
        if !input.entry_price.is_finite() || input.entry_price <= 0.0 {
            return Err(NorthflowError::ConfigError(format!(
                "entry_price must be finite and > 0, got {}",
                input.entry_price
            )));
        }
        if !input.stop_loss.is_finite() || input.stop_loss <= 0.0 {
            return Err(NorthflowError::ConfigError(format!(
                "stop_loss must be finite and > 0, got {}",
                input.stop_loss
            )));
        }

        let risk_per_unit = (input.entry_price - input.stop_loss).abs();
        if risk_per_unit <= 0.0 {
            return Err(NorthflowError::ConfigError(format!(
                "risk_per_unit (|entry - stop_loss|) must be > 0, got {risk_per_unit}"
            )));
        }

        let risk_amount = input.equity * config.risk_per_trade_pct / 100.0;
        let qty_by_risk = risk_amount / risk_per_unit;
        let max_qty_by_leverage = input.equity * config.max_leverage / input.entry_price;
        let qty = qty_by_risk.min(max_qty_by_leverage);

        if !qty.is_finite() || qty <= 0.0 {
            return Err(NorthflowError::ConfigError(format!(
                "calculated qty must be finite and > 0, got {qty}"
            )));
        }

        let notional = qty * input.entry_price;
        let leverage_used = notional / input.equity;

        if !notional.is_finite() || notional <= 0.0 {
            return Err(NorthflowError::ConfigError(format!(
                "calculated notional must be finite and > 0, got {notional}"
            )));
        }

        let epsilon = 1e-9;
        if leverage_used > config.max_leverage + epsilon {
            return Err(NorthflowError::ConfigError(format!(
                "leverage_used ({leverage_used}) exceeds max_leverage ({})",
                config.max_leverage
            )));
        }

        Ok(PositionSize {
            qty,
            qty_by_risk,
            max_qty_by_leverage,
            risk_amount,
            risk_per_unit,
            notional,
            leverage_used,
        })
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> PositionSizingConfig {
        PositionSizingConfig {
            risk_per_trade_pct: 1.0,
            max_leverage: 3.0,
        }
    }

    fn default_input() -> PositionSizingInput {
        PositionSizingInput {
            equity: 10_000.0,
            entry_price: 100.0,
            stop_loss: 98.0,
        }
    }

    #[test]
    fn position_sizing_rejects_invalid_equity() {
        let input = PositionSizingInput {
            equity: -1.0,
            ..default_input()
        };
        assert!(PositionSizer::calculate(&default_config(), &input).is_err());
    }

    #[test]
    fn position_sizing_rejects_zero_risk_per_trade() {
        let config = PositionSizingConfig {
            risk_per_trade_pct: 0.0,
            ..default_config()
        };
        assert!(PositionSizer::calculate(&config, &default_input()).is_err());
    }

    #[test]
    fn position_sizing_rejects_zero_max_leverage() {
        let config = PositionSizingConfig {
            max_leverage: 0.0,
            ..default_config()
        };
        assert!(PositionSizer::calculate(&config, &default_input()).is_err());
    }

    #[test]
    fn position_sizing_rejects_zero_entry_price() {
        let input = PositionSizingInput {
            entry_price: 0.0,
            ..default_input()
        };
        assert!(PositionSizer::calculate(&default_config(), &input).is_err());
    }

    #[test]
    fn position_sizing_rejects_zero_risk_per_unit() {
        // entry == stop_loss → risk_per_unit = 0
        let input = PositionSizingInput {
            entry_price: 100.0,
            stop_loss: 100.0,
            ..default_input()
        };
        assert!(PositionSizer::calculate(&default_config(), &input).is_err());
    }

    #[test]
    fn position_sizing_calculates_risk_amount() {
        // equity=10000, risk_pct=1.0 → risk_amount=100
        let ps = PositionSizer::calculate(&default_config(), &default_input()).unwrap();
        assert!((ps.risk_amount - 100.0).abs() < 1e-9);
    }

    #[test]
    fn position_sizing_calculates_qty_by_risk() {
        // risk_amount=100, risk_per_unit=|100-98|=2 → qty_by_risk=50
        let ps = PositionSizer::calculate(&default_config(), &default_input()).unwrap();
        assert!((ps.qty_by_risk - 50.0).abs() < 1e-9);
    }

    #[test]
    fn position_sizing_applies_leverage_cap() {
        // max_leverage=3 → max_qty = 10000*3/100 = 300
        // qty_by_risk = 10000*10/100 / 2 = 500 → capped at 300
        let config = PositionSizingConfig {
            risk_per_trade_pct: 10.0,
            max_leverage: 3.0,
        };
        let ps = PositionSizer::calculate(&config, &default_input()).unwrap();
        assert!(
            (ps.qty - 300.0).abs() < 1e-9,
            "expected 300, got {}",
            ps.qty
        );
        assert!((ps.max_qty_by_leverage - 300.0).abs() < 1e-9);
    }

    #[test]
    fn position_sizing_uses_risk_qty_when_below_leverage_cap() {
        // qty_by_risk=50 < max_qty=300 → qty=50
        let ps = PositionSizer::calculate(&default_config(), &default_input()).unwrap();
        assert!((ps.qty - 50.0).abs() < 1e-9);
    }

    #[test]
    fn position_sizing_outputs_notional_and_leverage() {
        // qty=50, entry=100 → notional=5000, leverage=5000/10000=0.5
        let ps = PositionSizer::calculate(&default_config(), &default_input()).unwrap();
        assert!((ps.notional - 5_000.0).abs() < 1e-9);
        assert!((ps.leverage_used - 0.5).abs() < 1e-9);
    }
}
