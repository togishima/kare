//! Core logic for kare: report parsers, analysis, and history storage.
//!
//! This crate is pure logic — no TUI, no network. Functions take file paths
//! or readers and return structs, so everything here is unit-testable.

pub mod analysis;
pub mod ci;
pub mod config;
pub mod db;
pub mod model;
pub mod parser;
