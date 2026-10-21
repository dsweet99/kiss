use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::SystemTime;

use clap::ValueEnum;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TestLangFilter {
    Python,
    Rust,
}

impl From<kiss::Language> for TestLangFilter {
    fn from(language: kiss::Language) -> Self {
        match language {
            kiss::Language::Python => Self::Python,
            kiss::Language::Rust => Self::Rust,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum TestChangeMode {
    Commit,
    Base,
    Main,
}

pub fn git_repo_root(repo: &Path) -> Result<PathBuf, String> {
    cached_repo_root(repo)
}

pub fn require_git_repo_root(repo: &Path) -> Result<PathBuf, String> {
    cached_repo_root(repo)
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct GitDirStamp {
    modified: SystemTime,
    len: u64,
}

struct RepoRootMemo {
    stamp: Option<GitDirStamp>,
    root: Result<PathBuf, String>,
}

fn repo_root_cache() -> &'static Mutex<BTreeMap<PathBuf, RepoRootMemo>> {
    static CACHE: Mutex<BTreeMap<PathBuf, RepoRootMemo>> = Mutex::new(BTreeMap::new());
    &CACHE
}

fn git_dir_stamp(dir: &Path) -> Option<GitDirStamp> {
    let meta = std::fs::metadata(dir.join(".git")).ok()?;
    Some(GitDirStamp {
        modified: meta.modified().ok()?,
        len: meta.len(),
    })
}

fn cache_get(key: &Path, stamp: Option<GitDirStamp>) -> Option<Result<PathBuf, String>> {
    let cache = repo_root_cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let hit = cache.get(key)?;
    (hit.stamp == stamp).then(|| hit.root.clone())
}

fn cache_put(key: PathBuf, stamp: Option<GitDirStamp>, root: Result<PathBuf, String>) {
    let mut cache = repo_root_cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    cache.insert(key, RepoRootMemo { stamp, root });
}

fn cached_repo_root(repo: &Path) -> Result<PathBuf, String> {
    let Some(key) = std::fs::canonicalize(repo).ok() else {
        return probe_repo_root(repo);
    };
    let stamp = git_dir_stamp(&key);
    if let Some(hit) = cache_get(&key, stamp) {
        return hit;
    }
    let probed = probe_repo_root(repo);
    cache_put(key, stamp, probed.clone());
    probed
}

fn probe_repo_root(repo: &Path) -> Result<PathBuf, String> {
    let inside = git_output(repo, &["rev-parse", "--is-inside-work-tree"])?;
    if inside.trim() != "true" {
        return Err("not a git repository".into());
    }
    let toplevel = git_output(repo, &["rev-parse", "--show-toplevel"])?;
    PathBuf::from(toplevel.trim())
        .canonicalize()
        .map_err(|err| format!("failed to canonicalize repo root: {err}"))
}

pub(crate) fn git_command(repo: &Path) -> Command {
    kiss::scrubbed_git_command(repo)
}

fn git_output(repo: &Path, args: &[&str]) -> Result<String, String> {
    let out = git_command(repo)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn git_ok(repo: &Path, args: &[&str]) -> bool {
    git_command(repo)
        .args(args)
        .output()
        .is_ok_and(|out| out.status.success())
}

pub fn resolve_main_branch_name(
    repo: &Path,
    main_branch_cfg: Option<&str>,
    main_branch_cli: Option<&str>,
) -> Result<String, String> {
    let name = main_branch_cli
        .map(str::to_string)
        .or_else(|| main_branch_cfg.map(str::to_string))
        .unwrap_or_else(|| "main".to_string());
    let candidates = [
        format!("origin/{name}"),
        name.clone(),
        "origin/master".to_string(),
        "master".to_string(),
    ];
    for c in &candidates {
        if git_ok(repo, &["rev-parse", "--verify", "--quiet", c]) {
            return Ok(c.clone());
        }
    }
    Err(format!(
        "error: cannot resolve main branch (tried origin/{name}, {name}, origin/master, master). Use --main-branch BRANCH or set [test] main_branch in .kissconfig."
    ))
}

pub fn merge_base(repo: &Path, other: &str) -> Result<String, String> {
    git_output(repo, &["merge-base", "HEAD", other])
        .map(|s| s.trim().to_string())
        .map_err(|_| format!("merge-base failed for {other}"))
}

pub fn commit_timestamp(repo: &Path, sha: &str) -> Result<i64, String> {
    let s = git_output(repo, &["show", "-s", "--format=%ct", sha])?;
    s.trim()
        .parse::<i64>()
        .map_err(|_| "invalid commit timestamp".to_string())
}

pub fn current_branch_short(repo: &Path) -> String {
    git_output(repo, &["rev-parse", "--abbrev-ref", "HEAD"])
        .map_or_else(|_| "HEAD".into(), |s| s.trim().to_string())
}

pub fn list_other_refs(repo: &Path, current: &str) -> Result<Vec<String>, String> {
    let out = git_output(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname:short)",
            "refs/heads",
            "refs/remotes",
        ],
    )?;
    let origin_current = format!("origin/{current}");
    let refs: Vec<String> = out
        .lines()
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .filter(|r| *r != current && *r != origin_current.as_str())
        .map(String::from)
        .collect();
    Ok(refs)
}

pub fn auto_detect_fork_commit(repo: &Path) -> Result<String, String> {
    let current = current_branch_short(repo);
    let refs = list_other_refs(repo, &current)?;
    if refs.is_empty() {
        return Err(
            "error: cannot auto-detect fork point (no other branches exist). Use --base-branch BRANCH."
                .into(),
        );
    }
    let mut best: Option<(i64, String)> = None;
    for r in refs {
        if let Ok(sha) = merge_base(repo, &r)
            && let Ok(ts) = commit_timestamp(repo, &sha)
        {
            best = match best {
                None => Some((ts, sha)),
                Some((bt, _)) if ts > bt => Some((ts, sha)),
                Some(prev) => Some(prev),
            };
        }
    }
    best.map(|(_, sha)| sha)
        .ok_or_else(|| "error: cannot auto-detect fork point (merge-base failed for all refs). Use --base-branch BRANCH.".into())
}

mod changes;
mod diff_paths;

pub use changes::{
    changed_lines_commit, changed_lines_since, changed_paths_commit, changed_paths_since,
    resolve_changed_source_paths, resolve_diff_target,
};
pub(crate) use changes::is_rust_planning_path;
#[cfg(test)]
pub(crate) use changes::parse_changed_lines_from_unified_diff;

#[cfg(test)]
#[path = "test_git/git_changes_test.rs"]
mod git_changes_test;

#[cfg(test)]
#[path = "test_git/git_changes_b_test.rs"]
mod git_changes_b_test;

#[cfg(test)]
#[path = "test_git/git_changes_c_test.rs"]
mod git_changes_c_test;
