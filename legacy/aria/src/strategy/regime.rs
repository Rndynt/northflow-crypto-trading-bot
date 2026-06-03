//! Market regime detection (trending / ranging / volatile / squeeze).

use super::state::SymbolState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Regime {
    TrendingBullish,
    TrendingBearish,
    Ranging,
    Volatile,
    Squeeze,
    Unknown,
}

impl Regime {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TrendingBullish => "TRENDING_BULLISH",
            Self::TrendingBearish => "TRENDING_BEARISH",
            Self::Ranging => "RANGING",
            Self::Volatile => "VOLATILE",
            Self::Squeeze => "SQUEEZE",
            Self::Unknown => "UNKNOWN",
        }
    }
}

pub struct RegimeDetector;

impl RegimeDetector {
    /// Derive the regime from the latest indicator snapshot.
    /// Uses available indicators to make best guess even on cold start.
    pub fn detect(state: &SymbolState) -> Regime {
        let chop = state.last_choppiness.unwrap_or(50.0);
        let bb = state.last_bb;
        let kupper = state.last_keltner_upper;
        let klower = state.last_keltner_lower;
        let plus = state.last_di_plus.unwrap_or(0.0);
        let minus = state.last_di_minus.unwrap_or(0.0);

        // Squeeze: BB inside Keltner
        if let (Some(bb), Some(u), Some(l)) = (bb, kupper, klower) {
            if bb.upper < u && bb.lower > l && chop > 55.0 {
                return Regime::Squeeze;
            }
        }

        // If ADX available, use it for precise classification
        if let Some(adx) = state.last_adx {
            if adx >= 40.0 {
                return Regime::Volatile;
            }
            if adx >= 25.0 && chop < 38.2 {
                if plus >= minus {
                    return Regime::TrendingBullish;
                }
                return Regime::TrendingBearish;
            }
            if adx < 20.0 || chop > 61.8 {
                return Regime::Ranging;
            }
        }

        // Fallback: use DI+/DI- and choppiness when ADX not available (cold start)
        if plus > 0.0 || minus > 0.0 {
            let di_sum = plus + minus;
            if di_sum > 30.0 {
                // Strong directional signal
                if chop < 45.0 {
                    if plus >= minus {
                        return Regime::TrendingBullish;
                    }
                    return Regime::TrendingBearish;
                }
            }
        }

        // BB width as volatility proxy
        if let Some(bb) = bb {
            let bb_width = if bb.upper > 0.0 {
                (bb.upper - bb.lower) / bb.upper * 100.0
            } else {
                0.0
            };
            if bb_width > 3.0 {
                return Regime::Volatile;
            }
        }

        // Choppiness-only fallback
        if chop > 61.8 {
            return Regime::Ranging;
        }
        if chop < 38.2 {
            // Likely trending but can't determine direction
            if plus >= minus {
                return Regime::TrendingBullish;
            }
            return Regime::TrendingBearish;
        }

        // Last resort: classify as Ranging instead of Unknown
        Regime::Ranging
    }
}
