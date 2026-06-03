use crate::strategy::state::PreSignal;

#[derive(Debug, Clone, Copy)]
pub struct TransactionCostModel {
    pub taker_fee_bps: f64,
    pub maker_fee_bps: f64,
    pub avg_slippage_bps: f64,
    pub market_impact_bps: f64,
}

impl TransactionCostModel {
    pub fn total_cost_bps(&self, size_usd: f64, daily_volume_usd: f64, is_maker: bool) -> f64 {
        let fee = if is_maker {
            self.maker_fee_bps
        } else {
            self.taker_fee_bps
        };
        fee + self.avg_slippage_bps + self.market_impact_bps(size_usd, daily_volume_usd)
    }

    pub fn round_trip_cost_bps(&self, size_usd: f64, daily_volume_usd: f64) -> f64 {
        2.0 * self.total_cost_bps(size_usd, daily_volume_usd, false)
    }

    pub fn market_impact_bps(&self, size_usd: f64, daily_volume_usd: f64) -> f64 {
        if size_usd <= 0.0 || daily_volume_usd <= 0.0 || self.market_impact_bps <= 0.0 {
            return 0.0;
        }
        self.market_impact_bps * (size_usd / daily_volume_usd).sqrt()
    }
}

impl PreSignal {
    pub fn gross_edge_bps(&self) -> f64 {
        if self.entry <= 0.0 {
            return 0.0;
        }
        (self.take_profit - self.entry).abs() / self.entry * 10_000.0
    }

    pub fn net_expected_edge_bps(
        &self,
        tcm: &TransactionCostModel,
        size_usd: f64,
        daily_volume_usd: f64,
    ) -> f64 {
        self.gross_edge_bps() - tcm.round_trip_cost_bps(size_usd, daily_volume_usd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Side;
    use crate::strategy::state::StrategyName;

    #[test]
    fn computes_round_trip_cost_and_edge() {
        let tcm = TransactionCostModel {
            taker_fee_bps: 4.0,
            maker_fee_bps: -1.0,
            avg_slippage_bps: 2.0,
            market_impact_bps: 10.0,
        };
        approx::assert_abs_diff_eq!(tcm.round_trip_cost_bps(10_000.0, 100_000_000.0), 12.2);

        let sig = PreSignal {
            signal_id: String::new(),
            symbol: "BTCUSDT".into(),
            strategy: StrategyName::Momentum,
            side: Side::Long,
            entry: 100.0,
            stop_loss: 99.0,
            take_profit: 101.0,
            ta_confidence: 80,
            reason: String::new(),
            atr: None,
        };
        approx::assert_abs_diff_eq!(
            sig.net_expected_edge_bps(&tcm, 10_000.0, 100_000_000.0),
            87.8
        );
    }
}
