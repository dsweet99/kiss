use std::collections::HashSet;
use std::path::{Path, PathBuf};

use kiss::{DuplicateCluster, Language, Violation, gather_files_by_lang};

pub fn gather_files(
    root: &Path,
    lang: Option<Language>,
    ignore_prefixes: &[String],
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let root = root.to_string_lossy().into_owned();
    gather_files_by_lang(&[root], lang, ignore_prefixes)
}

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

#[derive(Debug, Clone)]
pub struct FocusFilter {
    restrict: bool,
    paths: HashSet<PathBuf>,
}

impl FocusFilter {
    #[must_use]
    pub fn unrestricted() -> Self {
        Self {
            restrict: false,
            paths: HashSet::new(),
        }
    }

    #[must_use]
    pub fn restricting(paths: HashSet<PathBuf>) -> Self {
        Self {
            restrict: true,
            paths,
        }
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        self.restrict
    }

    pub fn cache_focus_paths(&self) -> Vec<String> {
        let mut paths: Vec<String> = self
            .paths
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect();
        paths.sort();
        paths
    }
}

pub fn build_focus_filter(
    focus_paths: &[String],
    universe: &str,
    lang: Option<Language>,
    ignore_prefixes: &[String],
) -> FocusFilter {
    if focus_paths.len() == 1 && focus_paths[0] == universe {
        FocusFilter::unrestricted()
    } else {
        let paths = build_focus_set(focus_paths, lang, ignore_prefixes);
        if paths.is_empty() {
            eprintln!(
                "Warning: focus path(s) matched no source files; reporting nothing for this focus."
            );
        }
        FocusFilter::restricting(paths)
    }
}

pub fn is_focus_file(file: &Path, filter: &FocusFilter) -> bool {
    !filter.restrict || filter.paths.contains(file)
}

pub fn filter_viols_by_focus(mut viols: Vec<Violation>, filter: &FocusFilter) -> Vec<Violation> {
    viols.retain(|v| is_focus_file(&v.file, filter));
    viols
}

pub fn filter_duplicates_by_focus(
    dups: Vec<DuplicateCluster>,
    filter: &FocusFilter,
) -> Vec<DuplicateCluster> {
    dups.into_iter()
        .filter(|cluster| {
            cluster
                .chunks
                .iter()
                .any(|c| is_focus_file(&c.file, filter))
        })
        .collect()
}
