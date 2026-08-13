//! Planning adapters for git modes, `.` (All), and explicit PATH / directory targets.

use kiss::Language;

use super::{
    PlannedSelectors, runners, rust_llvm_cov,
    targets::{ExpandedTargetPlan, expand_target_operands},
};

#[path = "plan_rust.rs"]
mod plan_rust;
use plan_rust::rust_population_current_for_all_selectors;

#[path = "plan_explicit.rs"]
mod plan_explicit;
use plan_explicit::plan_explicit_target_selectors;

#[path = "plan_vcs.rs"]
mod plan_vcs;
pub(crate) use plan_vcs::{PlanSelectorsRequest, plan_selectors};

pub(crate) enum TargetPlanKind<'a> {
    All,
    Targets(&'a [String]),
}

pub(crate) fn plan_target_selectors(
    kind: TargetPlanKind<'_>,
    ignore: &[String],
    extras: crate::test_runner::language_keyed::LanguageKeyed<&[String]>,
    lang_filter: Option<Language>,
) -> Result<PlannedSelectors, String> {
    let ignore_norm = kiss::normalize_ignore_prefixes(ignore);
    let cwd = std::env::current_dir().map_err(|e| format!("error: kiss test: {e}"))?;
    let repo_root = crate::test_git::require_git_repo_root(&cwd)
        .map_err(|e| format!("error: kiss test requires a git repository ({e})"))?;
    if matches!(lang_filter, Some(Language::Rust)) {
        rust_llvm_cov::validate_rust_extra_args(extras.rust)?;
    }
    match kind {
        TargetPlanKind::All => {
            plan_all_selectors(&repo_root, &ignore_norm, extras.python, lang_filter)
        }
        TargetPlanKind::Targets(targets) => {
            match expand_target_operands(&repo_root, targets, &ignore_norm, lang_filter)
                .map_err(|e| format!("error: kiss test: {e}"))?
            {
                ExpandedTargetPlan::All => {
                    plan_all_selectors(&repo_root, &ignore_norm, extras.python, lang_filter)
                }
                ExpandedTargetPlan::Files(files) => {
                    plan_explicit_target_selectors(
                        &repo_root,
                        &files,
                        &ignore_norm,
                        extras,
                        lang_filter,
                    )
                }
            }
        }
    }
}

fn plan_all_selectors(
    repo_root: &std::path::Path,
    ignore: &[String],
    python_extra: &[String],
    lang_filter: Option<Language>,
) -> Result<PlannedSelectors, String> {
    if let Some(planned) = try_plan_all_from_cache(repo_root, ignore, python_extra, lang_filter) {
        return Ok(planned);
    }
    let (py_sel, rs_sel) = discover_all_selectors(repo_root, ignore, python_extra, lang_filter)?;
    let fp = if lang_filter.is_none() {
        super::workspace_selector_cache::store_workspace_selectors(
            repo_root, ignore, &py_sel, &rs_sel,
        )
    } else {
        None
    };
    Ok(planned_all(
        repo_root,
        ignore,
        python_extra,
        py_sel,
        rs_sel,
        fp,
    ))
}

fn try_plan_all_from_cache(
    repo_root: &std::path::Path,
    ignore: &[String],
    python_extra: &[String],
    lang_filter: Option<Language>,
) -> Option<PlannedSelectors> {
    let cache_started = std::time::Instant::now();
    let (cached_py, cached_rs, fp) =
        super::workspace_selector_cache::load_cached_workspace_selectors(repo_root, ignore)?;
    let (py_sel, rs_sel) = match lang_filter {
        None => (cached_py, cached_rs),
        Some(Language::Python) => (cached_py, Vec::new()),
        Some(Language::Rust) => (Vec::new(), cached_rs),
    };
    crate::test_runner::emit_stage_time("plan_cache", cache_started.elapsed());
    Some(planned_all(
        repo_root,
        ignore,
        python_extra,
        py_sel,
        rs_sel,
        Some(fp),
    ))
}

fn timed_python_selectors(
    repo_root: &std::path::Path,
    ignore: &[String],
    python_extra: &[String],
) -> (Result<Vec<String>, String>, std::time::Duration) {
    let started = std::time::Instant::now();
    let out = runners::enumerate_workspace_python_selectors(repo_root, ignore, python_extra);
    (out, started.elapsed())
}

fn timed_rust_selectors(
    repo_root: &std::path::Path,
    ignore: &[String],
) -> (Result<Vec<String>, String>, std::time::Duration) {
    let started = std::time::Instant::now();
    let out = runners::enumerate_workspace_rust_selectors(repo_root, ignore);
    (out, started.elapsed())
}

fn discover_all_selectors(
    repo_root: &std::path::Path,
    ignore: &[String],
    python_extra: &[String],
    lang_filter: Option<Language>,
) -> Result<(Vec<String>, Vec<String>), String> {
    let want_python = lang_filter != Some(Language::Rust);
    let want_rust = lang_filter != Some(Language::Python);
    if want_python && want_rust {
        let (py_res, rs_res) = rayon::join(
            || timed_python_selectors(repo_root, ignore, python_extra),
            || timed_rust_selectors(repo_root, ignore),
        );
        let (py_sel, py_elapsed) = (py_res.0?, py_res.1);
        let (rs_sel, rs_elapsed) = (rs_res.0?, rs_res.1);
        crate::test_runner::emit_stage_time("plan_python", py_elapsed);
        crate::test_runner::emit_stage_time("plan_rust", rs_elapsed);
        return Ok((py_sel, rs_sel));
    }
    if want_python {
        let (py_sel, py_elapsed) = timed_python_selectors(repo_root, ignore, python_extra);
        crate::test_runner::emit_stage_time("plan_python", py_elapsed);
        return Ok((py_sel?, Vec::new()));
    }
    let (rs_sel, rs_elapsed) = timed_rust_selectors(repo_root, ignore);
    crate::test_runner::emit_stage_time("plan_rust", rs_elapsed);
    Ok((Vec::new(), rs_sel?))
}

fn planned_all(
    repo_root: &std::path::Path,
    ignore: &[String],
    python_extra: &[String],
    py_sel: Vec<String>,
    rs_sel: Vec<String>,
    workspace_files_fingerprint: Option<String>,
) -> PlannedSelectors {
    // Warm `kiss test .`: when coverage populations are already current for the
    // planned selector sets, run as selective reuse instead of re-populating
    // (avoids rediscovery + index republish on every third warm run).
    // Check cheap artifact presence before identity / tool-version work.
    let python_population_required = if py_sel.is_empty() {
        false
    } else if !crate::test_runner::python_coverage_index::python_coverage_index_file_present(
        repo_root,
    ) {
        true
    } else {
        !crate::test_runner::python_coverage_index::python_population_manifest_is_current_for_args_with_env_keys(
            repo_root,
            &py_sel,
            python_extra,
            crate::test_runner::python_coverage_index::PYTHON_COVERAGE_ENV_KEYS,
        )
    };
    let rust_population_required = if rs_sel.is_empty() {
        false
    } else {
        !rust_population_current_for_all_selectors(repo_root, &rs_sel)
    };
    PlannedSelectors {
        repo_root: repo_root.to_path_buf(),
        sel: crate::test_runner::language_keyed::LanguageKeyed {
            python: py_sel,
            rust: rs_sel,
        },
        population_required: crate::test_runner::language_keyed::LanguageKeyed {
            python: python_population_required,
            rust: rust_population_required,
        },
        source_paths: crate::test_runner::language_keyed::LanguageKeyed {
            python: Vec::new(),
            rust: Vec::new(),
        },
        vcs_source_paths: crate::test_runner::language_keyed::LanguageKeyed {
            python: 0,
            rust: 0,
        },
        snapshot_delta_modified: crate::test_runner::language_keyed::LanguageKeyed {
            python: 0,
            rust: 0,
        },
        snapshot_delta_structural: crate::test_runner::language_keyed::LanguageKeyed {
            python: false,
            rust: false,
        },
        prior_failure_selectors: crate::test_runner::language_keyed::LanguageKeyed {
            python: Vec::new(),
            rust: Vec::new(),
        },
        coverage_decision_engine_used: false,
        selection_basis: crate::test_runner::language_keyed::LanguageKeyed {
            python: crate::test_runner::coverage_decision::SelectionBasis::Current,
            rust: crate::test_runner::coverage_decision::SelectionBasis::Current,
        },
        ignore: ignore.to_vec(),
        workspace_files_fingerprint,
        skip_index_rebuild_after_selective: crate::test_runner::language_keyed::LanguageKeyed {
            python: false,
            rust: false,
        },
    }
}

pub(super) fn planned_from_selector_plan(
    repo_root: std::path::PathBuf,
    selector_plan: crate::test_runner::runners::SelectorPlan,
    ignore: Vec<String>,
) -> PlannedSelectors {
    PlannedSelectors {
        repo_root,
        sel: crate::test_runner::language_keyed::LanguageKeyed {
            python: selector_plan.selectors.python,
            rust: selector_plan.selectors.rust,
        },
        population_required: crate::test_runner::language_keyed::LanguageKeyed {
            python: selector_plan.population_required.python,
            rust: selector_plan.population_required.rust,
        },
        source_paths: selector_plan.source_paths,
        vcs_source_paths: selector_plan.vcs_source_paths,
        snapshot_delta_modified: selector_plan.snapshot_delta_modified,
        snapshot_delta_structural: selector_plan.snapshot_delta_structural,
        prior_failure_selectors: crate::test_runner::language_keyed::LanguageKeyed {
            python: selector_plan.prior_failure_selectors.python,
            rust: selector_plan.prior_failure_selectors.rust,
        },
        coverage_decision_engine_used: selector_plan.coverage_decision_engine_used,
        selection_basis: selector_plan.selection_basis,
        ignore,
        workspace_files_fingerprint: None,
        skip_index_rebuild_after_selective: crate::test_runner::language_keyed::LanguageKeyed {
            python: false,
            rust: false,
        },
    }
}

#[cfg(test)]
#[path = "plan_cold_test.rs"]
mod plan_cold_test;
