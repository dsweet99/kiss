use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::analyze_cache::fnv1a64;

const SCHEMA_VERSION: &str = "workspace-test-selectors-v9";
const PYTHON_CACHE_FILE: &str = "python_test_selectors.json";
const RUST_CACHE_FILE: &str = "rust_test_selectors.json";

#[path = "workspace_selector_cache_digest.rs"]
mod digest;
#[path = "workspace_selector_cache_python.rs"]
mod python_inventory;
use digest::{flush_persisted_digests, hash_file_contents};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct LanguageSelectorCache {
    schema_version: String,
    language: String,
    source_root: String,
    ignore: Vec<String>,
    #[serde(default)]
    collection_args: Vec<String>,
    #[serde(default)]
    plugin_identities: Vec<String>,
    files_fingerprint: String,
    selectors: Vec<String>,
}

struct LangFingerprints {
    python: String,
    rust: String,
}

fn cache_path(repo_root: &Path, name: &str) -> PathBuf {
    repo_root.join(".kiss").join(name)
}

fn durable_cache_path(repo_root: &Path, name: &str) -> PathBuf {
    repo_root.join("target").join("kiss-plan").join(name)
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

fn workspace_lang_fingerprints(
    repo_root: &Path,
    ignore: &[String],
) -> io::Result<LangFingerprints> {
    let mut fps = workspace_lang_fingerprints_git(repo_root, ignore)
        .or_else(|_| workspace_lang_fingerprints_walk(repo_root, ignore))?;
    fps.python = python_inventory::mix_collection_inventory(repo_root, ignore, &fps.python)?;
    flush_persisted_digests(repo_root);
    Ok(fps)
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
        h = hash_file_contents(h, rel, repo_root, &repo_root.join(rel))?;
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
        python: hash_rel_list(b"workspace-selectors-fp-v6-git-py", repo_root, &py_rels)?,
        rust: hash_rel_list(b"workspace-selectors-fp-v6-git-rs", repo_root, &rs_rels)?,
    })
}

fn workspace_lang_fingerprints_walk(
    repo_root: &Path,
    ignore: &[String],
) -> io::Result<LangFingerprints> {
    let mut py_h = fnv1a64(0xcbf2_9ce4_8422_2325, b"workspace-selectors-fp-v6-walk-py");
    let mut rs_h = fnv1a64(0xcbf2_9ce4_8422_2325, b"workspace-selectors-fp-v6-walk-rs");
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
            let hashed =
                hash_file_contents(if is_py { py_h } else { rs_h }, &rel, repo_root, &path)?;
            if is_py {
                py_h = hashed;
            } else {
                rs_h = hashed;
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

fn read_cache_at(path: &Path) -> Option<LanguageSelectorCache> {
    let bytes = fs::read(path).ok()?;
    let cache: LanguageSelectorCache = serde_json::from_slice(&bytes).ok()?;
    (cache.schema_version == SCHEMA_VERSION).then_some(cache)
}

fn write_cache_at(path: &Path, cache: &LanguageSelectorCache) -> io::Result<()> {
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
    cache: &LanguageSelectorCache,
    repo_root: &Path,
    ignore: &[String],
    collection_args: &[String],
    plugin_identities: &[String],
) -> bool {
    cache.source_root == normalized_root(repo_root)
        && cache.ignore == ignore
        && cache.collection_args == collection_args
        && cache.plugin_identities == plugin_identities
}

fn language_cache_matches(
    cache: &LanguageSelectorCache,
    repo_root: &Path,
    ignore: &[String],
    collection_args: &[String],
    plugin_identities: &[String],
    fingerprint: &str,
) -> bool {
    cache_identity_matches(cache, repo_root, ignore, collection_args, plugin_identities)
        && cache.files_fingerprint == fingerprint
}

fn read_language_cache(repo_root: &Path, name: &str) -> Option<LanguageSelectorCache> {
    read_cache_at(&cache_path(repo_root, name))
        .or_else(|| read_cache_at(&durable_cache_path(repo_root, name)))
}

pub(crate) fn load_cached_workspace_selectors(
    repo_root: &Path,
    ignore: &[String],
    python_extra: &[String],
) -> Option<(Vec<String>, Vec<String>, String)> {
    let fps = workspace_lang_fingerprints(repo_root, ignore).ok()?;
    let python = read_language_cache(repo_root, PYTHON_CACHE_FILE)?;
    let rust = read_language_cache(repo_root, RUST_CACHE_FILE)?;
    let plugins = kiss::TestSectionConfig::load().pytest_plugins;
    if !language_cache_matches(
        &python,
        repo_root,
        ignore,
        python_extra,
        &plugins,
        &fps.python,
    ) || !language_cache_matches(&rust, repo_root, ignore, &[], &[], &fps.rust)
    {
        return None;
    }
    rust_memo::remember_rust_selectors(
        &rust.source_root,
        ignore,
        &rust.files_fingerprint,
        &rust.selectors,
    );
    Some((
        python.selectors,
        rust.selectors,
        combined_files_fingerprint(&fps),
    ))
}

pub(crate) fn load_workspace_selectors_for_count(
    repo_root: &Path,
    ignore: &[String],
    python_extra: &[String],
) -> Option<(Vec<String>, Vec<String>)> {
    let python = read_language_cache(repo_root, PYTHON_CACHE_FILE)?;
    let rust = read_language_cache(repo_root, RUST_CACHE_FILE)?;
    let plugins = kiss::TestSectionConfig::load().pytest_plugins;
    if !cache_identity_matches(&python, repo_root, ignore, python_extra, &plugins)
        || !cache_identity_matches(&rust, repo_root, ignore, &[], &[])
    {
        return None;
    }
    Some((python.selectors, rust.selectors))
}

fn persist_selector_cache(repo_root: &Path, name: &str, cache: &LanguageSelectorCache) -> bool {
    let primary_ok = write_cache_at(&cache_path(repo_root, name), cache).is_ok();
    let durable_ok = write_cache_at(&durable_cache_path(repo_root, name), cache).is_ok();
    primary_ok || durable_ok
}

fn language_cache(
    root: String,
    ignore: &[String],
    language: &str,
    fingerprint: String,
    selectors: &[String],
    collection_args: &[String],
) -> LanguageSelectorCache {
    LanguageSelectorCache {
        schema_version: SCHEMA_VERSION.to_string(),
        language: language.to_string(),
        source_root: root,
        ignore: ignore.to_vec(),
        collection_args: collection_args.to_vec(),
        plugin_identities: if language == "python" {
            kiss::TestSectionConfig::load().pytest_plugins
        } else {
            Vec::new()
        },
        files_fingerprint: fingerprint,
        selectors: selectors.to_vec(),
    }
}

pub(crate) fn store_workspace_selectors(
    repo_root: &Path,
    ignore: &[String],
    python_selectors: &[String],
    rust_selectors: &[String],
    python_extra: &[String],
) -> Option<String> {
    let Ok(fps) = workspace_lang_fingerprints(repo_root, ignore) else {
        return None;
    };
    let root = normalized_root(repo_root);
    let python = language_cache(
        root.clone(),
        ignore,
        "python",
        fps.python.clone(),
        python_selectors,
        python_extra,
    );
    let rust = language_cache(
        root.clone(),
        ignore,
        "rust",
        fps.rust.clone(),
        rust_selectors,
        &[],
    );
    let python_ok = persist_selector_cache(repo_root, PYTHON_CACHE_FILE, &python);
    let rust_ok = persist_selector_cache(repo_root, RUST_CACHE_FILE, &rust);
    if python_ok && rust_ok {
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
pub(crate) use rust_memo::{
    cached_rust_selectors_if_rust_fingerprint_current, load_cached_rust_workspace_selectors,
    store_rust_workspace_selectors,
};

#[cfg(test)]
#[path = "workspace_selector_cache_test.rs"]
mod tests;
