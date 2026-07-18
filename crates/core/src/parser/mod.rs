//! Report parsers. One implementation per producing tool dialect.

pub mod junit;

use std::io::BufRead;

use crate::model::TestRun;

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("XML error: {0}")]
    Xml(#[from] quick_xml::Error),
    #[error("invalid report structure: {0}")]
    InvalidStructure(String),
}

/// Turns one report file into a [`TestRun`].
pub trait ReportParser {
    fn parse(&self, input: &mut dyn BufRead) -> Result<TestRun, ParseError>;
}
