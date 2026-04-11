use std::collections::HashSet;
use std::path::{Path, PathBuf};

use kiss::{DuplicateCluster, Language, Violation, find_source_files_with_ignore};

/// Files under `root` matching `lang` and not ignored.
pub fn gather_files(
    root: &Path,
    lang: Option<Language>,
    ignore_prefixes: &[String],
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let all = find_source_files_with_ignore(root, ignore_prefixes);
    let (mut py, mut rs) = (Vec::new(), Vec::new());
    for sf in all {
        let path = sf.path.canonicalize().unwrap_or(sf.path);
        match (sf.language, lang) {
            (Language::Python, None | Some(Language::Python)) => py.push(path),
            (Language::Rust, None | Some(Language::Rust)) => rs.push(path),
            _ => {}
        }
    }
    (py, rs)
}

/// Canonical paths for the given focus path list (files or directories).
pub fn build_focus_set(
    focus_paths: &[String],
    lang: Option<Language>,
    ignore_prefixes: &[String],
) -> HashSet<PathBuf> {
    let mut focus_set = HashSet::new();
    for focus_path in focus_paths {
        let path = Path::new(focus_path);
        if path.is_file() {
            if let Ok(canonical) = path.canonicalize() {
                focus_set.insert(canonical);
            }
        } else {
            let (py, rs) = gather_files(path, lang, ignore_prefixes);
            focus_set.extend(py);
            focus_set.extend(rs);
        }
    }
    focus_set
}

pub fn is_focus_file(file: &Path, focus_set: &HashSet<PathBuf>) -> bool {
    focus_set.is_empty() || focus_set.contains(file)
}

pub fn filter_viols_by_focus(
    mut viols: Vec<Violation>,
    focus_set: &HashSet<PathBuf>,
) -> Vec<Violation> {
    viols.retain(|v| is_focus_file(&v.file, focus_set));
    viols
}

pub fn filter_duplicates_by_focus(
    dups: Vec<DuplicateCluster>,
    focus_set: &HashSet<PathBuf>,
) -> Vec<DuplicateCluster> {
    dups
        .into_iter()
        .filter(|cluster| {
            cluster
                .chunks
                .iter()
                .any(|c| is_focus_file(&c.file, focus_set))
        })
        .collect()
}
