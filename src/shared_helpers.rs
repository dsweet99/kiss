//! Tiny identical helpers shared across coverage/cache call sites.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Collect present environment variables for an allowlist of keys.
pub fn env_map_from_allowlist(keys: &[&str]) -> BTreeMap<String, String> {
    keys.iter()
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .map(|value| ((*key).to_string(), value))
        })
        .collect()
}

/// PYTHONPATH for Python coverage/cache identity.
///
/// When unset, empty, or pointing at paths that do not include this repo root,
/// defaults to the canonical repo root. Otherwise a shell that exported another
/// project's PYTHONPATH (common when driving `kiss test` from a different tree)
/// would invalidate warm populations and re-run thousands of selectors.
pub fn pythonpath_for_coverage_identity(repo_root: &Path) -> String {
    let root = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    let root_s = root.to_string_lossy().into_owned();
    match std::env::var("PYTHONPATH") {
        Ok(value) if !value.is_empty() && pythonpath_contains_repo_root(&value, &root) => value,
        _ => root_s,
    }
}

fn pythonpath_contains_repo_root(pythonpath: &str, root: &Path) -> bool {
    std::env::split_paths(pythonpath).any(|part| {
        if part == root {
            return true;
        }
        part.canonicalize().is_ok_and(|canon| canon == root)
    })
}

/// Coverage-identity env map for Python (`PYTHONPATH` only, normalized).
pub fn python_coverage_env_map(repo_root: &Path) -> BTreeMap<String, String> {
    BTreeMap::from([(
        "PYTHONPATH".to_string(),
        pythonpath_for_coverage_identity(repo_root),
    )])
}

/// Sorted `entries/*.json` paths under a coverage/cache root.
pub fn json_entry_paths(cache_root: &Path) -> Vec<PathBuf> {
    let entries_dir = cache_root.join("entries");
    let Ok(entries) = fs::read_dir(entries_dir) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        })
        .collect();
    paths.sort();
    paths
}

/// Build a `git` Command rooted at `repo` with parent-process `GIT_*`
/// overrides removed so wrappers (notably pre-commit) cannot redirect
/// kiss into the wrapper's index/worktree.
pub fn scrubbed_git_command(repo: &Path) -> Command {
    let mut c = Command::new("git");
    c.current_dir(repo)
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_COMMON_DIR");
    c
}
