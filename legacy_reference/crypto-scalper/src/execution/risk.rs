//! Risk manager — position sizing, circuit breakers, daily loss / drawdown limits.

use crate::execution::tcm::TransactionCostModel;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskSnapshot {
    pub equity: f64,
    pub peak_equity: f64,
    pub open_positions: u32,
    pub realized_pnl_today: f64,
    pub daily_loss_pct: f64,
    pub drawdown_pct: f64,
    pub tripped: bool,
    pub trip_reason: Option<String>,
    /// True iff trading is paused (either tripped, frozen by SurvivalAgent,
    /// or both). Use `is_blocked()` rather than checking individual flags.
    #[serde(default)]
    pub frozen: bool,
    #[serde(default)]
    pub freeze_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RiskLimits {
    pub risk_per_trade_pct: f64,
    pub max_open_positions: u32,
    pub max_daily_loss_pct: f64,
    pub max_drawdown_pct: f64,
    pub max_leverage: u32,
    pub max_spread_pct: f64,
    pub min_reward_risk: f64,
    pub max_position_notional_pct: f64,
    pub min_net_edge_bps: f64,
    pub assumed_daily_volume_usd: f64,
    /// Minimum margin USD per trade. Floor for position sizing.
    pub min_margin_usd: f64,
}

#[derive(Debug)]
struct Inner {
    limits: RiskLimits,
    equity: f64,
    peak_equity: f64,
    open_positions: u32,
    realized_pnl_today: f64,
    tripped: bool,
    trip_reason: Option<String>,
    frozen: bool,
    freeze_reason: Option<String>,
    /// Multiplier applied on top of `risk_per_trade_pct` when sizing.
    /// SurvivalAgent uses this to scale risk down (or up) based on
    /// the current `survive_score`. Default = 1.0.
    size_multiplier: f64,
    /// Initial equity captured at boot. Used as the "death line" baseline:
    /// when current equity drops below `initial_equity * death_line_pct`
    /// the SurvivalAgent freezes everything.
    initial_equity: f64,
}

#[derive(Clone)]
pub struct RiskManager {
    inner: Arc<Mutex<Inner>>,
}

impl RiskManager {
    pub fn new(limits: RiskLimits, equity: f64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                limits,
                equity,
                peak_equity: equity,
                open_positions: 0,
                realized_pnl_today: 0.0,
                tripped: false,
                trip_reason: None,
                frozen: false,
                freeze_reason: None,
                size_multiplier: 1.0,
                initial_equity: equity,
            })),
        }
    }

    /// Replace the in-memory equity with a fresh value (e.g. fetched
    /// from the exchange). Adjusts `peak_equity` if the new value is
    /// higher. Resets `tripped` if equity recovers above the limits.
    /// Load persisted equity from disk (paper mode).
    pub fn load_equity_from_disk(&self) {
        const EQUITY_FILE: &str = "data/equity.json";
        if let Ok(data) = std::fs::read_to_string(EQUITY_FILE) {
            if let Ok(snap) = serde_json::from_str::<serde_json::Value>(&data) {
                let equity = snap.get("equity").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let peak = snap
                    .get("peak_equity")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let pnl_today = snap
                    .get("realized_pnl_today")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                if equity > 0.0 {
                    let mut i = self.inner.lock();
                    i.equity = equity;
                    i.peak_equity = peak.max(equity);
                    i.realized_pnl_today = pnl_today;
                    tracing::info!(equity, peak, pnl_today, "loaded persisted equity from disk");
                }
            }
        }
    }

    /// Save current equity to disk.
    pub fn save_equity_to_disk(&self) {
        const EQUITY_FILE: &str = "data/equity.json";
        let i = self.inner.lock();
        let snap = serde_json::json!({
            "equity": i.equity,
            "peak_equity": i.peak_equity,
            "initial_equity": i.initial_equity,
            "realized_pnl_today": i.realized_pnl_today,
            "updated_at": chrono::Utc::now().to_rfc3339(),
        });
        drop(i);
        if let Some(parent) = std::path::Path::new(EQUITY_FILE).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(EQUITY_FILE, snap.to_string());
    }

    pub fn set_equity(&self, equity: f64) {
        let mut i = self.inner.lock();
        i.equity = equity;
        if equity > i.peak_equity {
            i.peak_equity = equity;
        }
        drop(i);
        self.save_equity_to_disk();
    }

    pub fn equity(&self) -> f64 {
        self.inner.lock().equity
    }

    pub fn initial_equity(&self) -> f64 {
        self.inner.lock().initial_equity
    }

    pub fn open_positions(&self) -> u32 {
        self.inner.lock().open_positions
    }

    pub fn set_open_positions(&self, n: u32) {
        self.inner.lock().open_positions = n;
    }

    pub fn realized_pnl_today(&self) -> f64 {
        self.inner.lock().realized_pnl_today
    }

    /// SurvivalAgent calls this to scale all future trade sizing
    /// (multiplier ∈ [0.0, 2.0] with 0.0 meaning "do not size at all").
    pub fn set_max_leverage(&self, leverage: u32) {
        let mut i = self.inner.lock();
        i.limits.max_leverage = leverage.max(1);
    }

    pub fn set_risk_per_trade_pct(&self, pct: f64) {
        let mut i = self.inner.lock();
        i.limits.risk_per_trade_pct = pct.clamp(0.1, 10.0);
    }

    pub fn set_max_open_positions(&self, n: u32) {
        let mut i = self.inner.lock();
        i.limits.max_open_positions = n.max(1);
    }

    pub fn set_max_daily_loss_pct(&self, pct: f64) {
        let mut i = self.inner.lock();
        i.limits.max_daily_loss_pct = pct.clamp(1.0, 50.0);
    }

    pub fn set_size_multiplier(&self, m: f64) {
        let mut i = self.inner.lock();
        i.size_multiplier = m.clamp(0.0, 2.0);
    }

    pub fn size_multiplier(&self) -> f64 {
        self.inner.lock().size_multiplier
    }

    /// Pause new entries without tripping the daily-loss circuit.
    /// Used by SurvivalAgent / Telegram /freeze command.
    pub fn freeze(&self, reason: impl Into<String>) {
        let mut i = self.inner.lock();
        i.frozen = true;
        i.freeze_reason = Some(reason.into());
    }

    pub fn unfreeze(&self) {
        let mut i = self.inner.lock();
        i.frozen = false;
        i.freeze_reason = None;
    }

    pub fn is_frozen(&self) -> bool {
        self.inner.lock().frozen
    }

    pub fn is_blocked(&self) -> bool {
        let i = self.inner.lock();
        i.tripped || i.frozen
    }

    pub fn snapshot(&self) -> RiskSnapshot {
        let i = self.inner.lock();
        let dd = if i.peak_equity > 0.0 {
            ((i.peak_equity - i.equity) / i.peak_equity * 100.0).max(0.0)
        } else {
            0.0
        };
        let daily_loss_pct = if i.equity > 0.0 && i.realized_pnl_today < 0.0 {
            -i.realized_pnl_today / i.equity * 100.0
        } else {
            0.0
        };
        RiskSnapshot {
            equity: i.equity,
            peak_equity: i.peak_equity,
            open_positions: i.open_positions,
            realized_pnl_today: i.realized_pnl_today,
            daily_loss_pct,
            drawdown_pct: dd,
            tripped: i.tripped,
            trip_reason: i.trip_reason.clone(),
            frozen: i.frozen,
            freeze_reason: i.freeze_reason.clone(),
        }
    }

    pub fn limits(&self) -> RiskLimits {
        self.inner.lock().limits.clone()
    }

    /// True iff a new position can be opened under current risk state.
    pub fn can_open_position(&self) -> std::result::Result<(), String> {
        let i = self.inner.lock();
        if i.tripped {
            return Err(format!(
                "circuit tripped: {}",
                i.trip_reason.clone().unwrap_or_default()
            ));
        }
        if i.frozen {
            return Err(format!(
                "frozen by survival/operator: {}",
                i.freeze_reason.clone().unwrap_or_default()
            ));
        }
        if i.open_positions >= i.limits.max_open_positions {
            return Err(format!(
                "open positions {} / {}",
                i.open_positions, i.limits.max_open_positions
            ));
        }
        let dd = if i.peak_equity > 0.0 {
            ((i.peak_equity - i.equity) / i.peak_equity * 100.0).max(0.0)
        } else {
            0.0
        };
        if dd >= i.limits.max_drawdown_pct {
            return Err(format!("drawdown {dd:.2}% >= limit"));
        }
        if i.realized_pnl_today < 0.0
            && (-i.realized_pnl_today / i.equity * 100.0) >= i.limits.max_daily_loss_pct
        {
            return Err("daily loss limit reached".into());
        }
        Ok(())
    }

    pub fn validate_signal(
        &self,
        entry: f64,
        stop_loss: f64,
        take_profit: f64,
        side: &crate::data::Side,
        spread_pct: Option<f64>,
        tcm: &TransactionCostModel,
    ) -> std::result::Result<(), String> {
        let i = self.inner.lock();
        let risk_per_unit = (entry - stop_loss).abs();
        if entry <= 0.0 || risk_per_unit <= 0.0 {
            return Err("invalid entry/stop distance".into());
        }

        // Direction sanity — TP must be on correct side of entry
        // Without this check, a LONG with tp < entry passes R:R via .abs()
        match side {
            crate::data::Side::Long => {
                if stop_loss >= entry {
                    return Err(format!("LONG sl {stop_loss:.4} >= entry {entry:.4}"));
                }
                if take_profit <= entry {
                    return Err(format!(
                        "LONG tp {take_profit:.4} <= entry {entry:.4} — TP on wrong side"
                    ));
                }
            }
            crate::data::Side::Short => {
                if stop_loss <= entry {
                    return Err(format!("SHORT sl {stop_loss:.4} <= entry {entry:.4}"));
                }
                if take_profit >= entry {
                    return Err(format!(
                        "SHORT tp {take_profit:.4} >= entry {entry:.4} — TP on wrong side"
                    ));
                }
            }
        }

        let reward_per_unit = (take_profit - entry).abs();
        let rr = reward_per_unit / risk_per_unit;
        if rr < i.limits.min_reward_risk {
            return Err(format!(
                "reward/risk {rr:.2} < {:.2}",
                i.limits.min_reward_risk
            ));
        }
        if let Some(spread) = spread_pct {
            if spread > i.limits.max_spread_pct {
                return Err(format!(
                    "spread {spread:.4}% > {:.4}%",
                    i.limits.max_spread_pct
                ));
            }
        }
        let margin = i.equity * i.limits.risk_per_trade_pct / 100.0;
        let leverage_cap = i.equity * i.limits.max_leverage as f64 / entry.max(1e-9);
        let notional = margin * i.limits.max_leverage as f64;
        let size = (notional / entry.max(1e-9)).min(leverage_cap).max(0.0);
        // For HFT scalping: skip the net edge gate entirely when
        // min_net_edge_bps <= 0.  The reward/risk check above already
        // ensures positive expected value.  The TCM round-trip cost
        // model is calibrated for slower trading; at 5m scalping
        // frequency the per-trade cost is already captured by the
        // tighter SL/TP distances.
        if i.limits.min_net_edge_bps > 0.0 {
            let gross_edge_bps = reward_per_unit / entry * 10_000.0;
            let net_edge_bps = gross_edge_bps
                - tcm.round_trip_cost_bps(size * entry, i.limits.assumed_daily_volume_usd);
            if net_edge_bps < i.limits.min_net_edge_bps {
                return Err(format!(
                    "net edge {net_edge_bps:.2} bps < {:.2} bps after costs",
                    i.limits.min_net_edge_bps
                ));
            }
        }
        Ok(())
    }

    /// Calculate qty using risk-based sizing: risk_amount / SL_distance.
    /// This ensures each trade risks exactly risk_per_trade_pct of equity,
    /// regardless of stop loss distance. Wide SL = smaller position, tight SL = larger.
    pub fn calculate_size(&self, entry: f64, stop_loss: f64) -> f64 {
        let i = self.inner.lock();
        if entry <= 0.0 {
            return 0.0;
        }
        let risk_amount = i.equity * i.limits.risk_per_trade_pct / 100.0;
        if risk_amount <= 0.0 {
            return 0.0;
        }
        let sl_distance = (entry - stop_loss).abs();
        if sl_distance <= 0.0 {
            // Fallback: use margin-based sizing if SL is missing
            let margin = risk_amount;
            let notional = margin * i.limits.max_leverage as f64;
            let qty = notional / entry.max(1e-9);
            let leverage_cap = i.equity * i.limits.max_leverage as f64 / entry.max(1e-9);
            return qty.min(leverage_cap).max(0.0);
        }
        // Risk-based: qty = risk_amount / sl_distance_per_unit
        let qty = risk_amount / sl_distance;
        // Never lift size to satisfy a minimum-margin preference. This
        // function is the source of truth for risk-per-trade sizing, so
        // increasing qty here would silently risk more than the configured
        // percent whenever the stop distance is wide or policy multipliers
        // have reduced the trade. Exchange minimums must be handled as an
        // execution/risk rejection, not by overriding the risk budget.
        // Respect leverage cap
        let leverage_cap = i.equity * i.limits.max_leverage as f64 / entry.max(1e-9);
        let qty = qty.min(leverage_cap);

        // Enforce max position notional cap as a fraction of total leveraged capacity
        // (equity × max_leverage), NOT bare equity. This makes the config meaningful
        // for high-leverage accounts: 150% means max notional = 1.5× full capacity,
        // which is effectively a no-op since the leverage_cap above already binds at 100%.
        // Values below 100% let operators limit per-position size as a fraction of max.
        if i.limits.max_position_notional_pct > 0.0 {
            let max_notional = i.equity
                * i.limits.max_leverage as f64
                * i.limits.max_position_notional_pct
                / 100.0;
            let notional_cap = max_notional / entry.max(1e-9);
            qty.min(notional_cap).max(0.0)
        } else {
            qty.max(0.0)
        }
    }

    /// Calculate position size with LLM conviction scaling.
    /// `llm_size_pct`: 0.0-1.0 from LLM based on confidence, Kelly, and risk factors.
    /// High conviction (>70%) = 1.0, Medium (50-70%) = 0.7, Low (<50%) = 0.4
    /// Also applies funding rate adjustments if extreme.
    pub fn calculate_llm_sized_position(
        &self,
        entry: f64,
        stop_loss: f64,
        llm_size_pct: f64,
    ) -> f64 {
        let max_size = self.calculate_size(entry, stop_loss);
        let scaled_size = max_size * llm_size_pct.clamp(0.1, 1.0);
        scaled_size.max(0.0)
    }

    pub fn on_position_opened(&self) {
        self.inner.lock().open_positions += 1;
    }

    pub fn on_position_closed(&self, realized_pnl: f64) {
        {
            let mut i = self.inner.lock();
            if i.open_positions > 0 {
                i.open_positions -= 1;
            }
            i.realized_pnl_today += realized_pnl;
            i.equity += realized_pnl;
            if i.equity > i.peak_equity {
                i.peak_equity = i.equity;
            }
            let dd = if i.peak_equity > 0.0 {
                ((i.peak_equity - i.equity) / i.peak_equity * 100.0).max(0.0)
            } else {
                0.0
            };
            if dd >= i.limits.max_drawdown_pct && !i.tripped {
                i.tripped = true;
                i.trip_reason = Some(format!("max drawdown {dd:.2}%"));
            }
            let loss_pct = if i.equity > 0.0 && i.realized_pnl_today < 0.0 {
                -i.realized_pnl_today / i.equity * 100.0
            } else {
                0.0
            };
            if loss_pct >= i.limits.max_daily_loss_pct && !i.tripped {
                i.tripped = true;
                i.trip_reason = Some(format!("daily loss {loss_pct:.2}%"));
            }
        }
        self.save_equity_to_disk();
    }

    pub fn reset_daily(&self) {
        let mut i = self.inner.lock();
        i.realized_pnl_today = 0.0;
        i.tripped = false;
        i.trip_reason = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_limits() -> RiskLimits {
        RiskLimits {
            risk_per_trade_pct: 1.0,
            max_open_positions: 3,
            max_daily_loss_pct: 3.0,
            max_drawdown_pct: 10.0,
            max_leverage: 5,
            max_spread_pct: 0.03,
            min_reward_risk: 1.2,
            max_position_notional_pct: 100.0,
            min_net_edge_bps: 1.0,
            assumed_daily_volume_usd: 1_000_000_000.0,
            min_margin_usd: 6.0,
        }
    }

    #[test]
    fn size_calculation() {
        let r = RiskManager::new(default_limits(), 10_000.0);
        let size = r.calculate_size(100.0, 99.0);
        // equity 10000 * 1% = $100 risk; SL distance = $1; qty = 100 / 1 = 100.
        approx::assert_abs_diff_eq!(size, 100.0, epsilon = 1e-6);
    }

    #[test]
    fn circuit_trips_on_daily_loss() {
        let r = RiskManager::new(default_limits(), 1000.0);
        r.on_position_closed(-40.0); // 4% loss > 3% limit
        let s = r.snapshot();
        assert!(s.tripped);
        assert!(r.can_open_position().is_err());
    }

    #[test]
    fn circuit_trips_on_drawdown() {
        let r = RiskManager::new(default_limits(), 1000.0);
        r.on_position_closed(100.0); // peak 1100
        r.on_position_closed(-120.0); // dd ~ 11%
        let s = r.snapshot();
        assert!(s.tripped);
    }

    #[test]
    fn rejects_bad_reward_risk_and_wide_spread() {
        let r = RiskManager::new(default_limits(), 1000.0);
        let tcm = TransactionCostModel {
            taker_fee_bps: 4.0,
            maker_fee_bps: -1.0,
            avg_slippage_bps: 2.0,
            market_impact_bps: 1.0,
        };
        assert!(
            r.validate_signal(
                100.0,
                99.0,
                100.5,
                &crate::data::Side::Long,
                Some(0.01),
                &tcm
            )
            .is_err()
        );
        assert!(
            r.validate_signal(
                100.0,
                99.0,
                101.5,
                &crate::data::Side::Long,
                Some(0.04),
                &tcm
            )
            .is_err()
        );
        assert!(
            r.validate_signal(
                100.0,
                99.0,
                101.5,
                &crate::data::Side::Long,
                Some(0.01),
                &tcm
            )
            .is_ok()
        );
    }

    #[test]
    fn size_respects_leverage_cap() {
        let mut limits = default_limits();
        limits.max_leverage = 1; // only 1x
        limits.risk_per_trade_pct = 1.0;
        let r = RiskManager::new(limits, 1000.0);
        let size = r.calculate_size(100.0, 99.0);
        // risk = 1000 * 1% = $10; SL distance = $1; qty = 10,
        // which is exactly the 1x leverage cap.
        approx::assert_abs_diff_eq!(size, 10.0, epsilon = 1e-6);
    }

    #[test]
    fn rejects_signal_without_net_edge_after_costs() {
        let mut limits = default_limits();
        limits.min_reward_risk = 1.0;
        limits.min_net_edge_bps = 20.0;
        let r = RiskManager::new(limits, 1000.0);
        let tcm = TransactionCostModel {
            taker_fee_bps: 4.0,
            maker_fee_bps: -1.0,
            avg_slippage_bps: 2.0,
            market_impact_bps: 1.0,
        };
        assert!(
            r.validate_signal(
                100.0,
                99.9,
                100.1,
                &crate::data::Side::Long,
                Some(0.01),
                &tcm
            )
            .is_err()
        );
    }
}
