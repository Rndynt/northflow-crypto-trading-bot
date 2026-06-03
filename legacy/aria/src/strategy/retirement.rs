use crate::backtest::metrics::PerformanceMetrics;

#[derive(Debug, Clone, Copy)]
pub struct RetirementRule {
    pub min_trades: u32,
    pub min_profit_factor: f64,
    pub min_sharpe: f64,
    pub max_drawdown_pct: f64,
}

impl RetirementRule {
    pub fn should_retire(&self, metrics: &PerformanceMetrics) -> bool {
        metrics.trades >= self.min_trades
            && (metrics.profit_factor < self.min_profit_factor
                || metrics.sharpe < self.min_sharpe
                || metrics.max_drawdown_pct > self.max_drawdown_pct)
    }
}

impl Default for RetirementRule {
    fn default() -> Self {
        Self {
            min_trades: 30,
            min_profit_factor: 1.05,
            min_sharpe: 0.5,
            max_drawdown_pct: 8.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retires_underperforming_strategy() {
        let rule = RetirementRule::default();
        let metrics = PerformanceMetrics {
            trades: 40,
            profit_factor: 0.9,
            sharpe: 1.0,
            max_drawdown_pct: 3.0,
            ..PerformanceMetrics::default()
        };
        assert!(rule.should_retire(&metrics));
    }
}
