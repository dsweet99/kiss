use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::test_runner::coverage_decision::{
    ChangedDiff, CoverageFreshness, LanguagePlanner, PopulationPlan, SelectionDecision,
    TestSelector, full_population_plan,
};
use crate::test_runner::rust_coverage_index::{
    RUST_COVERAGE_ENV_KEYS, load_current_rust_population_state,
    select_rust_source_selectors_from_index, select_rust_source_selectors_hybrid,
};

use super::enumerate_workspace_rust_selectors;

pub(crate) fn rust_llvm_cov_backer(
    repo_root: &Path,
    rust_source_paths: &[PathBuf],
    rust_changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
    rust_test_args: &[String],
    ignore: &[String],
    changed_tests: &[TestSelector],
    prior_failures: &[TestSelector],
) -> Box<dyn LanguagePlanner> {
    Box::new(RustModule::new(
        repo_root,
        rust_source_paths,
        rust_changed_lines,
        rust_test_args,
        ignore,
        changed_tests,
        prior_failures,
    ))
}

pub(crate) struct RustModule {
    repo_root: PathBuf,
    rust_source_paths: Vec<PathBuf>,
    rust_changed_lines: BTreeMap<PathBuf, BTreeSet<u32>>,
    rust_test_args: Vec<String>,
    ignore: Vec<String>,
    changed_tests: Vec<TestSelector>,
    prior_failures: Vec<TestSelector>,
}

impl RustModule {
    pub(crate) fn new(
        repo_root: &Path,
        rust_source_paths: &[PathBuf],
        rust_changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
        rust_test_args: &[String],
        ignore: &[String],
        changed_tests: &[TestSelector],
        prior_failures: &[TestSelector],
    ) -> Self {
        RustModule {
            repo_root: repo_root.to_path_buf(),
            rust_source_paths: rust_source_paths.to_vec(),
            rust_changed_lines: rust_changed_lines.clone(),
            rust_test_args: rust_test_args.to_vec(),
            ignore: ignore.to_vec(),
            changed_tests: changed_tests.to_vec(),
            prior_failures: prior_failures.to_vec(),
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
        }
    }

    pub(crate) fn population_manifest_selectors(&self) -> Result<Vec<String>, String> {
        enumerate_workspace_rust_selectors(&self.repo_root, &self.ignore)
    }
}

impl LanguagePlanner for RustModule {
    fn language(&self) -> kiss::Language {
        kiss::Language::Rust
    }

    fn discover_universe(&self) -> Result<Vec<TestSelector>, String> {
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

    fn freshness(&self, universe: &[TestSelector]) -> Result<CoverageFreshness, String> {
        if self.rust_source_paths.is_empty() {
            return Ok(CoverageFreshness::Fresh);
        }
        let universe_ids = universe
            .iter()
            .map(|selector| selector.id.clone())
            .collect::<Vec<_>>();
        if load_current_rust_population_state(
            &self.repo_root,
            Some(&universe_ids),
            &self.rust_test_args,
        )
        .is_some()
        {
            Ok(CoverageFreshness::Fresh)
        } else {
            Ok(CoverageFreshness::Stale)
        }
    }

    fn population_plan(&self, universe: &[TestSelector]) -> PopulationPlan {
        full_population_plan(universe)
    }

    fn select(&self) -> Result<SelectionDecision, String> {
        let Some(selector_ids) = select_fresh_rust_source_selectors(
            &self.repo_root,
            &self.rust_source_paths,
            &self.rust_changed_lines,
            &self.rust_test_args,
        ) else {
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
}

pub(crate) fn select_fresh_rust_source_selectors(
    repo_root: &Path,
    rust_source_paths: &[PathBuf],
    rust_changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
    rust_test_args: &[String],
) -> Option<BTreeSet<String>> {
    if !rust_changed_lines.is_empty()
        && let Some(line_selectors) = select_rust_source_selectors_hybrid(
            repo_root,
            rust_source_paths,
            rust_changed_lines,
            rust_test_args,
        )
    {
        return Some(line_selectors);
    }
    select_rust_source_selectors_from_index(repo_root, rust_source_paths, rust_test_args)
}
