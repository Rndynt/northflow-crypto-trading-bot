use crate::core::{Candle, Side, Signal};
use crate::indicators::{Atr, Ema, Vwap};

pub struct StrategyParams {
    pub symbol: String,
    pub ema_fast: usize,
    pub ema_slow: usize,
    pub atr_period: usize,
    pub min_confidence: u8,
    pub min_reward_risk: f64,
    pub stop_atr_mult: f64,
    pub tp_atr_mult: f64,
}

impl Default for StrategyParams {
    fn default() -> Self {
        Self {
            symbol: "BTCUSDT".to_string(),
            ema_fast: 9,
            ema_slow: 21,
            atr_period: 14,
            min_confidence: 65,
            min_reward_risk: 1.5,
            stop_atr_mult: 1.5,
            tp_atr_mult: 3.0,
        }
    }
}

pub struct ScreenedVwapScalp {
    ema_fast: Ema,
    ema_slow: Ema,
    atr: Atr,
    vwap: Vwap,
    prev_fast: Option<f64>,
    prev_slow: Option<f64>,
    params: StrategyParams,
}

impl ScreenedVwapScalp {
    pub fn new(params: StrategyParams) -> Self {
        Self {
            ema_fast: Ema::new(params.ema_fast),
            ema_slow: Ema::new(params.ema_slow),
            atr: Atr::new(params.atr_period),
            vwap: Vwap::new(),
            prev_fast: None,
            prev_slow: None,
            params,
        }
    }

    pub fn next(&mut self, candle: Candle) -> Option<Signal> {
        let fast = self.ema_fast.next(candle.close);
        let slow = self.ema_slow.next(candle.close);
        let atr = self.atr.next(candle)?;
        let vwap = self.vwap.next(candle)?;

        let prev_fast = self.prev_fast.replace(fast)?;
        let prev_slow = self.prev_slow.replace(slow)?;

        let golden = prev_fast <= prev_slow && fast > slow;
        let death = prev_fast >= prev_slow && fast < slow;

        let price = candle.close;

        if golden && price > vwap {
            let stop = price - atr * self.params.stop_atr_mult;
            let tp = price + atr * self.params.tp_atr_mult;
            let rr = (tp - price) / (price - stop).max(f64::EPSILON);
            if rr < self.params.min_reward_risk {
                return None;
            }
            let confidence = score_confidence(rr, self.params.min_reward_risk);
            if confidence < self.params.min_confidence {
                return None;
            }
            return Some(Signal {
                symbol: self.params.symbol.clone(),
                strategy: "screened_vwap_scalp".to_string(),
                side: Side::Buy,
                entry: price,
                stop_loss: stop,
                take_profit: tp,
                confidence,
                reason: format!("golden cross EMA{}/{} above VWAP, ATR={:.4}", self.params.ema_fast, self.params.ema_slow, atr),
            });
        }

        if death && price < vwap {
            let stop = price + atr * self.params.stop_atr_mult;
            let tp = price - atr * self.params.tp_atr_mult;
            let rr = (price - tp) / (stop - price).max(f64::EPSILON);
            if rr < self.params.min_reward_risk {
                return None;
            }
            let confidence = score_confidence(rr, self.params.min_reward_risk);
            if confidence < self.params.min_confidence {
                return None;
            }
            return Some(Signal {
                symbol: self.params.symbol.clone(),
                strategy: "screened_vwap_scalp".to_string(),
                side: Side::Sell,
                entry: price,
                stop_loss: stop,
                take_profit: tp,
                confidence,
                reason: format!("death cross EMA{}/{} below VWAP, ATR={:.4}", self.params.ema_fast, self.params.ema_slow, atr),
            });
        }

        None
    }
}

fn score_confidence(rr: f64, min_rr: f64) -> u8 {
    let base = 60u8;
    let extra = ((rr - min_rr) * 10.0).min(39.0) as u8;
    base.saturating_add(extra)
}
