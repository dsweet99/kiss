use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::test_runner::coverage_decision::{
    ChangedDiff, CoverageFreshness, LanguagePlanner, PopulationPlan, SelectionDecision,
    TestSelector, full_population_plan,
};
use crate::test_runner::python_coverage_index::{
    PYTHON_COVERAGE_ENV_KEYS, python_population_environment_mismatch,
    python_population_manifest_is_current_for_args_with_env_keys,
    select_python_source_selectors_from_index, select_python_source_selectors_hybrid,
};
use crate::test_runner::runners::enumerate_workspace_python_selectors;

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

    #[cfg(test)]
    pub(crate) fn for_execution(repo_root: &Path, ignore: &[String]) -> Self {
        Self::for_execution_with_args(repo_root, ignore, &[])
    }

    pub(crate) fn for_execution_with_args(
        repo_root: &Path,
        ignore: &[String],
        test_args: &[String],
    ) -> Self {
        PythonModule {
            repo_root: repo_root.to_path_buf(),
            py_source_paths: Vec::new(),
            python_changed_lines: BTreeMap::new(),
            test_args: test_args.to_vec(),
            ignore: ignore.to_vec(),
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
            enumerate_workspace_python_selectors(&self.repo_root, &self.ignore, &self.test_args)?
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

        if python_population_environment_mismatch(
            &self.repo_root,
            &self.test_args,
            self.manifest_env_allowlist(),
        )
        .is_some()
        {
            return Ok(CoverageFreshness::Stale);
        }
        let universe_ids = universe
            .iter()
            .map(|selector| selector.id.clone())
            .collect::<Vec<_>>();
        let has_current_population = python_population_manifest_is_current_for_args_with_env_keys(
            &self.repo_root,
            &universe_ids,
            &self.test_args,
            self.manifest_env_allowlist(),
        )
            && crate::test_runner::python_coverage_index::load_current_python_coverage_index(
                &self.repo_root,
            )
            .is_some();
        let has_line_precise_entries = !self.python_changed_lines.is_empty()
            && crate::test_runner::python_coverage_index::python_index_covers_source_paths(
                &self.repo_root,
                &self.py_source_paths,
                &self.test_args,
            )
            && select_fresh_python_source_selectors(
                &self.repo_root,
                &self.py_source_paths,
                &self.python_changed_lines,
            )
            .is_some();
        if has_current_population {
            Ok(CoverageFreshness::Fresh)
        } else if has_line_precise_entries {
            Ok(CoverageFreshness::ReusablePrior)
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
            selectors: drop_ignored_python_selectors(selector_ids, &self.ignore),
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

fn drop_ignored_python_selectors(
    selector_ids: impl IntoIterator<Item = String>,
    ignore: &[String],
) -> Vec<TestSelector> {
    selector_ids
        .into_iter()
        .filter(|id| !kiss::selector_ignored_by_prefixes(id, ignore))
        .map(|id| TestSelector::new(kiss::Language::Python, id))
        .collect()
}

pub(crate) fn select_fresh_python_source_selectors(
    repo_root: &Path,
    py_source_paths: &[PathBuf],
    python_changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
) -> Option<BTreeSet<String>> {
    const LINE_PRECISE_FILE_LIMIT: usize = 1;
    if !python_changed_lines.is_empty()
        && python_changed_lines.len() <= LINE_PRECISE_FILE_LIMIT
        && let Some(line_selectors) =
            select_python_source_selectors_hybrid(repo_root, py_source_paths, python_changed_lines)
    {
        return Some(line_selectors);
    }
    select_python_source_selectors_from_index(repo_root, py_source_paths)
}

impl crate::test_runner::coverage_decision::SupportedLanguage for PythonModule {
    fn language(&self) -> kiss::Language {
        kiss::Language::Python
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_runner::coverage_decision::LanguagePlanner;
    use crate::test_runner::lang_python::collect::{
        full_suite_subprocess_collects_for_tests, reset_full_suite_subprocess_collects_for_tests,
        reset_python_collect_memo_for_tests,
    };
    use crate::test_runner::workspace_selector_cache::store_python_workspace_selectors;
    use std::fs;

    #[test]
    fn discover_universe_uses_workspace_selector_cache_without_pytest_collect() {
        reset_python_collect_memo_for_tests();
        reset_full_suite_subprocess_collects_for_tests();
        let tmp = tempfile::tempdir().unwrap();
        let tests = tmp.path().join("tests");
        fs::create_dir_all(&tests).unwrap();
        fs::write(
            tests.join("test_app.py"),
            "def test_value():\n    assert True\n",
        )
        .unwrap();
        let cached = vec![
            "tests/test_app.py::test_value".to_string(),
            "tests/test_other.py::test_cached_only".to_string(),
        ];
        assert!(store_python_workspace_selectors(
            tmp.path(),
            &[],
            &cached,
            &[]
        ));
        let before = full_suite_subprocess_collects_for_tests();
        let module = PythonModule::for_execution(tmp.path(), &[]);
        let discovered: Vec<String> = LanguagePlanner::discover_universe(&module)
            .unwrap()
            .into_iter()
            .map(|selector| selector.id)
            .collect();
        assert_eq!(discovered, cached);
        assert_eq!(full_suite_subprocess_collects_for_tests(), before);
    }

    #[test]
    #[allow(non_snake_case)]
    fn drop_ignored_python_selectors_skips_resources_paths() {
        let kept = drop_ignored_python_selectors(
            [
                "scripts/gen.py::test_ok".to_string(),
                "crates/ruff_linter/resources/test/fixtures/x.py::test_x".to_string(),
            ],
            &["resources".to_string()],
        );
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, "scripts/gen.py::test_ok");
    }

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
