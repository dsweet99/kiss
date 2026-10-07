use std::path::{Path, PathBuf};

use crate::test_runner::runners::enumerate_workspace_python_selectors;
use crate::test_runner::test_selection::{ChangedDiff, LanguagePlanner, TestSelector};

pub(crate) struct PythonBackerInput<'a> {
    pub(crate) repo_root: &'a Path,
    pub(crate) py_source_paths: &'a [PathBuf],
    pub(crate) test_args: &'a [String],
    pub(crate) ignore: &'a [String],
    pub(crate) changed_tests: &'a [TestSelector],
    pub(crate) prior_failures: &'a [TestSelector],
}

pub(crate) fn python_backer(input: PythonBackerInput<'_>) -> Box<dyn LanguagePlanner> {
    Box::new(PythonModule::new(input))
}

/// Plans Python tests. Kiss keeps no record of which tests reach which Python source, so
/// a changed Python source plans every Python test; tests whose records still hold are
/// then skipped by the runtime.
pub(crate) struct PythonModule {
    repo_root: PathBuf,
    py_source_paths: Vec<PathBuf>,
    test_args: Vec<String>,
    ignore: Vec<String>,
    changed_tests: Vec<TestSelector>,
    prior_failures: Vec<TestSelector>,
}

impl PythonModule {
    pub(crate) fn new(input: PythonBackerInput<'_>) -> Self {
        PythonModule {
            repo_root: input.repo_root.to_path_buf(),
            py_source_paths: input.py_source_paths.to_vec(),
            test_args: input.test_args.to_vec(),
            ignore: input.ignore.to_vec(),
            changed_tests: input.changed_tests.to_vec(),
            prior_failures: input.prior_failures.to_vec(),
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
        Self::new(PythonBackerInput {
            repo_root,
            py_source_paths: &[],
            test_args,
            ignore,
            changed_tests: &[],
            prior_failures: &[],
        })
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

    fn sources_changed(&self) -> bool {
        !self.py_source_paths.is_empty()
    }
}

impl crate::test_runner::test_selection::SupportedLanguage for PythonModule {
    fn language(&self) -> kiss::Language {
        kiss::Language::Python
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_runner::lang_python::collect::{
        full_suite_subprocess_collects_for_tests, reset_full_suite_subprocess_collects_for_tests,
        reset_python_collect_memo_for_tests,
    };
    use crate::test_runner::test_selection::SelectionBasis;
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
    fn changed_python_source_plans_the_whole_population() {
        let tmp = tempfile::tempdir().unwrap();
        let changed = TestSelector::new(kiss::Language::Python, "tests/test_app.py::test_changed");
        let prior = TestSelector::new(kiss::Language::Python, "tests/test_app.py::test_prior");
        let sources = [tmp.path().join("app.py")];
        let input = |py_source_paths| PythonBackerInput {
            repo_root: tmp.path(),
            py_source_paths,
            test_args: &[],
            ignore: &[],
            changed_tests: std::slice::from_ref(&changed),
            prior_failures: std::slice::from_ref(&prior),
        };
        let edited = PythonModule::new(input(&sources));
        assert!(edited.sources_changed());
        assert_eq!(edited.selection_basis(), SelectionBasis::Population);
        let unchanged = PythonModule::new(input(&[]));
        assert!(!unchanged.sources_changed());
        assert_eq!(unchanged.selection_basis(), SelectionBasis::Current);
        assert_eq!(
            unchanged.changed_tests(&ChangedDiff::new(Vec::new())),
            vec![changed]
        );
        assert_eq!(unchanged.prior_failures(), vec![prior]);
    }
}
