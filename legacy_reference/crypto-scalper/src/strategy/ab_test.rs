use crate::backtest::metrics::PerformanceMetrics;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariantWinner {
    Control,
    Treatment,
    Inconclusive,
}

pub fn compare_variants(
    control: &PerformanceMetrics,
    treatment: &PerformanceMetrics,
    min_pf_lift: f64,
    min_trade_count: u32,
) -> VariantWinner {
    if control.trades < min_trade_count || treatment.trades < min_trade_count {
        return VariantWinner::Inconclusive;
    }
    if treatment.profit_factor.is_finite()
        && control.profit_factor.is_finite()
        && treatment.profit_factor >= control.profit_factor * (1.0 + min_pf_lift)
        && treatment.sharpe >= control.sharpe
    {
        return VariantWinner::Treatment;
    }
    if control.profit_factor.is_finite()
        && treatment.profit_factor.is_finite()
        && control.profit_factor >= treatment.profit_factor * (1.0 + min_pf_lift)
        && control.sharpe >= treatment.sharpe
    {
        return VariantWinner::Control;
    }
    VariantWinner::Inconclusive
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chooses_better_variant() {
        let control = PerformanceMetrics {
            trades: 50,
            profit_factor: 1.1,
            sharpe: 0.8,
            ..PerformanceMetrics::default()
        };
        let treatment = PerformanceMetrics {
            trades: 50,
            profit_factor: 1.4,
            sharpe: 0.9,
            ..PerformanceMetrics::default()
        };
        assert_eq!(
            compare_variants(&control, &treatment, 0.1, 30),
            VariantWinner::Treatment
        );
    }
}
