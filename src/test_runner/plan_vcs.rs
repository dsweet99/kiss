//! VCS-mode planning (`--commit` / `--base` / `--main`) for `kiss test`.

use kiss::Language;

use super::runners;
use super::rust_llvm_cov;
use super::{PlannedSelectors, planned_from_selector_plan};
use crate::test_git::TestChangeMode;

pub(crate) struct PlanSelectorsRequest<'a> {
    pub mode: TestChangeMode,
    pub main_branch_cli: Option<&'a str>,
    pub base_branch_cli: Option<&'a str>,
    pub ignore: &'a [String],
    /// Per-language CLI extras packed for planning.
    pub extras: crate::test_runner::language_keyed::LanguageKeyed<&'a [String]>,
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
        rust_llvm_cov::validate_rust_extra_args(req.extras.rust)?;
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
        test_args: req.extras,
        lang_filter: lang_filter.map(|l| match l {
            crate::test_git::TestLangFilter::Python => Language::Python,
            crate::test_git::TestLangFilter::Rust => Language::Rust,
        }),
        ignore: &ignore_norm,
        extra_direct_python: &[],
        extra_direct_rust: &[],
        include_prior_failures: true,
    })?;
    Ok(planned_from_selector_plan(
        repo_root,
        selector_plan,
        ignore_norm,
    ))
}
