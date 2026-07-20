use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub(crate) fn changed_line_rels(
    repo_root: &Path,
    changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
    repo_relative_path: impl Fn(&Path, &Path) -> Option<String>,
) -> BTreeMap<String, BTreeSet<u32>> {
    let mut out = BTreeMap::new();
    for (path, lines) in changed_lines {
        if !lines.is_empty()
            && let Some(rel) = repo_relative_path(repo_root, path)
        {
            out.insert(rel, lines.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_line_rels_skips_empty_and_unmapped_paths() {
        let repo = Path::new("/repo");
        let mut changed = BTreeMap::new();
        changed.insert(PathBuf::from("/repo/src/lib.rs"), BTreeSet::from([3, 5]));
        changed.insert(PathBuf::from("/repo/src/empty.rs"), BTreeSet::new());
        changed.insert(PathBuf::from("/outside.rs"), BTreeSet::from([1]));

        let rels = changed_line_rels(repo, &changed, |root, path| {
            path.strip_prefix(root)
                .ok()
                .map(|rel| rel.to_string_lossy().to_string())
        });

        assert_eq!(rels.len(), 1);
        assert_eq!(rels["src/lib.rs"], BTreeSet::from([3, 5]));
    }
}
