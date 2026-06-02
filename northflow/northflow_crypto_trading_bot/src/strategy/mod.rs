use crate::core::{Candle, Signal};
use crate::indicators::{atr, ema, vwap};

pub struct EmaCrossoverParams {
    pub fast_period: usize,
    pub slow_period: usize,
    pub atr_period: usize,
    pub vwap_period: usize,
}

pub struct EmaCrossoverSignals {
    pub signals: Vec<Signal>,
    pub ema_fast: Vec<f64>,
    pub ema_slow: Vec<f64>,
    pub atr: Vec<f64>,
    pub vwap: Vec<f64>,
}

pub fn ema_crossover(candles: &[Candle], params: &EmaCrossoverParams) -> EmaCrossoverSignals {
    let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();
    let ema_fast = ema(&closes, params.fast_period);
    let ema_slow = ema(&closes, params.slow_period);
    let atr_vals = atr(candles, params.atr_period);
    let vwap_vals = vwap(candles, params.vwap_period);

    let n = candles.len();
    let mut signals = vec![Signal::Hold; n];

    for i in 1..n {
        let fast_prev = ema_fast[i - 1];
        let slow_prev = ema_slow[i - 1];
        let fast_curr = ema_fast[i];
        let slow_curr = ema_slow[i];

        if fast_prev.is_nan()
            || slow_prev.is_nan()
            || fast_curr.is_nan()
            || slow_curr.is_nan()
        {
            continue;
        }

        let price = candles[i].close;
        let vwap_val = vwap_vals[i];

        let golden_cross = fast_prev <= slow_prev && fast_curr > slow_curr;
        let death_cross = fast_prev >= slow_prev && fast_curr < slow_curr;

        if golden_cross && (!vwap_val.is_nan()) && price > vwap_val {
            signals[i] = Signal::Buy;
        } else if death_cross && (!vwap_val.is_nan()) && price < vwap_val {
            signals[i] = Signal::Sell;
        }
    }

    EmaCrossoverSignals {
        signals,
        ema_fast,
        ema_slow,
        atr: atr_vals,
        vwap: vwap_vals,
    }
}
