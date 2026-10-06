use std::path::{Path, PathBuf};

use crate::test_runner::coverage_decision::{
    ChangedDiff, CoverageFreshness, LanguagePlanner, PopulationPlan, SelectionBasis,
    SelectionDecision, TestSelector, full_population_plan,
};
use crate::test_runner::runners::enumerate_workspace_rust_selectors;

pub(crate) struct RustBackerInput<'a> {
    pub(crate) repo_root: &'a Path,
    pub(crate) rust_source_paths: &'a [PathBuf],
    pub(crate) ignore: &'a [String],
    pub(crate) changed_tests: &'a [TestSelector],
    pub(crate) prior_failures: &'a [TestSelector],
}

pub(crate) fn rust_backer(input: RustBackerInput<'_>) -> Box<dyn LanguagePlanner> {
    Box::new(RustModule::new(input))
}

/// Plans Rust tests. Kiss keeps no record of which tests reach which Rust source, so a
/// changed Rust source plans every Rust test; tests whose records still hold are then
/// skipped by the runtime.
pub(crate) struct RustModule {
    repo_root: PathBuf,
    rust_source_paths: Vec<PathBuf>,
    ignore: Vec<String>,
    changed_tests: Vec<TestSelector>,
    prior_failures: Vec<TestSelector>,
}

impl RustModule {
    pub(crate) fn new(input: RustBackerInput<'_>) -> Self {
        RustModule {
            repo_root: input.repo_root.to_path_buf(),
            rust_source_paths: input.rust_source_paths.to_vec(),
            ignore: input.ignore.to_vec(),
            changed_tests: input.changed_tests.to_vec(),
            prior_failures: input.prior_failures.to_vec(),
        }
    }

    pub(crate) fn for_execution(repo_root: &Path, ignore: &[String]) -> Self {
        Self::new(RustBackerInput {
            repo_root,
            rust_source_paths: &[],
            ignore,
            changed_tests: &[],
            prior_failures: &[],
        })
    }
}

impl LanguagePlanner for RustModule {
    fn language(&self) -> kiss::Language {
        kiss::Language::Rust
    }

    fn discover_universe(&self) -> Result<Vec<TestSelector>, String> {
        use crate::test_runner::workspace_selector_cache as selector_cache;
        let ids = match selector_cache::load_cached_rust_workspace_selectors(
            &self.repo_root,
            &self.ignore,
        ) {
            Some(cached) => cached,
            None => {
                let ids = enumerate_workspace_rust_selectors(&self.repo_root, &self.ignore)?;
                selector_cache::store_rust_workspace_selectors(&self.repo_root, &self.ignore, &ids);
                ids
            }
        };
        Ok(ids
            .into_iter()
            .map(|id| TestSelector::new(kiss::Language::Rust, id))
            .collect())
    }

    fn changed_tests(&self, _diff: &ChangedDiff) -> Vec<TestSelector> {
        self.changed_tests.clone()
    }

    fn prior_failures(&self) -> Vec<TestSelector> {
        self.prior_failures.clone()
    }

    fn freshness(&self, _universe: &[TestSelector]) -> Result<CoverageFreshness, String> {
        Ok(if self.rust_source_paths.is_empty() {
            CoverageFreshness::Fresh
        } else {
            CoverageFreshness::Stale
        })
    }

    fn population_plan(&self, universe: &[TestSelector]) -> PopulationPlan {
        full_population_plan(universe)
    }

    fn select(&self) -> Result<SelectionDecision, String> {
        Ok(SelectionDecision {
            selectors: Vec::new(),
            complete: true,
        })
    }

    fn manifest_env_allowlist(&self) -> &'static [&'static str] {
        super::nextest::RUST_IDENTITY_ENV_KEYS
    }

    fn selection_basis(&self) -> SelectionBasis {
        if self.rust_source_paths.is_empty() {
            SelectionBasis::Current
        } else {
            SelectionBasis::Population
        }
    }
}

impl crate::test_runner::coverage_decision::SupportedLanguage for RustModule {
    fn language(&self) -> kiss::Language {
        kiss::Language::Rust
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(root: &Path, sources: &[PathBuf]) -> RustModule {
        RustModule::new(RustBackerInput {
            repo_root: root,
            rust_source_paths: sources,
            ignore: &[],
            changed_tests: &[TestSelector::new(kiss::Language::Rust, "t::changed")],
            prior_failures: &[],
        })
    }

    #[test]
    fn changed_rust_source_plans_the_whole_population() {
        let tmp = tempfile::tempdir().unwrap();
        let changed = module(tmp.path(), &[tmp.path().join("src/lib.rs")]);
        assert_eq!(changed.freshness(&[]).unwrap(), CoverageFreshness::Stale);
        assert_eq!(changed.selection_basis(), SelectionBasis::Population);
    }

    #[test]
    fn without_changed_sources_only_changed_tests_are_planned() {
        let tmp = tempfile::tempdir().unwrap();
        let unchanged = module(tmp.path(), &[]);
        assert_eq!(unchanged.freshness(&[]).unwrap(), CoverageFreshness::Fresh);
        assert_eq!(unchanged.selection_basis(), SelectionBasis::Current);
        let decision = unchanged.select().unwrap();
        assert!(decision.complete && decision.selectors.is_empty());
        assert_eq!(
            unchanged.changed_tests(&ChangedDiff::new(Vec::new())).len(),
            1
        );
    }
}
