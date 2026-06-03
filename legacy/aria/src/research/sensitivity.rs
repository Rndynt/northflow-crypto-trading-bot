use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct ParameterPoint {
    pub params: BTreeMap<String, f64>,
    pub score: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SensitivitySummary {
    pub best_score: f64,
    pub median_score: f64,
    pub robustness_ratio: f64,
}

pub fn summarize_parameter_sensitivity(points: &[ParameterPoint]) -> Option<SensitivitySummary> {
    if points.is_empty() {
        return None;
    }
    let mut scores: Vec<f64> = points
        .iter()
        .map(|p| p.score)
        .filter(|x| x.is_finite())
        .collect();
    if scores.is_empty() {
        return None;
    }
    scores.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let best_score = scores[scores.len() - 1];
    let median_score = scores[scores.len() / 2];
    let robustness_ratio = if best_score.abs() > 1e-9 {
        median_score / best_score
    } else {
        0.0
    };
    Some(SensitivitySummary {
        best_score,
        median_score,
        robustness_ratio,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarizes_robustness() {
        let points = [1.0, 2.0, 3.0]
            .into_iter()
            .map(|score| ParameterPoint {
                params: BTreeMap::new(),
                score,
            })
            .collect::<Vec<_>>();
        let summary = summarize_parameter_sensitivity(&points).unwrap();
        approx::assert_abs_diff_eq!(summary.best_score, 3.0);
        approx::assert_abs_diff_eq!(summary.robustness_ratio, 2.0 / 3.0);
    }
}
