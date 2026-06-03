//! Quant Strategy 3 — Microstructure Mean Reversion.
//!
//! NOT Bollinger Band reversion. Uses VWAP deviation + order flow reversal.
//!
//! Edge: Price that deviates far from VWAP with LOW volume (no conviction)
//! tends to revert. When OFI also starts reversing, entry is confirmed.
//! This is statistical reversion, not "RSI oversold" guessing.

use super::Strategy;
use super::state::{PreSignal, StrategyName, SymbolState};
use crate::data::{Candle, Side};

pub struct MicrostructureReversion;

impl Strategy for MicrostructureReversion {
    fn name(&self) -> StrategyName {
        StrategyName::MeanReversion
    }

    fn evaluate(&self, s: &SymbolState, c: &Candle) -> Option<PreSignal> {
        let vwap = s.last_vwap?;
        let ofi = s.last_ofi?;
        let vpin = s.last_vpin.unwrap_or(0.5);
        let atr = s.last_atr.filter(|&a| a > 0.0 && a < c.close * 0.01)?;

        if vwap <= 0.0 {
            return None;
        }

        // VWAP deviation in % — how far price has strayed from fair value
        let deviation = (c.close - vwap) / vwap;

        // Quant reversion zone: price must have deviated enough to matter
        // but not so much that it's a breakout (not mean reversion anymore)
        let long_zone = deviation < -0.002 && deviation > -0.015; // -0.2% to -1.5%
        let short_zone = deviation > 0.002 && deviation < 0.015; // +0.2% to +1.5%

        if !long_zone && !short_zone {
            return None;
        }

        // KEY: OFI must be REVERSING direction (countering the move)
        // Long (price below VWAP): need buy flow to appear (OFI turning positive)
        // Short (price above VWAP): need sell flow to appear (OFI turning negative)
        let ofi_confirms_reversion = if long_zone {
            ofi > 0.1 // buy flow appearing as price drops = reversion signal
        } else {
            ofi < -0.1 // sell flow appearing as price rises = reversion signal
        };

        if !ofi_confirms_reversion {
            return None;
        }

        // Volume check: reversion works best on LOW volume stretches
        // (high volume = potential breakout, not reversion)
        if s.volume_sma > 0.0 {
            let vol_ratio = c.volume / s.volume_sma;
            if vol_ratio > 2.5 {
                // abnormally high volume = breakout not reversion
                return None;
            }
        }

        // VPIN: allow slightly higher since this is counter-trend
        // VPIN soft gate
        let vpin_penalty = if vpin > 0.50 {
            ((vpin - 0.50) * 40.0).min(20.0) as u8
        } else {
            0
        };

        let side = if long_zone { Side::Long } else { Side::Short };

        // Tighter SL for mean reversion (if it doesn't revert quickly, exit)
        let sl_dist = atr * 0.7;
        let tp_dist = deviation.abs() * vwap * 0.8; // target ~80% of the deviation
        let tp_dist = tp_dist.max(atr * 1.5); // minimum 1.5x ATR TP

        let (sl, tp) = match side {
            Side::Long => (c.close - sl_dist, c.close + tp_dist),
            Side::Short => (c.close + sl_dist, c.close - tp_dist),
        };

        // Confidence: deviation size + OFI strength + VPIN safety
        let mut confidence: f64 = 62.0;

        // Larger deviation = stronger reversion signal (statistically)
        if deviation.abs() > 0.007 {
            confidence += 8.0;
        } else if deviation.abs() > 0.004 {
            confidence += 4.0;
        }

        // OFI strength of reversal
        if ofi.abs() > 0.3 {
            confidence += 6.0;
        }

        // Lower VPIN = safer
        if vpin < 0.25 {
            confidence += 5.0;
        }

        Some(PreSignal {
                signal_id: String::new(),
            symbol: s.symbol.clone(),
            strategy: StrategyName::MeanReversion,
            side,
            entry: c.close,
            stop_loss: sl,
            take_profit: tp,
            ta_confidence: (confidence - vpin_penalty as f64).max(0.0).min(100.0) as u8,
            reason: format!(
                "VWAP_dev={:.3}% OFI={:.3} vpin={:.3} reversion_target={:.4}",
                deviation * 100.0,
                ofi,
                vpin,
                vwap
            ),
            atr: s.last_atr,
        })
    }
}
