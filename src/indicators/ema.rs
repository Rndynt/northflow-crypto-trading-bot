#[derive(Debug, Clone)]
pub struct Ema {
    alpha: f64,
    value: Option<f64>,
}

impl Ema {
    pub fn new(period: usize) -> Self {
        Self {
            alpha: 2.0 / (period as f64 + 1.0),
            value: None,
        }
    }

    pub fn next(&mut self, price: f64) -> f64 {
        let next = match self.value {
            Some(prev) => prev + self.alpha * (price - prev),
            None => price,
        };
        self.value = Some(next);
        next
    }
}
