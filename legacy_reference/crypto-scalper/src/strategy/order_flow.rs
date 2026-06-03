//! Quant Strategy 1 — Order Flow Imbalance (OFI) Scalping.
//!
//! Pure microstructure signal. No EMA, no RSI, no Bollinger.
//! Signal: When order flow is heavily one-sided AND low adverse selection.
//!
//! Edge: Market makers and informed traders leave footprints in order flow
//! before price moves. OFI predicts short-term price direction.

use super::Strategy;
use super::state::{PreSignal, StrategyName, SymbolState};
use crate::data::{Candle, Side};

pub struct OrderFlow;

impl Strategy for OrderFlow {
    fn name(&self) -> StrategyName {
        StrategyName::EmaRibbon // reuse slot — renamed via as_str override
    }

    fn evaluate(&self, s: &SymbolState, c: &Candle) -> Option<PreSignal> {
        let ofi = s.last_ofi.unwrap_or(0.0);
        let vpin = s.last_vpin.unwrap_or(0.35);
        let atr = s.last_atr.filter(|&a| a > 0.0 && a < c.close * 0.01)?;

        // Need at least some OFI signal to trade
        if ofi == 0.0 && s.last_ofi.is_none() {
            return None; // BookTicker not yet received — wait
        }

        // Gate 1: VPIN must be low — high VPIN = adverse selection = informed trader
        // is on the OTHER side of your trade. Very dangerous.
        // VPIN soft gate
        let vpin_penalty = if vpin > 0.50 {
            ((vpin - 0.50) * 40.0).min(20.0) as u8
        } else {
            0
        };

        // Gate 2: Order book imbalance from top-of-book
        let book_imbalance = s.order_book.bid_ask_ratio(5);
        // book_imbalance > 1.0 = more bids (bullish pressure)
        // book_imbalance < 1.0 = more asks (bearish pressure)

        // Gate 3: OFI must be significant — weak signal = noise
        let ofi_strong = ofi.abs() > 0.15; // relaxed from 0.3
        if !ofi_strong {
            return None;
        }

        // Signal: OFI and book imbalance must agree on direction
        let long_signal = ofi > 0.20 && book_imbalance > 1.08; // buyers dominating
        let short_signal = ofi < -0.20 && book_imbalance < 0.93; // sellers dominating

        if !long_signal && !short_signal {
            return None;
        }

        let side = if long_signal { Side::Long } else { Side::Short };

        // ATR-based SL/TP — quant sizing not TA levels
        let sl_dist = atr * 0.8;
        let tp_dist = atr * 2.0; // 2.5:1 R:R
        let (sl, tp) = match side {
            Side::Long => (c.close - sl_dist, c.close + tp_dist),
            Side::Short => (c.close + sl_dist, c.close - tp_dist),
        };

        // Confidence based on signal strength, not TA patterns
        let mut confidence: f64 = 63.0;

        // OFI magnitude bonus
        if ofi.abs() > 0.6 {
            confidence += 8.0;
        } else if ofi.abs() > 0.45 {
            confidence += 4.0;
        }

        // Book imbalance strength bonus
        let book_extreme = if long_signal {
            book_imbalance > 1.4
        } else {
            book_imbalance < 0.7
        };
        if book_extreme {
            confidence += 7.0;
        }

        // VPIN lower = safer = higher confidence
        if vpin < 0.25 {
            confidence += 5.0;
        }

        Some(PreSignal {
                signal_id: String::new(),
            symbol: s.symbol.clone(),
            strategy: StrategyName::EmaRibbon, // mapped to this slot
            side,
            entry: c.close,
            stop_loss: sl,
            take_profit: tp,
            ta_confidence: (confidence - vpin_penalty as f64).max(0.0).min(100.0) as u8,
            reason: format!(
                "OFI={:.3} book_imb={:.3} vpin={:.3} atr={:.2}",
                ofi, book_imbalance, vpin, atr
            ),
            atr: s.last_atr,
        })
    }
}
