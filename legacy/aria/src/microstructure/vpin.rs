use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct Vpin {
    bucket_size: f64,
    window: usize,
    current_buy: f64,
    current_sell: f64,
    buckets: VecDeque<f64>,
    /// Rolling history of VPIN values for adaptive thresholding
    value_history: VecDeque<f64>,
    /// How many VPIN values to keep for percentile calculation
    history_window: usize,
}

impl Vpin {
    pub fn new(bucket_size: f64, window: usize) -> Self {
        Self {
            bucket_size: bucket_size.max(1e-9),
            window: window.max(1),
            current_buy: 0.0,
            current_sell: 0.0,
            buckets: VecDeque::with_capacity(window.max(1)),
            value_history: VecDeque::with_capacity(500),
            history_window: 500,
        }
    }

    pub fn update(&mut self, buy_volume: f64, sell_volume: f64) -> Option<f64> {
        self.current_buy += buy_volume.max(0.0);
        self.current_sell += sell_volume.max(0.0);
        let total = self.current_buy + self.current_sell;
        if total < self.bucket_size {
            // Do not re-emit the previous VPIN value on every trade.  The signal
            // agent only needs a fresh value when a volume bucket closes; emitting
            // stale values here made percentile history fill with duplicates and
            // produced noisy repeated abnormal alerts.
            return None;
        }
        let imbalance = (self.current_buy - self.current_sell).abs() / total.max(1e-9);
        self.buckets.push_back(imbalance);
        if self.buckets.len() > self.window {
            self.buckets.pop_front();
        }
        self.current_buy = 0.0;
        self.current_sell = 0.0;
        let raw = self.value()?;
        self.record_value(raw);
        Some(raw)
    }

    pub fn value(&self) -> Option<f64> {
        if self.buckets.len() < self.window {
            return None;
        }
        Some(self.buckets.iter().sum::<f64>() / self.buckets.len() as f64)
    }

    /// Record a VPIN value into history for percentile calculation.
    pub fn record_value(&mut self, v: f64) {
        self.value_history.push_back(v);
        if self.value_history.len() > self.history_window {
            self.value_history.pop_front();
        }
    }

    /// Get the adaptive threshold (95th percentile of recent VPIN values).
    /// Returns None if not enough history.
    pub fn adaptive_threshold(&self) -> Option<f64> {
        if self.value_history.len() < 50 {
            return None;
        }
        let mut sorted: Vec<f64> = self.value_history.iter().copied().collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let idx = ((sorted.len() as f64) * 0.95) as usize;
        let idx = idx.min(sorted.len() - 1);
        Some(sorted[idx])
    }

    /// Check if VPIN is abnormally high (above 95th percentile).
    /// Returns (is_abnormal, raw_value, threshold).
    pub fn is_abnormal(&self) -> Option<(bool, f64, f64)> {
        let raw = self.value()?;
        match self.adaptive_threshold() {
            Some(thresh) => Some((raw > thresh, raw, thresh)),
            None => None, // Not enough history yet — don't flag
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_vpin_after_window_fills() {
        let mut vpin = Vpin::new(10.0, 2);
        assert!(vpin.update(10.0, 0.0).is_none());
        let value = vpin.update(5.0, 5.0).unwrap();
        approx::assert_abs_diff_eq!(value, 0.5);
    }

    #[test]
    fn adaptive_threshold_requires_history() {
        let vpin = Vpin::new(10.0, 2);
        // Not enough history
        assert!(vpin.adaptive_threshold().is_none());
    }

    #[test]
    fn does_not_reemit_stale_value_between_bucket_closes() {
        let mut vpin = Vpin::new(10.0, 2);
        assert!(vpin.update(10.0, 0.0).is_none());
        assert!(vpin.update(10.0, 0.0).is_some());
        assert!(vpin.update(1.0, 0.0).is_none());
    }
}
