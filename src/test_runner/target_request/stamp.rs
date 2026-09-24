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

pub(crate) fn capture_worktree_token(repo: &Path) -> String {
    let tracked = capture_tracked(repo)
        .map(|stamp| stamp.digest)
        .unwrap_or_default();
    let untracked = worktree_untracked_digest(repo).unwrap_or_default();
    let python =
        crate::test_runner::python_coverage_index::storage::python_source_input_fingerprint(repo)
            .unwrap_or_default();
    let rust =
        crate::test_runner::workspace_selector_cache::rust_full_source_fingerprint(repo, &[])
            .unwrap_or_default();
    digest_bytes(&[
        tracked.as_bytes(),
        untracked.as_bytes(),
        python.as_bytes(),
        rust.as_bytes(),
    ])
}

fn worktree_untracked_digest(repo: &Path) -> Result<String, String> {
    let listed = git_stdout_raw(repo, &["ls-files", "-z", "--others", "--exclude-standard"])?;
    let filtered: Vec<u8> = listed
        .split(|byte| *byte == 0)
        .filter(|path| {
            let text = String::from_utf8_lossy(path);
            !text.starts_with("target/") && !text.starts_with(".kiss/")
        })
        .flat_map(|path| path.iter().copied().chain(std::iter::once(0)))
        .collect();
    Ok(digest_bytes(&[&filtered]))
}

fn capture_tracked(repo: &Path) -> Result<TrackedTreeStamp, String> {
    let index = git_stdout_raw(repo, &["ls-files", "-s", "-z"])?;
    let dirty = git_stdout_raw(repo, &["diff-files", "-z", "--raw"])?;
    let staged = git_stdout_raw(repo, &["diff-index", "-z", "--cached", "--raw", "HEAD"])?;
    Ok(TrackedTreeStamp {
        digest: digest_bytes(&[&index, &dirty, &staged]),
    })
}

fn untracked_digest(repo: &Path) -> Result<String, String> {
    let listed = git_stdout_raw(repo, &["ls-files", "-z", "--others", "--exclude-standard"])?;
    Ok(digest_bytes(&[&listed]))
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
