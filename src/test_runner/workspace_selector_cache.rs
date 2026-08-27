use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::analyze_cache::fnv1a64;

const SCHEMA_VERSION: &str = "workspace-test-selectors-v4";
const CACHE_FILE_NAME: &str = "workspace_test_selectors.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct WorkspaceSelectorCache {
    schema_version: String,
    source_root: String,
    ignore: Vec<String>,
    python_files_fingerprint: String,
    rust_files_fingerprint: String,
    python_selectors: Vec<String>,
    rust_selectors: Vec<String>,
}

struct LangFingerprints {
    python: String,
    rust: String,
}

fn cache_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".kiss").join(CACHE_FILE_NAME)
}

fn durable_cache_path(repo_root: &Path) -> PathBuf {
    repo_root
        .join("target")
        .join("kiss-plan")
        .join(CACHE_FILE_NAME)
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
    kiss::path_ignored_by_prefixes(rel, ignore)
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

fn workspace_lang_fingerprints(
    repo_root: &Path,
    ignore: &[String],
) -> io::Result<LangFingerprints> {
    workspace_lang_fingerprints_git(repo_root, ignore)
        .or_else(|_| workspace_lang_fingerprints_walk(repo_root, ignore))
}

fn combined_files_fingerprint(fp: &LangFingerprints) -> String {
    format!("{}:{}", fp.python, fp.rust)
}

fn workspace_files_fingerprint(repo_root: &Path, ignore: &[String]) -> io::Result<String> {
    Ok(combined_files_fingerprint(&workspace_lang_fingerprints(
        repo_root, ignore,
    )?))
}

pub(crate) fn workspace_files_fingerprint_for_cache(
    repo_root: &Path,
    ignore: &[String],
) -> io::Result<String> {
    workspace_files_fingerprint(repo_root, ignore)
}

fn hash_rel_list(seed: &[u8], repo_root: &Path, rels: &[String]) -> io::Result<String> {
    let mut h = fnv1a64(0xcbf2_9ce4_8422_2325, seed);
    for rel in rels {
        let meta = fs::metadata(repo_root.join(rel))?;
        h = hash_file_meta(h, rel, &meta);
    }
    Ok(format!("{h:016x}"))
}

fn workspace_lang_fingerprints_git(
    repo_root: &Path,
    ignore: &[String],
) -> io::Result<LangFingerprints> {
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
    let mut py_rels = Vec::new();
    let mut rs_rels = Vec::new();
    for part in output
        .stdout
        .split(|b| *b == 0)
        .filter(|part| !part.is_empty())
    {
        let rel = String::from_utf8_lossy(part).replace('\\', "/");
        if ignored(&rel, ignore) {
            continue;
        }
        if rel.ends_with(".py") {
            py_rels.push(rel);
        } else if rel.ends_with(".rs") {
            rs_rels.push(rel);
        }
    }
    py_rels.sort();
    py_rels.dedup();
    rs_rels.sort();
    rs_rels.dedup();
    Ok(LangFingerprints {
        python: hash_rel_list(b"workspace-selectors-fp-v5-git-py", repo_root, &py_rels)?,
        rust: hash_rel_list(b"workspace-selectors-fp-v5-git-rs", repo_root, &rs_rels)?,
    })
}

fn workspace_lang_fingerprints_walk(
    repo_root: &Path,
    ignore: &[String],
) -> io::Result<LangFingerprints> {
    let mut py_h = fnv1a64(0xcbf2_9ce4_8422_2325, b"workspace-selectors-fp-v5-walk-py");
    let mut rs_h = fnv1a64(0xcbf2_9ce4_8422_2325, b"workspace-selectors-fp-v5-walk-rs");
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
            let is_py = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("py"));
            let is_rs = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("rs"));
            if !is_py && !is_rs {
                continue;
            }
            let meta = fs::metadata(&path)?;
            if is_py {
                py_h = hash_file_meta(py_h, &rel, &meta);
            } else {
                rs_h = hash_file_meta(rs_h, &rel, &meta);
            }
        }
    }
    Ok(LangFingerprints {
        python: format!("{py_h:016x}"),
        rust: format!("{rs_h:016x}"),
    })
}

pub(crate) fn normalized_root(repo_root: &Path) -> String {
    repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf())
        .to_string_lossy()
        .to_string()
}

fn read_cache_at(path: &Path) -> Option<WorkspaceSelectorCache> {
    let bytes = fs::read(path).ok()?;
    let cache: WorkspaceSelectorCache = serde_json::from_slice(&bytes).ok()?;
    (cache.schema_version == SCHEMA_VERSION).then_some(cache)
}

fn write_cache_at(path: &Path, cache: &WorkspaceSelectorCache) -> io::Result<()> {
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

fn cache_identity_matches(
    cache: &WorkspaceSelectorCache,
    repo_root: &Path,
    ignore: &[String],
) -> bool {
    cache.source_root == normalized_root(repo_root) && cache.ignore == ignore
}

fn combined_cache_matches(
    cache: &WorkspaceSelectorCache,
    repo_root: &Path,
    ignore: &[String],
    fps: &LangFingerprints,
) -> bool {
    cache_identity_matches(cache, repo_root, ignore)
        && cache.python_files_fingerprint == fps.python
        && cache.rust_files_fingerprint == fps.rust
}

pub(crate) fn load_cached_workspace_selectors(
    repo_root: &Path,
    ignore: &[String],
) -> Option<(Vec<String>, Vec<String>, String)> {
    let fps = workspace_lang_fingerprints(repo_root, ignore).ok()?;
    let fp = combined_files_fingerprint(&fps);
    if let Some(cache) = read_cache_at(&cache_path(repo_root))
        .filter(|cache| combined_cache_matches(cache, repo_root, ignore, &fps))
    {
        rust_memo::remember_rust_selectors(
            &cache.source_root,
            ignore,
            &cache.rust_files_fingerprint,
            &cache.rust_selectors,
        );
        return Some((cache.python_selectors, cache.rust_selectors, fp));
    }
    let durable = read_cache_at(&durable_cache_path(repo_root))?;
    if !combined_cache_matches(&durable, repo_root, ignore, &fps) {
        return None;
    }

    let _ = write_cache_at(&cache_path(repo_root), &durable);
    rust_memo::remember_rust_selectors(
        &durable.source_root,
        ignore,
        &durable.rust_files_fingerprint,
        &durable.rust_selectors,
    );
    Some((durable.python_selectors, durable.rust_selectors, fp))
}

pub(crate) fn load_workspace_selectors_for_count(
    repo_root: &Path,
    ignore: &[String],
) -> Option<(Vec<String>, Vec<String>)> {
    let root = normalized_root(repo_root);
    let cache = read_cache_at(&cache_path(repo_root))
        .or_else(|| read_cache_at(&durable_cache_path(repo_root)))?;
    if cache.source_root != root {
        return None;
    }
    if cache.ignore != ignore {
        return None;
    }
    Some((cache.python_selectors, cache.rust_selectors))
}

fn persist_selector_cache(repo_root: &Path, cache: &WorkspaceSelectorCache) -> bool {
    let primary_ok = write_cache_at(&cache_path(repo_root), cache).is_ok();
    let durable_ok = write_cache_at(&durable_cache_path(repo_root), cache).is_ok();
    primary_ok || durable_ok
}

pub(crate) fn store_workspace_selectors(
    repo_root: &Path,
    ignore: &[String],
    python_selectors: &[String],
    rust_selectors: &[String],
) -> Option<String> {
    let Ok(fps) = workspace_lang_fingerprints(repo_root, ignore) else {
        return None;
    };
    let root = normalized_root(repo_root);
    let cache = WorkspaceSelectorCache {
        schema_version: SCHEMA_VERSION.to_string(),
        source_root: root.clone(),
        ignore: ignore.to_vec(),
        python_files_fingerprint: fps.python.clone(),
        rust_files_fingerprint: fps.rust.clone(),
        python_selectors: python_selectors.to_vec(),
        rust_selectors: rust_selectors.to_vec(),
    };

    if persist_selector_cache(repo_root, &cache) {
        rust_memo::remember_rust_selectors(&root, ignore, &fps.rust, rust_selectors);
        Some(combined_files_fingerprint(&fps))
    } else {
        None
    }
}

#[path = "workspace_selector_cache_rust.rs"]
mod rust_memo;
#[cfg(test)]
pub(crate) use rust_memo::clear_rust_selector_memo_for_tests;
pub(crate) use rust_memo::{load_cached_rust_workspace_selectors, store_rust_workspace_selectors};

#[cfg(test)]
#[path = "workspace_selector_cache_test.rs"]
mod tests;
