use std::path::Path;

use kiss::Language;

use super::{
    PYTHON_CACHE_FILE, RUST_CACHE_FILE, cache_identity_matches, combined_files_fingerprint,
    language_cache, language_cache_matches, normalized_root, persist_selector_cache_for_identity,
    read_language_cache_for_identity, rust_memo, workspace_lang_fingerprints,
};

pub(crate) fn store_python_workspace_selectors(
    repo_root: &Path,
    ignore: &[String],
    python_selectors: &[String],
    python_extra: &[String],
) -> bool {
    let Ok(fps) = workspace_lang_fingerprints(repo_root, ignore) else {
        return false;
    };
    let cache = language_cache(
        normalized_root(repo_root),
        ignore,
        "python",
        fps.python,
        python_selectors,
        python_extra,
    );
    persist_selector_cache_for_identity(repo_root, PYTHON_CACHE_FILE, &cache)
}

pub(crate) fn load_cached_python_workspace_selectors(
    repo_root: &Path,
    ignore: &[String],
    python_extra: &[String],
) -> Option<Vec<String>> {
    load_cached_python_workspace_hit(repo_root, ignore, python_extra)
        .map(|(selectors, _)| selectors)
}

fn load_cached_python_workspace_hit(
    repo_root: &Path,
    ignore: &[String],
    python_extra: &[String],
) -> Option<(Vec<String>, String)> {
    let plugins = kiss::TestSectionConfig::load().pytest_plugins;
    let cache = read_language_cache_for_identity(
        repo_root,
        PYTHON_CACHE_FILE,
        "python",
        ignore,
        python_extra,
        &plugins,
    )?;
    if !cache_identity_matches(&cache, repo_root, ignore, python_extra, &plugins) {
        return None;
    }
    let fps = workspace_lang_fingerprints(repo_root, ignore).ok()?;
    language_cache_matches(
        &cache,
        repo_root,
        ignore,
        python_extra,
        &plugins,
        &fps.python,
    )
    .then_some((cache.selectors, cache.files_fingerprint))
}

pub(crate) fn load_cached_workspace_selectors(
    repo_root: &Path,
    ignore: &[String],
    python_extra: &[String],
) -> Option<(Vec<String>, Vec<String>, String)> {
    let plugins = kiss::TestSectionConfig::load().pytest_plugins;
    let python = read_language_cache_for_identity(
        repo_root,
        PYTHON_CACHE_FILE,
        "python",
        ignore,
        python_extra,
        &plugins,
    )?;
    let rust =
        read_language_cache_for_identity(repo_root, RUST_CACHE_FILE, "rust", ignore, &[], &[])?;
    if !cache_identity_matches(&python, repo_root, ignore, python_extra, &plugins)
        || !cache_identity_matches(&rust, repo_root, ignore, &[], &[])
    {
        return None;
    }
    let fps = workspace_lang_fingerprints(repo_root, ignore).ok()?;
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

pub(crate) fn load_cached_workspace_selectors_for_lang(
    repo_root: &Path,
    ignore: &[String],
    python_extra: &[String],
    lang_filter: Option<Language>,
) -> Option<(Vec<String>, Vec<String>, String)> {
    match lang_filter {
        Some(Language::Python) => {
            let (selectors, fp) =
                load_cached_python_workspace_hit(repo_root, ignore, python_extra)?;
            Some((selectors, Vec::new(), fp))
        }
        Some(Language::Rust) => {
            let (selectors, fp) = rust_memo::load_cached_rust_workspace_hit(repo_root, ignore)?;
            Some((Vec::new(), selectors, fp))
        }
        None => load_cached_workspace_selectors(repo_root, ignore, python_extra),
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SelectorCountNeed {
    pub(crate) python: bool,
    pub(crate) rust: bool,
}

pub(crate) fn load_workspace_selectors_for_count(
    repo_root: &Path,
    ignore: &[String],
    python_extra: &[String],
    need: SelectorCountNeed,
) -> Option<(Vec<String>, Vec<String>)> {
    let python = if need.python {
        load_cached_python_workspace_hit(repo_root, ignore, python_extra)?.0
    } else {
        Vec::new()
    };
    let rust = if need.rust {
        rust_memo::load_cached_rust_workspace_selectors(repo_root, ignore)?
    } else {
        Vec::new()
    };
    Some((python, rust))
}

pub(crate) fn python_selectors_for_rel_path(selectors: &[String], rel: &str) -> Vec<String> {
    let prefix = format!("{rel}::");
    selectors
        .iter()
        .filter(|selector| *selector == rel || selector.starts_with(&prefix))
        .cloned()
        .collect()
}
