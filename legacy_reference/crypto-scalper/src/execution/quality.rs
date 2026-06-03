use crate::data::Side;

#[derive(Debug, Clone, Default)]
pub struct ExecutionQuality {
    trades: Vec<TradeQualityRecord>,
}

impl ExecutionQuality {
    pub fn record(&mut self, trade: TradeQualityRecord) {
        self.trades.push(trade);
    }

    pub fn trades(&self) -> &[TradeQualityRecord] {
        &self.trades
    }

    pub fn avg_shortfall_bps(&self) -> Option<f64> {
        average(
            self.trades
                .iter()
                .map(TradeQualityRecord::implementation_shortfall_bps),
        )
    }

    pub fn avg_market_impact_bps(&self) -> Option<f64> {
        average(
            self.trades
                .iter()
                .map(TradeQualityRecord::market_impact_bps),
        )
    }
}

#[derive(Debug, Clone)]
pub struct TradeQualityRecord {
    pub symbol: String,
    pub decision_price: f64,
    pub arrival_price: f64,
    pub fill_price: f64,
    pub side: Side,
    pub size: f64,
}

fn average(values: impl Iterator<Item = f64>) -> Option<f64> {
    let mut sum = 0.0;
    let mut count = 0usize;
    for value in values {
        if value.is_finite() {
            sum += value;
            count += 1;
        }
    }
    if count == 0 {
        return None;
    }
    Some(sum / count as f64)
}

impl TradeQualityRecord {
    pub fn implementation_shortfall_bps(&self) -> f64 {
        directional_bps(self.decision_price, self.fill_price, self.side)
    }

    pub fn delay_cost_bps(&self) -> f64 {
        directional_bps(self.decision_price, self.arrival_price, self.side)
    }

    pub fn market_impact_bps(&self) -> f64 {
        directional_bps(self.arrival_price, self.fill_price, self.side)
    }
}

fn directional_bps(from: f64, to: f64, side: Side) -> f64 {
    if from <= 0.0 {
        return 0.0;
    }
    let direction = match side {
        Side::Long => 1.0,
        Side::Short => -1.0,
    };
    (to - from) / from * direction * 10_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decomposes_shortfall() {
        let rec = TradeQualityRecord {
            symbol: "BTCUSDT".into(),
            decision_price: 100.0,
            arrival_price: 100.1,
            fill_price: 100.2,
            side: Side::Long,
            size: 1.0,
        };
        approx::assert_abs_diff_eq!(rec.implementation_shortfall_bps(), 20.0, epsilon = 1e-9);
        approx::assert_abs_diff_eq!(rec.delay_cost_bps(), 10.0, epsilon = 1e-9);
        assert!(rec.market_impact_bps() > 9.0);
        let mut tracker = ExecutionQuality::default();
        tracker.record(rec);
        assert_eq!(tracker.trades().len(), 1);
        assert!(tracker.avg_shortfall_bps().unwrap() > 19.0);
    }
}
