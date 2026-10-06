use std::path::Path;

use kiss::{GateConfig, Language};

use crate::test_runner::ensure_runtime::EnsureFromPlanned;
use crate::test_runner::lang_iface::{AcceptMode, EnsureRequest};
use crate::test_runner::language_keyed::LanguageKeyed;
use crate::test_runner::runners;

pub(crate) fn ensure_request_for_all(
    repo_root: &Path,
    ignore: &[String],
    jobs: usize,
    lang_filter: Option<Language>,
    force: bool,
    gate: GateConfig,
    pytest_args: Vec<String>,
) -> Result<EnsureRequest, String> {
    let ignore_norm = kiss::normalize_ignore_prefixes(ignore);
    let mut planned_python = Vec::new();
    let mut planned_rust = Vec::new();
    let allowed = LanguageKeyed::from_fn(|language| language.allowed_by(lang_filter));
    if allowed.python {
        planned_python =
            crate::test_runner::workspace_selector_cache::load_cached_python_workspace_selectors(
                repo_root,
                &ignore_norm,
                &pytest_args,
            )
            .map_or_else(
                || {
                    runners::enumerate_workspace_python_selectors(
                        repo_root,
                        &ignore_norm,
                        &pytest_args,
                    )
                },
                Ok,
            )?;
    }
    if allowed.rust {
        planned_rust =
            crate::test_runner::workspace_selector_cache::load_cached_rust_workspace_selectors(
                repo_root,
                &ignore_norm,
            )
            .map_or_else(
                || runners::enumerate_workspace_rust_selectors(repo_root, &ignore_norm),
                Ok,
            )?;
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
            python: pytest_args,
            rust: vec![],
        },
        planned: LanguageKeyed {
            python: planned_python,
            rust: planned_rust,
        },
    })
}

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
