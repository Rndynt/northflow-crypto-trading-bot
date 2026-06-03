use crate::data::Side;
use crate::strategy::state::PreSignal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeframeVote {
    Bullish,
    Bearish,
    Neutral,
}

impl TimeframeVote {
    pub fn from_signal(signal: &PreSignal) -> Self {
        match signal.side {
            Side::Long => Self::Bullish,
            Side::Short => Self::Bearish,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WeightedVote {
    pub timeframe_secs: i64,
    pub vote: TimeframeVote,
    pub weight: f64,
}

pub fn aggregate_votes(votes: &[WeightedVote]) -> f64 {
    let total_weight: f64 = votes.iter().map(|v| v.weight.max(0.0)).sum();
    if total_weight <= 0.0 {
        return 0.0;
    }
    let directional = votes
        .iter()
        .map(|v| {
            let sign = match v.vote {
                TimeframeVote::Bullish => 1.0,
                TimeframeVote::Bearish => -1.0,
                TimeframeVote::Neutral => 0.0,
            };
            sign * v.weight.max(0.0)
        })
        .sum::<f64>();
    (directional / total_weight).clamp(-1.0, 1.0)
}

pub fn passes_timeframe_confirmation(
    signal: &PreSignal,
    votes: &[WeightedVote],
    min_abs: f64,
) -> bool {
    let aggregate = aggregate_votes(votes);
    match signal.side {
        Side::Long => aggregate >= min_abs,
        Side::Short => aggregate <= -min_abs,
    }
}

pub fn freshness_weight(age_candles: usize, half_life_candles: f64) -> f64 {
    if half_life_candles <= 0.0 {
        return 0.0;
    }
    0.5_f64.powf(age_candles as f64 / half_life_candles)
}

pub fn confidence_with_freshness(
    base_confidence: u8,
    age_candles: usize,
    half_life_candles: f64,
) -> u8 {
    (base_confidence as f64 * freshness_weight(age_candles, half_life_candles))
        .round()
        .clamp(0.0, 100.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy::state::StrategyName;

    #[test]
    fn aggregates_directional_votes() {
        let votes = vec![
            WeightedVote {
                timeframe_secs: 300,
                vote: TimeframeVote::Bullish,
                weight: 1.0,
            },
            WeightedVote {
                timeframe_secs: 900,
                vote: TimeframeVote::Bullish,
                weight: 2.0,
            },
            WeightedVote {
                timeframe_secs: 3600,
                vote: TimeframeVote::Bearish,
                weight: 1.0,
            },
        ];
        approx::assert_abs_diff_eq!(aggregate_votes(&votes), 0.5);
    }

    #[test]
    fn confirms_signal_direction() {
        let signal = PreSignal {
                signal_id: String::new(),
            symbol: "BTCUSDT".into(),
            strategy: StrategyName::Momentum,
            side: Side::Long,
            entry: 100.0,
            stop_loss: 99.0,
            take_profit: 102.0,
            ta_confidence: 70,
            reason: "test".into(),
            atr: None,
        };
        let votes = vec![WeightedVote {
            timeframe_secs: 300,
            vote: TimeframeVote::Bullish,
            weight: 1.0,
        }];
        assert!(passes_timeframe_confirmation(&signal, &votes, 0.5));
    }

    #[test]
    fn decays_stale_signal_confidence() {
        assert_eq!(confidence_with_freshness(80, 0, 4.0), 80);
        assert_eq!(confidence_with_freshness(80, 4, 4.0), 40);
    }
}
