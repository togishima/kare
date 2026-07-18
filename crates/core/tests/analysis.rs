//! Tests for findings analysis, health scoring, and config loading.

use kare_core::analysis::{analyze, HealthStatus};
use kare_core::config::{self, Config};
use kare_core::db::{History, RunMeta};
use kare_core::model::{Status, TestResult, TestRun};

fn test_result(suite: &str, name: &str, time_sec: f64, status: Status) -> TestResult {
    TestResult {
        suite: suite.to_string(),
        name: name.to_string(),
        time_sec,
        status,
    }
}

fn run(results: Vec<TestResult>) -> TestRun {
    TestRun {
        results,
        timestamp: None,
    }
}

fn meta(run_at: &str) -> RunMeta {
    RunMeta {
        run_at: run_at.to_string(),
        ingested_at: "2026-07-01T00:00:00Z".to_string(),
        git_ref: None,
        ci_job_url: None,
    }
}

#[test]
fn identical_runs_produce_no_flaky_or_regression() {
    let mut history = History::open_in_memory().expect("open in-memory db");
    let r = run(vec![test_result("Suite", "testA", 0.5, Status::Passed)]);
    history
        .insert_run(&r, &meta("2026-07-01T00:00:00Z"))
        .expect("insert run 1");
    history
        .insert_run(&r, &meta("2026-07-02T00:00:00Z"))
        .expect("insert run 2");

    let report = analyze(&history, &Config::default()).expect("analyze");
    assert!(report.flaky.is_empty());
    assert!(report.regression.is_empty());
    assert!(report.insufficient_history.is_none());
}

#[test]
fn flaky_test_detected_across_window() {
    let mut history = History::open_in_memory().expect("open in-memory db");
    let passed = run(vec![test_result("Suite", "testA", 0.1, Status::Passed)]);
    let failed = run(vec![test_result("Suite", "testA", 0.1, Status::Failed)]);

    history
        .insert_run(&passed, &meta("2026-07-01T00:00:00Z"))
        .expect("insert run 1");
    history
        .insert_run(&failed, &meta("2026-07-02T00:00:00Z"))
        .expect("insert run 2");
    history
        .insert_run(&passed, &meta("2026-07-03T00:00:00Z"))
        .expect("insert run 3");

    let report = analyze(&history, &Config::default()).expect("analyze");
    assert_eq!(report.flaky.len(), 1);
    let f = &report.flaky[0];
    assert_eq!(f.id, "Suite::testA");
    assert_eq!(f.failed_runs, 1);
    assert_eq!(f.window_runs, 3);
}

#[test]
fn skipped_results_are_ignored_for_flakiness() {
    let mut history = History::open_in_memory().expect("open in-memory db");
    let passed = run(vec![test_result("Suite", "testA", 0.1, Status::Passed)]);
    let skipped = run(vec![test_result("Suite", "testA", 0.1, Status::Skipped)]);

    history
        .insert_run(&passed, &meta("2026-07-01T00:00:00Z"))
        .expect("insert run 1");
    history
        .insert_run(&skipped, &meta("2026-07-02T00:00:00Z"))
        .expect("insert run 2");

    let report = analyze(&history, &Config::default()).expect("analyze");
    assert!(report.flaky.is_empty());
}

#[test]
fn regression_detected_above_threshold() {
    let mut history = History::open_in_memory().expect("open in-memory db");
    let prev = run(vec![test_result("Suite", "testA", 0.1, Status::Passed)]);
    let current = run(vec![test_result("Suite", "testA", 1.0, Status::Passed)]);

    history
        .insert_run(&prev, &meta("2026-07-01T00:00:00Z"))
        .expect("insert prev run");
    history
        .insert_run(&current, &meta("2026-07-02T00:00:00Z"))
        .expect("insert current run");

    let report = analyze(&history, &Config::default()).expect("analyze");
    assert_eq!(report.regression.len(), 1);
    let r = &report.regression[0];
    assert_eq!(r.id, "Suite::testA");
    assert!((r.prev_sec - 0.1).abs() < 1e-9);
    assert!((r.current_sec - 1.0).abs() < 1e-9);
    assert!((r.factor - 10.0).abs() < 1e-9);
}

#[test]
fn regression_below_threshold_is_not_detected() {
    let mut history = History::open_in_memory().expect("open in-memory db");
    let prev = run(vec![test_result("Suite", "testA", 0.1, Status::Passed)]);
    let current = run(vec![test_result("Suite", "testA", 0.15, Status::Passed)]);

    history
        .insert_run(&prev, &meta("2026-07-01T00:00:00Z"))
        .expect("insert prev run");
    history
        .insert_run(&current, &meta("2026-07-02T00:00:00Z"))
        .expect("insert current run");

    let report = analyze(&history, &Config::default()).expect("analyze");
    assert!(report.regression.is_empty());
}

#[test]
fn slow_findings_only_above_threshold_and_time_descending() {
    let mut history = History::open_in_memory().expect("open in-memory db");
    let r = run(vec![
        test_result("Suite", "fast", 0.2, Status::Passed),
        test_result("Suite", "slowA", 1.5, Status::Passed),
        test_result("Suite", "slowB", 3.0, Status::Passed),
    ]);
    history
        .insert_run(&r, &meta("2026-07-01T00:00:00Z"))
        .expect("insert run");

    let report = analyze(&history, &Config::default()).expect("analyze");
    assert_eq!(report.slow.len(), 2);
    assert_eq!(report.slow[0].id, "Suite::slowB");
    assert_eq!(report.slow[1].id, "Suite::slowA");
}

#[test]
fn single_run_marks_insufficient_history() {
    let mut history = History::open_in_memory().expect("open in-memory db");
    let r = run(vec![test_result("Suite", "testA", 0.1, Status::Passed)]);
    history
        .insert_run(&r, &meta("2026-07-01T00:00:00Z"))
        .expect("insert run");

    let report = analyze(&history, &Config::default()).expect("analyze");
    assert_eq!(
        report.insufficient_history.as_deref(),
        Some("insufficient history (1 run)")
    );
    assert!(report.flaky.is_empty());
    assert!(report.regression.is_empty());
}

#[test]
fn score_caps_flaky_penalty_at_configured_max() {
    let mut history = History::open_in_memory().expect("open in-memory db");
    let names: Vec<String> = (0..5).map(|i| format!("test{i}")).collect();
    let passed = run(names
        .iter()
        .map(|n| test_result("Suite", n, 0.1, Status::Passed))
        .collect());
    let failed = run(names
        .iter()
        .map(|n| test_result("Suite", n, 0.1, Status::Failed))
        .collect());

    history
        .insert_run(&passed, &meta("2026-07-01T00:00:00Z"))
        .expect("insert run 1");
    history
        .insert_run(&failed, &meta("2026-07-02T00:00:00Z"))
        .expect("insert run 2");

    let report = analyze(&history, &Config::default()).expect("analyze");
    // 5 flaky tests * weight 8 = 40, capped at flaky_max (32).
    assert_eq!(report.flaky.len(), 5);
    assert_eq!(report.score, 68);
    assert_eq!(report.status, HealthStatus::NeedsPruning);
}

#[test]
fn config_load_missing_path_returns_default() {
    let cfg = config::load(std::path::Path::new("/nonexistent/dir/kare.toml"))
        .expect("missing file is not an error");
    assert_eq!(cfg.thresholds.slow_sec, 1.0);
    assert_eq!(cfg.weights.flaky, 8);
}

#[test]
fn config_load_partial_toml_fills_remaining_defaults() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("kare.toml");
    std::fs::write(&path, "[thresholds]\nslow_sec = 2.5\n").expect("write partial toml");

    let cfg = config::load(&path).expect("valid partial toml");
    assert_eq!(cfg.thresholds.slow_sec, 2.5);
    assert_eq!(cfg.thresholds.flaky_window_runs, 10);
    assert_eq!(cfg.weights.flaky, 8);
}

#[test]
fn config_load_invalid_toml_is_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("kare.toml");
    std::fs::write(&path, "not valid toml [[[").expect("write garbage");

    let result = config::load(&path);
    assert!(result.is_err());
}
