use crate::core::Signal;

#[derive(Debug, Clone)]
pub struct RiskParams {
    pub initial_equity: f64,
    pub risk_per_trade_pct: f64,
    pub max_open_positions: usize,
    pub max_leverage: f64,
    pub min_reward_risk: f64,
    pub max_daily_loss_pct: f64,
    pub max_drawdown_pct: f64,
    pub taker_fee_bps: f64,
    pub slippage_bps: f64,
    pub spread_bps: f64,
    pub market_impact_bps: f64,
}

impl Default for RiskParams {
    fn default() -> Self {
        Self {
            initial_equity: 5000.0,
            risk_per_trade_pct: 0.25,
            max_open_positions: 1,
            max_leverage: 3.0,
            min_reward_risk: 1.5,
            max_daily_loss_pct: 1.5,
            max_drawdown_pct: 5.0,
            taker_fee_bps: 4.0,
            slippage_bps: 2.0,
            spread_bps: 1.0,
            market_impact_bps: 1.0,
        }
    }
}

impl RiskParams {
    /// Total round-trip cost in basis points (entry + exit)
    pub fn round_trip_bps(&self) -> f64 {
        2.0 * (self.taker_fee_bps + self.slippage_bps + self.spread_bps + self.market_impact_bps)
    }

    /// Cost as a fraction of notional (one-way)
    pub fn cost_fraction(&self) -> f64 {
        (self.taker_fee_bps + self.slippage_bps + self.spread_bps + self.market_impact_bps)
            / 10_000.0
    }
}

pub struct RiskManager {
    pub params: RiskParams,
    pub equity: f64,
    pub peak_equity: f64,
    pub open_positions: usize,
    pub daily_loss: f64,
}

impl RiskManager {
    pub fn new(params: RiskParams) -> Self {
        let equity = params.initial_equity;
        Self {
            equity,
            peak_equity: equity,
            open_positions: 0,
            daily_loss: 0.0,
            params,
        }
    }

    /// Returns position size (qty) if the signal passes risk checks, otherwise None.
    pub fn size_position(&self, signal: &Signal) -> Option<f64> {
        if self.open_positions >= self.params.max_open_positions {
            return None;
        }
        let drawdown_pct = (self.peak_equity - self.equity) / self.peak_equity * 100.0;
        if drawdown_pct >= self.params.max_drawdown_pct {
            return None;
        }
        let daily_loss_pct = self.daily_loss / self.params.initial_equity * 100.0;
        if daily_loss_pct >= self.params.max_daily_loss_pct {
            return None;
        }
        if !signal.valid_geometry() || signal.reward_risk() < self.params.min_reward_risk {
            return None;
        }
        let risk_amount = self.equity * self.params.risk_per_trade_pct / 100.0;
        let stop_dist = (signal.entry - signal.stop_loss).abs();
        if stop_dist <= 0.0 {
            return None;
        }
        let qty = risk_amount / stop_dist;
        let max_qty = (self.equity * self.params.max_leverage) / signal.entry;
        Some(qty.min(max_qty))
    }

    pub fn open(&mut self) {
        self.open_positions += 1;
    }

    pub fn close(&mut self, pnl: f64) {
        self.equity += pnl;
        if pnl < 0.0 {
            self.daily_loss += pnl.abs();
        }
        if self.equity > self.peak_equity {
            self.peak_equity = self.equity;
        }
        if self.open_positions > 0 {
            self.open_positions -= 1;
        }
    }

    pub fn reset_daily(&mut self) {
        self.daily_loss = 0.0;
    }
}
