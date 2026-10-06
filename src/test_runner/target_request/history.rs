use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::resolved::ReverseRecord;

pub(crate) fn historical_covering_selectors(
    repo_root: &Path,
    historical_paths: &[String],
) -> Vec<String> {
    let abs: Vec<PathBuf> = historical_paths
        .iter()
        .map(|path| repo_root.join(path))
        .collect();
    let keys: Vec<String> = historical_paths
        .iter()
        .flat_map(|path| index_lookup_keys(repo_root, path))
        .collect();
    let mut selectors = BTreeSet::new();
    for language in kiss::Language::ALL {
        selectors.extend(
            crate::test_runner::lang_registry::rules_for(language)
                .historical_covering_selectors(repo_root, &keys, &abs),
        );
    }
    selectors.into_iter().collect()
}

pub(crate) fn reverse_records(repo_root: &Path, paths: &[String]) -> Vec<ReverseRecord> {
    let mut records: Vec<ReverseRecord> = paths
        .iter()
        .filter(|path| index_has_path(repo_root, path))
        .map(|path| ReverseRecord {
            path: path.clone(),
            selectors: historical_covering_selectors(repo_root, std::slice::from_ref(path)),
        })
        .collect();
    records.sort_by(|left, right| left.path.cmp(&right.path));
    records
}

fn index_lookup_keys(repo_root: &Path, path: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let candidate = Path::new(path);
    if let Ok(rel) = candidate.strip_prefix(repo_root) {
        keys.push(rel.to_string_lossy().replace('\\', "/"));
    }
    let spelled = path.trim_start_matches("./");
    if !keys.iter().any(|key| key == spelled) {
        keys.push(spelled.to_string());
    }
    keys
}

fn index_has_path(repo_root: &Path, path: &str) -> bool {
    let keys = index_lookup_keys(repo_root, path);
    kiss::Language::ALL.into_iter().any(|language| {
        crate::test_runner::lang_registry::rules_for(language).indexes_path(repo_root, &keys)
    })
}
