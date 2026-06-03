//! Risk rejection attribution — deterministic models for tracking rejected signals.
//!
//! `RiskRejection` captures one guard-failure per record.  A single signal that
//! fails N guards produces N rows.
//!
//! `SignalFlowSummary` accumulates per-symbol signal counts and is finalised after
//! the replay loop completes via `SignalFlowSummary::finalise`.

// ── RiskRejection ─────────────────────────────────────────────────────────────

/// One risk rejection record.
///
/// A single signal that fails N guards produces N rows, one per failed guard.
///
/// The `stage` field identifies where in the engine loop the rejection occurred:
/// - `"initial_risk"` — rejected at signal-candle close before a pending entry is queued.
/// - `"actual_entry"` — initially preapproved, then rejected when re-risked using the real
///   next-candle open fill price (including adverse slippage).
#[derive(Debug, Clone)]
pub struct RiskRejection {
    pub signal_id: String,
    pub stage: String,
    pub timestamp: i64,
    pub side: String,
    pub regime: String,
    pub reason: String,
    pub equity: f64,
    pub peak_equity: f64,
    pub drawdown_pct: f64,
    pub daily_realized_pnl: f64,
    pub expected_reward_bps: f64,
    pub expected_cost_bps: f64,
    pub expected_net_edge_bps: f64,
}

// ── SignalFlowSummary ─────────────────────────────────────────────────────────

/// Counters for the full signal-to-trade funnel.
///
/// Incremented during the replay loop; finalised once at the end via
/// `SignalFlowSummary::finalise`.
#[derive(Debug, Clone, Default)]
pub struct SignalFlowSummary {
    /// Number of signals produced by the strategy.
    pub signals_generated: usize,
    /// Signals that passed the initial risk assessment at signal-close price.
    pub signals_preapproved: usize,
    /// Signals rejected at initial risk assessment (at signal-close price).
    pub signals_rejected_initial_risk: usize,
    /// Signals rejected at actual-entry re-risk (at next-candle open).
    pub signals_rejected_actual_entry: usize,
    /// Positions actually opened.
    pub trades_opened: usize,
    /// Trades closed (set by `finalise`).
    pub trades_closed: usize,
    /// Total `RiskRejection` rows (set by `finalise`).
    pub risk_rejections: usize,
    /// Rejection rows with reason `max_drawdown_reached`.
    pub rejections_max_drawdown: usize,
    /// Rejection rows with reason `daily_loss_limit_reached`.
    pub rejections_daily_loss: usize,
    /// Rejection rows with reason `reward_risk_below_minimum`.
    pub rejections_reward_risk: usize,
    /// Rejection rows with reason `expected_net_edge_not_positive`.
    pub rejections_expected_net_edge: usize,
    /// All other rejection rows.
    pub rejections_other: usize,
}

impl SignalFlowSummary {
    /// Finalise aggregated counts from collected rejection rows.
    ///
    /// Must be called once after the replay loop completes, before returning
    /// `BacktestResult`.
    pub fn finalise(&mut self, rejections: &[RiskRejection], trades_closed: usize) {
        self.trades_closed = trades_closed;
        self.risk_rejections = rejections.len();
        for r in rejections {
            match r.reason.as_str() {
                "max_drawdown_reached" => self.rejections_max_drawdown += 1,
                "daily_loss_limit_reached" => self.rejections_daily_loss += 1,
                "reward_risk_below_minimum" => self.rejections_reward_risk += 1,
                "expected_net_edge_not_positive" => self.rejections_expected_net_edge += 1,
                _ => self.rejections_other += 1,
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_rejection(reason: &str) -> RiskRejection {
        make_rejection_with_stage(reason, "initial_risk")
    }

    fn make_rejection_with_stage(reason: &str, stage: &str) -> RiskRejection {
        RiskRejection {
            signal_id: "SIG-BT-00000001".to_string(),
            stage: stage.to_string(),
            timestamp: 1_700_000_000_000,
            side: "long".to_string(),
            regime: "bullish".to_string(),
            reason: reason.to_string(),
            equity: 9_500.0,
            peak_equity: 10_000.0,
            drawdown_pct: 5.0,
            daily_realized_pnl: -100.0,
            expected_reward_bps: 200.0,
            expected_cost_bps: 8.0,
            expected_net_edge_bps: 192.0,
        }
    }

    #[test]
    fn risk_rejection_has_stage() {
        let r = make_rejection_with_stage("max_drawdown_reached", "initial_risk");
        assert_eq!(r.stage, "initial_risk");

        let r2 = make_rejection_with_stage("reward_risk_below_minimum", "actual_entry");
        assert_eq!(r2.stage, "actual_entry");
    }

    #[test]
    fn risk_rejection_reason_counts_are_stable() {
        let rejections = vec![
            make_rejection("max_drawdown_reached"),
            make_rejection("daily_loss_limit_reached"),
            make_rejection("reward_risk_below_minimum"),
            make_rejection("expected_net_edge_not_positive"),
            make_rejection("actual_entry_invalid_geometry"),
            make_rejection("actual_entry_risk_error"),
        ];
        let mut flow = SignalFlowSummary::default();
        flow.finalise(&rejections, 2);

        assert_eq!(flow.trades_closed, 2);
        assert_eq!(flow.risk_rejections, 6);
        assert_eq!(flow.rejections_max_drawdown, 1);
        assert_eq!(flow.rejections_daily_loss, 1);
        assert_eq!(flow.rejections_reward_risk, 1);
        assert_eq!(flow.rejections_expected_net_edge, 1);
        assert_eq!(
            flow.rejections_other, 2,
            "geometry and risk_error rows are 'other'"
        );
    }

    #[test]
    fn finalise_with_empty_rejections_gives_zero_counts() {
        let mut flow = SignalFlowSummary::default();
        flow.finalise(&[], 0);
        assert_eq!(flow.risk_rejections, 0);
        assert_eq!(flow.rejections_max_drawdown, 0);
        assert_eq!(flow.rejections_daily_loss, 0);
        assert_eq!(flow.rejections_reward_risk, 0);
        assert_eq!(flow.rejections_expected_net_edge, 0);
        assert_eq!(flow.rejections_other, 0);
    }

    #[test]
    fn finalise_sets_trades_closed() {
        let mut flow = SignalFlowSummary::default();
        flow.finalise(&[], 7);
        assert_eq!(flow.trades_closed, 7);
    }

    #[test]
    fn finalise_counts_multiple_rows_for_same_reason() {
        let rejections = vec![
            make_rejection("max_drawdown_reached"),
            make_rejection("max_drawdown_reached"),
            make_rejection("max_drawdown_reached"),
        ];
        let mut flow = SignalFlowSummary::default();
        flow.finalise(&rejections, 0);
        assert_eq!(flow.rejections_max_drawdown, 3);
        assert_eq!(flow.risk_rejections, 3);
    }
}
