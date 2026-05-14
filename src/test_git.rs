use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TestLangFilter {
    Python,
    Rust,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TestChangeMode {
    Commit,
    Base,
    Main,
}

pub fn assert_git_repo(repo: &Path) -> Result<(), String> {
    let out = git_output(repo, &["rev-parse", "--is-inside-work-tree"])?;
    if out.trim() != "true" {
        return Err("not a git repository".into());
    }
    Ok(())
}

pub fn git_repo_root(repo: &Path) -> Result<PathBuf, String> {
    let s = git_output(repo, &["rev-parse", "--show-toplevel"])?;
    let p = PathBuf::from(s.trim());
    p.canonicalize()
        .map_err(|e| format!("failed to canonicalize repo root: {e}"))
}

// Build a `git` Command rooted at `repo` with all parent-process
// `GIT_*` overrides removed. Otherwise an outer wrapper (notably
// pre-commit, which exports `GIT_INDEX_FILE` to isolate staged
// content from hooks) silently redirects every git call to the
// wrapper's repo instead of `repo`, which would corrupt the user's
// real index when `kiss test` runs from inside such a wrapper.
pub(crate) fn git_command(repo: &Path) -> Command {
    let mut c = Command::new("git");
    c.current_dir(repo)
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_COMMON_DIR");
    c
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
        .status()
        .is_ok_and(|s| s.success())
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
    git_output(repo, &["rev-parse", "--abbrev-ref", "HEAD"]).map_or_else(
        |_| "HEAD".into(),
        |s| s.trim().to_string(),
    )
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
    let mut names = Vec::new();
    let d = git_output(
        repo,
        &["diff", "--name-only", "--diff-filter=AM", "HEAD"],
    )?;
    names.extend(d.lines().map(str::trim).filter(|s| !s.is_empty()).map(String::from));
    let u = git_output(repo, &["ls-files", "--others", "--exclude-standard"])?;
    names.extend(u.lines().map(str::trim).filter(|s| !s.is_empty()).map(String::from));
    names.sort();
    names.dedup();
    Ok(names)
}

pub fn changed_paths_since(repo: &Path, rev: &str) -> Result<Vec<String>, String> {
    let d = git_output(
        repo,
        &["diff", "--name-only", "--diff-filter=AM", rev],
    )?;
    Ok(d
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect())
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
        None => ext.eq_ignore_ascii_case("py") || ext.eq_ignore_ascii_case("rs"),
        Some(TestLangFilter::Python) => ext.eq_ignore_ascii_case("py"),
        Some(TestLangFilter::Rust) => ext.eq_ignore_ascii_case("rs"),
    }
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
        let Ok(meta) = abs.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        if !lang_ok(&abs, lang_filter) {
            continue;
        }
        if let Ok(c) = abs.canonicalize() {
            out.push(c);
        }
    }
    out.sort();
    out.dedup();
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
