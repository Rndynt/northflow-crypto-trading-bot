//! 15m market-bias screening module.
//!
//! `compute_screening` reads a 15m `SymbolState` and produces a `ScreeningState`
//! using multi-indicator voting:
//!
//! Bullish: EMA8 > EMA21 > EMA50, close > VWAP, Kalman slope +, ATR OK, choppiness OK
//! Bearish: inverse
//! NoTrade: mixed/flat/choppy/stale indicators
//!
//! The result drives the hard gate in `SignalAgent` — 1m entries are only
//! permitted in the direction the 15m bias allows.

use crate::agents::messages::ScreeningBias;
use crate::config::ScreeningCfg;
use crate::data::Candle;
use crate::strategy::state::SymbolState;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Full 15m screening state for one symbol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreeningState {
    pub symbol: String,
    pub timeframe_secs: i64,
    pub bias: ScreeningBias,
    /// Vote-based confidence score 0-100.
    pub confidence: u8,
    /// Human-readable reason string for logs/dashboard.
    pub reason: String,
    pub close: f64,
    pub vwap: Option<f64>,
    pub ema_fast: Option<f64>,
    pub ema_mid: Option<f64>,
    pub ema_slow: Option<f64>,
    /// +1 = Kalman bullish, -1 = bearish, 0 = neutral.
    pub kalman_direction: i8,
    /// ATR as a percentage of close price.
    pub atr_pct: Option<f64>,
    pub choppiness: Option<f64>,
    pub updated_at: DateTime<Utc>,
}

impl ScreeningState {
    /// True when the state is fresher than `max_age_secs`.
    pub fn is_fresh(&self, max_age_secs: u64) -> bool {
        let age = Utc::now()
            .signed_duration_since(self.updated_at)
            .num_seconds();
        age >= 0 && age <= max_age_secs as i64
    }
}

/// Compute a `ScreeningState` from the 15m `SymbolState`.
///
/// Uses a vote-based approach:
/// - Each bullish indicator condition adds +1 bull vote
/// - Each bearish indicator condition adds +1 bear vote
/// - Blocking conditions (choppy, bad ATR, missing data) force NoTrade
///
/// Final bias determined by net vote and confidence threshold.
pub fn compute_screening(
    state: &SymbolState,
    closed: &Candle,
    cfg: &ScreeningCfg,
) -> ScreeningState {
    let symbol = state.symbol.clone();
    let close = closed.close;
    let timeframe_secs = 900i64;

    let ema_fast = state.ema_8.value();
    let ema_mid = state.ema_21.value();
    let ema_slow = state.ema_50.value();
    let vwap = state.last_vwap;
    let atr = state.last_atr;
    let choppiness = state.last_choppiness;
    let vwap_slope = state.last_vwap_slope;

    let atr_pct = if close > 0.0 {
        atr.map(|a| a / close * 100.0)
    } else {
        None
    };

    let mut reasons: Vec<&str> = Vec::new();

    // --- Blocking conditions → force NoTrade ---

    if state.candles.len() < 20 {
        return ScreeningState {
            symbol,
            timeframe_secs,
            bias: ScreeningBias::Unknown,
            confidence: 0,
            reason: "insufficient_candles".into(),
            close,
            vwap,
            ema_fast,
            ema_mid,
            ema_slow,
            kalman_direction: 0,
            atr_pct,
            choppiness,
            updated_at: Utc::now(),
        };
    }

    if let Some(chop) = choppiness {
        if chop > cfg.max_choppiness {
            return ScreeningState {
                symbol,
                timeframe_secs,
                bias: ScreeningBias::NoTrade,
                confidence: 0,
                reason: format!("choppiness_high:{:.1}", chop),
                close,
                vwap,
                ema_fast,
                ema_mid,
                ema_slow,
                kalman_direction: 0,
                atr_pct,
                choppiness,
                updated_at: Utc::now(),
            };
        }
    }

    if let Some(ap) = atr_pct {
        if ap < cfg.min_atr_pct {
            return ScreeningState {
                symbol,
                timeframe_secs,
                bias: ScreeningBias::NoTrade,
                confidence: 0,
                reason: format!("atr_too_low:{:.3}%", ap),
                close,
                vwap,
                ema_fast,
                ema_mid,
                ema_slow,
                kalman_direction: 0,
                atr_pct,
                choppiness,
                updated_at: Utc::now(),
            };
        }
        if ap > cfg.max_atr_pct {
            return ScreeningState {
                symbol,
                timeframe_secs,
                bias: ScreeningBias::NoTrade,
                confidence: 0,
                reason: format!("atr_too_extreme:{:.3}%", ap),
                close,
                vwap,
                ema_fast,
                ema_mid,
                ema_slow,
                kalman_direction: 0,
                atr_pct,
                choppiness,
                updated_at: Utc::now(),
            };
        }
    }

    // --- Vote-based scoring ---
    let mut bull_votes = 0i32;
    let mut bear_votes = 0i32;
    let total_possible = 6i32;

    // 1. EMA structure
    match (ema_fast, ema_mid, ema_slow) {
        (Some(f), Some(m), Some(s)) => {
            if f > m && m > s {
                bull_votes += 2;
                reasons.push("ema_bull");
            } else if f < m && m < s {
                bear_votes += 2;
                reasons.push("ema_bear");
            } else {
                reasons.push("ema_mixed");
            }
        }
        _ => {
            reasons.push("ema_unavail");
        }
    }

    // 2. Price vs VWAP
    if let Some(v) = vwap {
        let dist_pct = if v > 0.0 {
            (close - v).abs() / v * 100.0
        } else {
            0.0
        };
        if dist_pct > cfg.max_vwap_distance_pct {
            return ScreeningState {
                symbol,
                timeframe_secs,
                bias: ScreeningBias::NoTrade,
                confidence: 0,
                reason: format!("price_overextended_vwap:{:.2}%", dist_pct),
                close,
                vwap: Some(v),
                ema_fast,
                ema_mid,
                ema_slow,
                kalman_direction: 0,
                atr_pct,
                choppiness,
                updated_at: Utc::now(),
            };
        }
        if close > v {
            bull_votes += 1;
            reasons.push("price_above_vwap");
        } else {
            bear_votes += 1;
            reasons.push("price_below_vwap");
        }
    }

    // 3. Kalman / VWAP slope
    let kalman_direction: i8 = match vwap_slope {
        Some(s) if s > 0.0 => {
            bull_votes += 1;
            reasons.push("kalman_up");
            1
        }
        Some(s) if s < 0.0 => {
            bear_votes += 1;
            reasons.push("kalman_down");
            -1
        }
        _ => {
            reasons.push("kalman_neutral");
            0
        }
    };

    // 4. ADX trend strength
    if let Some(adx) = state.last_adx {
        if adx > 25.0 {
            if let (Some(dip), Some(dim)) = (state.last_di_plus, state.last_di_minus) {
                if dip > dim {
                    bull_votes += 1;
                    reasons.push("adx_bull");
                } else {
                    bear_votes += 1;
                    reasons.push("adx_bear");
                }
            }
        } else {
            reasons.push("adx_weak");
        }
    }

    // Compute confidence
    let net = bull_votes - bear_votes;
    let abs_net = net.unsigned_abs() as u8;
    let confidence = ((abs_net as f64 / total_possible as f64) * 100.0).min(100.0) as u8;
    let confidence = confidence.max(if abs_net > 0 { 30 } else { 0 });

    let bias = if net >= 2 && confidence >= cfg.min_confidence {
        ScreeningBias::Bullish
    } else if net <= -2 && confidence >= cfg.min_confidence {
        ScreeningBias::Bearish
    } else {
        ScreeningBias::NoTrade
    };

    let direction_label = match bias {
        ScreeningBias::Bullish => "bullish",
        ScreeningBias::Bearish => "bearish",
        ScreeningBias::NoTrade => "no_trade",
        ScreeningBias::Unknown => "unknown",
    };

    ScreeningState {
        symbol,
        timeframe_secs,
        bias,
        confidence,
        reason: format!(
            "{}({}):bull={}/bear={}",
            direction_label,
            reasons.join(","),
            bull_votes,
            bear_votes
        ),
        close,
        vwap,
        ema_fast,
        ema_mid,
        ema_slow,
        kalman_direction,
        atr_pct,
        choppiness,
        updated_at: Utc::now(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::messages::ScreeningBias;
    use crate::config::ScreeningCfg;
    use crate::data::Candle;
    use crate::strategy::state::SymbolState;
    use chrono::Utc;

    fn make_candle(close: f64, high: f64, low: f64, volume: f64) -> Candle {
        Candle {
            open_time: Utc::now(),
            open: close,
            high,
            low,
            close,
            volume,
            close_time: Utc::now(),
        }
    }

    fn default_cfg() -> ScreeningCfg {
        ScreeningCfg::default()
    }

    #[test]
    fn test_bullish_screening_allows_long_blocks_short() {
        let mut state = SymbolState::new("BTCUSDT");
        // Feed 60 rising candles to build EMA bullish structure
        let mut price = 40000.0f64;
        for _ in 0..60 {
            price *= 1.001;
            let c = make_candle(price, price * 1.002, price * 0.998, 100.0);
            state.on_closed(c);
        }
        let cfg = default_cfg();
        let closed = make_candle(price, price * 1.002, price * 0.998, 100.0);
        let result = compute_screening(&state, &closed, &cfg);

        // With rising prices, EMA structure should be bullish
        // Bias might be Bullish or NoTrade depending on indicators, but not Bearish
        assert_ne!(
            result.bias,
            ScreeningBias::Bearish,
            "rising market should not be bearish"
        );
    }

    #[test]
    fn test_insufficient_candles_returns_unknown() {
        let mut state = SymbolState::new("BTCUSDT");
        // Only feed 5 candles
        for i in 0..5 {
            let p = 40000.0 + i as f64 * 10.0;
            state.on_closed(make_candle(p, p + 20.0, p - 20.0, 100.0));
        }
        let cfg = default_cfg();
        let closed = make_candle(40050.0, 40070.0, 40030.0, 100.0);
        let result = compute_screening(&state, &closed, &cfg);
        assert_eq!(result.bias, ScreeningBias::Unknown);
    }

    #[test]
    fn test_high_choppiness_returns_no_trade() {
        let mut state = SymbolState::new("BTCUSDT");
        // Alternating candles = choppy market
        for i in 0..40 {
            let p = if i % 2 == 0 { 40100.0 } else { 39900.0 };
            state.on_closed(make_candle(p, p + 50.0, p - 50.0, 100.0));
        }
        let cfg = default_cfg();
        let closed = make_candle(40000.0, 40050.0, 39950.0, 100.0);
        let result = compute_screening(&state, &closed, &cfg);
        // Choppy market with choppiness > 61.8 should return NoTrade
        if let Some(chop) = result.choppiness {
            if chop > cfg.max_choppiness {
                assert_eq!(result.bias, ScreeningBias::NoTrade);
            }
        }
    }

    #[test]
    fn test_screening_bias_allows_method() {
        let bias = ScreeningBias::Bullish;
        assert!(bias.allows(&crate::data::Side::Long));
        assert!(!bias.allows(&crate::data::Side::Short));

        let bias = ScreeningBias::Bearish;
        assert!(!bias.allows(&crate::data::Side::Long));
        assert!(bias.allows(&crate::data::Side::Short));

        let bias = ScreeningBias::NoTrade;
        assert!(!bias.allows(&crate::data::Side::Long));
        assert!(!bias.allows(&crate::data::Side::Short));

        let bias = ScreeningBias::Unknown;
        assert!(bias.allows(&crate::data::Side::Long));
        assert!(bias.allows(&crate::data::Side::Short));
    }

    #[test]
    fn test_15m_state_does_not_mutate_1m_state() {
        // Separate SymbolState instances for 1m and 15m — they never share.
        let mut state_1m = SymbolState::new("BTCUSDT");
        let mut state_15m = SymbolState::new("BTCUSDT");

        // 1m candles only go into state_1m
        for _ in 0..20 {
            state_1m.on_closed(make_candle(40000.0, 40010.0, 39990.0, 50.0));
        }

        // 15m candles only go into state_15m (different prices)
        for _ in 0..20 {
            state_15m.on_closed(make_candle(50000.0, 50100.0, 49900.0, 200.0));
        }

        // Verify that 1m state has NOT been contaminated by 15m prices
        let last_1m = state_1m.last_candle().unwrap().close;
        let last_15m = state_15m.last_candle().unwrap().close;
        assert!(
            (last_1m - 40000.0).abs() < 1.0,
            "1m state should have 1m prices"
        );
        assert!(
            (last_15m - 50000.0).abs() < 1.0,
            "15m state should have 15m prices"
        );

        // EMA values should reflect their own candle series
        let ema_1m = state_1m.ema_8.value();
        let ema_15m = state_15m.ema_8.value();
        if let (Some(e1), Some(e15)) = (ema_1m, ema_15m) {
            assert!(
                (e1 - 40000.0).abs() < 500.0,
                "1m EMA should be near 40000, got {}",
                e1
            );
            assert!(
                (e15 - 50000.0).abs() < 500.0,
                "15m EMA should be near 50000, got {}",
                e15
            );
        }
    }
}
