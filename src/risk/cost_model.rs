//! Cost model — deterministic round-trip cost estimation.
//!
//! Components:
//!   entry fee, exit fee, spread, slippage, market impact, stop slippage
//!
//! Does not fetch live fees. Does not call network. Does not place orders.

use crate::core::NorthflowError;

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CostModelConfig {
    pub taker_fee_bps: f64,
    pub slippage_bps: f64,
    pub spread_bps: f64,
    pub market_impact_bps: f64,
    pub stop_slippage_bps: f64,
}

// ── Input ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CostModelInput {
    pub entry_price: f64,
    pub exit_price: f64,
    pub qty: f64,
}

// ── Output ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CostBreakdown {
    pub entry_notional: f64,
    pub exit_notional: f64,
    pub entry_fee: f64,
    pub exit_fee: f64,
    pub spread_cost: f64,
    pub slippage_cost: f64,
    pub market_impact_cost: f64,
    pub stop_slippage_cost: f64,
    pub total_expected_cost: f64,
    pub total_adverse_cost: f64,
    pub total_expected_cost_bps: f64,
    pub total_adverse_cost_bps: f64,
}

// ── Model ─────────────────────────────────────────────────────────────────────

pub struct CostModel;

impl CostModel {
    pub fn calculate(
        config: &CostModelConfig,
        input: &CostModelInput,
    ) -> Result<CostBreakdown, NorthflowError> {
        // Validate bps config values.
        for (name, val) in [
            ("taker_fee_bps", config.taker_fee_bps),
            ("slippage_bps", config.slippage_bps),
            ("spread_bps", config.spread_bps),
            ("market_impact_bps", config.market_impact_bps),
            ("stop_slippage_bps", config.stop_slippage_bps),
        ] {
            if !val.is_finite() || val < 0.0 {
                return Err(NorthflowError::ConfigError(format!(
                    "{name} must be finite and >= 0, got {val}"
                )));
            }
        }

        // Validate input.
        if !input.entry_price.is_finite() || input.entry_price <= 0.0 {
            return Err(NorthflowError::ConfigError(format!(
                "entry_price must be finite and > 0, got {}",
                input.entry_price
            )));
        }
        if !input.exit_price.is_finite() || input.exit_price <= 0.0 {
            return Err(NorthflowError::ConfigError(format!(
                "exit_price must be finite and > 0, got {}",
                input.exit_price
            )));
        }
        if !input.qty.is_finite() || input.qty <= 0.0 {
            return Err(NorthflowError::ConfigError(format!(
                "qty must be finite and > 0, got {}",
                input.qty
            )));
        }

        let entry_notional = input.entry_price * input.qty;
        let exit_notional = input.exit_price * input.qty;
        let avg_notional = (entry_notional + exit_notional) / 2.0;

        let entry_fee = entry_notional * config.taker_fee_bps / 10_000.0;
        let exit_fee = exit_notional * config.taker_fee_bps / 10_000.0;
        let spread_cost = avg_notional * config.spread_bps / 10_000.0;
        let slippage_cost = avg_notional * config.slippage_bps / 10_000.0 * 2.0;
        let market_impact_cost = avg_notional * config.market_impact_bps / 10_000.0;
        let stop_slippage_cost = avg_notional * config.stop_slippage_bps / 10_000.0;

        let total_expected_cost =
            entry_fee + exit_fee + spread_cost + slippage_cost + market_impact_cost;
        let total_adverse_cost = total_expected_cost + stop_slippage_cost;

        let total_expected_cost_bps = total_expected_cost / avg_notional * 10_000.0;
        let total_adverse_cost_bps = total_adverse_cost / avg_notional * 10_000.0;

        // Sanity-check all outputs are finite and non-negative.
        for (name, val) in [
            ("entry_fee", entry_fee),
            ("exit_fee", exit_fee),
            ("spread_cost", spread_cost),
            ("slippage_cost", slippage_cost),
            ("market_impact_cost", market_impact_cost),
            ("stop_slippage_cost", stop_slippage_cost),
            ("total_expected_cost", total_expected_cost),
            ("total_adverse_cost", total_adverse_cost),
        ] {
            if !val.is_finite() || val < 0.0 {
                return Err(NorthflowError::DataError(format!(
                    "cost calculation produced invalid {name}: {val}"
                )));
            }
        }

        Ok(CostBreakdown {
            entry_notional,
            exit_notional,
            entry_fee,
            exit_fee,
            spread_cost,
            slippage_cost,
            market_impact_cost,
            stop_slippage_cost,
            total_expected_cost,
            total_adverse_cost,
            total_expected_cost_bps,
            total_adverse_cost_bps,
        })
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> CostModelConfig {
        CostModelConfig {
            taker_fee_bps: 4.0,
            slippage_bps: 2.0,
            spread_bps: 1.0,
            market_impact_bps: 1.0,
            stop_slippage_bps: 5.0,
        }
    }

    fn default_input() -> CostModelInput {
        // entry=100, exit=110, qty=10
        // entry_notional = 1000, exit_notional = 1100, avg = 1050
        CostModelInput {
            entry_price: 100.0,
            exit_price: 110.0,
            qty: 10.0,
        }
    }

    #[test]
    fn cost_model_rejects_negative_bps() {
        let mut cfg = default_config();
        cfg.taker_fee_bps = -1.0;
        assert!(CostModel::calculate(&cfg, &default_input()).is_err());
    }

    #[test]
    fn cost_model_rejects_invalid_price() {
        let input = CostModelInput {
            entry_price: 0.0,
            ..default_input()
        };
        assert!(CostModel::calculate(&default_config(), &input).is_err());
    }

    #[test]
    fn cost_model_rejects_invalid_qty() {
        let input = CostModelInput {
            qty: -5.0,
            ..default_input()
        };
        assert!(CostModel::calculate(&default_config(), &input).is_err());
    }

    #[test]
    fn cost_model_calculates_entry_fee() {
        // entry_notional=1000, fee=4bps → 1000*4/10000 = 0.4
        let b = CostModel::calculate(&default_config(), &default_input()).unwrap();
        assert!((b.entry_fee - 0.4).abs() < 1e-9, "got {}", b.entry_fee);
    }

    #[test]
    fn cost_model_calculates_exit_fee() {
        // exit_notional=1100, fee=4bps → 1100*4/10000 = 0.44
        let b = CostModel::calculate(&default_config(), &default_input()).unwrap();
        assert!((b.exit_fee - 0.44).abs() < 1e-9, "got {}", b.exit_fee);
    }

    #[test]
    fn cost_model_calculates_spread_cost() {
        // avg_notional=1050, spread=1bps → 1050*1/10000 = 0.105
        let b = CostModel::calculate(&default_config(), &default_input()).unwrap();
        assert!(
            (b.spread_cost - 0.105).abs() < 1e-9,
            "got {}",
            b.spread_cost
        );
    }

    #[test]
    fn cost_model_calculates_slippage_cost() {
        // avg_notional=1050, slippage=2bps *2 → 1050*2/10000*2 = 0.42
        let b = CostModel::calculate(&default_config(), &default_input()).unwrap();
        assert!(
            (b.slippage_cost - 0.42).abs() < 1e-9,
            "got {}",
            b.slippage_cost
        );
    }

    #[test]
    fn cost_model_calculates_market_impact_cost() {
        // avg_notional=1050, market_impact=1bps → 1050*1/10000 = 0.105
        let b = CostModel::calculate(&default_config(), &default_input()).unwrap();
        assert!(
            (b.market_impact_cost - 0.105).abs() < 1e-9,
            "got {}",
            b.market_impact_cost
        );
    }

    #[test]
    fn cost_model_calculates_stop_slippage_cost() {
        // avg_notional=1050, stop_slippage=5bps → 1050*5/10000 = 0.525
        let b = CostModel::calculate(&default_config(), &default_input()).unwrap();
        assert!(
            (b.stop_slippage_cost - 0.525).abs() < 1e-9,
            "got {}",
            b.stop_slippage_cost
        );
    }

    #[test]
    fn cost_model_total_expected_cost_excludes_stop_slippage() {
        // entry_fee=0.4 + exit_fee=0.44 + spread=0.105 + slippage=0.42 + impact=0.105 = 1.47
        let b = CostModel::calculate(&default_config(), &default_input()).unwrap();
        let expected =
            b.entry_fee + b.exit_fee + b.spread_cost + b.slippage_cost + b.market_impact_cost;
        assert!(
            (b.total_expected_cost - expected).abs() < 1e-9,
            "got {}",
            b.total_expected_cost
        );
        // stop_slippage NOT included
        assert!(b.total_expected_cost < b.total_adverse_cost);
    }

    #[test]
    fn cost_model_total_adverse_cost_includes_stop_slippage() {
        let b = CostModel::calculate(&default_config(), &default_input()).unwrap();
        let expected = b.total_expected_cost + b.stop_slippage_cost;
        assert!(
            (b.total_adverse_cost - expected).abs() < 1e-9,
            "got {}",
            b.total_adverse_cost
        );
    }

    #[test]
    fn cost_model_outputs_cost_bps() {
        let b = CostModel::calculate(&default_config(), &default_input()).unwrap();
        assert!(b.total_expected_cost_bps > 0.0);
        assert!(b.total_adverse_cost_bps > b.total_expected_cost_bps);
        // sanity: adverse bps = adverse_cost / avg_notional * 10000
        let avg_notional = (100.0 * 10.0 + 110.0 * 10.0) / 2.0;
        let expected_adverse_bps = b.total_adverse_cost / avg_notional * 10_000.0;
        assert!((b.total_adverse_cost_bps - expected_adverse_bps).abs() < 1e-9);
    }
}
