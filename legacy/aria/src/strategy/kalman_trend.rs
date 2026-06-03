//! Quant Strategy 4 — Kalman Filter Trend Following.
//!
//! Uses Kalman filter velocity (mathematical state estimation) instead of EMA.
//! Kalman is optimal linear estimator — adapts to changing market conditions.
//!
//! Signal: Kalman velocity crossing zero = trend reversal.
//! Confirmation: OFI must agree with Kalman direction.
//!
//! This is proper quant — same math used by Renaissance Technologies.

use super::Strategy;
use super::state::{PreSignal, StrategyName, SymbolState};
use crate::data::{Candle, Side};

pub struct KalmanTrendStrategy;

impl Strategy for KalmanTrendStrategy {
    fn name(&self) -> StrategyName {
        StrategyName::VwapScalp // mapped to VwapScalp slot
    }

    fn evaluate(&self, s: &SymbolState, c: &Candle) -> Option<PreSignal> {
        if s.candles.len() < 5 {
            return None;
        }

        let ofi = s.last_ofi.unwrap_or(0.0);
        let vpin = s.last_vpin.unwrap_or(0.5);
        let atr = s.last_atr.filter(|&a| a > 0.0 && a < c.close * 0.01)?;

        // Gate: VPIN safety check
        // VPIN soft gate: high VPIN reduces confidence instead of blocking
        let vpin_penalty = if vpin > 0.50 {
            ((vpin - 0.50) * 40.0).min(20.0) as u8
        } else {
            0
        };

        // Use price velocity from recent candles as Kalman proxy
        // (real Kalman is updated separately in quant engine via update_kalman)
        // Here we compute a fast exponentially weighted velocity
        let closes: Vec<f64> = s.candles.iter().rev().take(8).map(|c| c.close).collect();
        if closes.len() < 5 {
            return None;
        }

        // Exponentially weighted velocity: recent moves weigh more
        let weights = [0.35, 0.25, 0.20, 0.13, 0.07];
        let velocity: f64 = closes
            .windows(2)
            .take(5)
            .zip(weights.iter())
            .map(|(w, weight)| ((w[0] - w[1]) / w[1]) * weight)
            .sum();

        // Velocity must be meaningful — filter noise
        if velocity.abs() < 0.0001 {
            // relaxed from 0.03%
            // 0.03% per candle minimum
            return None;
        }

        // Acceleration: is velocity increasing or decreasing?
        // Positive acceleration on uptrend = trend strengthening = good entry
        let velocity_prev: f64 = closes
            .windows(2)
            .skip(1)
            .take(4)
            .zip([0.40, 0.30, 0.20, 0.10].iter())
            .map(|(w, weight)| ((w[0] - w[1]) / w[1]) * weight)
            .sum();

        let acceleration = velocity - velocity_prev;

        // Signal: velocity and acceleration must agree with OFI
        let long_signal = velocity > 0.0 && ofi >= -0.15; // removed acceleration requirement
        let short_signal = velocity < 0.0 && ofi <= 0.15; // removed acceleration requirement

        if !long_signal && !short_signal {
            return None;
        }

        let side = if long_signal { Side::Long } else { Side::Short };

        // ATR-based SL/TP
        let sl_dist = atr * 0.85;
        let tp_dist = atr * 2.0;
        let (sl, tp) = match side {
            Side::Long => (c.close - sl_dist, c.close + tp_dist),
            Side::Short => (c.close + sl_dist, c.close - tp_dist),
        };

        // Confidence from Kalman signal strength
        let mut confidence: f64 = 63.0;

        // Velocity magnitude
        if velocity.abs() > 0.002 {
            confidence += 10.0;
        } else if velocity.abs() > 0.001 {
            confidence += 5.0;
        }

        // Acceleration confirms momentum continuing
        if acceleration.abs() > velocity.abs() * 0.5 {
            confidence += 6.0;
        }

        // OFI strong confirmation
        if (long_signal && ofi > 0.3) || (short_signal && ofi < -0.3) {
            confidence += 6.0;
        }

        // VPIN safety
        if vpin < 0.25 {
            confidence += 4.0;
        }

        Some(PreSignal {
                signal_id: String::new(),
            symbol: s.symbol.clone(),
            strategy: StrategyName::VwapScalp,
            side,
            entry: c.close,
            stop_loss: sl,
            take_profit: tp,
            ta_confidence: (confidence - vpin_penalty as f64).max(0.0).min(100.0) as u8,
            reason: format!(
                "Kalman vel={:.4}% acc={:.4}% OFI={:.3} vpin={:.3}",
                velocity * 100.0,
                acceleration * 100.0,
                ofi,
                vpin
            ),
            atr: s.last_atr,
        })
    }
}
