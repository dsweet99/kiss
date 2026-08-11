//! Cached workspace selector lists for `kiss test .` (All) warm planning.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::analyze_cache::fnv1a64;

const SCHEMA_VERSION: &str = "workspace-test-selectors-v3";
const CACHE_FILE_NAME: &str = "workspace_test_selectors.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct WorkspaceSelectorCache {
    schema_version: String,
    source_root: String,
    ignore: Vec<String>,
    files_fingerprint: String,
    python_selectors: Vec<String>,
    rust_selectors: Vec<String>,
}

fn cache_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".kiss").join(CACHE_FILE_NAME)
}

fn should_skip_dir(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | "target"
            | ".kiss"
            | ".venv"
            | "venv"
            | "__pycache__"
            | ".pytest_cache"
            | ".rslip_cache"
            | "node_modules"
    )
}

fn ignored(rel: &str, ignore: &[String]) -> bool {
    ignore.iter().any(|prefix| rel == prefix || rel.starts_with(&format!("{prefix}/")))
}

fn hash_file_meta(h: u64, rel: &str, meta: &fs::Metadata) -> u64 {
    let mtime = match meta.modified() {
        Ok(t) => t
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0),
        Err(_) => 0,
    };
    let mut acc = fnv1a64(h, rel.as_bytes());
    acc = fnv1a64(acc, &meta.len().to_le_bytes());
    fnv1a64(acc, &mtime.to_le_bytes())
}

fn workspace_files_fingerprint(repo_root: &Path, ignore: &[String]) -> io::Result<String> {
    if let Ok(fp) = workspace_files_fingerprint_git(repo_root, ignore) {
        return Ok(fp);
    }
    workspace_files_fingerprint_walk(repo_root, ignore)
}

/// Shared fingerprint for other warm caches (report-id map, etc.).
pub(crate) fn workspace_files_fingerprint_for_cache(
    repo_root: &Path,
    ignore: &[String],
) -> io::Result<String> {
    workspace_files_fingerprint(repo_root, ignore)
}

fn workspace_files_fingerprint_git(repo_root: &Path, ignore: &[String]) -> io::Result<String> {
    // Include untracked (non-ignored) sources: discovery finds their tests, but
    // plain `git ls-files` would leave the selector cache falsely current.
    let output = kiss::scrubbed_git_command(repo_root)
        .args([
            "ls-files",
            "-z",
            "-c",
            "-o",
            "--exclude-standard",
            "--",
            "*.py",
            "*.rs",
        ])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("git ls-files failed"));
    }
    let mut rels = output
        .stdout
        .split(|b| *b == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).replace('\\', "/"))
        .filter(|rel| !ignored(rel, ignore))
        .collect::<Vec<_>>();
    rels.sort();
    rels.dedup();
    let mut h = fnv1a64(0xcbf2_9ce4_8422_2325, b"workspace-selectors-fp-v4-git");
    for rel in rels {
        let meta = fs::metadata(repo_root.join(&rel))?;
        h = hash_file_meta(h, &rel, &meta);
    }
    Ok(format!("{h:016x}"))
}

fn workspace_files_fingerprint_walk(repo_root: &Path, ignore: &[String]) -> io::Result<String> {
    let mut h = fnv1a64(0xcbf2_9ce4_8422_2325, b"workspace-selectors-fp-v2");
    let mut stack = vec![repo_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir)?;
        let mut paths: Vec<_> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            let rel = path
                .strip_prefix(repo_root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            if path.is_dir() {
                if should_skip_dir(name) || ignored(&rel, ignore) {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if ignored(&rel, ignore) {
                continue;
            }
            let is_py = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("py"));
            let is_rs = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("rs"));
            if !(is_py || is_rs) {
                continue;
            }
            let meta = fs::metadata(&path)?;
            h = hash_file_meta(h, &rel, &meta);
        }
    }
    Ok(format!("{h:016x}"))
}

pub(crate) fn normalized_root(repo_root: &Path) -> String {
    repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf())
        .to_string_lossy()
        .to_string()
}

fn read_cache(repo_root: &Path) -> Option<WorkspaceSelectorCache> {
    let bytes = fs::read(cache_path(repo_root)).ok()?;
    let cache: WorkspaceSelectorCache = serde_json::from_slice(&bytes).ok()?;
    (cache.schema_version == SCHEMA_VERSION).then_some(cache)
}

fn write_cache(repo_root: &Path, cache: &WorkspaceSelectorCache) -> io::Result<()> {
    let path = cache_path(repo_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!(
        "tmp.{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let mut file = File::create(&tmp)?;
    serde_json::to_writer(&mut file, cache).map_err(io::Error::other)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    fs::rename(tmp, path)?;
    Ok(())
}

pub(crate) fn load_cached_workspace_selectors(
    repo_root: &Path,
    ignore: &[String],
) -> Option<(Vec<String>, Vec<String>, String)> {
    let cache = read_cache(repo_root)?;
    if cache.source_root != normalized_root(repo_root) || cache.ignore != ignore {
        return None;
    }
    let fp = workspace_files_fingerprint(repo_root, ignore).ok()?;
    if cache.files_fingerprint != fp {
        return None;
    }
    Some((cache.python_selectors, cache.rust_selectors, fp))
}

pub(crate) fn store_workspace_selectors(
    repo_root: &Path,
    ignore: &[String],
    python_selectors: &[String],
    rust_selectors: &[String],
) -> Option<String> {
    let Ok(files_fingerprint) = workspace_files_fingerprint(repo_root, ignore) else {
        return None;
    };
    let cache = WorkspaceSelectorCache {
        schema_version: SCHEMA_VERSION.to_string(),
        source_root: normalized_root(repo_root),
        ignore: ignore.to_vec(),
        files_fingerprint: files_fingerprint.clone(),
        python_selectors: python_selectors.to_vec(),
        rust_selectors: rust_selectors.to_vec(),
    };
    let _ = write_cache(repo_root, &cache);
    Some(files_fingerprint)
}

#[cfg(test)]
#[path = "workspace_selector_cache_test.rs"]
mod tests;
