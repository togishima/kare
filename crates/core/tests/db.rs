//! Tests for the SQLite history layer.

use kare_core::ci;
use kare_core::db::{resolve_run_at, History, RunMeta};
use kare_core::parser::junit::JunitParser;
use kare_core::parser::ReportParser;
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time};

const FIXTURE: &str = include_str!("fixtures/phpunit-11.xml");

/// Builds a UTC `OffsetDateTime` without pulling in the `time` crate's
/// `macros` feature just for tests.
fn utc(year: i32, month: Month, day: u8, hour: u8, minute: u8, second: u8) -> OffsetDateTime {
    let date = Date::from_calendar_date(year, month, day).expect("valid date");
    let time = Time::from_hms(hour, minute, second).expect("valid time");
    PrimitiveDateTime::new(date, time).assume_utc()
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
fn insert_run_twice_and_list_by_run_at_desc() {
    let run = JunitParser
        .parse(&mut FIXTURE.as_bytes())
        .expect("fixture must parse");

    let mut history = History::open_in_memory().expect("open in-memory db");

    // Insert the newer run first, the older one second — recent_runs must
    // still come back ordered by run_at, not by insertion (ingest) order.
    history
        .insert_run(&run, &meta("2026-07-10T00:00:00Z"))
        .expect("insert newer run");
    history
        .insert_run(&run, &meta("2026-07-01T00:00:00Z"))
        .expect("insert older run");

    assert_eq!(history.run_count().expect("run_count"), 2);

    let runs = history.recent_runs(10).expect("recent_runs");
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].run_at, "2026-07-10T00:00:00Z");
    assert_eq!(runs[1].run_at, "2026-07-01T00:00:00Z");
}

#[test]
fn open_recovers_from_corrupt_db_and_stays_usable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("history.db");
    std::fs::write(&db_path, b"\x00\x01not a sqlite database\xff\xfe").expect("write garbage");

    let opened = History::open(&db_path).expect("open must recover, not fail");
    assert!(opened.recovered);

    let corrupt_path = dir.path().join("history.db.corrupt");
    assert!(corrupt_path.exists());

    let run = JunitParser
        .parse(&mut FIXTURE.as_bytes())
        .expect("fixture must parse");
    let mut history = opened.history;
    let id = history
        .insert_run(&run, &meta("2026-07-01T00:00:00Z"))
        .expect("recovered db must accept inserts");
    assert_eq!(id, 1);
}

#[test]
fn resolve_run_at_prefers_explicit_and_normalizes_offset_to_utc() {
    let now = utc(2026, Month::July, 18, 0, 0, 0);
    let resolved = resolve_run_at(Some("2026-07-01T09:00:00+09:00"), None, now).expect("resolves");
    assert_eq!(resolved, "2026-07-01T00:00:00Z");
}

#[test]
fn resolve_run_at_treats_naive_timestamp_as_utc() {
    let now = utc(2026, Month::July, 18, 0, 0, 0);
    let resolved = resolve_run_at(None, Some("2026-07-01T09:00:00"), now).expect("resolves");
    assert_eq!(resolved, "2026-07-01T09:00:00Z");
}

#[test]
fn resolve_run_at_falls_back_to_now() {
    let now = utc(2026, Month::July, 18, 12, 34, 56);
    let resolved = resolve_run_at(None, None, now).expect("resolves");
    assert_eq!(resolved, "2026-07-18T12:34:56Z");
}

#[test]
fn resolve_run_at_rejects_unparseable_input() {
    let now = utc(2026, Month::July, 18, 0, 0, 0);
    let err = resolve_run_at(Some("not-a-timestamp"), None, now);
    assert!(err.is_err());
}

#[test]
fn ci_detect_gitlab_only() {
    let vars = [("CI_COMMIT_SHA", "abc123")];
    let info = ci::detect(&|name| {
        vars.iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.to_string())
    });
    assert_eq!(info.git_ref.as_deref(), Some("abc123"));
    assert_eq!(info.job_url, None);
}

#[test]
fn ci_detect_github_full() {
    let vars = [
        ("GITHUB_SHA", "def456"),
        ("GITHUB_SERVER_URL", "https://github.com"),
        ("GITHUB_REPOSITORY", "togishima/kare"),
        ("GITHUB_RUN_ID", "42"),
    ];
    let info = ci::detect(&|name| {
        vars.iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.to_string())
    });
    assert_eq!(info.git_ref.as_deref(), Some("def456"));
    assert_eq!(
        info.job_url.as_deref(),
        Some("https://github.com/togishima/kare/actions/runs/42")
    );
}

#[test]
fn ci_detect_prefers_gitlab_when_both_present() {
    let vars = [
        ("CI_COMMIT_SHA", "abc123"),
        ("CI_JOB_URL", "https://gitlab.example/jobs/1"),
        ("GITHUB_SHA", "def456"),
        ("GITHUB_SERVER_URL", "https://github.com"),
        ("GITHUB_REPOSITORY", "togishima/kare"),
        ("GITHUB_RUN_ID", "42"),
    ];
    let info = ci::detect(&|name| {
        vars.iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.to_string())
    });
    assert_eq!(info.git_ref.as_deref(), Some("abc123"));
    assert_eq!(
        info.job_url.as_deref(),
        Some("https://gitlab.example/jobs/1")
    );
}

#[test]
fn ci_detect_none_present() {
    let info = ci::detect(&|_name| None);
    assert_eq!(info.git_ref, None);
    assert_eq!(info.job_url, None);
}
