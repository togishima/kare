//! Data model shared by parsers and analysis.

/// Outcome of a single test case execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Passed,
    Failed,
    Error,
    Skipped,
}

impl Status {
    /// Stable string form used for DB storage.
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Passed => "passed",
            Status::Failed => "failed",
            Status::Error => "error",
            Status::Skipped => "skipped",
        }
    }

    /// Inverse of [`Status::as_str`].
    pub fn parse(s: &str) -> Option<Status> {
        match s {
            "passed" => Some(Status::Passed),
            "failed" => Some(Status::Failed),
            "error" => Some(Status::Error),
            "skipped" => Some(Status::Skipped),
            _ => None,
        }
    }
}

/// A single test case result within one run.
#[derive(Debug, Clone, PartialEq)]
pub struct TestResult {
    /// Owning suite. For PHPUnit this is the test class FQCN taken from the
    /// `class` attribute — never the surrounding `<testsuite>` names, which
    /// contain machine-specific paths.
    pub suite: String,
    /// Test name as reported, including data set suffixes
    /// (e.g. `testAdd with data set "zero"`).
    pub name: String,
    pub time_sec: f64,
    pub status: Status,
}

impl TestResult {
    /// Stable identifier for tracking the same test across runs and machines.
    pub fn canonical_id(&self) -> String {
        format!("{}::{}", self.suite, self.name)
    }
}

/// Parsed contents of one or more report files belonging to a single CI run.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TestRun {
    pub results: Vec<TestResult>,
    /// Timestamp reported by the tool, if any. PHPUnit emits none, so runs
    /// are usually dated at ingest time or via an explicit override.
    pub timestamp: Option<String>,
}

impl TestRun {
    /// Sum of individual test case times. Suite-level `time` attributes are
    /// ignored on purpose: they are absent or inconsistent across dialects.
    pub fn total_time_sec(&self) -> f64 {
        self.results.iter().map(|r| r.time_sec).sum()
    }

    /// Combines reports from sharded CI jobs into one run.
    pub fn merge(mut self, other: TestRun) -> TestRun {
        self.results.extend(other.results);
        self.timestamp = self.timestamp.or(other.timestamp);
        self
    }
}
