use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::types::GitFocus;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum GitStampKind {
    Commit,
    AutomaticBase,
    ExplicitBase,
    DefaultMain,
    ConfiguredMain,
    ExplicitMain,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TrackedTreeStamp {
    pub digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RefPresence {
    pub name: String,
    pub oid: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GitDepStamp {
    pub kind: GitStampKind,
    pub tracked: TrackedTreeStamp,
    pub head_oid: Option<String>,
    pub untracked: Option<String>,
    pub merge_base_sha: Option<String>,
    pub explicit_ref: Option<RefPresence>,
    pub candidates: Vec<RefPresence>,
}

pub(crate) fn capture_git_dep_stamp(repo: &Path, focus: &GitFocus) -> Result<GitDepStamp, String> {
    let tracked = capture_tracked(repo)?;
    match focus {
        GitFocus::Commit => stamp_commit(repo, tracked),
        GitFocus::AutomaticBase => stamp_automatic_base(repo, tracked),
        GitFocus::ExplicitBase { branch } => stamp_explicit_base(repo, tracked, branch),
        GitFocus::DefaultMain => stamp_main(repo, tracked, GitStampKind::DefaultMain, "main"),
        GitFocus::ConfiguredMain { name } => {
            stamp_main(repo, tracked, GitStampKind::ConfiguredMain, name)
        }
        GitFocus::ExplicitMain { branch } => stamp_explicit_main(repo, tracked, branch),
    }
}

fn stamp_commit(repo: &Path, tracked: TrackedTreeStamp) -> Result<GitDepStamp, String> {
    Ok(GitDepStamp {
        kind: GitStampKind::Commit,
        tracked,
        head_oid: Some(git_stdout(repo, &["rev-parse", "HEAD"])?),
        untracked: Some(untracked_digest(repo)?),
        merge_base_sha: None,
        explicit_ref: None,
        candidates: Vec::new(),
    })
}

fn stamp_automatic_base(repo: &Path, tracked: TrackedTreeStamp) -> Result<GitDepStamp, String> {
    let current = crate::test_git::current_branch_short(repo);
    let names = crate::test_git::list_other_refs(repo, &current)?;
    let candidates = ref_inventory(repo, &names);
    let merge_base_sha = crate::test_git::auto_detect_fork_commit(repo).ok();
    Ok(GitDepStamp {
        kind: GitStampKind::AutomaticBase,
        tracked,
        head_oid: None,
        untracked: None,
        merge_base_sha,
        explicit_ref: None,
        candidates,
    })
}

fn stamp_explicit_base(
    repo: &Path,
    tracked: TrackedTreeStamp,
    branch: &str,
) -> Result<GitDepStamp, String> {
    let explicit_ref = Some(ref_presence(repo, branch));
    let merge_base_sha = crate::test_git::merge_base(repo, branch).ok();
    Ok(GitDepStamp {
        kind: GitStampKind::ExplicitBase,
        tracked,
        head_oid: None,
        untracked: None,
        merge_base_sha,
        explicit_ref,
        candidates: Vec::new(),
    })
}

fn stamp_main(
    repo: &Path,
    tracked: TrackedTreeStamp,
    kind: GitStampKind,
    name: &str,
) -> Result<GitDepStamp, String> {
    let names = main_fallback_names(name);
    let candidates = ref_inventory(repo, &names);
    Ok(GitDepStamp {
        kind,
        tracked,
        head_oid: None,
        untracked: None,
        merge_base_sha: None,
        explicit_ref: None,
        candidates,
    })
}

fn stamp_explicit_main(
    repo: &Path,
    tracked: TrackedTreeStamp,
    branch: &str,
) -> Result<GitDepStamp, String> {
    Ok(GitDepStamp {
        kind: GitStampKind::ExplicitMain,
        tracked,
        head_oid: None,
        untracked: None,
        merge_base_sha: None,
        explicit_ref: Some(ref_presence(repo, branch)),
        candidates: Vec::new(),
    })
}

fn main_fallback_names(name: &str) -> Vec<String> {
    vec![
        format!("origin/{name}"),
        name.to_string(),
        "origin/master".to_string(),
        "master".to_string(),
    ]
}

pub(crate) fn capture_worktree_token(
    repo: &Path,
    lang: Option<kiss::Language>,
) -> String {
    // Semantic B (kt_bug.md #15): when `--lang` partitions the request, omit the
    // other language's tracked/untracked source paths and source fingerprints so
    // identity matches WatchPathFilter's lang deny (no oneshot false-miss / watch
    // idle diverge on other-lang edits). Non-source support paths stay bilingual.
    let tracked = capture_tracked_for_worktree(repo, lang)
        .map(|stamp| stamp.digest)
        .unwrap_or_default();
    let untracked = worktree_untracked_digest(repo, lang).unwrap_or_default();
    let include_python = !matches!(lang, Some(kiss::Language::Rust));
    let include_rust = !matches!(lang, Some(kiss::Language::Python));
    let python = if include_python {
        crate::test_runner::python_coverage_index::storage::python_source_input_fingerprint(repo)
            .unwrap_or_default()
    } else {
        String::new()
    };
    let rust = if include_rust {
        crate::test_runner::workspace_selector_cache::rust_full_source_fingerprint(repo, &[])
            .unwrap_or_default()
    } else {
        String::new()
    };
    // evidence_key expands Rust include! / #[path] targets (including gitignored files)
    // that the rust_full / git-untracked stamps omit; fold those bytes in so worktree
    // match remains a sound premise for skipping source re-digest on ready freshness.
    let rust_includes = if include_rust {
        rust_expanded_include_extras_fingerprint(repo)
    } else {
        String::new()
    };
    digest_bytes(&[
        tracked.as_bytes(),
        untracked.as_bytes(),
        python.as_bytes(),
        rust.as_bytes(),
        rust_includes.as_bytes(),
    ])
}

/// Whether a repo-relative path feeds the lang-partitioned worktree token.
/// Other-language *sources* are omitted; support / config / non-source stay in.
fn path_feeds_lang_worktree(path: &str, lang: Option<kiss::Language>) -> bool {
    let rel = Path::new(path);
    let is_py = rel
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("py"));
    let is_rs = kiss::Language::is_rust_path(rel)
        || rel
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"));
    match lang {
        None => true,
        Some(kiss::Language::Rust) => !is_py,
        Some(kiss::Language::Python) => !is_rs,
    }
}

fn filter_ls_files_stage_z(raw: &[u8], lang: Option<kiss::Language>) -> Vec<u8> {
    raw.split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .filter(|record| {
            let text = String::from_utf8_lossy(record);
            let path = text.split('\t').nth(1).unwrap_or("");
            path_feeds_lang_worktree(path, lang)
        })
        .flat_map(|record| record.iter().copied().chain(std::iter::once(0)))
        .collect()
}

fn filter_raw_diff_z(raw: &[u8], lang: Option<kiss::Language>) -> Vec<u8> {
    let mut out = Vec::new();
    let records: Vec<&[u8]> = raw
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .collect();
    let mut idx = 0;
    while idx < records.len() {
        let meta = records[idx];
        idx += 1;
        let meta_text = String::from_utf8_lossy(meta);
        let status = meta_text.split_whitespace().last().unwrap_or("");
        let rename_or_copy = status.starts_with('R') || status.starts_with('C');
        let path1 = records.get(idx).copied().unwrap_or_default();
        idx += 1;
        let path1_text = String::from_utf8_lossy(path1);
        if rename_or_copy {
            let path2 = records.get(idx).copied().unwrap_or_default();
            idx += 1;
            let path2_text = String::from_utf8_lossy(path2);
            if path_feeds_lang_worktree(&path1_text, lang)
                || path_feeds_lang_worktree(&path2_text, lang)
            {
                out.extend_from_slice(meta);
                out.push(0);
                out.extend_from_slice(path1);
                out.push(0);
                out.extend_from_slice(path2);
                out.push(0);
            }
        } else if path_feeds_lang_worktree(&path1_text, lang) {
            out.extend_from_slice(meta);
            out.push(0);
            out.extend_from_slice(path1);
            out.push(0);
        }
    }
    out
}

/// Content fingerprint of Rust paths reached only via `include!` / `#[path]` expansion
/// (not walk/git discovery).
fn rust_expanded_include_extras_fingerprint(repo: &Path) -> String {
    let root = repo.to_string_lossy().into_owned();
    let (_, discovered) = kiss::gather_files_by_lang_opts(
        std::slice::from_ref(&root),
        Some(kiss::Language::Rust),
        &[],
        false,
    );
    let expanded = kiss::expand_rust_files(discovered.clone());
    let baseline: std::collections::HashSet<_> = discovered.into_iter().collect();
    let mut extras = Vec::new();
    for path in expanded {
        if !baseline.contains(&path) {
            let rel = path
                .strip_prefix(repo)
                .map(|item| item.to_string_lossy().into_owned())
                .unwrap_or_else(|_| path.to_string_lossy().into_owned());
            let bytes = std::fs::read(&path).unwrap_or_default();
            extras.push((rel, digest_bytes(&[&bytes])));
        }
    }
    extras.sort();
    let mut parts: Vec<u8> = Vec::new();
    for (rel, digest) in extras {
        parts.extend_from_slice(rel.as_bytes());
        parts.push(0);
        parts.extend_from_slice(digest.as_bytes());
        parts.push(0);
    }
    digest_bytes(&[&parts])
}

fn worktree_untracked_digest(
    repo: &Path,
    lang: Option<kiss::Language>,
) -> Result<String, String> {
    let listed = git_stdout_raw(repo, &["ls-files", "-z", "--others", "--exclude-standard"])?;
    let filtered: Vec<u8> = listed
        .split(|byte| *byte == 0)
        .filter(|path| {
            let text = String::from_utf8_lossy(path);
            !text.starts_with("target/")
                && !text.starts_with(".kiss/")
                && path_feeds_lang_worktree(&text, lang)
        })
        .flat_map(|path| path.iter().copied().chain(std::iter::once(0)))
        .collect();
    Ok(digest_bytes(&[&filtered]))
}

fn capture_tracked_for_worktree(
    repo: &Path,
    lang: Option<kiss::Language>,
) -> Result<TrackedTreeStamp, String> {
    let index = filter_ls_files_stage_z(&git_stdout_raw(repo, &["ls-files", "-s", "-z"])?, lang);
    let dirty = filter_raw_diff_z(
        &git_stdout_raw(repo, &["diff-files", "-z", "--raw"])?,
        lang,
    );
    let staged = filter_raw_diff_z(
        &git_stdout_raw(repo, &["diff-index", "-z", "--cached", "--raw", "HEAD"])?,
        lang,
    );
    Ok(TrackedTreeStamp {
        digest: digest_bytes(&[&index, &dirty, &staged]),
    })
}

fn capture_tracked(repo: &Path) -> Result<TrackedTreeStamp, String> {
    // GitDepStamp stays bilingual (shared VCS identity for focus stamps).
    capture_tracked_for_worktree(repo, None)
}

fn untracked_digest(repo: &Path) -> Result<String, String> {
    // Same cache-path filter as worktree_untracked_digest: kiss runtime files under
    // `.kiss/` / `target/` must not flap the commit stamp while a run is in flight.
    worktree_untracked_digest(repo, None)
}

fn ref_inventory(repo: &Path, names: &[String]) -> Vec<RefPresence> {
    names.iter().map(|name| ref_presence(repo, name)).collect()
}

fn ref_presence(repo: &Path, name: &str) -> RefPresence {
    let oid = git_ok(repo, &["rev-parse", "--verify", "--quiet", name])
        .then(|| git_stdout(repo, &["rev-parse", name]).ok())
        .flatten();
    RefPresence {
        name: name.to_string(),
        oid,
    }
}

fn git_stdout(repo: &Path, args: &[&str]) -> Result<String, String> {
    let raw = git_stdout_raw(repo, args)?;
    Ok(String::from_utf8_lossy(&raw).trim().to_string())
}

fn git_stdout_raw(repo: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = git_cmd(repo, args)
        .output()
        .map_err(|err| format!("failed to run git: {err}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(out.stdout)
}

fn git_ok(repo: &Path, args: &[&str]) -> bool {
    git_cmd(repo, args)
        .output()
        .is_ok_and(|out| out.status.success())
}

fn git_cmd(repo: &Path, args: &[&str]) -> Command {
    let mut cmd = crate::test_git::git_command(repo);
    cmd.args(args);
    cmd
}

fn digest_bytes(parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod lang_partition_tests {
    use super::capture_worktree_token;
    use crate::test_runner::test_mode_fixtures::{git_in, init_git};
    use crate::test_runner::watch::WatchPathFilter;
    use crate::test_runner::target_request::workspace_request;
    use std::fs;
    use std::path::Path;

    fn bilingual_repo() -> tempfile::TempDir {
        let tmp = tempfile::TempDir::new().unwrap();
        init_git(&tmp);
        fs::write(tmp.path().join(".gitignore"), "/target\n/.kiss\n").unwrap();
        fs::create_dir_all(tmp.path().join("src")).unwrap();
        fs::write(tmp.path().join("app.py"), "x = 1\n").unwrap();
        fs::write(tmp.path().join("src/lib.rs"), "pub fn a() {}\n").unwrap();
        assert!(
            git_in(tmp.path())
                .args(["add", "-A"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            git_in(tmp.path())
                .args(["commit", "-m", "bilingual"])
                .status()
                .unwrap()
                .success()
        );
        tmp
    }

    #[test]

    fn lang_filtered_worktree_ignores_other_language() {
        // kt_bug.md class #15 lock (semantic B): `--lang rust` worktree must ignore
        // Python-only edits; `--lang python` must ignore Rust-only edits. Watch may
        // keep denying the other language once identity matches.
        let tmp = bilingual_repo();
        let root = tmp.path();

        let rust_before = capture_worktree_token(root, Some(kiss::Language::Rust));
        fs::write(root.join("app.py"), "x = 2\n").unwrap();
        let rust_after_py = capture_worktree_token(root, Some(kiss::Language::Rust));
        assert_eq!(
            rust_before, rust_after_py,
            "--lang rust worktree must not move on Python-only edit"
        );
        fs::write(root.join("src/lib.rs"), "pub fn a() { /* edited */ }\n").unwrap();
        let rust_after_rs = capture_worktree_token(root, Some(kiss::Language::Rust));
        assert_ne!(
            rust_after_py, rust_after_rs,
            "--lang rust worktree must move on Rust source edit"
        );

        // Reset Rust, probe Python partition.
        fs::write(root.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
        fs::write(root.join("app.py"), "x = 1\n").unwrap();
        let py_before = capture_worktree_token(root, Some(kiss::Language::Python));
        fs::write(root.join("src/lib.rs"), "pub fn a() { /* again */ }\n").unwrap();
        let py_after_rs = capture_worktree_token(root, Some(kiss::Language::Python));
        assert_eq!(
            py_before, py_after_rs,
            "--lang python worktree must not move on Rust-only edit"
        );
        fs::write(root.join("app.py"), "x = 3\n").unwrap();
        let py_after_py = capture_worktree_token(root, Some(kiss::Language::Python));
        assert_ne!(
            py_after_rs, py_after_py,
            "--lang python worktree must move on Python source edit"
        );

        // None stays bilingual: other-lang edits still move the token.
        fs::write(root.join("app.py"), "x = 1\n").unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
        let none_before = capture_worktree_token(root, None);
        fs::write(root.join("app.py"), "x = 9\n").unwrap();
        let none_after_py = capture_worktree_token(root, None);
        assert_ne!(
            none_before, none_after_py,
            "unfiltered worktree must still move on Python edit"
        );

        // Mirror production `reload::path_filter`: lang_filter is request.language().
        let rust_filter = WatchPathFilter::build(
            root,
            &[],
            Some(kiss::Language::Rust),
            &workspace_request(Some(kiss::Language::Rust), &[]),
        );
        assert!(
            !rust_filter.is_relevant(Path::new("app.py")),
            "watch --lang rust may keep denying app.py once identity matches"
        );
        assert!(
            rust_filter.is_relevant(Path::new("src/lib.rs")),
            "watch --lang rust must stay relevant for Rust sources"
        );
        let py_filter = WatchPathFilter::build(
            root,
            &[],
            Some(kiss::Language::Python),
            &workspace_request(Some(kiss::Language::Python), &[]),
        );
        assert!(
            !py_filter.is_relevant(Path::new("src/lib.rs")),
            "watch --lang python may keep denying src/lib.rs once identity matches"
        );
        assert!(
            py_filter.is_relevant(Path::new("app.py")),
            "watch --lang python must stay relevant for Python sources"
        );
    }
}
