use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use kare_core::analysis::{self, HealthStatus, Report};
use kare_core::ci;
use kare_core::config;
use kare_core::db::{now_utc_rfc3339, resolve_run_at, History, RunMeta};
use kare_core::model::{Status, TestRun};
use kare_core::parser::junit::JunitParser;
use kare_core::parser::ReportParser;

/// Test suite health check from CI artifacts — built for PHPUnit,
/// accepts any JUnit XML.
#[derive(Parser)]
#[command(name = "kare", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// JUnit XML report file(s) to ingest, then report on immediately. Used
    /// only when no subcommand is given.
    #[arg(long = "input")]
    input: Vec<PathBuf>,
    /// Path to the history DB.
    #[arg(long, default_value = ".kare/history.db")]
    db: PathBuf,
    /// Explicit run timestamp (RFC3339, or naive seconds form assumed
    /// UTC). Overrides the XML timestamp and the current time.
    #[arg(long)]
    at: Option<String>,
    /// Git ref for this run. Overrides CI auto-detection.
    #[arg(long = "git-ref")]
    git_ref: Option<String>,
    /// CI job URL for this run. Overrides CI auto-detection.
    #[arg(long = "ci-job-url")]
    ci_job_url: Option<String>,
    /// Path to the kare.toml config file. Missing file falls back to
    /// defaults.
    #[arg(long, default_value = "kare.toml")]
    config: PathBuf,
    /// Exit with code 2 if the health score is below this threshold.
    #[arg(long = "fail-under")]
    fail_under: Option<u32>,
}

#[derive(Subcommand)]
enum Command {
    /// Parse JUnit XML reports and store them in the history DB.
    Ingest {
        /// JUnit XML report file(s) to ingest. Repeat for sharded CI jobs.
        #[arg(long = "input", required = true)]
        input: Vec<PathBuf>,
        /// Path to the history DB.
        #[arg(long, default_value = ".kare/history.db")]
        db: PathBuf,
        /// Explicit run timestamp (RFC3339, or naive seconds form assumed
        /// UTC). Overrides the XML timestamp and the current time.
        #[arg(long)]
        at: Option<String>,
        /// Git ref for this run. Overrides CI auto-detection.
        #[arg(long = "git-ref")]
        git_ref: Option<String>,
        /// CI job URL for this run. Overrides CI auto-detection.
        #[arg(long = "ci-job-url")]
        ci_job_url: Option<String>,
    },
}

/// Process exit codes.
///
/// 0 = success, 1 = execution error (an `Err` returned from `main`, handled
/// by `anyhow`), 2 = quality gate failure (`--fail-under`).
const EXIT_QUALITY_GATE: i32 = 2;

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Ingest {
            input,
            db,
            at,
            git_ref,
            ci_job_url,
        }) => ingest_command(&input, &db, at.as_deref(), git_ref, ci_job_url),
        None => {
            if cli.input.is_empty() {
                Cli::command().print_help()?;
                println!();
                return Ok(());
            }
            default_command(&cli)
        }
    }
}

fn ingest_command(
    inputs: &[PathBuf],
    db_path: &Path,
    at: Option<&str>,
    git_ref: Option<String>,
    ci_job_url: Option<String>,
) -> Result<()> {
    let run = parse_inputs(inputs)?;
    let mut history = open_history(db_path)?;
    let outcome = ingest_into(&mut history, &run, at, git_ref, ci_job_url)?;

    println!(
        "Ingested {} tests ({} failures) as run #{} at {}",
        outcome.total_tests, outcome.total_failures, outcome.run_id, outcome.run_at
    );
    Ok(())
}

fn default_command(cli: &Cli) -> Result<()> {
    let run = parse_inputs(&cli.input)?;
    let mut history = open_history(&cli.db)?;
    ingest_into(
        &mut history,
        &run,
        cli.at.as_deref(),
        cli.git_ref.clone(),
        cli.ci_job_url.clone(),
    )?;

    let cfg = config::load(&cli.config)
        .with_context(|| format!("failed to load config at {}", cli.config.display()))?;
    let report = analysis::analyze(&history, &cfg).context("failed to analyze history")?;

    print_report(&report);

    if let Some(threshold) = cli.fail_under {
        if report.score < threshold {
            eprintln!(
                "kare: score {} is below threshold {}",
                report.score, threshold
            );
            std::process::exit(EXIT_QUALITY_GATE);
        }
    }

    Ok(())
}

/// Result of ingesting one run into the history DB.
struct IngestOutcome {
    run_id: i64,
    run_at: String,
    total_tests: usize,
    total_failures: usize,
}

fn parse_inputs(inputs: &[PathBuf]) -> Result<TestRun> {
    let mut run = TestRun::default();
    for path in inputs {
        let file =
            File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
        let mut reader = BufReader::new(file);
        let parsed = JunitParser
            .parse(&mut reader)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        run = run.merge(parsed);
    }
    Ok(run)
}

fn open_history(db_path: &Path) -> Result<History> {
    let opened = History::open(db_path)
        .with_context(|| format!("failed to open history DB at {}", db_path.display()))?;
    if opened.recovered {
        eprintln!(
            "warning: history DB was corrupt; moved aside to {}.corrupt and recreated",
            db_path.display()
        );
    }
    Ok(opened.history)
}

fn ingest_into(
    history: &mut History,
    run: &TestRun,
    at: Option<&str>,
    git_ref: Option<String>,
    ci_job_url: Option<String>,
) -> Result<IngestOutcome> {
    let now = time::OffsetDateTime::now_utc();
    let run_at =
        resolve_run_at(at, run.timestamp.as_deref(), now).context("failed to resolve run_at")?;

    let detected = ci::detect(&|name| std::env::var(name).ok());
    let git_ref = git_ref.or(detected.git_ref);
    let ci_job_url = ci_job_url.or(detected.job_url);

    let total_tests = run.results.len();
    let total_failures = run
        .results
        .iter()
        .filter(|r| matches!(r.status, Status::Failed | Status::Error))
        .count();

    let meta = RunMeta {
        run_at: run_at.clone(),
        ingested_at: now_utc_rfc3339(),
        git_ref,
        ci_job_url,
    };

    let run_id = history
        .insert_run(run, &meta)
        .context("failed to insert run into history DB")?;

    Ok(IngestOutcome {
        run_id,
        run_at,
        total_tests,
        total_failures,
    })
}

fn print_report(report: &Report) {
    println!("[Kare] Test Health Report");
    println!("{}", "-".repeat(50));
    let status_label = match report.status {
        HealthStatus::Healthy => "✅ Healthy",
        HealthStatus::NeedsPruning => "⚠️ Needs Pruning",
        HealthStatus::Overgrown => "🔴 Overgrown",
    };
    println!("Status: {status_label}");
    println!("Score:  {} / 100", report.score);
    println!();

    let has_findings = !report.slow.is_empty()
        || !report.flaky.is_empty()
        || !report.regression.is_empty()
        || report.cost.is_some();

    if !has_findings {
        println!("Findings: none 🎉");
    } else {
        println!("Findings:");
        for f in &report.slow {
            println!(
                "- 🐢 {:<12}'{}' took {:.2}s (threshold {:.1}s)",
                "Slow:", f.id, f.time_sec, f.threshold_sec
            );
        }
        for f in &report.flaky {
            println!(
                "- 📉 {:<12}'{}' failed {}/{} runs in history",
                "Flaky:", f.id, f.failed_runs, f.window_runs
            );
        }
        for f in &report.regression {
            println!(
                "- 🔺 {:<12}'{}' {:.2}s → {:.2}s (x{:.1})",
                "Regression:", f.id, f.prev_sec, f.current_sec, f.factor
            );
        }
        if let Some(cost) = &report.cost {
            println!(
                "- 💰 {:<12}total {:.1} min ≈ {:.2} per run",
                "Cost:", cost.total_min, cost.amount
            );
        }
    }

    if let Some(msg) = &report.insufficient_history {
        println!("({msg} — flaky/regression skipped)");
    }
}
