//! SQLite-backed history storage.
//!
//! Schema is versioned via `PRAGMA user_version` so future migrations can
//! detect and upgrade older databases in place.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::model::{Status, TestRun};

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("history database is corrupt: {0}")]
    Corrupt(String),
    #[error("invalid timestamp {0:?}")]
    InvalidTimestamp(String),
}

const SCHEMA_V1: &str = "
CREATE TABLE runs (
  id INTEGER PRIMARY KEY,
  run_at TEXT NOT NULL,
  ingested_at TEXT NOT NULL,
  git_ref TEXT,
  ci_job_url TEXT,
  total_time_sec REAL NOT NULL,
  total_tests INTEGER NOT NULL,
  total_failures INTEGER NOT NULL
);
CREATE TABLE test_results (
  run_id INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  suite TEXT NOT NULL,
  name TEXT NOT NULL,
  time_sec REAL NOT NULL,
  status TEXT NOT NULL,
  PRIMARY KEY (run_id, suite, name)
);
CREATE INDEX idx_test_results_test ON test_results(suite, name);
";

/// Connection wrapper for the history database.
pub struct History {
    conn: Connection,
}

/// Result of [`History::open`], surfacing whether the DB had to be recreated.
pub struct Opened {
    pub history: History,
    pub recovered: bool,
}

/// Run-level metadata supplied by the caller at ingest time.
pub struct RunMeta {
    pub run_at: String,
    pub ingested_at: String,
    pub git_ref: Option<String>,
    pub ci_job_url: Option<String>,
}

/// Summary row for listing recent runs.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RunSummary {
    pub id: i64,
    pub run_at: String,
    pub git_ref: Option<String>,
    pub total_time_sec: f64,
    pub total_tests: i64,
    pub total_failures: i64,
}

/// A single stored test result row, as read back from `test_results`.
#[derive(Debug, Clone)]
pub struct StoredResult {
    pub suite: String,
    pub name: String,
    pub time_sec: f64,
    pub status: Status,
}

impl History {
    /// Opens (creating if needed) the history DB at `path`.
    ///
    /// A DB that fails to open, fails `PRAGMA quick_check`, or fails
    /// migration is treated as corrupt: it is moved aside to
    /// `<path>.corrupt` (overwriting any previous file there) and a fresh
    /// DB is created in its place, with `recovered` set to `true`.
    pub fn open(path: &Path) -> Result<Opened, DbError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        match Self::try_open(path) {
            Ok(history) => Ok(Opened {
                history,
                recovered: false,
            }),
            Err(_) => {
                if path.exists() {
                    std::fs::rename(path, corrupt_path(path))?;
                }
                let history = Self::try_open(path)?;
                Ok(Opened {
                    history,
                    recovered: true,
                })
            }
        }
    }

    /// Opens an in-memory DB. For tests only.
    pub fn open_in_memory() -> Result<History, DbError> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    fn try_open(path: &Path) -> Result<History, DbError> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<History, DbError> {
        conn.pragma_update(None, "foreign_keys", "ON")?;

        let quick_check: String = conn.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        if quick_check != "ok" {
            return Err(DbError::Corrupt(quick_check));
        }

        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        match version {
            0 => {
                conn.execute_batch(SCHEMA_V1)?;
                conn.pragma_update(None, "user_version", 1)?;
            }
            1 => {}
            other => {
                return Err(DbError::Corrupt(format!(
                    "unsupported schema version {other}"
                )))
            }
        }

        Ok(History { conn })
    }

    /// Inserts a run and its test results in one transaction. `test_results`
    /// rows are upserted (INSERT OR REPLACE) so re-ingesting a shard that
    /// overlaps a prior one in the same run never fails.
    pub fn insert_run(&mut self, run: &TestRun, meta: &RunMeta) -> Result<i64, DbError> {
        let total_tests = run.results.len() as i64;
        let total_failures = run
            .results
            .iter()
            .filter(|r| matches!(r.status, Status::Failed | Status::Error))
            .count() as i64;
        let total_time_sec = run.total_time_sec();

        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO runs (run_at, ingested_at, git_ref, ci_job_url, total_time_sec, total_tests, total_failures)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                meta.run_at,
                meta.ingested_at,
                meta.git_ref,
                meta.ci_job_url,
                total_time_sec,
                total_tests,
                total_failures,
            ],
        )?;
        let run_id = tx.last_insert_rowid();

        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO test_results (run_id, suite, name, time_sec, status)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for result in &run.results {
                stmt.execute(params![
                    run_id,
                    result.suite,
                    result.name,
                    result.time_sec,
                    result.status.as_str(),
                ])?;
            }
        }

        tx.commit()?;
        Ok(run_id)
    }

    pub fn run_count(&self) -> Result<i64, DbError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM runs", [], |row| row.get(0))?;
        Ok(count)
    }

    /// Runs ordered by `run_at` (then `id`) descending, not by ingest order,
    /// so out-of-order backfills still list newest-first.
    pub fn recent_runs(&self, limit: usize) -> Result<Vec<RunSummary>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, run_at, git_ref, total_time_sec, total_tests, total_failures
             FROM runs
             ORDER BY run_at DESC, id DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(RunSummary {
                id: row.get(0)?,
                run_at: row.get(1)?,
                git_ref: row.get(2)?,
                total_time_sec: row.get(3)?,
                total_tests: row.get(4)?,
                total_failures: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    /// All test results stored for a given run.
    pub fn results_of_run(&self, run_id: i64) -> Result<Vec<StoredResult>, DbError> {
        let mut stmt = self
            .conn
            .prepare("SELECT suite, name, time_sec, status FROM test_results WHERE run_id = ?1")?;
        let rows = stmt.query_map(params![run_id], |row| {
            let status: String = row.get(3)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, f64>(2)?,
                status,
            ))
        })?;

        let mut results = Vec::new();
        for row in rows {
            let (suite, name, time_sec, status_str) = row?;
            let status = Status::parse(&status_str)
                .ok_or_else(|| DbError::Corrupt(format!("invalid status {status_str:?}")))?;
            results.push(StoredResult {
                suite,
                name,
                time_sec,
                status,
            });
        }
        Ok(results)
    }
}

fn corrupt_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".corrupt");
    PathBuf::from(name)
}

/// Resolves the `run_at` to store: explicit override > XML timestamp > now.
/// Accepts RFC3339 or a seconds-precision naive form (assumed UTC), and
/// always returns a normalized UTC RFC3339 (`Z`-suffixed) string.
pub fn resolve_run_at(
    explicit: Option<&str>,
    xml_timestamp: Option<&str>,
    now: OffsetDateTime,
) -> Result<String, DbError> {
    if let Some(s) = explicit {
        return normalize_timestamp(s);
    }
    if let Some(s) = xml_timestamp {
        return normalize_timestamp(s);
    }
    format_utc(now)
}

pub fn now_utc_rfc3339() -> String {
    format_utc(OffsetDateTime::now_utc()).expect("formatting the current time cannot fail")
}

fn normalize_timestamp(s: &str) -> Result<String, DbError> {
    if let Ok(dt) = OffsetDateTime::parse(s, &Rfc3339) {
        return format_utc(dt);
    }

    let naive_format =
        time::format_description::parse("[year]-[month]-[day]T[hour]:[minute]:[second]")
            .expect("naive timestamp format description is valid");
    let primitive = time::PrimitiveDateTime::parse(s, &naive_format)
        .map_err(|_| DbError::InvalidTimestamp(s.to_string()))?;
    format_utc(primitive.assume_utc())
}

fn format_utc(dt: OffsetDateTime) -> Result<String, DbError> {
    dt.to_offset(time::UtcOffset::UTC)
        .format(&Rfc3339)
        .map_err(|e| DbError::InvalidTimestamp(e.to_string()))
}
