use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::analyze_cache::fnv1a64;

const SCHEMA_VERSION: &str = "workspace-test-selectors-v10";
const PYTHON_CACHE_FILE: &str = "python_test_selectors.json";
const RUST_CACHE_FILE: &str = "rust_test_selectors.json";

#[cfg(test)]
static WORKSPACE_FINGERPRINT_COMPUTATIONS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
#[cfg(test)]
static COUNTED_FINGERPRINT_ROOT: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[path = "workspace_selector_cache_digest.rs"]
mod digest;
#[path = "workspace_selector_cache_fresh.rs"]
mod fresh;
#[path = "workspace_selector_cache_inventory.rs"]
mod inventory;
pub(crate) use fresh::begin_inventory_session;
pub(crate) use inventory::{
    rust_selector_inputs_fingerprint_for_cache, workspace_source_inventory_fingerprint_for_cache,
};
#[path = "workspace_selector_cache_lookup.rs"]
mod lookup;
#[path = "workspace_selector_cache_python.rs"]
mod python_inventory;
use digest::flush_persisted_digests;
#[cfg(test)]
pub(crate) use lookup::load_cached_workspace_selectors;
pub(crate) use lookup::{
    SelectorCountNeed, load_cached_python_workspace_selectors,
    load_cached_workspace_selectors_for_lang, load_workspace_selectors_for_count,
    python_selectors_for_rel_path, store_python_workspace_selectors,
};

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

#[derive(Clone)]
pub(super) struct LangFingerprints {
    python: String,
    rust: String,
}

fn cache_path(repo_root: &Path, name: &str) -> PathBuf {
    repo_root.join(".kiss").join(name)
}

fn durable_cache_path(repo_root: &Path, name: &str) -> PathBuf {
    repo_root.join("target").join("kiss-plan").join(name)
}

fn identity_cache_name(
    name: &str,
    language: &str,
    ignore: &[String],
    collection_args: &[String],
    plugin_identities: &[String],
) -> String {
    let mut h = fnv1a64(
        0xcbf2_9ce4_8422_2325,
        b"workspace-selector-cache-identity-v1",
    );
    h = fnv1a64(h, &(language.len() as u64).to_le_bytes());
    h = fnv1a64(h, language.as_bytes());
    for (tag, values) in [
        (b"ignore".as_slice(), ignore),
        (b"collection-args".as_slice(), collection_args),
        (b"plugins".as_slice(), plugin_identities),
    ] {
        h = fnv1a64(h, tag);
        h = fnv1a64(h, &(values.len() as u64).to_le_bytes());
        for value in values {
            h = fnv1a64(h, &(value.len() as u64).to_le_bytes());
            h = fnv1a64(h, value.as_bytes());
        }
    }
    let stem = name.strip_suffix(".json").unwrap_or(name);
    format!("{stem}.{h:016x}.json")
}

pub(super) fn workspace_lang_fingerprints(
    repo_root: &Path,
    ignore: &[String],
) -> io::Result<LangFingerprints> {
    if let Some(fingerprints) = fresh::recall_fingerprints(repo_root, ignore) {
        return Ok(fingerprints);
    }
    #[cfg(test)]
    if COUNTED_FINGERPRINT_ROOT
        .lock()
        .ok()
        .and_then(|root| root.clone())
        .is_some_and(|root| root == normalized_root(repo_root))
    {
        WORKSPACE_FINGERPRINT_COMPUTATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    let mut fps = inventory::workspace_lang_fingerprints_git(repo_root, ignore)
        .or_else(|_| inventory::workspace_lang_fingerprints_walk(repo_root, ignore))?;
    fps.python = python_inventory::mix_collection_inventory(repo_root, ignore, &fps.python)?;
    flush_persisted_digests(repo_root);
    fresh::remember_fingerprints(repo_root, ignore, &fps);
    Ok(fps)
}

#[cfg(test)]
fn reset_workspace_fingerprint_computation_count(repo_root: &Path) {
    WORKSPACE_FINGERPRINT_COMPUTATIONS.store(0, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut root) = COUNTED_FINGERPRINT_ROOT.lock() {
        *root = Some(normalized_root(repo_root));
    }
}

#[cfg(test)]
fn workspace_fingerprint_computation_count() -> usize {
    WORKSPACE_FINGERPRINT_COMPUTATIONS.load(std::sync::atomic::Ordering::Relaxed)
}

pub(super) fn combined_files_fingerprint(fp: &LangFingerprints) -> String {
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

pub(super) fn cache_identity_matches(
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

pub(super) fn language_cache_matches(
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

pub(super) fn read_language_cache(repo_root: &Path, name: &str) -> Option<LanguageSelectorCache> {
    read_cache_at(&cache_path(repo_root, name))
        .or_else(|| read_cache_at(&durable_cache_path(repo_root, name)))
}

pub(super) fn read_all_language_caches(repo_root: &Path, name: &str) -> Vec<LanguageSelectorCache> {
    let stem = name.strip_suffix(".json").unwrap_or(name);
    let keyed_prefix = format!("{stem}.");
    let mut paths = Vec::new();
    for dir in [
        repo_root.join(".kiss"),
        repo_root.join("target").join("kiss-plan"),
    ] {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        paths.extend(
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
                        return false;
                    };
                    file_name == name
                        || (file_name.starts_with(&keyed_prefix) && file_name.ends_with(".json"))
                }),
        );
    }
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| read_cache_at(&path))
        .collect()
}

pub(super) fn read_language_cache_for_identity(
    repo_root: &Path,
    name: &str,
    language: &str,
    ignore: &[String],
    collection_args: &[String],
    plugin_identities: &[String],
) -> Option<LanguageSelectorCache> {
    let keyed = identity_cache_name(name, language, ignore, collection_args, plugin_identities);
    read_language_cache(repo_root, &keyed).or_else(|| {
        let cache = read_language_cache(repo_root, name)?;
        cache_identity_matches(
            &cache,
            repo_root,
            ignore,
            collection_args,
            plugin_identities,
        )
        .then_some(cache)
    })
}

pub(super) fn persist_selector_cache(
    repo_root: &Path,
    name: &str,
    cache: &LanguageSelectorCache,
) -> bool {
    let primary_ok = write_cache_at(&cache_path(repo_root, name), cache).is_ok();
    let durable_ok = write_cache_at(&durable_cache_path(repo_root, name), cache).is_ok();
    primary_ok || durable_ok
}

pub(super) fn persist_selector_cache_for_identity(
    repo_root: &Path,
    name: &str,
    cache: &LanguageSelectorCache,
) -> bool {
    let keyed = identity_cache_name(
        name,
        &cache.language,
        &cache.ignore,
        &cache.collection_args,
        &cache.plugin_identities,
    );
    let keyed_ok = persist_selector_cache(repo_root, &keyed, cache);
    let latest_ok = persist_selector_cache(repo_root, name, cache);
    keyed_ok || latest_ok
}

pub(super) fn language_cache(
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
    let python_ok = persist_selector_cache_for_identity(repo_root, PYTHON_CACHE_FILE, &python);
    let rust_ok = persist_selector_cache_for_identity(repo_root, RUST_CACHE_FILE, &rust);
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
    cached_rust_selectors_if_rust_fingerprint_current, load_cached_rust_workspace_fingerprint,
    load_cached_rust_workspace_selectors, store_rust_workspace_selectors,
};

#[cfg(test)]
#[path = "workspace_selector_cache_test.rs"]
mod tests;
