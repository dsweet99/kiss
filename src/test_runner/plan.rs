//! Planning adapters for git modes, `.` (All), and explicit PATH / directory targets.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use kiss::Language;

use super::{
    PlannedSelectors, runners, rust_llvm_cov,
    targets::{ExpandedTargetPlan, expand_target_operands, resolve_target_operands},
};
use crate::test_git::TestChangeMode;

pub(crate) enum TargetPlanKind<'a> {
    All,
    Targets(&'a [String]),
}

pub(crate) fn plan_target_selectors(
    kind: TargetPlanKind<'_>,
    ignore: &[String],
    extra: &[String],
    python_extra: &[String],
    lang_filter: Option<Language>,
) -> Result<PlannedSelectors, String> {
    let ignore_norm = kiss::normalize_ignore_prefixes(ignore);
    let cwd = std::env::current_dir().map_err(|e| format!("error: kiss test: {e}"))?;
    let repo_root = crate::test_git::require_git_repo_root(&cwd)
        .map_err(|e| format!("error: kiss test requires a git repository ({e})"))?;
    if matches!(lang_filter, Some(Language::Rust)) {
        rust_llvm_cov::validate_rust_extra_args(extra)?;
    }
    match kind {
        TargetPlanKind::All => plan_all_selectors(&repo_root, &ignore_norm, python_extra, lang_filter),
        TargetPlanKind::Targets(targets) => {
            match expand_target_operands(&repo_root, targets, &ignore_norm, lang_filter)
                .map_err(|e| format!("error: kiss test: {e}"))?
            {
                ExpandedTargetPlan::All => {
                    plan_all_selectors(&repo_root, &ignore_norm, python_extra, lang_filter)
                }
                ExpandedTargetPlan::Files(files) => plan_explicit_target_selectors(
                    &repo_root,
                    &files,
                    &ignore_norm,
                    extra,
                    python_extra,
                    lang_filter,
                ),
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
    if let Some((cached_py, cached_rs)) =
        super::workspace_selector_cache::load_cached_workspace_selectors(repo_root, ignore)
    {
        super::emit_test_progress("kiss test: using cached selectors");
        let (py_sel, rs_sel) = match lang_filter {
            None => (cached_py, cached_rs),
            Some(Language::Python) => (cached_py, Vec::new()),
            Some(Language::Rust) => (Vec::new(), cached_rs),
        };
        return Ok(planned_all(repo_root, ignore, python_extra, py_sel, rs_sel));
    }
    let mut py_sel = Vec::new();
    let mut rs_sel = Vec::new();
    if lang_filter != Some(Language::Rust) {
        super::emit_test_progress("kiss test: collecting python selectors");
        py_sel = runners::enumerate_workspace_python_selectors(repo_root, ignore, python_extra)?;
    }
    if lang_filter != Some(Language::Python) {
        super::emit_test_progress("kiss test: collecting rust selectors");
        rs_sel = runners::enumerate_workspace_rust_selectors(repo_root, ignore)?;
    }
    if lang_filter.is_none() {
        super::workspace_selector_cache::store_workspace_selectors(
            repo_root, ignore, &py_sel, &rs_sel,
        );
    }
    Ok(planned_all(repo_root, ignore, python_extra, py_sel, rs_sel))
}

fn planned_all(
    repo_root: &std::path::Path,
    ignore: &[String],
    python_extra: &[String],
    py_sel: Vec<String>,
    rs_sel: Vec<String>,
) -> PlannedSelectors {
    // Warm `kiss test .`: when coverage populations are already current for the
    // planned selector sets, run as selective reuse instead of re-populating
    // (avoids rediscovery + index republish on every third warm run).
    if !py_sel.is_empty() {
        super::emit_test_progress("kiss test: checking python coverage population");
    }
    let python_population_required = !(py_sel.is_empty()
        || (crate::test_runner::python_coverage_index::python_population_manifest_is_current_for_args_with_env_keys(
            repo_root,
            &py_sel,
            python_extra,
            crate::test_runner::python_coverage_index::PYTHON_COVERAGE_ENV_KEYS,
        ) && crate::test_runner::python_coverage_index::load_current_python_coverage_index(repo_root)
            .is_some()));
    if !rs_sel.is_empty() {
        super::emit_test_progress("kiss test: checking rust coverage population");
    }
    let rust_population_required = !(rs_sel.is_empty()
        || rust_population_current_for_all_selectors(repo_root, &rs_sel));
    PlannedSelectors {
        repo_root: repo_root.to_path_buf(),
        py_sel,
        rs_sel,
        python_population_required,
        rust_population_required,
        rust_source_paths: Vec::new(),
        rust_vcs_source_paths: 0,
        rust_snapshot_delta_modified: 0,
        rust_snapshot_delta_structural: false,
        python_prior_failure_selectors: Vec::new(),
        rust_prior_failure_selectors: Vec::new(),
        coverage_decision_engine_used: false,
        rust_selection_basis: crate::test_runner::coverage_decision::RustSelectionBasis::Current,
        ignore: ignore.to_vec(),
    }
}

fn rust_population_current_for_all_selectors(
    repo_root: &std::path::Path,
    selectors: &[String],
) -> bool {
    let Ok(identity) =
        crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity(repo_root, &[])
    else {
        return false;
    };
    let mut expected = selectors.to_vec();
    expected.sort();
    expected.dedup();
    rust_llvm_cov_runner::load_current_population_state(
        &crate::test_runner::rust_coverage_index::rust_coverage_cache_root(repo_root),
        repo_root,
        &identity,
        Some(&expected),
    )
    .is_some()
}

fn plan_explicit_target_selectors(
    repo_root: &std::path::Path,
    targets: &[String],
    ignore: &[String],
    extra: &[String],
    python_extra: &[String],
    lang_filter: Option<Language>,
) -> Result<PlannedSelectors, String> {
    let query = resolve_target_operands(repo_root, targets, lang_filter, ignore, python_extra)
        .map_err(|e| format!("error: kiss test: {e}"))?;
    let mut source_paths = Vec::new();
    source_paths.extend(query.python_files.iter().cloned());
    source_paths.extend(query.rust_files.iter().cloned());
    source_paths.extend(query.python_lines.keys().cloned());
    source_paths.extend(query.rust_lines.keys().cloned());
    source_paths.sort();
    source_paths.dedup();
    let mut changed_lines: BTreeMap<PathBuf, BTreeSet<u32>> = BTreeMap::new();
    for (path, lines) in query.python_lines.iter().chain(query.rust_lines.iter()) {
        changed_lines
            .entry(path.clone())
            .or_default()
            .extend(lines.iter().copied());
    }
    let direct_python: Vec<_> = query.direct_python.into_iter().collect();
    let direct_rust: Vec<_> = query.direct_rust.into_iter().collect();
    let input = runners::CombinedSelectorInput {
        repo_root,
        source_paths: &source_paths,
        test_paths: &[],
        changed_lines: &changed_lines,
        rust_test_args: extra,
        python_test_args: python_extra,
        lang_filter,
        ignore,
        extra_direct_python: &direct_python,
        extra_direct_rust: &direct_rust,
    };
    let selector_plan = runners::combined_selectors_with_direct(input)?;
    Ok(planned_from_selector_plan(
        repo_root.to_path_buf(),
        selector_plan,
        ignore.to_vec(),
    ))
}

fn planned_from_selector_plan(
    repo_root: PathBuf,
    selector_plan: crate::test_runner::runners::SelectorPlan,
    ignore: Vec<String>,
) -> PlannedSelectors {
    PlannedSelectors {
        repo_root,
        py_sel: selector_plan.py_selectors,
        rs_sel: selector_plan.rust_selectors,
        python_population_required: selector_plan.python_population_required,
        rust_population_required: selector_plan.rust_population_required,
        rust_source_paths: selector_plan.rust_source_paths,
        rust_vcs_source_paths: selector_plan.rust_vcs_source_paths,
        rust_snapshot_delta_modified: selector_plan.rust_snapshot_delta_modified,
        rust_snapshot_delta_structural: selector_plan.rust_snapshot_delta_structural,
        python_prior_failure_selectors: selector_plan.python_prior_failure_selectors,
        rust_prior_failure_selectors: selector_plan.rust_prior_failure_selectors,
        coverage_decision_engine_used: selector_plan.coverage_decision_engine_used,
        rust_selection_basis: selector_plan.rust_selection_basis,
        ignore,
    }
}

pub(crate) struct PlanSelectorsRequest<'a> {
    pub mode: TestChangeMode,
    pub main_branch_cli: Option<&'a str>,
    pub base_branch_cli: Option<&'a str>,
    pub ignore: &'a [String],
    pub extra: &'a [String],
    pub python_extra: &'a [String],
    pub lang_filter: Option<Language>,
    pub config_main_branch: Option<&'a str>,
}

pub(crate) fn plan_selectors(req: PlanSelectorsRequest<'_>) -> Result<PlannedSelectors, String> {
    let ignore_norm = kiss::normalize_ignore_prefixes(req.ignore);
    let cwd = std::env::current_dir().map_err(|e| format!("error: kiss test: {e}"))?;
    let repo_root = crate::test_git::require_git_repo_root(&cwd)
        .map_err(|e| format!("error: kiss test requires a git repository ({e})"))?;
    let diff_target = crate::test_git::resolve_diff_target(
        &repo_root,
        req.mode,
        req.config_main_branch,
        req.main_branch_cli,
        req.base_branch_cli,
    )?;
    let rel_changed = match req.mode {
        TestChangeMode::Commit => crate::test_git::changed_paths_commit(&repo_root)?,
        TestChangeMode::Base | TestChangeMode::Main => {
            crate::test_git::changed_paths_since(&repo_root, diff_target.as_ref().unwrap())?
        }
    };
    let rel_changed_lines = match req.mode {
        TestChangeMode::Commit => crate::test_git::changed_lines_commit(&repo_root)?,
        TestChangeMode::Base | TestChangeMode::Main => {
            crate::test_git::changed_lines_since(&repo_root, diff_target.as_ref().unwrap())?
        }
    };
    let lang_filter = req.lang_filter.map(|l| match l {
        Language::Python => crate::test_git::TestLangFilter::Python,
        Language::Rust => crate::test_git::TestLangFilter::Rust,
    });
    if matches!(lang_filter, Some(crate::test_git::TestLangFilter::Rust)) {
        rust_llvm_cov::validate_rust_extra_args(req.extra)?;
    }
    let abs_paths = crate::test_git::resolve_changed_source_paths(
        &repo_root,
        &rel_changed,
        &ignore_norm,
        lang_filter,
    );
    let changed_lines = crate::test_git::resolve_changed_line_paths(
        &repo_root,
        &rel_changed_lines,
        &ignore_norm,
        lang_filter,
    );
    let (source_changed, test_changed) = runners::partition_changed_paths(&abs_paths);
    let selector_plan = runners::combined_selectors_with_direct(runners::CombinedSelectorInput {
        repo_root: &repo_root,
        source_paths: &source_changed,
        test_paths: &test_changed,
        changed_lines: &changed_lines,
        rust_test_args: req.extra,
        python_test_args: req.python_extra,
        lang_filter: lang_filter.map(|l| match l {
            crate::test_git::TestLangFilter::Python => Language::Python,
            crate::test_git::TestLangFilter::Rust => Language::Rust,
        }),
        ignore: &ignore_norm,
        extra_direct_python: &[],
        extra_direct_rust: &[],
    })?;
    Ok(planned_from_selector_plan(
        repo_root,
        selector_plan,
        ignore_norm,
    ))
}
