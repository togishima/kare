//! Tests for the text/markdown `Report` renderers.

use kare_core::analysis::{
    CostInfo, FlakyFinding, HealthStatus, RegressionFinding, Report, SlowFinding, SCHEMA_VERSION,
};
use kare_core::db::RunSummary;
use kare_core::render::{to_markdown, to_text};

fn run_summary() -> RunSummary {
    RunSummary {
        id: 1,
        run_at: "2026-07-01T00:00:00Z".to_string(),
        git_ref: None,
        total_time_sec: 12.3 * 60.0,
        total_tests: 42,
        total_failures: 0,
    }
}

fn report_with_findings() -> Report {
    Report {
        schema_version: SCHEMA_VERSION,
        score: 92,
        status: HealthStatus::Healthy,
        run: run_summary(),
        slow: vec![SlowFinding {
            id: "Suite::test".to_string(),
            time_sec: 2.31,
            threshold_sec: 1.0,
        }],
        flaky: vec![FlakyFinding {
            id: "Suite::test".to_string(),
            failed_runs: 3,
            window_runs: 10,
        }],
        regression: vec![RegressionFinding {
            id: "Suite::test".to_string(),
            prev_sec: 0.50,
            current_sec: 1.20,
            factor: 2.4,
        }],
        cost: Some(CostInfo {
            total_min: 12.3,
            amount: 0.62,
        }),
        insufficient_history: Some("insufficient history (2 runs)".to_string()),
    }
}

fn report_without_findings() -> Report {
    Report {
        schema_version: SCHEMA_VERSION,
        score: 100,
        status: HealthStatus::Healthy,
        run: run_summary(),
        slow: Vec::new(),
        flaky: Vec::new(),
        regression: Vec::new(),
        cost: None,
        insufficient_history: None,
    }
}

#[test]
fn to_text_renders_findings_and_insufficient_history() {
    let report = report_with_findings();
    let out = to_text(&report);

    let expected = format!(
        "[Kare] Test Health Report\n\
         {}\n\
         Status: ✅ Healthy\n\
         Score:  92 / 100\n\
         \n\
         Findings:\n\
         - 🐢 {:<12}'Suite::test' took 2.31s (threshold 1.0s)\n\
         - 📉 {:<12}'Suite::test' failed 3/10 runs in history\n\
         - 🔺 {:<12}'Suite::test' 0.50s → 1.20s (x2.4)\n\
         - 💰 {:<12}total 12.3 min ≈ 0.62 per run\n\
         (insufficient history (2 runs) — flaky/regression skipped)\n",
        "-".repeat(50),
        "Slow:",
        "Flaky:",
        "Regression:",
        "Cost:",
    );

    assert_eq!(out, expected);
}

#[test]
fn to_text_renders_no_findings() {
    let report = report_without_findings();
    let out = to_text(&report);

    let expected = format!(
        "[Kare] Test Health Report\n\
         {}\n\
         Status: ✅ Healthy\n\
         Score:  100 / 100\n\
         \n\
         Findings: none 🎉\n",
        "-".repeat(50),
    );

    assert_eq!(out, expected);
}

#[test]
fn to_markdown_renders_findings_and_insufficient_history() {
    let report = report_with_findings();
    let out = to_markdown(&report);

    let expected = "# 🌿 Kare Test Health Report\n\
         \n\
         **Status:** ✅ Healthy\n\
         **Score:** 92 / 100\n\
         \n\
         ## Findings\n\
         \n\
         | Type | Test | Detail |\n\
         |------|------|--------|\n\
         | 🐢 Slow | `Suite::test` | 2.31s (threshold 1.0s) |\n\
         | 📉 Flaky | `Suite::test` | failed 3/10 runs |\n\
         | 🔺 Regression | `Suite::test` | 0.50s → 1.20s (x2.4) |\n\
         | 💰 Cost | — | total 12.3 min ≈ 0.62 per run |\n\
         \n\
         _insufficient history (2 runs) — flaky/regression skipped_\n";

    assert_eq!(out, expected);
}

#[test]
fn to_markdown_renders_no_findings() {
    let report = report_without_findings();
    let out = to_markdown(&report);

    let expected = "# 🌿 Kare Test Health Report\n\
         \n\
         **Status:** ✅ Healthy\n\
         **Score:** 100 / 100\n\
         \n\
         ## Findings\n\
         \n\
         No findings 🎉\n";

    assert_eq!(out, expected);
}
