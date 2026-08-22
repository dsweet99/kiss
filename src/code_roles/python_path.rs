use std::path::Path;

use crate::test_refs::is_in_test_directory;

#[must_use]
pub fn has_python_test_filename(path: &Path) -> bool {
    let is_py = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("py"));
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| {
            is_py
                && (name.starts_with("test_")
                    || (name.len() > 8 && name[..name.len() - 3].ends_with("_test")))
        })
}

#[must_use]
pub fn is_default_pytest_collect_candidate(path: &Path) -> bool {
    has_python_test_filename(path)
}

#[must_use]
pub fn is_python_test_module_path(path: &Path) -> bool {
    is_conftest(path) || has_python_test_filename(path) || is_in_test_directory(path)
}

#[must_use]
pub fn is_conftest(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("conftest.py"))
}

#[cfg(test)]
mod python_path_test {
    use super::*;
    use std::path::Path;

    #[test]
    fn seeds_and_collect_candidates_differ_for_helpers() {
        assert!(is_python_test_module_path(Path::new("tests/helpers.py")));
        assert!(!is_default_pytest_collect_candidate(Path::new(
            "tests/helpers.py"
        )));
        assert!(is_python_test_module_path(Path::new("test_foo.py")));
        assert!(is_python_test_module_path(Path::new("foo_test.py")));
        assert!(is_python_test_module_path(Path::new("conftest.py")));
        assert!(!is_python_test_module_path(Path::new("src/foo.py")));
        assert!(!is_default_pytest_collect_candidate(Path::new(
            "conftest.py"
        )));
    }
}
