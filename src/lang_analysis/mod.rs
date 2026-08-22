mod analysis;
mod graph;
mod parser;
mod roles;
mod units;

use std::path::Path;

pub use analysis::{LanguageAnalysis, PythonAnalysis, RustAnalysis};
pub use graph::{LanguageGraph, build_graphs};
pub use parser::LanguageParser;
pub use roles::{LanguageCodeRoles, classify_parsed_sources, parse_then_classify};
pub use units::LanguageUnits;

use crate::discovery::Language;
use crate::parsing::{ParseError, ParsedFile, create_parser, parse_file as parse_python_file};
use crate::py_metrics::{
    FileMetrics as PyFileMetrics, compute_file_metrics as compute_py_file_metrics,
};
use crate::rust_fn_metrics::{RustFileMetrics, compute_rust_file_metrics};
use crate::rust_parsing::{ParsedRustFile, RustParseError, parse_rust_file};

pub fn parse_python_path(path: &Path) -> Result<ParsedFile, ParseError> {
    let mut parser = create_parser()?;
    parse_python_file(&mut parser, path)
}

pub fn parse_rust_path(path: &Path) -> Result<ParsedRustFile, RustParseError> {
    parse_rust_file(path)
}

pub enum FileMetrics {
    Python(PyFileMetrics),
    Rust(RustFileMetrics),
}

pub fn compute_file_metrics_for_language(
    language: Language,
    py: Option<&ParsedFile>,
    rs: Option<&ParsedRustFile>,
) -> Option<FileMetrics> {
    match language {
        Language::Python => py.map(|p| FileMetrics::Python(compute_py_file_metrics(p))),
        Language::Rust => rs.map(|p| FileMetrics::Rust(compute_rust_file_metrics(p))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn analysis_language_identity() {
        assert_eq!(PythonAnalysis.language(), Language::Python);
        assert_eq!(RustAnalysis.language(), Language::Rust);
    }

    #[test]
    fn parse_and_metric_helpers_cover_both_languages() {
        let tmp = tempfile::tempdir().unwrap();
        let py = tmp.path().join("m.py");
        let rs = tmp.path().join("m.rs");
        std::fs::write(&py, "def f():\n    return 1\n").unwrap();
        std::fs::write(&rs, "pub fn f() -> i32 { 1 }\n").unwrap();

        let parsed_py = parse_python_path(&py).expect("python parse");
        let parsed_rs = parse_rust_path(&rs).expect("rust parse");
        assert!(matches!(
            compute_file_metrics_for_language(Language::Python, Some(&parsed_py), None),
            Some(FileMetrics::Python(_))
        ));
        assert!(matches!(
            compute_file_metrics_for_language(Language::Rust, None, Some(&parsed_rs)),
            Some(FileMetrics::Rust(_))
        ));
        assert!(compute_file_metrics_for_language(Language::Python, None, None).is_none());
        let _ = std::io::stdout().flush();
    }
}
