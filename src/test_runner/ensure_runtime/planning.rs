//! Single planning API that produces `EnsureRequest` for test and cov.

use std::path::{Path, PathBuf};

use kiss::Language;

use crate::test_runner::lang_iface::{AcceptMode, EnsureRequest};
use crate::test_runner::{PlannedSelectors, runners};

/// Build an All-mode ensure request from the shared planning API (cov / full universe).
///
/// Discovers selectors for `repo_root` (not cwd), matching `plan_all_selectors` policy.
pub(crate) fn ensure_request_for_all(
    repo_root: &Path,
    ignore: &[String],
    jobs: usize,
    lang_filter: Option<Language>,
    force: bool,
) -> Result<EnsureRequest, String> {
    let python_extra = kiss::TestSectionConfig::load().pytest_plugin_cli_args();
    let ignore_norm = kiss::normalize_ignore_prefixes(ignore);
    let mut planned_python = Vec::new();
    let mut planned_rust = Vec::new();
    if lang_filter != Some(Language::Rust) {
        planned_python =
            runners::enumerate_workspace_python_selectors(repo_root, &ignore_norm, &python_extra)?;
    }
    if lang_filter != Some(Language::Python) {
        planned_rust = runners::enumerate_workspace_rust_selectors(repo_root, &ignore_norm)?;
    }
    Ok(EnsureRequest {
        repo_root: repo_root.to_path_buf(),
        mode: AcceptMode::All,
        lang_filter,
        ignore: ignore_norm,
        force,
        jobs,
        python_extra,
        rust_extra: vec![],
        planned_python,
        planned_rust,
    })
}

/// Build an ensure request from already-planned selectors (kiss test).
#[allow(clippy::too_many_arguments)] // mirrors EnsureRequest field set at the planning boundary
pub(crate) fn ensure_request_from_planned(
    planned: &PlannedSelectors,
    mode: AcceptMode,
    lang_filter: Option<Language>,
    force: bool,
    jobs: usize,
    python_extra: &[String],
    rust_extra: &[String],
    repo_root_override: Option<PathBuf>,
) -> EnsureRequest {
    EnsureRequest {
        repo_root: repo_root_override.unwrap_or_else(|| planned.repo_root.clone()),
        mode,
        lang_filter,
        ignore: planned.ignore.clone(),
        force,
        jobs,
        python_extra: python_extra.to_vec(),
        rust_extra: rust_extra.to_vec(),
        planned_python: planned.py_sel.clone(),
        planned_rust: planned.rs_sel.clone(),
    }
}

/// Cov All-mode request with an explicit planned universe (e.g. incomplete repair).
pub(crate) fn ensure_request_for_selectors(
    repo_root: &Path,
    ignore: &[String],
    jobs: usize,
    lang_filter: Language,
    force: bool,
    python: Vec<String>,
    rust: Vec<String>,
) -> EnsureRequest {
    let python_extra = kiss::TestSectionConfig::load().pytest_plugin_cli_args();
    EnsureRequest {
        repo_root: repo_root.to_path_buf(),
        mode: AcceptMode::All,
        lang_filter: Some(lang_filter),
        ignore: ignore.to_vec(),
        force,
        jobs,
        python_extra,
        rust_extra: vec![],
        planned_python: python,
        planned_rust: rust,
    }
}
