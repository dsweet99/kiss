mod coverage_decision;
pub(crate) mod last_status;
mod line_selection;
mod python_coverage_index;
mod run_logic;
mod runners;
mod rust_coverage_index;
mod rust_llvm_cov;
mod validation;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Duration;

use kiss::Language;

use crate::test_git::TestChangeMode;
pub(crate) use run_logic::run_selectors;
pub use validation::ValidateSelectionCmdArgs;
#[cfg(test)]
pub(crate) use validation::ValidationReport;
pub(crate) use validation::validation_report;

pub struct RunTestCmdArgs<'a> {
    pub mode: TestChangeMode,
    pub main_branch_cli: Option<&'a str>,
    pub base_branch_cli: Option<&'a str>,
    pub dry_run: bool,
    pub force_rerun: bool,
    pub metrics: bool,
    pub jobs: usize,
    pub extra: &'a [String],
    pub ignore: &'a [String],
    pub lang_filter: Option<Language>,
    pub config_main_branch: Option<&'a str>,
}

pub fn run_test(a: RunTestCmdArgs<'_>) -> i32 {
    let RunTestCmdArgs {
        mode,
        main_branch_cli,
        base_branch_cli,
        dry_run,
        force_rerun,
        metrics,
        jobs,
        extra,
        ignore,
        lang_filter,
        config_main_branch,
    } = a;
    let plan_started = std::time::Instant::now();
    match plan_selectors(
        mode,
        main_branch_cli,
        base_branch_cli,
        ignore,
        extra,
        lang_filter,
        config_main_branch,
    ) {
        Ok(planned) => match run_selectors(
            &planned,
            SelectorRunOptions {
                dry_run,
                force_rerun,
                metrics,
                jobs,
                extra,
                plan_duration: plan_started.elapsed(),
            },
        ) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

pub fn validate_selection(a: ValidateSelectionCmdArgs<'_>) -> i32 {
    if let Err(e) = a.validate_dry_run_request() {
        eprintln!("{e}");
        return 2;
    }
    if a.fixture_name() == Some("tiny-recall") {
        return match validation::run_tiny_recall_fixture(&a) {
            Ok(report) => {
                report.print();
                i32::from(!report.has_full_recall())
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        };
    }
    match plan_selectors(
        a.change_mode(),
        a.main_branch_arg(),
        a.base_branch_arg(),
        a.planning_ignore_args(),
        a.planning_extra_args(),
        a.normalized_lang_filter(),
        a.config_main_branch,
    )
    .and_then(|planned| validation_report(&planned, a.normalized_lang_filter()))
    {
        Ok(report) => {
            report.print(a.dry_run);
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

pub(crate) struct PlannedSelectors {
    pub repo_root: PathBuf,
    pub py_sel: Vec<String>,
    pub rs_sel: Vec<String>,
    pub python_population_required: bool,
    pub python_population_selectors: Vec<String>,
    pub rust_source_paths: Vec<PathBuf>,
    pub rust_changed_lines: BTreeMap<PathBuf, BTreeSet<u32>>,
    pub rust_source_population_paths: Vec<PathBuf>,
    pub python_prior_failure_selectors: Vec<String>,
    pub rust_prior_failure_selectors: Vec<String>,
    pub coverage_decision_engine_used: bool,
    pub ignore: Vec<String>,
}

pub(crate) struct SelectorRunOptions<'a> {
    pub dry_run: bool,
    pub force_rerun: bool,
    pub metrics: bool,
    pub jobs: usize,
    pub extra: &'a [String],
    pub plan_duration: Duration,
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
            let Some(ref rev) = diff_target else {
                return Err("error: kiss test: internal error (missing diff target)".into());
            };
            crate::test_git::changed_paths_since(&repo_root, rev)?
        }
    };
    let rel_changed_lines = match mode {
        TestChangeMode::Commit => crate::test_git::changed_lines_commit(&repo_root)?,
        TestChangeMode::Base | TestChangeMode::Main => {
            let Some(ref rev) = diff_target else {
                return Err("error: kiss test: internal error (missing diff target)".into());
            };
            crate::test_git::changed_lines_since(&repo_root, rev)?
        }
    };
    let lang_filter = lang_filter.map(|l| match l {
        Language::Python => crate::test_git::TestLangFilter::Python,
        Language::Rust => crate::test_git::TestLangFilter::Rust,
    });
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
    Ok(PlannedSelectors {
        repo_root,
        py_sel: selector_plan.py_selectors,
        rs_sel: selector_plan.rust_selectors,
        python_population_required: selector_plan.python_population_required,
        python_population_selectors: selector_plan.python_population_selectors,
        rust_source_paths: selector_plan.rust_source_paths,
        rust_changed_lines: selector_plan.rust_changed_lines,
        rust_source_population_paths: selector_plan.rust_source_population_paths,
        python_prior_failure_selectors: selector_plan.python_prior_failure_selectors,
        rust_prior_failure_selectors: selector_plan.rust_prior_failure_selectors,
        coverage_decision_engine_used: selector_plan.coverage_decision_engine_used,
        ignore: ignore_norm,
    })
}

#[cfg(test)]
mod coverage_witness {
    use super::*;

    #[test]
    fn witness_validation_types() {
        let args = ValidateSelectionCmdArgs {
            mode: TestChangeMode::Commit,
            main_branch_cli: None,
            base_branch_cli: None,
            dry_run: true,
            jobs: 1,
            extra: &[],
            ignore: &[],
            lang_filter: None,
            fixture: None,
            config_main_branch: None,
        };
        assert!(args.validate_dry_run_request().is_ok());
        let report = ValidationReport {
            selected_python: 0,
            selected_rust: 0,
            full_python: 1,
            full_rust: 1,
            python_population_required: false,
            rust_population_required: false,
        };
        assert_eq!(report.selection_ratio(), Some(0.0));
        report.print(true);
    }
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod mod_test;

#[cfg(test)]
#[path = "mod_run_api_test.rs"]
mod mod_run_api_test;

#[cfg(test)]
#[path = "python_coverage_index_witness_test.rs"]
mod python_coverage_index_witness_test;

#[cfg(test)]
#[path = "runners_test.rs"]
mod runners_test;

#[cfg(test)]
#[path = "runners_request_test.rs"]
mod runners_request_test;
