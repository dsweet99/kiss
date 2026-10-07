use std::path::{Path, PathBuf};

use crate::test_runner::runners::enumerate_workspace_rust_selectors;
use crate::test_runner::test_selection::{ChangedDiff, LanguagePlanner, TestSelector};

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

    fn sources_changed(&self) -> bool {
        !self.rust_source_paths.is_empty()
    }
}

impl crate::test_runner::test_selection::SupportedLanguage for RustModule {
    fn language(&self) -> kiss::Language {
        kiss::Language::Rust
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_runner::test_selection::SelectionBasis;

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
        assert!(changed.sources_changed());
        assert_eq!(changed.selection_basis(), SelectionBasis::Population);
    }

    #[test]
    fn without_changed_sources_only_changed_tests_are_planned() {
        let tmp = tempfile::tempdir().unwrap();
        let unchanged = module(tmp.path(), &[]);
        assert!(!unchanged.sources_changed());
        assert_eq!(unchanged.selection_basis(), SelectionBasis::Current);
        assert_eq!(
            unchanged.changed_tests(&ChangedDiff::new(Vec::new())).len(),
            1
        );
    }
}
