use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub(crate) fn changed_line_rels(
    repo_root: &Path,
    changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
    repo_relative_path: impl Fn(&Path, &Path) -> Option<String>,
) -> BTreeMap<String, BTreeSet<u32>> {
    let mut out = BTreeMap::new();
    for (path, lines) in changed_lines {
        if lines.is_empty() {
            continue;
        }
        if let Some(rel) = repo_relative_path(repo_root, path) {
            out.insert(rel, lines.clone());
        }
    }
    out
}
