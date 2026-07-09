use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::enumerate_workspace_python_selectors;
use crate::test_runner::coverage_decision::{
    ChangedDiff, CoverageFreshness, LanguagePlanner, PopulationPlan, SelectionDecision,
    TestSelector, full_population_plan,
};
use crate::test_runner::python_coverage_index::{
    PYTHON_COVERAGE_ENV_KEYS, python_population_manifest_is_current_for_args_with_env_keys,
    select_python_source_selectors_from_index, select_python_source_selectors_hybrid,
};

pub(crate) struct PythonModule {
    repo_root: PathBuf,
    py_source_paths: Vec<PathBuf>,
    python_changed_lines: BTreeMap<PathBuf, BTreeSet<u32>>,
    test_args: Vec<String>,
    ignore: Vec<String>,
    changed_tests: Vec<TestSelector>,
    prior_failures: Vec<TestSelector>,
}

impl PythonModule {
    pub(crate) fn new(
        repo_root: &Path,
        py_source_paths: &[PathBuf],
        python_changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
        test_args: &[String],
        ignore: &[String],
        changed_tests: &[TestSelector],
        prior_failures: &[TestSelector],
    ) -> Self {
        PythonModule {
            repo_root: repo_root.to_path_buf(),
            py_source_paths: py_source_paths.to_vec(),
            python_changed_lines: python_changed_lines.clone(),
            test_args: test_args.to_vec(),
            ignore: ignore.to_vec(),
            changed_tests: changed_tests.to_vec(),
            prior_failures: prior_failures.to_vec(),
        }
    }

    pub(crate) fn for_execution() -> Self {
        PythonModule {
            repo_root: PathBuf::new(),
            py_source_paths: Vec::new(),
            python_changed_lines: BTreeMap::new(),
            test_args: Vec::new(),
            ignore: Vec::new(),
            changed_tests: Vec::new(),
            prior_failures: Vec::new(),
        }
    }
}

impl LanguagePlanner for PythonModule {
    fn language(&self) -> kiss::Language {
        kiss::Language::Python
    }

    fn discover_universe(&self) -> Result<Vec<TestSelector>, String> {
        Ok(
            enumerate_workspace_python_selectors(&self.repo_root, &self.ignore)?
                .into_iter()
                .map(|id| TestSelector::new(kiss::Language::Python, id))
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
        if self.py_source_paths.is_empty() {
            return Ok(CoverageFreshness::Fresh);
        }
        let universe_ids = universe
            .iter()
            .map(|selector| selector.id.clone())
            .collect::<Vec<_>>();
        if python_population_manifest_is_current_for_args_with_env_keys(
            &self.repo_root,
            &universe_ids,
            &self.test_args,
            self.manifest_env_allowlist(),
        ) {
            Ok(CoverageFreshness::Fresh)
        } else {
            Ok(CoverageFreshness::Stale)
        }
    }

    fn population_plan(&self, universe: &[TestSelector]) -> PopulationPlan {
        full_population_plan(universe)
    }

    fn select(&self) -> Result<SelectionDecision, String> {
        let Some(selector_ids) = select_fresh_python_source_selectors(
            &self.repo_root,
            &self.py_source_paths,
            &self.python_changed_lines,
        ) else {
            return Ok(SelectionDecision {
                selectors: Vec::new(),
                complete: false,
            });
        };
        Ok(SelectionDecision {
            selectors: selector_ids
                .into_iter()
                .map(|id| TestSelector::new(kiss::Language::Python, id))
                .collect(),
            complete: true,
        })
    }

    fn manifest_env_allowlist(&self) -> &'static [&'static str] {
        PYTHON_COVERAGE_ENV_KEYS
    }
}

pub(crate) fn python_population_backer(
    repo_root: &Path,
    py_source_paths: &[PathBuf],
    python_changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
    test_args: &[String],
    ignore: &[String],
    changed_tests: &[TestSelector],
    prior_failures: &[TestSelector],
) -> Box<dyn LanguagePlanner> {
    Box::new(PythonModule::new(
        repo_root,
        py_source_paths,
        python_changed_lines,
        test_args,
        ignore,
        changed_tests,
        prior_failures,
    ))
}

pub(crate) fn select_fresh_python_source_selectors(
    repo_root: &Path,
    py_source_paths: &[PathBuf],
    python_changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
) -> Option<BTreeSet<String>> {
    if !python_changed_lines.is_empty()
        && let Some(line_selectors) =
            select_python_source_selectors_hybrid(repo_root, py_source_paths, python_changed_lines)
    {
        return Some(line_selectors);
    }
    select_python_source_selectors_from_index(repo_root, py_source_paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(non_snake_case)]
    fn PythonModule_struct_literal_exposes_static_policy() {
        let tmp = tempfile::tempdir().unwrap();
        let changed = TestSelector::new(kiss::Language::Python, "tests/test_app.py::test_changed");
        let prior = TestSelector::new(kiss::Language::Python, "tests/test_app.py::test_prior");
        let module = PythonModule {
            repo_root: tmp.path().to_path_buf(),
            py_source_paths: Vec::new(),
            python_changed_lines: BTreeMap::new(),
            test_args: Vec::new(),
            ignore: Vec::new(),
            changed_tests: vec![changed.clone()],
            prior_failures: vec![prior.clone()],
        };

        assert_eq!(module.language(), kiss::Language::Python);
        assert_eq!(
            module.changed_tests(&ChangedDiff::new(Vec::new())),
            vec![changed]
        );
        assert_eq!(module.prior_failures(), vec![prior]);
        assert_eq!(module.manifest_env_allowlist(), PYTHON_COVERAGE_ENV_KEYS);
    }
}
