#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    pub fn as_str(self) -> &'static str {
        match self {
            Side::Buy => "buy",
            Side::Sell => "sell",
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Candle {
    pub timestamp: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

impl Candle {
    pub fn is_valid(self) -> bool {
        let values = [self.open, self.high, self.low, self.close];
        values.iter().all(|v| v.is_finite() && *v > 0.0)
            && self.volume.is_finite()
            && self.high >= self.low
            && self.open >= self.low
            && self.open <= self.high
            && self.close >= self.low
            && self.close <= self.high
    }
}

#[derive(Debug, Clone)]
pub struct Signal {
    pub symbol: String,
    pub strategy: String,
    pub side: Side,
    pub entry: f64,
    pub stop_loss: f64,
    pub take_profit: f64,
    pub confidence: u8,
    pub reason: String,
}

impl Signal {
    pub fn reward_risk(&self) -> f64 {
        let risk = (self.entry - self.stop_loss).abs();
        if risk <= 0.0 { return 0.0; }
        (self.take_profit - self.entry).abs() / risk
    }

    pub fn valid_geometry(&self) -> bool {
        if self.entry <= 0.0 || self.stop_loss <= 0.0 || self.take_profit <= 0.0 { return false; }
        match self.side {
            Side::Buy => self.stop_loss < self.entry && self.take_profit > self.entry,
            Side::Sell => self.stop_loss > self.entry && self.take_profit < self.entry,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SimTrade {
    pub symbol: String,
    pub strategy: String,
    pub side: Side,
    pub entry_time: i64,
    pub exit_time: i64,
    pub entry: f64,
    pub exit: f64,
    pub qty: f64,
    pub net_pnl: f64,
    pub exit_reason: String,
}
