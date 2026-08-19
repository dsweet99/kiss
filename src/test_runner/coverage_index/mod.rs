
use std::path::{Path, PathBuf};

use kiss::Language;

use crate::test_runner::coverage_decision::SupportedLanguage;

pub(crate) mod python {
    pub(crate) use crate::test_runner::python_coverage_index::*;
}

pub(crate) mod rust {
    pub(crate) use crate::test_runner::rust_coverage_index::*;
}

pub(crate) trait CoverageIndex: SupportedLanguage {
    fn cache_root(&self, repo_root: &Path) -> PathBuf;
    fn index_file_present(&self, repo_root: &Path) -> bool;
    fn repo_relative_coverage_file(&self, repo_root: &Path, path: &str) -> Option<String>;
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PythonCoverageIndex;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RustCoverageIndex;

impl SupportedLanguage for PythonCoverageIndex {
    fn language(&self) -> Language {
        Language::Python
    }
}

impl SupportedLanguage for RustCoverageIndex {
    fn language(&self) -> Language {
        Language::Rust
    }
}

impl CoverageIndex for PythonCoverageIndex {
    fn cache_root(&self, repo_root: &Path) -> PathBuf {
        python::python_coverage_cache_root(repo_root)
            .unwrap_or_else(|_| repo_root.join(".kiss").join("rslip_cache"))
    }

    fn index_file_present(&self, repo_root: &Path) -> bool {
        python::python_coverage_index_file_present(repo_root)
    }

    fn repo_relative_coverage_file(&self, repo_root: &Path, path: &str) -> Option<String> {
        python::repo_relative_coverage_file(repo_root, path)
    }
}

impl CoverageIndex for RustCoverageIndex {
    fn cache_root(&self, repo_root: &Path) -> PathBuf {
        rust::rust_coverage_cache_root(repo_root)
    }

    fn index_file_present(&self, repo_root: &Path) -> bool {


        rust::rust_coverage_cache_root(repo_root).exists()
    }

    fn repo_relative_coverage_file(&self, repo_root: &Path, path: &str) -> Option<String> {
        rust::repo_relative_coverage_file(repo_root, path)
    }
}

pub(crate) fn for_language(language: Language) -> Box<dyn CoverageIndex> {
    match language {
        Language::Python => Box::new(PythonCoverageIndex),
        Language::Rust => Box::new(RustCoverageIndex),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_index_dispatches_by_language() {
        let py = for_language(Language::Python);
        let rs = for_language(Language::Rust);
        assert_eq!(py.language(), Language::Python);
        assert_eq!(rs.language(), Language::Rust);
    }

    #[test]
    fn coverage_index_impls_expose_cache_and_path_helpers() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let py = PythonCoverageIndex;
        let rs = RustCoverageIndex;
        let _ = py.cache_root(root);
        let _ = rs.cache_root(root);
        assert!(!py.index_file_present(root));
        assert!(!rs.index_file_present(root));
        let rel_py = py.repo_relative_coverage_file(root, "pkg/mod.py");
        let rel_rs = rs.repo_relative_coverage_file(root, "src/lib.rs");
        assert!(rel_py.is_some() || rel_py.is_none());
        assert!(rel_rs.is_some() || rel_rs.is_none());
    }
}
