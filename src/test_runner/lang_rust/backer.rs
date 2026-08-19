use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::test_runner::coverage_decision::{
    ChangedDiff, CoverageFreshness, LanguagePlanner, PopulationPlan, SelectionBasis,
    SelectionDecision, TestSelector, full_population_plan,
};
use crate::test_runner::rust_coverage_index::{
    RUST_COVERAGE_ENV_KEYS, ResolvedRustPopulation, resolve_rust_population_state,
    select_rust_source_selectors_for_basis,
};

use crate::test_runner::runners::enumerate_workspace_rust_selectors;

pub(crate) struct RustBackerInput<'a> {
    pub(crate) repo_root: &'a Path,
    pub(crate) rust_source_paths: &'a [PathBuf],
    pub(crate) rust_changed_lines: &'a BTreeMap<PathBuf, BTreeSet<u32>>,
    pub(crate) rust_test_args: &'a [String],
    pub(crate) ignore: &'a [String],
    pub(crate) changed_tests: &'a [TestSelector],
    pub(crate) prior_failures: &'a [TestSelector],
    pub(crate) resolved: Option<ResolvedRustPopulation>,
}

/// Build the Rust language planner (selection basis is on `LanguagePlanner`).
pub(crate) fn rust_llvm_cov_backer(input: RustBackerInput<'_>) -> Box<dyn LanguagePlanner> {
    Box::new(RustModule::new_with_resolved(input))
}

pub(crate) struct RustModule {
    repo_root: PathBuf,
    rust_source_paths: Vec<PathBuf>,
    rust_changed_lines: BTreeMap<PathBuf, BTreeSet<u32>>,
    rust_test_args: Vec<String>,
    ignore: Vec<String>,
    changed_tests: Vec<TestSelector>,
    prior_failures: Vec<TestSelector>,
    resolved: OnceLock<Result<ResolvedRustPopulation, String>>,
}

impl RustModule {
    #[cfg(test)]
    pub(crate) fn new(
        repo_root: &Path,
        rust_source_paths: &[PathBuf],
        rust_changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
        rust_test_args: &[String],
        ignore: &[String],
        changed_tests: &[TestSelector],
        prior_failures: &[TestSelector],
    ) -> Self {
        Self::new_with_resolved(RustBackerInput {
            repo_root,
            rust_source_paths,
            rust_changed_lines,
            rust_test_args,
            ignore,
            changed_tests,
            prior_failures,
            resolved: None,
        })
    }

    pub(crate) fn new_with_resolved(input: RustBackerInput<'_>) -> Self {
        let resolved_cell = OnceLock::new();
        if let Some(resolved) = input.resolved {
            let _ = resolved_cell.set(Ok(resolved));
        }
        RustModule {
            repo_root: input.repo_root.to_path_buf(),
            rust_source_paths: input.rust_source_paths.to_vec(),
            rust_changed_lines: input.rust_changed_lines.clone(),
            rust_test_args: input.rust_test_args.to_vec(),
            ignore: input.ignore.to_vec(),
            changed_tests: input.changed_tests.to_vec(),
            prior_failures: input.prior_failures.to_vec(),
            resolved: resolved_cell,
        }
    }

    pub(crate) fn for_execution(repo_root: &Path, ignore: &[String]) -> Self {
        RustModule {
            repo_root: repo_root.to_path_buf(),
            rust_source_paths: Vec::new(),
            rust_changed_lines: BTreeMap::new(),
            rust_test_args: Vec::new(),
            ignore: ignore.to_vec(),
            changed_tests: Vec::new(),
            prior_failures: Vec::new(),
            resolved: OnceLock::new(),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn population_manifest_selectors(&self) -> Result<Vec<String>, String> {
        enumerate_workspace_rust_selectors(&self.repo_root, &self.ignore)
    }


    fn resolved_state(&self) -> Result<&ResolvedRustPopulation, String> {
        self.resolved
            .get_or_init(|| {
                resolve_rust_population_state(
                    &self.repo_root,
                    &self.ignore,
                    &self.rust_source_paths,
                    &self.rust_test_args,
                )
            })
            .as_ref()
            .map_err(|err| err.clone())
    }
}

impl LanguagePlanner for RustModule {
    fn language(&self) -> kiss::Language {
        kiss::Language::Rust
    }

    fn discover_universe(&self) -> Result<Vec<TestSelector>, String> {
        if let Some((_py, cached_rs, _fp)) =
            crate::test_runner::workspace_selector_cache::load_cached_workspace_selectors(
                &self.repo_root,
                &self.ignore,
            )
        {
            return Ok(cached_rs
                .into_iter()
                .map(|id| TestSelector::new(kiss::Language::Rust, id))
                .collect());
        }
        Ok(
            enumerate_workspace_rust_selectors(&self.repo_root, &self.ignore)?
                .into_iter()
                .map(|id| TestSelector::new(kiss::Language::Rust, id))
                .collect(),
        )
    }

    fn changed_tests(&self, _diff: &ChangedDiff) -> Vec<TestSelector> {
        self.changed_tests.clone()
    }

    fn prior_failures(&self) -> Vec<TestSelector> {
        self.prior_failures.clone()
    }

    fn freshness(&self, _universe: &[TestSelector]) -> Result<CoverageFreshness, String> {
        if self.rust_source_paths.is_empty()
            && self.changed_tests.is_empty()
            && self.resolved.get().is_none()
        {
            return Ok(CoverageFreshness::Fresh);
        }
        let resolved = self.resolved_state()?;
        Ok(resolved.freshness())
    }

    fn population_plan(&self, universe: &[TestSelector]) -> PopulationPlan {
        full_population_plan(universe)
    }

    fn select(&self) -> Result<SelectionDecision, String> {
        let resolved = self.resolved_state()?;
        let selector_ids = select_rust_source_selectors_for_basis(
            &self.repo_root,
            &self.rust_source_paths,
            &self.rust_changed_lines,
            &self.rust_test_args,
            resolved,
        );
        let Some(selector_ids) = selector_ids else {
            return Ok(SelectionDecision {
                selectors: Vec::new(),
                complete: false,
            });
        };
        let selectors = selector_ids
            .into_iter()
            .map(|id| TestSelector::new(kiss::Language::Rust, id))
            .collect();
        Ok(SelectionDecision {
            selectors,
            complete: true,
        })
    }

    fn manifest_env_allowlist(&self) -> &'static [&'static str] {
        RUST_COVERAGE_ENV_KEYS
    }

    fn selection_basis(&self) -> SelectionBasis {
        if self.rust_source_paths.is_empty()
            && self.changed_tests.is_empty()
            && self.resolved.get().is_none()
        {
            return SelectionBasis::Current;
        }
        self.resolved_state()
            .map(ResolvedRustPopulation::basis)
            .unwrap_or(SelectionBasis::Current)
    }
}

#[cfg(test)]
pub(crate) fn select_fresh_rust_source_selectors(
    repo_root: &Path,
    rust_source_paths: &[PathBuf],
    rust_changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
    rust_test_args: &[String],
) -> Option<BTreeSet<String>> {
    let resolved =
        resolve_rust_population_state(repo_root, &[], rust_source_paths, rust_test_args).ok()?;
    select_rust_source_selectors_for_basis(
        repo_root,
        rust_source_paths,
        rust_changed_lines,
        rust_test_args,
        &resolved,
    )
}

impl crate::test_runner::coverage_decision::SupportedLanguage for RustModule {
    fn language(&self) -> kiss::Language {
        kiss::Language::Rust
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_runner::coverage_decision::CoverageFreshness;
    use rust_llvm_cov_runner::RustPopulationState;

    #[test]
    fn freshness_trusts_resolved_partial_current_population() {
        let tmp = tempfile::tempdir().unwrap();
        let resolved = ResolvedRustPopulation::Current {
            state: RustPopulationState {
                input_fingerprint: "input".to_string(),
                generation_fingerprint: "generation".to_string(),
                selection_context_fingerprint: "selection".to_string(),
                entries_fingerprint: "entries".to_string(),
                selectors: vec!["tests::selected_by_changed_source".to_string()],
                line_index: BTreeMap::new(),
                ordinary_source_digests: BTreeMap::new(),
                test_binaries: BTreeMap::new(),
            },
        };
        let module = RustModule::new_with_resolved(RustBackerInput {
            repo_root: tmp.path(),
            rust_source_paths: &[tmp.path().join("src").join("lib.rs")],
            rust_changed_lines: &BTreeMap::new(),
            rust_test_args: &[],
            ignore: &[],
            changed_tests: &[],
            prior_failures: &[],
            resolved: Some(resolved),
        });
        let universe = [TestSelector::new(
            kiss::Language::Rust,
            "tests::full_universe_member",
        )];

        assert_eq!(
            module.freshness(&universe).unwrap(),
            CoverageFreshness::Fresh
        );
    }
}
