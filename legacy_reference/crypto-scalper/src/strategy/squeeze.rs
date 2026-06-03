//! Strategy E — Volatility Squeeze & Expansion.
//!
//! Tuned for HFT: lower ROC threshold for expansion detection, tighter SL.

use super::Strategy;
use super::state::{PreSignal, StrategyName, SymbolState};
use crate::data::{Candle, Side};

pub struct Squeeze;

impl Strategy for Squeeze {
    fn name(&self) -> StrategyName {
        StrategyName::Squeeze
    }

    fn evaluate(&self, s: &SymbolState, c: &Candle) -> Option<PreSignal> {
        let bb = s.last_bb?;
        let ku = s.last_keltner_upper?;
        let kl = s.last_keltner_lower?;
        let roc = s.last_roc.unwrap_or(0.0);

        // Check if we're still inside the squeeze (BB inside Keltner)
        let in_squeeze = bb.upper < ku && bb.lower > kl;
        if in_squeeze {
            return None; // wait for expansion
        }

        // Expansion detected — use ROC for direction.
        // Lower threshold: 0.1% (was 0.3%) — crypto moves fast in squeeze release.
        let atr = s.last_atr.unwrap_or(c.close * 0.003);
        let (side, reason, sl, tp) = if roc > 0.1 && c.close > bb.mid {
            (
                Side::Long,
                format!("Squeeze expand up, ROC {roc:.2}%"),
                c.close - atr,       // 1× ATR SL
                c.close + atr * 2.0, // 2× ATR TP (1:2 R:R)
            )
        } else if roc < -0.1 && c.close < bb.mid {
            (
                Side::Short,
                format!("Squeeze expand down, ROC {roc:.2}%"),
                c.close + atr,
                c.close - atr * 2.0,
            )
        } else {
            return None;
        };

        let mut score: f64 = 64.0;
        // Stronger ROC = higher confidence
        score += roc.abs().min(3.0) * 4.0;
        // OFI alignment
        let aligned_ofi = (side == Side::Long && s.last_ofi.unwrap_or(0.0) > 0.0)
            || (side == Side::Short && s.last_ofi.unwrap_or(0.0) < 0.0);
        if aligned_ofi {
            score += 5.0;
        }
        // Price beyond BB band = stronger expansion
        if (side == Side::Long && c.close > bb.upper) || (side == Side::Short && c.close < bb.lower)
        {
            score += 3.0;
        }

        Some(PreSignal {
                signal_id: String::new(),
            symbol: s.symbol.clone(),
            strategy: StrategyName::Squeeze,
            side,
            entry: c.close,
            stop_loss: sl,
            take_profit: tp,
            ta_confidence: score.clamp(0.0, 100.0) as u8,
            reason,
            atr: s.last_atr,
        })
    }
}
