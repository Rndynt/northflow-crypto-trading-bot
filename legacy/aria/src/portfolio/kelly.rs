pub fn kelly_fraction(win_rate: f64, avg_win: f64, avg_loss: f64, cap: f64) -> f64 {
    if !(0.0..=1.0).contains(&win_rate) || avg_win <= 0.0 || avg_loss <= 0.0 || cap <= 0.0 {
        return 0.0;
    }
    let payoff = avg_win / avg_loss;
    let raw = win_rate - (1.0 - win_rate) / payoff;
    raw.max(0.0).min(cap)
}

pub fn portfolio_kelly_adjustment(kelly: f64, correlation: f64) -> f64 {
    if kelly <= 0.0 {
        return 0.0;
    }
    let corr = correlation.clamp(0.0, 1.0);
    kelly * (1.0 - corr * 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_capped_kelly() {
        approx::assert_abs_diff_eq!(kelly_fraction(0.55, 2.0, 1.0, 0.2), 0.2);
        approx::assert_abs_diff_eq!(kelly_fraction(0.4, 1.0, 1.0, 0.2), 0.0);
        approx::assert_abs_diff_eq!(portfolio_kelly_adjustment(0.2, 1.0), 0.1);
    }
}
