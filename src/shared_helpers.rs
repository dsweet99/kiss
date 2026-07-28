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
