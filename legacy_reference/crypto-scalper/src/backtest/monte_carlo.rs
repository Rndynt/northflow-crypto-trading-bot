#[derive(Debug, Clone, Copy)]
pub struct MonteCarloDrawdown {
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
}

pub fn drawdown_confidence_intervals(
    pnls: &[f64],
    simulations: usize,
) -> Option<MonteCarloDrawdown> {
    if pnls.is_empty() || simulations == 0 {
        return None;
    }
    let mut drawdowns = Vec::with_capacity(simulations);
    for seed in 0..simulations {
        let mut shuffled = pnls.to_vec();
        deterministic_shuffle(&mut shuffled, seed as u64 + 1);
        drawdowns.push(max_drawdown_pct(&shuffled));
    }
    drawdowns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(MonteCarloDrawdown {
        p50: percentile(&drawdowns, 0.50),
        p95: percentile(&drawdowns, 0.95),
        p99: percentile(&drawdowns, 0.99),
    })
}

fn max_drawdown_pct(pnls: &[f64]) -> f64 {
    let mut equity = 0.0;
    let mut peak = 0.0;
    let mut max_dd = 0.0;
    for pnl in pnls {
        equity += pnl;
        if equity > peak {
            peak = equity;
        }
        if peak > 0.0 {
            let dd = (peak - equity) / peak * 100.0;
            if dd > max_dd {
                max_dd = dd;
            }
        }
    }
    max_dd
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    let idx = ((sorted.len() - 1) as f64 * p.clamp(0.0, 1.0)).round() as usize;
    sorted[idx]
}

fn deterministic_shuffle(values: &mut [f64], mut state: u64) {
    for i in (1..values.len()).rev() {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let j = (state as usize) % (i + 1);
        values.swap(i, j);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_drawdown_intervals() {
        let pnls = [10.0, -5.0, 12.0, -8.0, 4.0, -3.0];
        let ci = drawdown_confidence_intervals(&pnls, 20).unwrap();
        assert!(ci.p95 >= ci.p50);
    }
}
