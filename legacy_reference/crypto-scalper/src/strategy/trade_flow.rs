//! Quant Strategy 2 — Trade Flow Toxicity Gate + Momentum.
//!
//! Uses VPIN (Volume-synchronized Probability of Informed trading) to detect
//! when informed traders are active. When VPIN is LOW (uninformed flow),
//! ride the momentum. When VPIN is HIGH, stay out.
//!
//! This is pure quant: statistical model of market microstructure.

use super::Strategy;
use super::state::{PreSignal, StrategyName, SymbolState};
use crate::data::{Candle, Side};

pub struct TradeFlow;

impl Strategy for TradeFlow {
    fn name(&self) -> StrategyName {
        StrategyName::Momentum
    }

    fn evaluate(&self, s: &SymbolState, c: &Candle) -> Option<PreSignal> {
        if s.candles.len() < 5 {
            // relaxed from 10
            return None;
        }

        let vpin = s.last_vpin.unwrap_or(0.35); // default=0.35 safe until VPIN warms up
        let ofi = s.last_ofi.unwrap_or(0.0);
        let atr = s.last_atr.filter(|&a| a > 0.0 && a < c.close * 0.01)?;

        // Core quant gate: VPIN must be LOW = uninformed retail flow
        // Informed traders (hedge funds, whales) = adverse selection = we lose
        // VPIN < 0.35: safe, flow is mostly uninformed
        // VPIN 0.35-0.50: caution
        // VPIN > 0.50: informed traders active, STAY OUT
        // VPIN soft gate
        let vpin_penalty = if vpin > 0.50 {
            ((vpin - 0.50) * 40.0).min(20.0) as u8
        } else {
            0
        };

        // Price velocity: compare last 3 closes for micro-momentum
        let closes: Vec<f64> = s.candles.iter().rev().take(4).map(|c| c.close).collect();
        if closes.len() < 4 {
            return None;
        }

        // Short-term momentum: slope of last 3 prices (linear regression-like)
        let velocity = (closes[0] - closes[3]) / closes[3]; // % change over 3 candles

        // Minimum velocity threshold to avoid trading chop
        if velocity.abs() < 0.0002 {
            // relaxed from 0.05%
            // 0.05% minimum move
            return None;
        }

        // OFI and velocity must agree (both bullish or both bearish)
        let long_signal = velocity > 0.0 && ofi >= 0.0;
        let short_signal = velocity < 0.0 && ofi <= 0.0;

        if !long_signal && !short_signal {
            return None;
        }

        let side = if long_signal { Side::Long } else { Side::Short };

        // ATR-based SL/TP
        let sl_dist = atr * 0.9;
        let tp_dist = atr * 2.2;
        let (sl, tp) = match side {
            Side::Long => (c.close - sl_dist, c.close + tp_dist),
            Side::Short => (c.close + sl_dist, c.close - tp_dist),
        };

        // Confidence: purely from signal strength metrics, not TA patterns
        let mut confidence: f64 = 62.0;

        // Velocity strength
        if velocity.abs() > 0.002 {
            confidence += 8.0;
        } else if velocity.abs() > 0.001 {
            confidence += 4.0;
        }

        // VPIN safety bonus — the lower the safer
        if vpin < 0.20 {
            confidence += 8.0;
        } else if vpin < 0.30 {
            confidence += 4.0;
        }

        // OFI confirmation strength
        if ofi.abs() > 0.4 {
            confidence += 5.0;
        }

        Some(PreSignal {
                signal_id: String::new(),
            symbol: s.symbol.clone(),
            strategy: StrategyName::Momentum,
            side,
            entry: c.close,
            stop_loss: sl,
            take_profit: tp,
            ta_confidence: (confidence - vpin_penalty as f64).max(0.0).min(100.0) as u8,
            reason: format!(
                "VPIN={:.3} velocity={:.4}% OFI={:.3} atr={:.4}",
                vpin,
                velocity * 100.0,
                ofi,
                atr
            ),
            atr: s.last_atr,
        })
    }
}
