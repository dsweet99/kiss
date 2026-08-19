//! Flag non-doc comments when `comment_removal_enabled` is true.

use std::path::Path;

use crate::parsing::ParsedFile;
use crate::rust_parsing::ParsedRustFile;
use crate::violation::Violation;

mod python;
mod rust_scan;

pub const COMMENT_METRIC: &str = "comment";

pub fn collect_comment_violations(
    py_parsed: &[ParsedFile],
    rs_parsed: &[ParsedRustFile],
) -> Vec<Violation> {
    let mut out = Vec::new();
    for parsed in py_parsed {
        python::append_python_comment_violations(parsed, &mut out);
    }
    for parsed in rs_parsed {
        rust_scan::append_rust_comment_violations(parsed, &mut out);
    }
    out
}

pub fn has_non_doc_comments(py_parsed: &[ParsedFile], rs_parsed: &[ParsedRustFile]) -> bool {
    !collect_comment_violations(py_parsed, rs_parsed).is_empty()
}

pub(crate) fn comment_violation(file: &Path, line: usize) -> Violation {
    let unit = file
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.display().to_string());
    Violation::builder(file)
        .line(line)
        .unit_name(unit)
        .metric(COMMENT_METRIC)
        .value(1)
        .threshold(0)
        .message("Comment found (threshold: 0)")
        .suggestion("Remove this comment. Keep documentation in a docstring or doc comment.")
        .build()
}

#[cfg(test)]
#[path = "comments_test.rs"]
mod comments_test;
