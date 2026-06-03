use crate::backtest::metrics::PerformanceMetrics;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkForwardSplit {
    pub train_start: usize,
    pub train_end: usize,
    pub test_start: usize,
    pub test_end: usize,
}

#[derive(Debug, Clone)]
pub struct WalkForwardWindow {
    pub split: WalkForwardSplit,
    pub in_sample: PerformanceMetrics,
    pub out_of_sample: PerformanceMetrics,
    pub degradation_pct: f64,
    pub robust: bool,
}

#[derive(Debug, Clone)]
pub struct WalkForwardResult {
    pub windows: Vec<WalkForwardWindow>,
    pub combined_oos_metrics: PerformanceMetrics,
    pub oos_profit_factor: f64,
    pub oos_sharpe: f64,
    pub is_robust: bool,
    pub avg_degradation_pct: f64,
}

pub fn walk_forward_splits(
    len: usize,
    train_window: usize,
    test_window: usize,
    step: usize,
) -> Vec<WalkForwardSplit> {
    if len == 0 || train_window == 0 || test_window == 0 || step == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut train_start = 0usize;
    while train_start + train_window + test_window <= len {
        let train_end = train_start + train_window;
        let test_end = train_end + test_window;
        out.push(WalkForwardSplit {
            train_start,
            train_end,
            test_start: train_end,
            test_end,
        });
        train_start += step;
    }
    out
}

pub fn evaluate_walk_forward(
    splits: &[WalkForwardSplit],
    in_sample_pnls: &[Vec<f64>],
    oos_pnls: &[Vec<f64>],
    annualization_factor: f64,
    min_oos_trades: u32,
) -> WalkForwardResult {
    let mut windows = Vec::new();
    let mut combined = Vec::new();
    for (idx, split) in splits.iter().copied().enumerate() {
        let is = in_sample_pnls.get(idx).map(Vec::as_slice).unwrap_or(&[]);
        let oos = oos_pnls.get(idx).map(Vec::as_slice).unwrap_or(&[]);
        let in_sample = PerformanceMetrics::from_trades_annualized(is, annualization_factor);
        let out_of_sample = PerformanceMetrics::from_trades_annualized(oos, annualization_factor);
        combined.extend_from_slice(oos);
        let degradation_pct =
            if in_sample.profit_factor.is_finite() && in_sample.profit_factor > 0.0 {
                ((in_sample.profit_factor - out_of_sample.profit_factor) / in_sample.profit_factor
                    * 100.0)
                    .max(0.0)
            } else {
                0.0
            };
        let robust = out_of_sample.trades >= min_oos_trades
            && out_of_sample.profit_factor.is_finite()
            && in_sample.profit_factor.is_finite()
            && out_of_sample.profit_factor >= in_sample.profit_factor * 0.7;
        windows.push(WalkForwardWindow {
            split,
            in_sample,
            out_of_sample,
            degradation_pct,
            robust,
        });
    }
    let combined_oos_metrics =
        PerformanceMetrics::from_trades_annualized(&combined, annualization_factor);
    let avg_degradation_pct = if windows.is_empty() {
        0.0
    } else {
        windows.iter().map(|w| w.degradation_pct).sum::<f64>() / windows.len() as f64
    };
    let is_robust = !windows.is_empty() && windows.iter().all(|w| w.robust);
    WalkForwardResult {
        oos_profit_factor: combined_oos_metrics.profit_factor,
        oos_sharpe: combined_oos_metrics.sharpe,
        windows,
        combined_oos_metrics,
        is_robust,
        avg_degradation_pct,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_rolling_splits() {
        let splits = walk_forward_splits(100, 50, 10, 10);
        assert_eq!(splits.len(), 5);
        assert_eq!(
            splits[0],
            WalkForwardSplit {
                train_start: 0,
                train_end: 50,
                test_start: 50,
                test_end: 60
            }
        );
        assert_eq!(splits[4].test_end, 100);
    }

    #[test]
    fn evaluates_oos_robustness() {
        let splits = walk_forward_splits(100, 40, 10, 10);
        let is = vec![vec![2.0, -1.0, 2.0, -1.0]; splits.len()];
        let oos = vec![vec![1.0, -1.0, 1.0, -0.5]; splits.len()];
        let result = evaluate_walk_forward(&splits, &is, &oos, 365.0, 2);
        assert_eq!(result.windows.len(), splits.len());
        assert!(result.combined_oos_metrics.trades > 0);
    }
}
