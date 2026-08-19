
use std::path::PathBuf;

use super::analysis::{LanguageAnalysis, PythonAnalysis, RustAnalysis};
use crate::parsing::{ParseError, ParsedFile, parse_files};
use crate::rust_parsing::{ParsedRustFile, RustParseError, parse_rust_files};

pub type ParseResults<T, E> = Vec<Result<T, E>>;

pub trait LanguageParser: LanguageAnalysis {
    type Parsed;
    type Error;
    fn parse_many(&self, paths: &[PathBuf]) -> Result<ParseResults<Self::Parsed, Self::Error>, Self::Error>;
}

impl LanguageParser for PythonAnalysis {
    type Parsed = ParsedFile;
    type Error = ParseError;

    fn parse_many(
        &self,
        paths: &[PathBuf],
    ) -> Result<Vec<Result<Self::Parsed, Self::Error>>, Self::Error> {
        parse_files(paths)
    }
}

impl LanguageParser for RustAnalysis {
    type Parsed = ParsedRustFile;
    type Error = RustParseError;

    fn parse_many(
        &self,
        paths: &[PathBuf],
    ) -> Result<Vec<Result<Self::Parsed, Self::Error>>, Self::Error> {
        Ok(parse_rust_files(paths))
    }
}
