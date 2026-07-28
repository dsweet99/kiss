//! Planning adapters for git modes, `all`, and explicit PATH targets.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use kiss::Language;

use super::{PlannedSelectors, runners, rust_llvm_cov, targets::resolve_target_operands};
use crate::test_git::TestChangeMode;

pub(crate) enum TargetPlanKind<'a> {
    All,
    Targets(&'a [String]),
}

pub(crate) fn plan_target_selectors(
    kind: TargetPlanKind<'_>,
    ignore: &[String],
    extra: &[String],
    lang_filter: Option<Language>,
) -> Result<PlannedSelectors, String> {
    let ignore_norm = kiss::normalize_ignore_prefixes(ignore);
    let cwd = std::env::current_dir().map_err(|e| format!("error: kiss test: {e}"))?;
    crate::test_git::assert_git_repo(&cwd)
        .map_err(|e| format!("error: kiss test requires a git repository ({e})"))?;
    let repo_root = crate::test_git::git_repo_root(&cwd)?;
    if matches!(lang_filter, Some(Language::Rust)) {
        rust_llvm_cov::validate_rust_extra_args(extra)?;
    }
    match kind {
        TargetPlanKind::All => plan_all_selectors(&repo_root, &ignore_norm, lang_filter),
        TargetPlanKind::Targets(targets) => {
            plan_explicit_target_selectors(&repo_root, targets, &ignore_norm, extra, lang_filter)
        }
    }
}

fn plan_all_selectors(
    repo_root: &std::path::Path,
    ignore: &[String],
    lang_filter: Option<Language>,
) -> Result<PlannedSelectors, String> {
    let mut py_sel = Vec::new();
    let mut rs_sel = Vec::new();
    if lang_filter != Some(Language::Rust) {
        py_sel = runners::enumerate_workspace_python_selectors(repo_root, ignore)?;
    }
    if lang_filter != Some(Language::Python) {
        rs_sel = runners::enumerate_workspace_rust_selectors(repo_root, ignore)?;
    }
    Ok(PlannedSelectors {
        repo_root: repo_root.to_path_buf(),
        py_sel,
        rs_sel,
        python_population_required: false,
        rust_population_required: false,
        rust_source_paths: Vec::new(),
        rust_vcs_source_paths: 0,
        rust_snapshot_delta_modified: 0,
        rust_snapshot_delta_structural: false,
        python_prior_failure_selectors: Vec::new(),
        rust_prior_failure_selectors: Vec::new(),
        coverage_decision_engine_used: false,
        rust_selection_basis: crate::test_runner::coverage_decision::RustSelectionBasis::Current,
        ignore: ignore.to_vec(),
    })
}

fn plan_explicit_target_selectors(
    repo_root: &std::path::Path,
    targets: &[String],
    ignore: &[String],
    extra: &[String],
    lang_filter: Option<Language>,
) -> Result<PlannedSelectors, String> {
    let query = resolve_target_operands(repo_root, targets, lang_filter, ignore, extra)
        .map_err(|e| format!("error: kiss test: {e}"))?;
    let mut source_paths = Vec::new();
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

pub(crate) fn plan_selectors(
    mode: TestChangeMode,
    main_branch_cli: Option<&str>,
    base_branch_cli: Option<&str>,
    ignore: &[String],
    extra: &[String],
    lang_filter: Option<Language>,
    config_main_branch: Option<&str>,
) -> Result<PlannedSelectors, String> {
    let ignore_norm = kiss::normalize_ignore_prefixes(ignore);
    let cwd = std::env::current_dir().map_err(|e| format!("error: kiss test: {e}"))?;
    crate::test_git::assert_git_repo(&cwd)
        .map_err(|e| format!("error: kiss test requires a git repository ({e})"))?;
    let repo_root = crate::test_git::git_repo_root(&cwd)?;
    let diff_target = crate::test_git::resolve_diff_target(
        &repo_root,
        mode,
        config_main_branch,
        main_branch_cli,
        base_branch_cli,
    )?;
    let rel_changed = match mode {
        TestChangeMode::Commit => crate::test_git::changed_paths_commit(&repo_root)?,
        TestChangeMode::Base | TestChangeMode::Main => {
            crate::test_git::changed_paths_since(&repo_root, diff_target.as_ref().unwrap())?
        }
    };
    let rel_changed_lines = match mode {
        TestChangeMode::Commit => crate::test_git::changed_lines_commit(&repo_root)?,
        TestChangeMode::Base | TestChangeMode::Main => {
            crate::test_git::changed_lines_since(&repo_root, diff_target.as_ref().unwrap())?
        }
    };
    let lang_filter = lang_filter.map(|l| match l {
        Language::Python => crate::test_git::TestLangFilter::Python,
        Language::Rust => crate::test_git::TestLangFilter::Rust,
    });
    if matches!(lang_filter, Some(crate::test_git::TestLangFilter::Rust)) {
        rust_llvm_cov::validate_rust_extra_args(extra)?;
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
    let selector_plan = runners::combined_selectors(
        &repo_root,
        &source_changed,
        &test_changed,
        &changed_lines,
        extra,
        lang_filter.map(|l| match l {
            crate::test_git::TestLangFilter::Python => Language::Python,
            crate::test_git::TestLangFilter::Rust => Language::Rust,
        }),
        &ignore_norm,
    )?;
    Ok(planned_from_selector_plan(
        repo_root,
        selector_plan,
        ignore_norm,
    ))
}
