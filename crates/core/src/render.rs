//! Renderers turning a [`Report`] into text or markdown. JSON is produced
//! directly via `serde_json::to_string_pretty` on `Report` by callers, since
//! it is the canonical output contract that text/markdown mirror.

use std::fmt::Write;

use crate::analysis::{HealthStatus, Report};

fn status_label(status: HealthStatus) -> &'static str {
    match status {
        HealthStatus::Healthy => "✅ Healthy",
        HealthStatus::NeedsPruning => "⚠️ Needs Pruning",
        HealthStatus::Overgrown => "🔴 Overgrown",
    }
}

fn has_findings(report: &Report) -> bool {
    !report.slow.is_empty()
        || !report.flaky.is_empty()
        || !report.regression.is_empty()
        || report.cost.is_some()
}

/// Renders `report` as human-readable text, identical to what `kare`
/// previously printed directly to stdout.
pub fn to_text(report: &Report) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "[Kare] Test Health Report");
    let _ = writeln!(out, "{}", "-".repeat(50));
    let _ = writeln!(out, "Status: {}", status_label(report.status));
    let _ = writeln!(out, "Score:  {} / 100", report.score);
    let _ = writeln!(out);

    if !has_findings(report) {
        let _ = writeln!(out, "Findings: none 🎉");
    } else {
        let _ = writeln!(out, "Findings:");
        for f in &report.slow {
            let _ = writeln!(
                out,
                "- 🐢 {:<12}'{}' took {:.2}s (threshold {:.1}s)",
                "Slow:", f.id, f.time_sec, f.threshold_sec
            );
        }
        for f in &report.flaky {
            let _ = writeln!(
                out,
                "- 📉 {:<12}'{}' failed {}/{} runs in history",
                "Flaky:", f.id, f.failed_runs, f.window_runs
            );
        }
        for f in &report.regression {
            let _ = writeln!(
                out,
                "- 🔺 {:<12}'{}' {:.2}s → {:.2}s (x{:.1})",
                "Regression:", f.id, f.prev_sec, f.current_sec, f.factor
            );
        }
        if let Some(cost) = &report.cost {
            let _ = writeln!(
                out,
                "- 💰 {:<12}total {:.1} min ≈ {:.2} per run",
                "Cost:", cost.total_min, cost.amount
            );
        }
    }

    if let Some(msg) = &report.insufficient_history {
        let _ = writeln!(out, "({msg} — flaky/regression skipped)");
    }

    out
}

/// Renders `report` as a markdown report.
pub fn to_markdown(report: &Report) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# 🌿 Kare Test Health Report");
    let _ = writeln!(out);
    let _ = writeln!(out, "**Status:** {}", status_label(report.status));
    let _ = writeln!(out, "**Score:** {} / 100", report.score);
    let _ = writeln!(out);
    let _ = writeln!(out, "## Findings");
    let _ = writeln!(out);

    if !has_findings(report) {
        let _ = writeln!(out, "No findings 🎉");
    } else {
        let _ = writeln!(out, "| Type | Test | Detail |");
        let _ = writeln!(out, "|------|------|--------|");
        for f in &report.slow {
            let _ = writeln!(
                out,
                "| 🐢 Slow | `{}` | {:.2}s (threshold {:.1}s) |",
                f.id, f.time_sec, f.threshold_sec
            );
        }
        for f in &report.flaky {
            let _ = writeln!(
                out,
                "| 📉 Flaky | `{}` | failed {}/{} runs |",
                f.id, f.failed_runs, f.window_runs
            );
        }
        for f in &report.regression {
            let _ = writeln!(
                out,
                "| 🔺 Regression | `{}` | {:.2}s → {:.2}s (x{:.1}) |",
                f.id, f.prev_sec, f.current_sec, f.factor
            );
        }
        if let Some(cost) = &report.cost {
            let _ = writeln!(
                out,
                "| 💰 Cost | — | total {:.1} min ≈ {:.2} per run |",
                cost.total_min, cost.amount
            );
        }
    }

    if let Some(msg) = &report.insufficient_history {
        let _ = writeln!(out);
        let _ = writeln!(out, "_{msg} — flaky/regression skipped_");
    }

    out
}
