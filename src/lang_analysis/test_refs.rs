use std::path::Path;

use super::analysis::{LanguageAnalysis, PythonAnalysis, RustAnalysis};
use crate::rust_test_refs::is_rust_test_file;
use crate::test_refs::{is_in_test_directory, is_test_file};

pub trait LanguageTestRefs: LanguageAnalysis {
    fn is_test_path(&self, path: &Path) -> bool;
}

impl LanguageTestRefs for PythonAnalysis {
    fn is_test_path(&self, path: &Path) -> bool {
        is_test_file(path) || is_in_test_directory(path)
    }
}

impl LanguageTestRefs for RustAnalysis {
    fn is_test_path(&self, path: &Path) -> bool {
        is_rust_test_file(path)
    }
}
