use std::collections::BTreeSet;
use std::path::Path;

use kiss::Language;

use super::TargetSelectionQuery;

pub(super) fn insert_direct(query: &mut TargetSelectionQuery, language: Language, selector: String) {
    match language {
        Language::Python => {
            query.direct_python.insert(selector);
        }
        Language::Rust => {
            query.direct_rust.insert(selector);
        }
    }
}

pub(super) fn insert_file(query: &mut TargetSelectionQuery, language: Language, abs: &Path) {
    match language {
        Language::Python => {
            query.python_files.insert(abs.to_path_buf());
        }
        Language::Rust => {
            query.rust_files.insert(abs.to_path_buf());
        }
    }
}

pub(super) fn insert_lines(
    query: &mut TargetSelectionQuery,
    language: Language,
    abs: &Path,
    lines: BTreeSet<u32>,
) {
    let map = match language {
        Language::Python => &mut query.python_lines,
        Language::Rust => &mut query.rust_lines,
    };
    map.entry(abs.to_path_buf()).or_default().extend(lines);
}

pub(super) fn repo_relative(repo_root: &Path, abs: &Path) -> Option<String> {
    let root = repo_root.canonicalize().ok()?;
    let abs = abs.canonicalize().ok()?;
    abs.strip_prefix(root)
        .ok()
        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
}

pub(super) fn language_label(language: Language) -> &'static str {
    super::super::language_label(language)
}
