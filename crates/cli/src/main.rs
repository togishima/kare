use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use kare_core::ci;
use kare_core::db::{now_utc_rfc3339, resolve_run_at, History, RunMeta};
use kare_core::model::{Status, TestRun};
use kare_core::parser::junit::JunitParser;
use kare_core::parser::ReportParser;
use std::fmt::Write;

/// Test suite health check from CI artifacts — built for PHPUnit,
/// accepts any JUnit XML.
#[derive(Parser)]
#[command(name = "kare", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
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
    /// Display test history for a specific test.
    History {
        /// Test identifier in format 'Suite::testName'.
        #[arg(long)]
        test: String,
        /// Path to the history DB.
        #[arg(long, default_value = ".kare/history.db")]
        db: PathBuf,
        /// Maximum number of runs to display.
        #[arg(long, default_value = "20")]
        limit: usize,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Ingest {
            input,
            db,
            at,
            git_ref,
            ci_job_url,
        } => ingest(&input, &db, at.as_deref(), git_ref, ci_job_url),
        Command::History { test, db, limit } => show_history(&test, &db, limit),
    }
}

fn ingest(
    inputs: &[PathBuf],
    db_path: &Path,
    at: Option<&str>,
    git_ref: Option<String>,
    ci_job_url: Option<String>,
) -> Result<()> {
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

    let now = time::OffsetDateTime::now_utc();
    let run_at =
        resolve_run_at(at, run.timestamp.as_deref(), now).context("failed to resolve run_at")?;

    let detected = ci::detect(&|name| std::env::var(name).ok());
    let git_ref = git_ref.or(detected.git_ref);
    let ci_job_url = ci_job_url.or(detected.job_url);

    let opened = History::open(db_path)
        .with_context(|| format!("failed to open history DB at {}", db_path.display()))?;
    if opened.recovered {
        eprintln!(
            "warning: history DB was corrupt; moved aside to {}.corrupt and recreated",
            db_path.display()
        );
    }
    let mut history = opened.history;

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
        .insert_run(&run, &meta)
        .context("failed to insert run into history DB")?;

    println!(
        "Ingested {total_tests} tests ({total_failures} failures) as run #{run_id} at {run_at}"
    );

    Ok(())
}

fn show_history(test_id: &str, db_path: &Path, limit: usize) -> Result<()> {
    let (suite, name) = test_id
        .split_once("::")
        .ok_or_else(|| anyhow::anyhow!("expected format 'Suite::testName'"))?;

    let opened = History::open(db_path)
        .with_context(|| format!("failed to open history DB at {}", db_path.display()))?;
    if opened.recovered {
        eprintln!(
            "warning: history DB was corrupt; moved aside to {}.corrupt and recreated",
            db_path.display()
        );
    }
    let history = opened.history;

    let rows = history
        .test_history(suite, name, limit)
        .context("failed to query test history")?;

    if rows.is_empty() {
        println!("no history for '{test_id}'");
        return Ok(());
    }

    println!("History for '{test_id}' (last {} runs)", rows.len());

    for row in rows {
        let mut line = String::new();
        write!(
            &mut line,
            "{}  {:<8}  {:.3}s",
            row.run_at,
            row.status.as_str(),
            row.time_sec
        )?;

        if let Some(git_ref) = &row.git_ref {
            let short_ref = if git_ref.len() > 12 {
                &git_ref[..12]
            } else {
                git_ref
            };
            write!(&mut line, "  ({short_ref})")?;
        }

        println!("{}", line);
    }

    Ok(())
}
