use crate::core::Candle;

#[derive(Debug, Clone, Default)]
pub struct Vwap {
    pv_sum: f64,
    volume_sum: f64,
}

impl Vwap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn next(&mut self, candle: Candle) -> Option<f64> {
        if candle.volume <= 0.0 {
            return None;
        }
        let typical = (candle.high + candle.low + candle.close) / 3.0;
        self.pv_sum += typical * candle.volume;
        self.volume_sum += candle.volume;
        if self.volume_sum <= 0.0 {
            None
        } else {
            Some(self.pv_sum / self.volume_sum)
        }
    }
}
