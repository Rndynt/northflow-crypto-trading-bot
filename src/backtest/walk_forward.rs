//! Walk-forward windowing — Phase 6 stub for future Phase 7+ analysis.
//!
//! Does not implement parameter optimisation.  Only produces deterministic
//! rolling index windows over a fixed-length dataset.

// ── WalkForwardWindow ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkForwardWindow {
    pub train_start: usize,
    pub train_end: usize,
    pub test_start: usize,
    pub test_end: usize,
}

// ── Builder ───────────────────────────────────────────────────────────────────

/// Build deterministic rolling walk-forward windows.
///
/// Validation rules:
/// - Any zero length → empty Vec.
/// - total_len < train_len + test_len → empty Vec.
/// - Windows advance by `step` each iteration.
pub fn build_walk_forward_windows(
    total_len: usize,
    train_len: usize,
    test_len: usize,
    step: usize,
) -> Vec<WalkForwardWindow> {
    if total_len == 0 || train_len == 0 || test_len == 0 || step == 0 {
        return Vec::new();
    }
    if total_len < train_len + test_len {
        return Vec::new();
    }

    let mut windows = Vec::new();
    let mut train_start = 0;

    loop {
        let train_end = train_start + train_len;
        let test_start = train_end;
        let test_end = test_start + test_len;

        if test_end > total_len {
            break;
        }

        windows.push(WalkForwardWindow {
            train_start,
            train_end,
            test_start,
            test_end,
        });

        train_start += step;
    }

    windows
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_forward_returns_empty_for_zero_lengths() {
        assert!(build_walk_forward_windows(0, 10, 5, 5).is_empty());
        assert!(build_walk_forward_windows(100, 0, 5, 5).is_empty());
        assert!(build_walk_forward_windows(100, 10, 0, 5).is_empty());
        assert!(build_walk_forward_windows(100, 10, 5, 0).is_empty());
    }

    #[test]
    fn walk_forward_returns_empty_when_not_enough_data() {
        // total < train + test → empty
        assert!(build_walk_forward_windows(14, 10, 5, 5).is_empty());
        // total == train + test produces exactly one window, not empty
        // (tested separately in walk_forward_exact_fit_produces_one_window)
        assert!(build_walk_forward_windows(9, 5, 5, 5).is_empty());
    }

    #[test]
    fn walk_forward_builds_deterministic_windows() {
        let ws = build_walk_forward_windows(30, 10, 5, 5);
        assert!(!ws.is_empty());

        let first = &ws[0];
        assert_eq!(first.train_start, 0);
        assert_eq!(first.train_end, 10);
        assert_eq!(first.test_start, 10);
        assert_eq!(first.test_end, 15);

        let second = &ws[1];
        assert_eq!(second.train_start, 5);
        assert_eq!(second.train_end, 15);
        assert_eq!(second.test_start, 15);
        assert_eq!(second.test_end, 20);
    }

    #[test]
    fn walk_forward_windows_do_not_exceed_total_len() {
        let ws = build_walk_forward_windows(20, 10, 5, 3);
        for w in &ws {
            assert!(w.test_end <= 20);
        }
    }

    #[test]
    fn walk_forward_exact_fit_produces_one_window() {
        let ws = build_walk_forward_windows(15, 10, 5, 5);
        assert_eq!(ws.len(), 1);
        let w = &ws[0];
        assert_eq!(w.train_start, 0);
        assert_eq!(w.train_end, 10);
        assert_eq!(w.test_start, 10);
        assert_eq!(w.test_end, 15);
    }
}
