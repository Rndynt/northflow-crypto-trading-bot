use crate::data::Side;

#[derive(Debug, Clone)]
pub struct PositionExposure {
    pub symbol: String,
    pub side: Side,
    pub notional_usd: f64,
}

pub fn gross_exposure(positions: &[PositionExposure]) -> f64 {
    positions.iter().map(|p| p.notional_usd.abs()).sum()
}

pub fn net_exposure(positions: &[PositionExposure]) -> f64 {
    positions
        .iter()
        .map(|p| match p.side {
            Side::Long => p.notional_usd,
            Side::Short => -p.notional_usd,
        })
        .sum()
}

pub fn can_add_position(
    positions: &[PositionExposure],
    proposed: &PositionExposure,
    equity_usd: f64,
    max_gross_exposure_pct: f64,
) -> bool {
    if equity_usd <= 0.0 || max_gross_exposure_pct <= 0.0 {
        return false;
    }
    let max_gross = equity_usd * max_gross_exposure_pct / 100.0;
    gross_exposure(positions) + proposed.notional_usd.abs() <= max_gross
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculates_exposure_caps() {
        let positions = vec![
            PositionExposure {
                symbol: "BTCUSDT".into(),
                side: Side::Long,
                notional_usd: 1000.0,
            },
            PositionExposure {
                symbol: "ETHUSDT".into(),
                side: Side::Short,
                notional_usd: 500.0,
            },
        ];
        approx::assert_abs_diff_eq!(gross_exposure(&positions), 1500.0);
        approx::assert_abs_diff_eq!(net_exposure(&positions), 500.0);
        let proposed = PositionExposure {
            symbol: "SOLUSDT".into(),
            side: Side::Long,
            notional_usd: 400.0,
        };
        assert!(can_add_position(&positions, &proposed, 5000.0, 50.0));
    }
}
