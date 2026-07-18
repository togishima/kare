//! JUnit XML parser, PHPUnit dialect first.
//!
//! PHPUnit nests `<testsuite>` arbitrarily deep (config path → named suite →
//! class FQCN → data provider group) and marks outcomes with child elements:
//! `<failure>`, `<error>`, `<skipped/>`. A `<testcase>` with no child passed.

use std::io::BufRead;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::model::{Status, TestResult, TestRun};
use crate::parser::{ParseError, ReportParser};

pub struct JunitParser;

impl ReportParser for JunitParser {
    fn parse(&self, input: &mut dyn BufRead) -> Result<TestRun, ParseError> {
        let mut reader = Reader::from_reader(input);
        let mut buf = Vec::new();
        let mut run = TestRun::default();
        // Names of currently open <testsuite> elements, outermost first.
        let mut suite_stack: Vec<String> = Vec::new();
        // The currently open <testcase>, if any.
        let mut current: Option<TestResult> = None;

        loop {
            match reader.read_event_into(&mut buf)? {
                Event::Start(e) => match e.name().as_ref() {
                    b"testsuite" => {
                        suite_stack.push(attr(&e, "name")?.unwrap_or_default());
                        if run.timestamp.is_none() {
                            run.timestamp = attr(&e, "timestamp")?;
                        }
                    }
                    b"testcase" => current = Some(new_result(&e, &suite_stack)?),
                    b"failure" => escalate(&mut current, Status::Failed),
                    b"error" => escalate(&mut current, Status::Error),
                    b"skipped" => escalate(&mut current, Status::Skipped),
                    _ => {}
                },
                Event::Empty(e) => match e.name().as_ref() {
                    b"testcase" => run.results.push(new_result(&e, &suite_stack)?),
                    b"failure" => escalate(&mut current, Status::Failed),
                    b"error" => escalate(&mut current, Status::Error),
                    b"skipped" => escalate(&mut current, Status::Skipped),
                    _ => {}
                },
                Event::End(e) => match e.name().as_ref() {
                    b"testsuite" => {
                        suite_stack.pop();
                    }
                    b"testcase" => {
                        if let Some(result) = current.take() {
                            run.results.push(result);
                        }
                    }
                    _ => {}
                },
                Event::Eof => break,
                _ => {}
            }
            buf.clear();
        }

        Ok(run)
    }
}

/// Builds a result from a `<testcase>` element, initially `Passed`.
fn new_result(e: &BytesStart, suite_stack: &[String]) -> Result<TestResult, ParseError> {
    let name = attr(e, "name")?
        .ok_or_else(|| ParseError::InvalidStructure("<testcase> without name".into()))?;
    // Prefer the `class` attribute (PHPUnit: FQCN, machine-independent).
    // Surrounding suite names are only a fallback for dialects without it.
    let suite = match attr(e, "class")? {
        Some(class) => class,
        None => suite_stack
            .last()
            .cloned()
            .ok_or_else(|| ParseError::InvalidStructure("<testcase> outside <testsuite>".into()))?,
    };
    let time_sec = attr(e, "time")?
        .and_then(|t| t.parse::<f64>().ok())
        .unwrap_or(0.0);
    Ok(TestResult {
        suite,
        name,
        time_sec,
        status: Status::Passed,
    })
}

/// Applies an outcome child element to the open testcase. Error outranks
/// Failed outranks Skipped so mixed markers keep the most severe one.
fn escalate(current: &mut Option<TestResult>, status: Status) {
    let rank = |s: Status| match s {
        Status::Passed => 0,
        Status::Skipped => 1,
        Status::Failed => 2,
        Status::Error => 3,
    };
    if let Some(result) = current {
        if rank(status) > rank(result.status) {
            result.status = status;
        }
    }
}

fn attr(e: &BytesStart, name: &str) -> Result<Option<String>, ParseError> {
    for a in e.attributes() {
        let a = a.map_err(|err| ParseError::InvalidStructure(err.to_string()))?;
        if a.key.as_ref() == name.as_bytes() {
            let value = a
                .unescape_value()
                .map_err(|err| ParseError::InvalidStructure(err.to_string()))?;
            return Ok(Some(value.into_owned()));
        }
    }
    Ok(None)
}
