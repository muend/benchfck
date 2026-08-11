use crate::{
    bf::{BfProgram, MachineState},
    schema::{Family, TaskRecord},
    tasks::Verification,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCriticality {
    Benign,
    Critical,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FailureMode {
    Correct,
    Drift,
    Collapse,
    ParseFailure,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ItemMetric {
    pub schema_version: String,
    pub task_id: String,
    pub family: Family,
    pub correct: bool,
    pub parse_failure: bool,
    pub tokens_used: u64,
    pub n_ideal: u64,
    pub overhead_ratio: f64,
    pub abstraction_gain: Option<f64>,
    pub first_divergence_step: Option<u64>,
    pub criticality: Option<ErrorCriticality>,
    pub failure_mode: FailureMode,
}

pub fn item_metric(
    task: &TaskRecord,
    v: &Verification,
    tokens_used: u64,
    n_ideal: u64,
    first_divergence_step: Option<u64>,
    criticality: Option<ErrorCriticality>,
) -> ItemMetric {
    let rho = if n_ideal == 0 {
        f64::INFINITY
    } else {
        tokens_used as f64 / n_ideal as f64
    };
    ItemMetric {
        schema_version: "benchfck.metric.v3".into(),
        task_id: task.task_id.clone(),
        family: task.family,
        correct: v.correct,
        parse_failure: v.parse_failure,
        tokens_used,
        n_ideal,
        overhead_ratio: rho,
        abstraction_gain: if v.correct && tokens_used > 0 {
            Some(n_ideal as f64 / tokens_used as f64)
        } else {
            None
        },
        first_divergence_step,
        criticality,
        failure_mode: if v.correct {
            FailureMode::Correct
        } else if v.parse_failure {
            FailureMode::ParseFailure
        } else if v.detail.contains("mismatch") {
            FailureMode::Drift
        } else {
            FailureMode::Collapse
        },
    }
}

pub fn criticality(
    program: &BfProgram,
    claimed: MachineState,
    input: &[u8],
    reference_output: &[u8],
    step_cap: u64,
) -> ErrorCriticality {
    match program.continue_from(claimed, input, step_cap, false) {
        Ok(r) if r.state.output == reference_output => ErrorCriticality::Benign,
        _ => ErrorCriticality::Critical,
    }
}

/// Adaptive binary search over an exact probe callback. Returns the first
/// divergent completed step with O(log N) probes when divergence is monotone.
pub fn first_divergence<F>(n_steps: u64, mut probe: F) -> Option<u64>
where
    F: FnMut(u64) -> bool,
{
    if n_steps == 0 || probe(n_steps) {
        return None;
    }
    let (mut lo, mut hi) = (0, n_steps);
    while lo + 1 < hi {
        let mid = lo + (hi - lo) / 2;
        if probe(mid) { lo = mid } else { hi = mid }
    }
    Some(hi)
}

pub fn estimate_n_star(mut accuracy_by_trace: Vec<(u64, f64)>) -> Option<f64> {
    accuracy_by_trace.sort_by_key(|x| x.0);
    for w in accuracy_by_trace.windows(2) {
        let (a, b) = (w[0], w[1]);
        if (a.1 - 0.5) * (b.1 - 0.5) <= 0.0 && a.1 != b.1 {
            return Some(a.0 as f64 + (0.5 - a.1) * (b.0 - a.0) as f64 / (b.1 - a.1));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binary_search_finds_boundary() {
        assert_eq!(first_divergence(100, |t| t < 37), Some(37));
    }
    #[test]
    fn n_star_interpolates() {
        assert_eq!(estimate_n_star(vec![(10, 0.8), (20, 0.2)]), Some(15.0));
    }
}
