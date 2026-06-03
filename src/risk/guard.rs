//! Risk guard — validates a Signal against risk context and computes RiskAssessment.
//!
//! RiskEngine::assess() returns:
//!   Ok(RiskAssessment { approved: true, .. })  — all guards pass; position sizing included.
//!   Ok(RiskAssessment { approved: false, .. }) — one or more guards failed; sizing absent.
//!   Err(NorthflowError)                         — invalid input or config (not a normal rejection).
//!
//! Does not place orders. Does not simulate fills. Does not mutate equity.

use crate::core::{NorthflowError, Signal};
use crate::risk::cost_model::{CostModel, CostModelConfig, CostModelInput};
use crate::risk::position_sizing::{PositionSizer, PositionSizingConfig, PositionSizingInput};

// ── RiskConfig ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RiskConfig {
    pub risk_per_trade_pct: f64,
    pub max_open_positions: usize,
    pub max_leverage: f64,
    pub min_reward_risk: f64,
    pub max_daily_loss_pct: f64,
    pub max_drawdown_pct: f64,
}

// ── RiskContext ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RiskContext {
    pub equity: f64,
    pub peak_equity: f64,
    pub daily_realized_pnl: f64,
    pub open_positions: usize,
}

// ── RiskAssessment ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RiskAssessment {
    pub approved: bool,
    pub signal_id: String,
    pub qty: Option<f64>,
    pub notional: Option<f64>,
    pub leverage_used: Option<f64>,
    pub risk_amount: Option<f64>,
    pub risk_per_unit: Option<f64>,
    pub reward_risk: f64,
    pub expected_reward_bps: f64,
    pub expected_cost_bps: f64,
    pub expected_net_edge_bps: f64,
    pub passed: Vec<String>,
    pub failed: Vec<String>,
}

// ── RiskEngine ────────────────────────────────────────────────────────────────

pub struct RiskEngine;

impl RiskEngine {
    pub fn assess(
        risk_config: &RiskConfig,
        cost_config: &CostModelConfig,
        context: &RiskContext,
        signal: &Signal,
    ) -> Result<RiskAssessment, NorthflowError> {
        // ── 1. Validate signal ────────────────────────────────────────────────
        signal.validate()?;

        // ── 2. Validate risk config ───────────────────────────────────────────
        if !risk_config.risk_per_trade_pct.is_finite() || risk_config.risk_per_trade_pct <= 0.0 {
            return Err(NorthflowError::ConfigError(format!(
                "risk_per_trade_pct must be finite and > 0, got {}",
                risk_config.risk_per_trade_pct
            )));
        }
        if !risk_config.max_leverage.is_finite() || risk_config.max_leverage <= 0.0 {
            return Err(NorthflowError::ConfigError(format!(
                "max_leverage must be finite and > 0, got {}",
                risk_config.max_leverage
            )));
        }
        if !risk_config.min_reward_risk.is_finite() || risk_config.min_reward_risk <= 0.0 {
            return Err(NorthflowError::ConfigError(format!(
                "min_reward_risk must be finite and > 0, got {}",
                risk_config.min_reward_risk
            )));
        }
        if !risk_config.max_daily_loss_pct.is_finite() || risk_config.max_daily_loss_pct <= 0.0 {
            return Err(NorthflowError::ConfigError(format!(
                "max_daily_loss_pct must be finite and > 0, got {}",
                risk_config.max_daily_loss_pct
            )));
        }
        if !risk_config.max_drawdown_pct.is_finite() || risk_config.max_drawdown_pct <= 0.0 {
            return Err(NorthflowError::ConfigError(format!(
                "max_drawdown_pct must be finite and > 0, got {}",
                risk_config.max_drawdown_pct
            )));
        }

        // ── 3. Validate risk context ──────────────────────────────────────────
        if !context.equity.is_finite() || context.equity <= 0.0 {
            return Err(NorthflowError::ConfigError(format!(
                "equity must be finite and > 0, got {}",
                context.equity
            )));
        }
        if !context.peak_equity.is_finite() || context.peak_equity <= 0.0 {
            return Err(NorthflowError::ConfigError(format!(
                "peak_equity must be finite and > 0, got {}",
                context.peak_equity
            )));
        }

        let mut passed: Vec<String> = Vec::new();
        let mut failed: Vec<String> = Vec::new();

        // ── 4. Max open positions ─────────────────────────────────────────────
        if context.open_positions >= risk_config.max_open_positions {
            failed.push("max_open_positions_reached".to_string());
        } else {
            passed.push("max_open_positions_ok".to_string());
        }

        // ── 5. Daily loss guard ───────────────────────────────────────────────
        let daily_loss_pct = (-context.daily_realized_pnl).max(0.0) / context.equity * 100.0;
        if daily_loss_pct >= risk_config.max_daily_loss_pct {
            failed.push("daily_loss_limit_reached".to_string());
        } else {
            passed.push("daily_loss_ok".to_string());
        }

        // ── 6. Max drawdown guard ─────────────────────────────────────────────
        let drawdown_pct = if context.peak_equity >= context.equity {
            (context.peak_equity - context.equity) / context.peak_equity * 100.0
        } else {
            // peak < equity is unexpected but treat as no drawdown for robustness.
            0.0
        };
        if drawdown_pct >= risk_config.max_drawdown_pct {
            failed.push("max_drawdown_reached".to_string());
        } else {
            passed.push("drawdown_ok".to_string());
        }

        // ── 7. Minimum reward/risk ────────────────────────────────────────────
        let reward_risk = signal.reward_risk();
        let epsilon = 1e-9_f64;
        if reward_risk + epsilon < risk_config.min_reward_risk {
            failed.push("reward_risk_below_minimum".to_string());
        } else {
            passed.push("reward_risk_ok".to_string());
        }

        // ── 8. Position sizing ────────────────────────────────────────────────
        let sizing_config = PositionSizingConfig {
            risk_per_trade_pct: risk_config.risk_per_trade_pct,
            max_leverage: risk_config.max_leverage,
        };
        let sizing_input = PositionSizingInput {
            equity: context.equity,
            entry_price: signal.entry_price,
            stop_loss: signal.stop_loss,
        };
        let position_size = PositionSizer::calculate(&sizing_config, &sizing_input)?;

        // ── 9. Cost estimate (using TP as exit for expected scenario) ─────────
        let cost_input = CostModelInput {
            entry_price: signal.entry_price,
            exit_price: signal.take_profit,
            qty: position_size.qty,
        };
        let cost_breakdown = CostModel::calculate(cost_config, &cost_input)?;

        // ── 10. Expected net edge ─────────────────────────────────────────────
        let expected_reward_bps = signal.expected_reward_bps;
        let expected_cost_bps = cost_breakdown.total_adverse_cost_bps;
        let expected_net_edge_bps = expected_reward_bps - expected_cost_bps;

        if expected_net_edge_bps <= 0.0 {
            failed.push("expected_net_edge_not_positive".to_string());
        } else {
            passed.push("expected_net_edge_positive".to_string());
        }

        // ── 11. Build assessment ──────────────────────────────────────────────
        let approved = failed.is_empty();

        let assessment = if approved {
            RiskAssessment {
                approved: true,
                signal_id: signal.signal_id.as_str().to_string(),
                qty: Some(position_size.qty),
                notional: Some(position_size.notional),
                leverage_used: Some(position_size.leverage_used),
                risk_amount: Some(position_size.risk_amount),
                risk_per_unit: Some(position_size.risk_per_unit),
                reward_risk,
                expected_reward_bps,
                expected_cost_bps,
                expected_net_edge_bps,
                passed,
                failed,
            }
        } else {
            RiskAssessment {
                approved: false,
                signal_id: signal.signal_id.as_str().to_string(),
                qty: None,
                notional: None,
                leverage_used: None,
                risk_amount: None,
                risk_per_unit: None,
                reward_risk,
                expected_reward_bps,
                expected_cost_bps,
                expected_net_edge_bps,
                passed,
                failed,
            }
        };

        Ok(assessment)
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Side, Signal, SignalId, StrategyId, Symbol, Timeframe};

    fn default_risk_config() -> RiskConfig {
        RiskConfig {
            risk_per_trade_pct: 1.0,
            max_open_positions: 3,
            max_leverage: 3.0,
            min_reward_risk: 1.5,
            max_daily_loss_pct: 2.0,
            max_drawdown_pct: 5.0,
        }
    }

    fn default_cost_config() -> CostModelConfig {
        CostModelConfig {
            taker_fee_bps: 4.0,
            slippage_bps: 2.0,
            spread_bps: 1.0,
            market_impact_bps: 1.0,
            stop_slippage_bps: 5.0,
        }
    }

    fn default_context() -> RiskContext {
        RiskContext {
            equity: 10_000.0,
            peak_equity: 10_000.0,
            daily_realized_pnl: 0.0,
            open_positions: 0,
        }
    }

    /// Long signal with RR≈2.0, expected_reward_bps=200 (enough to clear cost).
    fn valid_signal() -> Signal {
        Signal {
            signal_id: SignalId::new("SIG-BT-00000001"),
            symbol: Symbol::new("BTCUSDT").unwrap(),
            strategy_id: StrategyId::new("screened_vwap_scalp"),
            side: Side::Long,
            entry_timeframe: Timeframe::OneMinute,
            screening_timeframe: Timeframe::FifteenMinute,
            confirmation_timeframe: Timeframe::FiveMinute,
            entry_time: 1_700_000_000,
            entry_price: 30_000.0,
            stop_loss: 29_700.0,   // risk = 300
            take_profit: 30_600.0, // reward = 600 → RR = 2.0
            confidence: 75,
            regime: "bullish".to_string(),
            entry_reason: "ema_cross".to_string(),
            filters_passed: vec!["screening_bullish".to_string()],
            filters_failed: vec![],
            expected_reward_bps: 200.0,
            estimated_cost_bps: 8.0,
            expected_net_edge_bps: 192.0,
        }
    }

    #[test]
    fn risk_engine_rejects_invalid_signal() {
        let mut sig = valid_signal();
        sig.signal_id = SignalId::new(""); // invalid
        let result = RiskEngine::assess(
            &default_risk_config(),
            &default_cost_config(),
            &default_context(),
            &sig,
        );
        assert!(result.is_err());
    }

    #[test]
    fn risk_engine_rejects_max_open_positions() {
        let ctx = RiskContext {
            open_positions: 3, // >= max_open_positions (3)
            ..default_context()
        };
        let a = RiskEngine::assess(
            &default_risk_config(),
            &default_cost_config(),
            &ctx,
            &valid_signal(),
        )
        .unwrap();
        assert!(!a.approved);
        assert!(a.failed.contains(&"max_open_positions_reached".to_string()));
    }

    #[test]
    fn risk_engine_rejects_daily_loss_limit() {
        // daily_loss_pct = 200/10000*100 = 2.0 >= max_daily_loss_pct(2.0)
        let ctx = RiskContext {
            daily_realized_pnl: -200.0,
            ..default_context()
        };
        let a = RiskEngine::assess(
            &default_risk_config(),
            &default_cost_config(),
            &ctx,
            &valid_signal(),
        )
        .unwrap();
        assert!(!a.approved);
        assert!(a.failed.contains(&"daily_loss_limit_reached".to_string()));
    }

    #[test]
    fn risk_engine_rejects_max_drawdown() {
        // drawdown = (11000-10000)/11000*100 ≈ 9.09% >= 5%
        let ctx = RiskContext {
            equity: 10_000.0,
            peak_equity: 11_000.0,
            ..default_context()
        };
        let a = RiskEngine::assess(
            &default_risk_config(),
            &default_cost_config(),
            &ctx,
            &valid_signal(),
        )
        .unwrap();
        assert!(!a.approved);
        assert!(a.failed.contains(&"max_drawdown_reached".to_string()));
    }

    #[test]
    fn risk_engine_rejects_low_reward_risk() {
        // RR = 1.0 < min_reward_risk(1.5)
        let mut sig = valid_signal();
        sig.take_profit = 30_300.0; // reward=300, risk=300 → RR=1.0
        let a = RiskEngine::assess(
            &default_risk_config(),
            &default_cost_config(),
            &default_context(),
            &sig,
        )
        .unwrap();
        assert!(!a.approved);
        assert!(a.failed.contains(&"reward_risk_below_minimum".to_string()));
    }

    #[test]
    fn risk_engine_rejects_non_positive_net_edge() {
        // Set expected_reward_bps very low so net edge ≤ 0
        let mut sig = valid_signal();
        sig.expected_reward_bps = 1.0; // far below any reasonable cost
        // keep RR ≥ 1.5
        let a = RiskEngine::assess(
            &default_risk_config(),
            &default_cost_config(),
            &default_context(),
            &sig,
        )
        .unwrap();
        assert!(!a.approved);
        assert!(
            a.failed
                .contains(&"expected_net_edge_not_positive".to_string())
        );
    }

    #[test]
    fn risk_engine_approves_valid_signal() {
        let a = RiskEngine::assess(
            &default_risk_config(),
            &default_cost_config(),
            &default_context(),
            &valid_signal(),
        )
        .unwrap();
        assert!(a.approved, "failed reasons: {:?}", a.failed);
    }

    #[test]
    fn approved_assessment_contains_qty() {
        let a = RiskEngine::assess(
            &default_risk_config(),
            &default_cost_config(),
            &default_context(),
            &valid_signal(),
        )
        .unwrap();
        assert!(a.approved);
        assert!(a.qty.is_some());
        assert!(a.notional.is_some());
        assert!(a.leverage_used.is_some());
        assert!(a.risk_amount.is_some());
        assert!(a.risk_per_unit.is_some());
        assert!(a.qty.unwrap() > 0.0);
    }

    #[test]
    fn rejected_assessment_has_no_qty() {
        let ctx = RiskContext {
            open_positions: 3,
            ..default_context()
        };
        let a = RiskEngine::assess(
            &default_risk_config(),
            &default_cost_config(),
            &ctx,
            &valid_signal(),
        )
        .unwrap();
        assert!(!a.approved);
        assert!(a.qty.is_none());
        assert!(a.notional.is_none());
        assert!(a.leverage_used.is_none());
    }

    #[test]
    fn risk_engine_passed_and_failed_reasons_are_stable() {
        let a = RiskEngine::assess(
            &default_risk_config(),
            &default_cost_config(),
            &default_context(),
            &valid_signal(),
        )
        .unwrap();
        assert!(a.passed.contains(&"max_open_positions_ok".to_string()));
        assert!(a.passed.contains(&"daily_loss_ok".to_string()));
        assert!(a.passed.contains(&"drawdown_ok".to_string()));
        assert!(a.passed.contains(&"reward_risk_ok".to_string()));
        assert!(a.passed.contains(&"expected_net_edge_positive".to_string()));
        assert!(a.failed.is_empty());
    }
}
