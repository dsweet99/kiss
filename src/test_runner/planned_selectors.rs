//! Planned selector and run-option types for `kiss test`.

use std::path::PathBuf;
use std::time::Duration;

use kiss::Language;

use crate::bin_cli::args::TestInvocation;

use super::RunTestCmdArgs;

#[derive(Clone)]
pub(crate) struct PlannedSelectors {
    pub repo_root: PathBuf,
    /// Planned selectors keyed by language (not parallel py_sel/rs_sel product fields).
    pub sel: crate::test_runner::language_keyed::LanguageKeyed<Vec<String>>,
    pub population_required: crate::test_runner::language_keyed::LanguageKeyed<bool>,
    /// Per-language source paths involved in this plan.
    pub source_paths: crate::test_runner::language_keyed::LanguageKeyed<Vec<PathBuf>>,
    pub vcs_source_paths: crate::test_runner::language_keyed::LanguageKeyed<usize>,
    pub snapshot_delta_modified: crate::test_runner::language_keyed::LanguageKeyed<usize>,
    pub snapshot_delta_structural: crate::test_runner::language_keyed::LanguageKeyed<bool>,
    pub prior_failure_selectors: crate::test_runner::language_keyed::LanguageKeyed<Vec<String>>,
    pub coverage_decision_engine_used: bool,
    /// Per-language selection basis from each `LanguagePlanner`.
    pub selection_basis: crate::test_runner::language_keyed::LanguageKeyed<
        crate::test_runner::coverage_decision::SelectionBasis,
    >,
    pub ignore: Vec<String>,
    /// Workspace py/rs fingerprint from selector-cache planning (warm seal fast path).
    pub workspace_files_fingerprint: Option<String>,
    /// Pure explicit test operands: skip full index rebuild after selective cache miss.
    pub skip_index_rebuild_after_selective:
        crate::test_runner::language_keyed::LanguageKeyed<bool>,
}

pub(crate) struct SelectorRunOptions<'a> {
    pub dry_run: bool,
    pub force_rerun: bool,
    pub metrics: bool,
    pub jobs: usize,
    /// Per-language CLI extras (pytest plugins / cargo test args).
    pub extras: crate::test_runner::language_keyed::LanguageKeyed<&'a [String]>,
    pub plan_duration: Duration,
    /// Same session gate the CLI loaded for this `kiss test` invocation.
    pub gate: kiss::GateConfig,
}

pub(crate) fn should_force_cold_initialization(
    a: &RunTestCmdArgs<'_>,
    repo_root: &std::path::Path,
) -> bool {
    matches!(a.invocation, TestInvocation::Base | TestInvocation::Main)
        && !a.dry_run
        && !a.force_rerun
        && !a.metrics
        && a.extra.is_empty()
        && a.ignore.is_empty()
        && a.lang_filter.is_none()
        && !repo_root.join(".kiss").exists()
}

pub(crate) fn apply_cold_initialization_population(
    a: &RunTestCmdArgs<'_>,
    planned: &mut PlannedSelectors,
) {
    if !should_force_cold_initialization(a, &planned.repo_root) {
        return;
    }
    match a.lang_filter {
        Some(Language::Python) => planned.population_required.python = true,
        Some(Language::Rust) => planned.population_required.rust = true,
        None => {
            planned.population_required.python = true;
            planned.population_required.rust = true;
        }
    }
}

/// `kiss test . --force` requires a full population phase, even when the prior
/// generation is current. Selective invocations (`Targets`, `Commit`, `Base`,
/// `Main`) keep their existing selector plan; `--force` only bypasses caches
/// for those planned selectors.
pub(crate) fn apply_force_all_population(a: &RunTestCmdArgs<'_>, planned: &mut PlannedSelectors) {
    if !a.force_rerun {
        return;
    }
    if !matches!(a.invocation, TestInvocation::All) {
        return;
    }
    match a.lang_filter {
        Some(Language::Python) => {
            if !planned.sel.python.is_empty() {
                planned.population_required.python = true;
            }
        }
        Some(Language::Rust) => {
            if !planned.sel.rust.is_empty() {
                planned.population_required.rust = true;
            }
        }
        None => {
            if !planned.sel.python.is_empty() {
                planned.population_required.python = true;
            }
            if !planned.sel.rust.is_empty() {
                planned.population_required.rust = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bin_cli::args::TestInvocation;
    use crate::test_runner::test_mode_fixtures::empty_planned_selectors;

    fn args(invocation: TestInvocation, force: bool, lang: Option<Language>) -> RunTestCmdArgs<'static> {
        RunTestCmdArgs {
            invocation,
            main_branch_cli: None,
            base_branch_cli: None,
            dry_run: false,
            force_rerun: force,
            force_bad: false,
            metrics: false,
            jobs: 1,
            extra: &[],
            python_extra: &[],
            ignore: &[],
            lang_filter: lang,
            config_main_branch: None,
            gate_config: kiss::GateConfig::default(),
        }
    }

    #[test]
    fn force_and_cold_helpers_cover_language_branches() {
        let tmp = tempfile::tempdir().unwrap();
        let mut planned = empty_planned_selectors(tmp.path().to_path_buf());
        planned.sel.python = vec!["t.py::test_a".into()];
        planned.sel.rust = vec!["crate::t".into()];

        let cold = args(TestInvocation::Base, false, None);
        assert!(should_force_cold_initialization(&cold, tmp.path()));
        apply_cold_initialization_population(&cold, &mut planned);
        assert!(planned.population_required.python);
        assert!(planned.population_required.rust);

        planned.population_required.python = false;
        planned.population_required.rust = false;
        apply_force_all_population(&args(TestInvocation::All, true, Some(Language::Python)), &mut planned);
        assert!(planned.population_required.python);
        apply_force_all_population(&args(TestInvocation::All, true, Some(Language::Rust)), &mut planned);
        assert!(planned.population_required.rust);
        planned.population_required.python = false;
        planned.population_required.rust = false;
        apply_force_all_population(&args(TestInvocation::All, true, None), &mut planned);
        assert!(planned.population_required.python && planned.population_required.rust);
        apply_force_all_population(&args(TestInvocation::Commit, true, None), &mut planned);
    }
}
