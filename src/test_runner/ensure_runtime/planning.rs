//! Single planning API that produces `EnsureRequest` for test and cov.

use std::path::Path;

use kiss::{GateConfig, Language};

use crate::test_runner::lang_iface::{AcceptMode, EnsureRequest};
use crate::test_runner::language_keyed::LanguageKeyed;
use crate::test_runner::ensure_runtime::EnsureFromPlanned;
use crate::test_runner::runners;

/// Build an All-mode ensure request from the shared planning API (cov / full universe).
///
/// Discovers selectors for `repo_root` (not cwd), matching `plan_all_selectors` policy.
pub(crate) fn ensure_request_for_all(
    repo_root: &Path,
    ignore: &[String],
    jobs: usize,
    lang_filter: Option<Language>,
    force: bool,
    gate: GateConfig,
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
        force_selectors: Vec::new(),
        jobs,
        gate,
        extras: LanguageKeyed {
            python: python_extra,
            rust: vec![],
        },
        planned: LanguageKeyed {
            python: planned_python,
            rust: planned_rust,
        },
    })
}

/// Build an ensure request from already-planned selectors (kiss test).
pub(crate) fn ensure_request_from_planned(args: EnsureFromPlanned<'_>) -> EnsureRequest {
    EnsureRequest {
        repo_root: args
            .repo_root_override
            .unwrap_or_else(|| args.planned.repo_root.clone()),
        mode: args.mode,
        lang_filter: args.lang_filter,
        ignore: args.planned.ignore.clone(),
        force: args.force,
        force_selectors: args.force_selectors,
        jobs: args.jobs,
        gate: args.gate,
        extras: LanguageKeyed {
            python: args.extras.python.to_vec(),
            rust: args.extras.rust.to_vec(),
        },
        planned: LanguageKeyed {
            python: args.planned.sel.python.clone(),
            rust: args.planned.sel.rust.clone(),
        },
    }
}

/// Cov All-mode request with an explicit planned universe (e.g. incomplete repair).
#[allow(clippy::too_many_arguments)] // mirrors EnsureRequest field set at the planning boundary
pub(crate) fn ensure_request_for_selectors(
    repo_root: &Path,
    ignore: &[String],
    jobs: usize,
    lang_filter: Language,
    force: bool,
    python: Vec<String>,
    rust: Vec<String>,
    gate: GateConfig,
) -> EnsureRequest {
    let python_extra = kiss::TestSectionConfig::load().pytest_plugin_cli_args();
    EnsureRequest {
        repo_root: repo_root.to_path_buf(),
        mode: AcceptMode::All,
        lang_filter: Some(lang_filter),
        ignore: ignore.to_vec(),
        force,
        force_selectors: Vec::new(),
        jobs,
        gate,
        extras: LanguageKeyed {
            python: python_extra,
            rust: vec![],
        },
        planned: LanguageKeyed {
            python,
            rust,
        },
    }
}
