use crate::core::Candle;

pub fn ema(values: &[f64], period: usize) -> Vec<f64> {
    if values.is_empty() || period == 0 {
        return vec![];
    }
    let k = 2.0 / (period as f64 + 1.0);
    let mut result = vec![f64::NAN; values.len()];
    let start = period.saturating_sub(1);
    if start >= values.len() {
        return result;
    }
    let seed: f64 = values[..period.min(values.len())].iter().sum::<f64>()
        / period.min(values.len()) as f64;
    result[start] = seed;
    for i in (start + 1)..values.len() {
        result[i] = values[i] * k + result[i - 1] * (1.0 - k);
    }
    result
}

pub fn atr(candles: &[Candle], period: usize) -> Vec<f64> {
    if candles.len() < 2 {
        return vec![f64::NAN; candles.len()];
    }
    let mut tr_values: Vec<f64> = vec![f64::NAN];
    for i in 1..candles.len() {
        let c = &candles[i];
        let prev_close = candles[i - 1].close;
        let tr = (c.high - c.low)
            .max((c.high - prev_close).abs())
            .max((c.low - prev_close).abs());
        tr_values.push(tr);
    }

    let mut result = vec![f64::NAN; candles.len()];
    if candles.len() <= period {
        return result;
    }
    let seed: f64 = tr_values[1..=period].iter().sum::<f64>() / period as f64;
    result[period] = seed;
    let k = 1.0 / period as f64;
    for i in (period + 1)..candles.len() {
        result[i] = tr_values[i] * k + result[i - 1] * (1.0 - k);
    }
    result
}

pub fn vwap(candles: &[Candle], period: usize) -> Vec<f64> {
    let mut result = vec![f64::NAN; candles.len()];
    for i in period.saturating_sub(1)..candles.len() {
        let start = if i + 1 >= period { i + 1 - period } else { 0 };
        let window = &candles[start..=i];
        let total_vol: f64 = window.iter().map(|c| c.volume).sum();
        if total_vol == 0.0 {
            continue;
        }
        let typical_vol: f64 = window
            .iter()
            .map(|c| ((c.high + c.low + c.close) / 3.0) * c.volume)
            .sum();
        result[i] = typical_vol / total_vol;
    }
    result
}
