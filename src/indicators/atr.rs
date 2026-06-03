use crate::core::Candle;

#[derive(Debug, Clone)]
pub struct Atr {
    period: usize,
    prev_close: Option<f64>,
    values: Vec<f64>,
}

impl Atr {
    pub fn new(period: usize) -> Self {
        Self {
            period,
            prev_close: None,
            values: Vec::new(),
        }
    }

    pub fn next(&mut self, candle: Candle) -> Option<f64> {
        let tr = match self.prev_close {
            Some(prev) => (candle.high - candle.low)
                .max((candle.high - prev).abs())
                .max((candle.low - prev).abs()),
            None => candle.high - candle.low,
        };
        self.prev_close = Some(candle.close);
        self.values.push(tr);
        if self.values.len() > self.period {
            self.values.remove(0);
        }
        if self.values.len() < self.period {
            return None;
        }
        Some(self.values.iter().sum::<f64>() / self.values.len() as f64)
    }
}
