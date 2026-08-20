use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::ValueEnum;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TestLangFilter {
    Python,
    Rust,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum TestChangeMode {
    Commit,
    Base,
    Main,
}

pub fn git_repo_root(repo: &Path) -> Result<PathBuf, String> {
    let s = git_output(repo, &["rev-parse", "--show-toplevel"])?;
    let p = PathBuf::from(s.trim());
    p.canonicalize()
        .map_err(|e| format!("failed to canonicalize repo root: {e}"))
}

pub fn require_git_repo_root(repo: &Path) -> Result<PathBuf, String> {
    let out = git_output(repo, &["rev-parse", "--is-inside-work-tree"])?;
    if out.trim() != "true" {
        return Err("not a git repository".into());
    }
    git_repo_root(repo)
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

pub fn changed_paths_commit(repo: &Path) -> Result<Vec<String>, String> {
    let mut names = BTreeSet::new();
    names.extend(changed_paths_from_diff(repo, &["diff"], Some("HEAD"))?);
    let u = git_output(repo, &["ls-files", "--others", "--exclude-standard"])?;
    names.extend(
        u.lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from),
    );
    Ok(names.into_iter().collect())
}

pub fn changed_paths_since(repo: &Path, rev: &str) -> Result<Vec<String>, String> {
    changed_paths_from_diff(repo, &["diff"], Some(rev))
}

fn changed_paths_from_diff(
    repo: &Path,
    diff_prefix: &[&str],
    rev: Option<&str>,
) -> Result<Vec<String>, String> {
    let mut names = BTreeSet::new();
    for filter in ["AM", "D"] {
        let mut args: Vec<&str> = diff_prefix.to_vec();
        args.extend(["--name-only", "--diff-filter", filter]);
        if let Some(rev) = rev {
            args.push(rev);
        }
        let out = git_output(repo, &args)?;
        names.extend(
            out.lines()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from),
        );
    }
    Ok(names.into_iter().collect())
}

pub fn changed_lines_commit(repo: &Path) -> Result<BTreeMap<String, BTreeSet<u32>>, String> {
    changed_lines_for_diff(repo, &["diff", "--unified=0", "--diff-filter=AM", "HEAD"])
}

pub fn changed_lines_since(
    repo: &Path,
    rev: &str,
) -> Result<BTreeMap<String, BTreeSet<u32>>, String> {
    changed_lines_for_diff(repo, &["diff", "--unified=0", "--diff-filter=AM", rev])
}

fn changed_lines_for_diff(
    repo: &Path,
    args: &[&str],
) -> Result<BTreeMap<String, BTreeSet<u32>>, String> {
    let diff = git_output(repo, args)?;
    Ok(parse_changed_lines_from_unified_diff(&diff))
}

pub(crate) fn parse_changed_lines_from_unified_diff(diff: &str) -> BTreeMap<String, BTreeSet<u32>> {
    let mut out = BTreeMap::new();
    let mut current_file: Option<String> = None;
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ ") {
            current_file = path
                .strip_prefix("b/")
                .filter(|path| *path != "/dev/null")
                .map(str::to_string);
            continue;
        }
        if !line.starts_with("@@") {
            continue;
        }
        let Some(file) = current_file.as_ref() else {
            continue;
        };
        let Some(range) = line.split_whitespace().nth(2) else {
            continue;
        };
        let Some((start, len)) = parse_unified_new_range(range) else {
            continue;
        };
        if len == 0 {
            continue;
        }
        let lines = out.entry(file.clone()).or_insert_with(BTreeSet::new);
        for line_no in start..start.saturating_add(len) {
            lines.insert(line_no);
        }
    }
    out
}

fn parse_unified_new_range(range: &str) -> Option<(u32, u32)> {
    let range = range.strip_prefix('+')?;
    let (start, len) = range
        .split_once(',')
        .map_or((range, "1"), |(start, len)| (start, len));
    Some((start.parse().ok()?, len.parse().ok()?))
}

fn rel_path_ignored(rel: &str, ignore: &[String]) -> bool {
    ignore.iter().any(|p| {
        let p = p.as_str();
        rel == p || rel.starts_with(&format!("{p}/"))
    })
}

fn lang_ok(path: &Path, lang_filter: Option<TestLangFilter>) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match lang_filter {
        None => ext.eq_ignore_ascii_case("py") || is_rust_planning_path(path),
        Some(TestLangFilter::Python) => ext.eq_ignore_ascii_case("py"),
        Some(TestLangFilter::Rust) => is_rust_planning_path(path),
    }
}

fn is_rust_planning_path(path: &Path) -> bool {
    kiss::Language::is_rust_path(path) || rust_llvm_cov_runner::is_rust_cov_cache_input(path)
}

pub fn resolve_changed_source_paths(
    repo_root: &Path,
    rel_names: &[String],
    ignore: &[String],
    lang_filter: Option<TestLangFilter>,
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for rel in rel_names {
        if rel_path_ignored(rel, ignore) {
            continue;
        }
        let abs = repo_root.join(rel);
        let include_missing_rust = matches!(lang_filter, None | Some(TestLangFilter::Rust))
            && is_rust_planning_path(&abs)
            && !kiss::is_rust_test_file(&abs);
        match abs.metadata() {
            Ok(meta) if meta.is_file() => {
                if !lang_ok(&abs, lang_filter) {
                    continue;
                }
                if let Ok(c) = abs.canonicalize() {
                    out.push(c);
                }
            }
            _ if include_missing_rust && lang_ok(&abs, lang_filter) => {
                out.push(abs);
            }
            _ => continue,
        }
    }
    out.sort();
    out.dedup();
    out
}

pub fn resolve_changed_line_paths(
    repo_root: &Path,
    rel_lines: &BTreeMap<String, BTreeSet<u32>>,
    ignore: &[String],
    lang_filter: Option<TestLangFilter>,
) -> BTreeMap<PathBuf, BTreeSet<u32>> {
    let mut out = BTreeMap::new();
    for (rel, lines) in rel_lines {
        if lines.is_empty() || rel_path_ignored(rel, ignore) {
            continue;
        }
        let abs = repo_root.join(rel);
        let Ok(meta) = abs.metadata() else {
            continue;
        };
        if !meta.is_file() || !lang_ok(&abs, lang_filter) {
            continue;
        }
        if let Ok(c) = abs.canonicalize() {
            out.insert(c, lines.clone());
        }
    }
    out
}

pub fn resolve_diff_target(
    repo: &Path,
    mode: TestChangeMode,
    main_branch_cfg: Option<&str>,
    main_branch_cli: Option<&str>,
    base_branch_cli: Option<&str>,
) -> Result<Option<String>, String> {
    match mode {
        TestChangeMode::Commit => Ok(None),
        TestChangeMode::Main => {
            let m = resolve_main_branch_name(repo, main_branch_cfg, main_branch_cli)?;
            Ok(Some(m))
        }
        TestChangeMode::Base => base_branch_cli.map_or_else(
            || auto_detect_fork_commit(repo).map(Some),
            |b| merge_base(repo, b).map(Some),
        ),
    }
}

#[cfg(test)]
#[path = "test_git/git_changes_test.rs"]
mod git_changes_test;

#[cfg(test)]
#[path = "test_git/git_changes_b_test.rs"]
mod git_changes_b_test;
