//! Strategy D — EMA Ribbon + RSI pullback entries.
//!
//! Tuned for HFT: works with just EMA8 + EMA21 (no EMA50/200 required).
//! This means it fires during warmup too — critical for day-1 operation.

use super::Strategy;
use super::state::{PreSignal, StrategyName, SymbolState};
use crate::data::{Candle, Side};

pub struct EmaRibbon;

impl Strategy for EmaRibbon {
    fn name(&self) -> StrategyName {
        StrategyName::EmaRibbon
    }

    fn evaluate(&self, s: &SymbolState, c: &Candle) -> Option<PreSignal> {
        // Minimum required: EMA8 + EMA21 (fast pair).
        // EMA50/200 are optional confirmation — don't block if missing.
        let e8 = s.ema_8.value()?;
        let e21 = s.ema_21.value()?;
        let rsi = s.last_rsi.unwrap_or(50.0);

        // ATR-based SL/TP for proper position sizing at high leverage.
        // Fallback: 0.35% of close if ATR not yet warmed up.
        let atr = s.last_atr.unwrap_or(c.close * 0.0035);

        let e50 = s.ema_50.value();
        let e200 = s.ema_200.value();

        // Ribbon alignment — be flexible:
        // - Full alignment (8>21>50>close>200) = strong
        // - Partial alignment (8>21 + close above/below 200) = acceptable
        // - Minimal (8>21) = still valid for scalping
        let bullish_ribbon = e8 > e21;
        let bearish_ribbon = e8 < e21;

        // Extra confirmation if EMA50/200 are available
        let ema50_confirms_bull = e50.map(|e| e21 > e).unwrap_or(true);
        let ema50_confirms_bear = e50.map(|e| e21 < e).unwrap_or(true);
        let ema200_confirms_bull = e200.map(|e| c.close > e).unwrap_or(true);
        let ema200_confirms_bear = e200.map(|e| c.close < e).unwrap_or(true);

        if bullish_ribbon && ema50_confirms_bull && ema200_confirms_bull {
            // Pullback entry: price dipped near EMA21 from above
            let pullback_zone = c.low <= e21 * 1.010 && c.close > e21 * 0.995;
            if pullback_zone && rsi > 25.0 && rsi < 75.0 {
                let sl = c.close - atr; // 1× ATR SL
                let tp = c.close + atr * 2.0; // 2× ATR TP (1:2 R:R)
                let mut score: f64 = 66.0;
                // Full ribbon alignment bonus
                if e50.is_some() && e200.is_some() {
                    score += 5.0;
                }
                if rsi > 45.0 && rsi < 60.0 {
                    score += 5.0;
                }
                return Some(PreSignal {
                signal_id: String::new(),
                    symbol: s.symbol.clone(),
                    strategy: StrategyName::EmaRibbon,
                    side: Side::Long,
                    entry: c.close,
                    stop_loss: sl,
                    take_profit: tp,
                    ta_confidence: score.clamp(0.0, 100.0) as u8,
                    reason: format!("Ribbon bull + pullback EMA21 {e21:.4} RSI {rsi:.1}"),
                    atr: s.last_atr,
                });
            }
        }

        if bearish_ribbon && ema50_confirms_bear && ema200_confirms_bear {
            let pullback_zone = c.high >= e21 * 0.990 && c.close < e21 * 1.005;
            if pullback_zone && rsi > 25.0 && rsi < 75.0 {
                let sl = c.close + atr; // 1× ATR SL
                let tp = c.close - atr * 2.0; // 2× ATR TP (1:2 R:R)
                let mut score: f64 = 66.0;
                if e50.is_some() && e200.is_some() {
                    score += 5.0;
                }
                if rsi > 40.0 && rsi < 55.0 {
                    score += 5.0;
                }
                return Some(PreSignal {
                signal_id: String::new(),
                    symbol: s.symbol.clone(),
                    strategy: StrategyName::EmaRibbon,
                    side: Side::Short,
                    entry: c.close,
                    stop_loss: sl,
                    take_profit: tp,
                    ta_confidence: score.clamp(0.0, 100.0) as u8,
                    reason: format!("Ribbon bear + pullback EMA21 {e21:.4} RSI {rsi:.1}"),
                    atr: s.last_atr,
                });
            }
        }

        None
    }
}
