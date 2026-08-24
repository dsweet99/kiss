use std::path::Path;

use rayon::prelude::*;

use crate::code_roles::{CodeRole, SourceRoleIndex};
use crate::parsing::ParsedFile;
use crate::rust_parsing::ParsedRustFile;
use crate::violation::Violation;

mod clap_docs;
mod python;
mod rust_scan;

pub const COMMENT_METRIC: &str = "comment";
pub const DOC_METRIC: &str = "doc";

pub fn collect_comment_violations(
    py_parsed: &[ParsedFile],
    rs_parsed: &[ParsedRustFile],
) -> Vec<Violation> {
    collect_comment_violations_with_roles(py_parsed, rs_parsed, None)
}

pub fn collect_comment_violations_with_roles(
    py_parsed: &[ParsedFile],
    rs_parsed: &[ParsedRustFile],
    roles: Option<&SourceRoleIndex>,
) -> Vec<Violation> {
    let mut out: Vec<Violation> = py_parsed
        .par_iter()
        .flat_map(|parsed| {
            let mut file_out = Vec::new();
            python::append_python_comment_violations(parsed, roles, &mut file_out);
            file_out
        })
        .collect();
    for parsed in rs_parsed {
        rust_scan::append_rust_comment_violations(parsed, roles, &mut out);
    }
    out
}

pub fn collect_doc_violations(
    py_parsed: &[ParsedFile],
    rs_parsed: &[ParsedRustFile],
    docs_allowed: &[String],
    repo_root: &Path,
) -> Vec<Violation> {
    collect_doc_violations_with_roles(py_parsed, rs_parsed, docs_allowed, repo_root, None)
}

pub fn collect_doc_violations_with_roles(
    py_parsed: &[ParsedFile],
    rs_parsed: &[ParsedRustFile],
    docs_allowed: &[String],
    repo_root: &Path,
    roles: Option<&SourceRoleIndex>,
) -> Vec<Violation> {
    let allowed = normalize_allowed_dirs(docs_allowed);
    let mut out = Vec::new();
    for parsed in py_parsed {
        if path_in_allowed_dirs(&parsed.path, repo_root, &allowed) {
            continue;
        }
        python::append_python_doc_violations(parsed, roles, &mut out);
    }
    for parsed in rs_parsed {
        if path_in_allowed_dirs(&parsed.path, repo_root, &allowed) {
            continue;
        }
        rust_scan::append_rust_doc_violations(parsed, roles, &mut out);
    }
    out
}

pub fn has_non_doc_comments(py_parsed: &[ParsedFile], rs_parsed: &[ParsedRustFile]) -> bool {
    has_non_doc_comments_with_roles(py_parsed, rs_parsed, None)
}

pub fn has_non_doc_comments_with_roles(
    py_parsed: &[ParsedFile],
    rs_parsed: &[ParsedRustFile],
    roles: Option<&SourceRoleIndex>,
) -> bool {
    !collect_comment_violations_with_roles(py_parsed, rs_parsed, roles).is_empty()
}

pub(super) fn skip_test_only_line(
    roles: Option<&SourceRoleIndex>,
    path: &Path,
    line: usize,
) -> bool {
    roles.is_some_and(|roles| roles.role_at(path, line) == CodeRole::TestOnly)
}

pub(crate) fn normalize_allowed_dirs(dirs: &[String]) -> Vec<String> {
    dirs.iter()
        .map(|p| p.trim().trim_end_matches('/').to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

pub(crate) fn path_in_allowed_dirs(path: &Path, repo_root: &Path, allowed: &[String]) -> bool {
    let rel = repo_relative(path, repo_root);
    allowed
        .iter()
        .any(|prefix| prefix == "." || rel == *prefix || rel.starts_with(&format!("{prefix}/")))
}

fn repo_relative(path: &Path, repo_root: &Path) -> String {
    if path.is_relative() {
        return path_to_unix(path).trim_start_matches("./").to_string();
    }
    if let Ok(rel) = path.strip_prefix(repo_root) {
        return path_to_unix(rel);
    }
    let Ok(root) = repo_root.canonicalize() else {
        return path_to_unix(path);
    };
    if let Ok(rel) = path.strip_prefix(&root) {
        return path_to_unix(rel);
    }
    path.canonicalize()
        .ok()
        .and_then(|canon| canon.strip_prefix(&root).ok().map(path_to_unix))
        .unwrap_or_else(|| path_to_unix(path))
}

fn path_to_unix(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub(crate) fn comment_violation(file: &Path, line: usize) -> Violation {
    metric_violation(
        file,
        line,
        COMMENT_METRIC,
        "Comment found (threshold: 0)",
        "Remove this comment. Keep documentation in a docstring or doc comment.",
    )
}

pub(crate) fn doc_violation(file: &Path, line: usize) -> Violation {
    metric_violation(
        file,
        line,
        DOC_METRIC,
        "Documentation found outside docs_allowed (threshold: 0)",
        "Move this file under a docs_allowed directory, or remove the docstring / doc comment.",
    )
}

fn metric_violation(
    file: &Path,
    line: usize,
    metric: &'static str,
    message: &'static str,
    suggestion: &'static str,
) -> Violation {
    let unit = file
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.display().to_string());
    Violation::builder(file)
        .line(line)
        .unit_name(unit)
        .metric(metric)
        .value(1)
        .threshold(0)
        .message(message)
        .suggestion(suggestion)
        .build()
}

#[cfg(test)]
#[path = "comments_test.rs"]
mod comments_test;
