//! Diagnostics: findings (slow / flaky / regression) and the overall health
//! score computed from history.

use std::collections::HashMap;

use crate::config::Config;
use crate::db::{DbError, History, RunSummary, StoredResult};
use crate::model::Status;

/// Version of the [`Report`] JSON output contract. Bump when the shape of
/// `Report` changes in a way that breaks consumers.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum AnalysisError {
    #[error("database error: {0}")]
    Db(#[from] DbError),
    #[error("no runs found in history")]
    NoRuns,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SlowFinding {
    pub id: String,
    pub time_sec: f64,
    pub threshold_sec: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FlakyFinding {
    pub id: String,
    pub failed_runs: u32,
    pub window_runs: u32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RegressionFinding {
    pub id: String,
    pub prev_sec: f64,
    pub current_sec: f64,
    pub factor: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CostInfo {
    pub total_min: f64,
    pub amount: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum HealthStatus {
    Healthy,
    NeedsPruning,
    Overgrown,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Report {
    /// Version of the JSON output contract this report conforms to.
    pub schema_version: u32,
    /// 0..=100.
    pub score: u32,
    pub status: HealthStatus,
    /// The run this report is about — the one with the latest `run_at`.
    pub run: RunSummary,
    /// Time-descending.
    pub slow: Vec<SlowFinding>,
    /// Failed-run-count-descending.
    pub flaky: Vec<FlakyFinding>,
    /// Factor-descending.
    pub regression: Vec<RegressionFinding>,
    pub cost: Option<CostInfo>,
    /// Set (and flaky/regression left empty) when history has fewer than 2
    /// runs, since both findings need a prior run to compare against.
    pub insufficient_history: Option<String>,
}

fn canonical_id(suite: &str, name: &str) -> String {
    format!("{suite}::{name}")
}

/// Analyzes the most recent run in `history` against `config`, producing a
/// [`Report`]. Errors if the history has no runs at all.
pub fn analyze(history: &History, config: &Config) -> Result<Report, AnalysisError> {
    let total_runs = history.run_count()?;
    if total_runs == 0 {
        return Err(AnalysisError::NoRuns);
    }

    // Fetch enough runs to cover both the flaky window and the single
    // previous run needed for regression detection.
    let fetch_limit = config.thresholds.flaky_window_runs.max(2);
    let runs = history.recent_runs(fetch_limit)?;
    let target_run = runs.first().cloned().ok_or(AnalysisError::NoRuns)?;

    let results = history.results_of_run(target_run.id)?;

    let mut slow: Vec<SlowFinding> = results
        .iter()
        .filter(|r| r.time_sec >= config.thresholds.slow_sec)
        .map(|r| SlowFinding {
            id: canonical_id(&r.suite, &r.name),
            time_sec: r.time_sec,
            threshold_sec: config.thresholds.slow_sec,
        })
        .collect();
    slow.sort_by(|a, b| b.time_sec.total_cmp(&a.time_sec));

    let (flaky, regression, insufficient_history) = if total_runs < 2 {
        let noun = if total_runs == 1 { "run" } else { "runs" };
        (
            Vec::new(),
            Vec::new(),
            Some(format!("insufficient history ({total_runs} {noun})")),
        )
    } else {
        let window_len = runs.len().min(config.thresholds.flaky_window_runs);
        let flaky = compute_flaky(history, &runs[..window_len])?;
        let regression = match runs.get(1) {
            Some(prev_run) => compute_regression(history, prev_run.id, &results, config)?,
            None => Vec::new(),
        };
        (flaky, regression, None)
    };

    let cost = if config.cost.per_min > 0.0 {
        let total_min = target_run.total_time_sec / 60.0;
        Some(CostInfo {
            total_min,
            amount: total_min * config.cost.per_min,
        })
    } else {
        None
    };

    let flaky_penalty = (flaky.len() as u32)
        .saturating_mul(config.weights.flaky)
        .min(config.weights.flaky_max);
    let slow_penalty = (slow.len() as u32)
        .saturating_mul(config.weights.slow)
        .min(config.weights.slow_max);
    let regression_penalty = (regression.len() as u32)
        .saturating_mul(config.weights.regression)
        .min(config.weights.regression_max);
    let total_penalty = flaky_penalty
        .saturating_add(slow_penalty)
        .saturating_add(regression_penalty);
    let score = 100u32.saturating_sub(total_penalty);

    let status = if score >= 85 {
        HealthStatus::Healthy
    } else if score >= 60 {
        HealthStatus::NeedsPruning
    } else {
        HealthStatus::Overgrown
    };

    Ok(Report {
        schema_version: SCHEMA_VERSION,
        score,
        status,
        run: target_run,
        slow,
        flaky,
        regression,
        cost,
        insufficient_history,
    })
}

/// A test is flaky within `window` if it shows up both passing and
/// failing/erroring at least once. `skipped` results are ignored entirely.
fn compute_flaky(
    history: &History,
    window: &[RunSummary],
) -> Result<Vec<FlakyFinding>, AnalysisError> {
    let mut per_test: HashMap<String, (u32, u32)> = HashMap::new();
    for run in window {
        let results = history.results_of_run(run.id)?;
        for r in &results {
            if r.status == Status::Skipped {
                continue;
            }
            let entry = per_test
                .entry(canonical_id(&r.suite, &r.name))
                .or_insert((0, 0));
            entry.1 += 1;
            if matches!(r.status, Status::Failed | Status::Error) {
                entry.0 += 1;
            }
        }
    }

    let mut flaky: Vec<FlakyFinding> = per_test
        .into_iter()
        .filter(|(_, (failed_runs, window_runs))| *failed_runs > 0 && *failed_runs < *window_runs)
        .map(|(id, (failed_runs, window_runs))| FlakyFinding {
            id,
            failed_runs,
            window_runs,
        })
        .collect();
    flaky.sort_by_key(|b| std::cmp::Reverse(b.failed_runs));
    Ok(flaky)
}

fn compute_regression(
    history: &History,
    prev_run_id: i64,
    current_results: &[StoredResult],
    config: &Config,
) -> Result<Vec<RegressionFinding>, AnalysisError> {
    let prev_results = history.results_of_run(prev_run_id)?;
    let prev_map: HashMap<String, f64> = prev_results
        .iter()
        .map(|r| (canonical_id(&r.suite, &r.name), r.time_sec))
        .collect();

    let mut regression = Vec::new();
    for r in current_results {
        let id = canonical_id(&r.suite, &r.name);
        let Some(&prev_sec) = prev_map.get(&id) else {
            continue;
        };
        // A zero-duration baseline makes the growth factor meaningless.
        if prev_sec == 0.0 {
            continue;
        }
        let current_sec = r.time_sec;
        if current_sec >= config.thresholds.regression_factor * prev_sec
            && current_sec - prev_sec >= config.thresholds.regression_min_sec
        {
            regression.push(RegressionFinding {
                id,
                prev_sec,
                current_sec,
                factor: current_sec / prev_sec,
            });
        }
    }
    regression.sort_by(|a, b| b.factor.total_cmp(&a.factor));
    Ok(regression)
}
