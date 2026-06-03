//! MarketRegime — deterministic market regime classification from indicator snapshots.
//!
//! Rules:
//!   Bullish  — ema_50 > ema_200 AND close > ema_50
//!   Bearish  — ema_50 < ema_200 AND close < ema_50
//!   Neutral  — EMA values present but bullish/bearish rules do not pass
//!   Unknown  — ema_50 or ema_200 missing (indicator still warming up)
//!
//! No ML, no Kalman, no HMM. Deterministic rule evaluation only.

use crate::core::Candle;
use crate::indicators::IndicatorSnapshot;

// ── MarketRegime ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketRegime {
    Bullish,
    Bearish,
    Neutral,
    Unknown,
}

impl MarketRegime {
    /// Stable lowercase string representation (safe for signal `regime` field).
    pub fn as_str(self) -> &'static str {
        match self {
            MarketRegime::Bullish => "bullish",
            MarketRegime::Bearish => "bearish",
            MarketRegime::Neutral => "neutral",
            MarketRegime::Unknown => "unknown",
        }
    }
}

// ── Classification helper ─────────────────────────────────────────────────────

/// Classify the market regime for a single timeframe.
///
/// `candle` supplies the closing price.
/// `snapshot` supplies the EMA values for that timeframe.
///
/// Returns `Unknown` when either `ema_50` or `ema_200` is `None`.
pub fn classify_screening_regime(candle: Candle, snapshot: &IndicatorSnapshot) -> MarketRegime {
    let (ema_50, ema_200) = match (snapshot.ema_50, snapshot.ema_200) {
        (Some(a), Some(b)) => (a, b),
        _ => return MarketRegime::Unknown,
    };

    let close = candle.close;

    if ema_50 > ema_200 && close > ema_50 {
        MarketRegime::Bullish
    } else if ema_50 < ema_200 && close < ema_50 {
        MarketRegime::Bearish
    } else {
        MarketRegime::Neutral
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Candle;
    use crate::indicators::IndicatorSnapshot;

    fn candle(close: f64) -> Candle {
        Candle {
            timestamp: 1_700_000_000_000,
            open: close - 1.0,
            high: close + 1.0,
            low: close - 1.0,
            close,
            volume: 100.0,
        }
    }

    fn snapshot_with(ema_50: Option<f64>, ema_200: Option<f64>) -> IndicatorSnapshot {
        IndicatorSnapshot {
            ema_50,
            ema_200,
            ..Default::default()
        }
    }

    // ── Unknown ──────────────────────────────────────────────────────────────

    #[test]
    fn regime_unknown_when_missing_ema() {
        let snap = snapshot_with(None, None);
        assert_eq!(
            classify_screening_regime(candle(100.0), &snap),
            MarketRegime::Unknown
        );
    }

    #[test]
    fn regime_unknown_when_ema50_missing() {
        let snap = snapshot_with(None, Some(90.0));
        assert_eq!(
            classify_screening_regime(candle(100.0), &snap),
            MarketRegime::Unknown
        );
    }

    #[test]
    fn regime_unknown_when_ema200_missing() {
        let snap = snapshot_with(Some(100.0), None);
        assert_eq!(
            classify_screening_regime(candle(105.0), &snap),
            MarketRegime::Unknown
        );
    }

    // ── Bullish ───────────────────────────────────────────────────────────────

    #[test]
    fn regime_bullish_when_ema50_above_ema200_and_close_above_ema50() {
        // ema_50=100, ema_200=90 → ema_50 > ema_200 ✓
        // close=105 > ema_50=100 ✓
        let snap = snapshot_with(Some(100.0), Some(90.0));
        assert_eq!(
            classify_screening_regime(candle(105.0), &snap),
            MarketRegime::Bullish
        );
    }

    // ── Bearish ───────────────────────────────────────────────────────────────

    #[test]
    fn regime_bearish_when_ema50_below_ema200_and_close_below_ema50() {
        // ema_50=90, ema_200=100 → ema_50 < ema_200 ✓
        // close=85 < ema_50=90 ✓
        let snap = snapshot_with(Some(90.0), Some(100.0));
        assert_eq!(
            classify_screening_regime(candle(85.0), &snap),
            MarketRegime::Bearish
        );
    }

    // ── Neutral ───────────────────────────────────────────────────────────────

    #[test]
    fn regime_neutral_when_ema_relationship_exists_but_close_filter_fails() {
        // ema_50=100 > ema_200=90, but close=95 < ema_50=100 → not Bullish, not Bearish → Neutral
        let snap = snapshot_with(Some(100.0), Some(90.0));
        assert_eq!(
            classify_screening_regime(candle(95.0), &snap),
            MarketRegime::Neutral
        );
    }

    #[test]
    fn regime_neutral_when_ema50_equals_ema200() {
        // ema_50 == ema_200 → neither > nor < → Neutral
        let snap = snapshot_with(Some(100.0), Some(100.0));
        assert_eq!(
            classify_screening_regime(candle(105.0), &snap),
            MarketRegime::Neutral
        );
    }

    // ── as_str stability ──────────────────────────────────────────────────────

    #[test]
    fn regime_as_str_is_stable() {
        assert_eq!(MarketRegime::Bullish.as_str(), "bullish");
        assert_eq!(MarketRegime::Bearish.as_str(), "bearish");
        assert_eq!(MarketRegime::Neutral.as_str(), "neutral");
        assert_eq!(MarketRegime::Unknown.as_str(), "unknown");
    }
}
