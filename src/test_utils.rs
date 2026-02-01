//! Test utilities for parsing Python source code in tests.

use crate::parsing::{ParsedFile, create_parser, parse_file};
use std::io::Write;

/// Parse Python source code from a string into a `ParsedFile`.
/// Creates a temporary file under the hood since the parser expects a file path.
pub fn parse_python_source(code: &str) -> ParsedFile {
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    write!(tmp, "{code}").unwrap();
    let mut parser = create_parser().unwrap();
    parse_file(&mut parser, tmp.path()).unwrap()
}
