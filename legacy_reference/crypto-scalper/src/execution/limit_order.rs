use crate::data::Side;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitOrderStyle {
    PostOnly,
    JoinBest,
    CrossSpread,
}

#[derive(Debug, Clone, Copy)]
pub struct LimitOrderPlan {
    pub price: f64,
    pub style: LimitOrderStyle,
    pub expected_fill_probability: f64,
}

pub fn fill_probability(queue_ahead_qty: f64, expected_trade_qty: f64, horizon_secs: f64) -> f64 {
    if queue_ahead_qty <= 0.0 {
        return 1.0;
    }
    if expected_trade_qty <= 0.0 || horizon_secs <= 0.0 {
        return 0.0;
    }
    (expected_trade_qty * horizon_secs / queue_ahead_qty).clamp(0.0, 1.0)
}

pub fn plan_limit_order(
    side: Side,
    best_bid: f64,
    best_ask: f64,
    fair_price: f64,
    queue_ahead_qty: f64,
    expected_trade_qty_per_sec: f64,
    horizon_secs: f64,
) -> Option<LimitOrderPlan> {
    if best_bid <= 0.0 || best_ask <= best_bid || fair_price <= 0.0 {
        return None;
    }
    let fill_prob = fill_probability(queue_ahead_qty, expected_trade_qty_per_sec, horizon_secs);
    let mid = (best_bid + best_ask) / 2.0;
    let plan = match side {
        Side::Long if fair_price > best_ask => LimitOrderPlan {
            price: best_ask,
            style: LimitOrderStyle::CrossSpread,
            expected_fill_probability: 1.0,
        },
        Side::Short if fair_price < best_bid => LimitOrderPlan {
            price: best_bid,
            style: LimitOrderStyle::CrossSpread,
            expected_fill_probability: 1.0,
        },
        Side::Long if fair_price >= mid && fill_prob >= 0.5 => LimitOrderPlan {
            price: best_bid,
            style: LimitOrderStyle::JoinBest,
            expected_fill_probability: fill_prob,
        },
        Side::Short if fair_price <= mid && fill_prob >= 0.5 => LimitOrderPlan {
            price: best_ask,
            style: LimitOrderStyle::JoinBest,
            expected_fill_probability: fill_prob,
        },
        Side::Long => LimitOrderPlan {
            price: best_bid,
            style: LimitOrderStyle::PostOnly,
            expected_fill_probability: fill_prob,
        },
        Side::Short => LimitOrderPlan {
            price: best_ask,
            style: LimitOrderStyle::PostOnly,
            expected_fill_probability: fill_prob,
        },
    };
    Some(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimates_fill_probability_and_plan() {
        approx::assert_abs_diff_eq!(fill_probability(100.0, 10.0, 5.0), 0.5);
        let plan = plan_limit_order(Side::Long, 100.0, 100.1, 100.2, 100.0, 1.0, 5.0).unwrap();
        assert_eq!(plan.style, LimitOrderStyle::CrossSpread);
        approx::assert_abs_diff_eq!(plan.price, 100.1);
    }
}
