pub(crate) mod detection;

pub use detection::{is_in_test_directory, is_pytest_nodeid_source_file, is_test_file};

#[cfg(test)]
mod detection_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_file_naming_patterns() {
        assert!(is_test_file(Path::new("tests/test_foo.py")));
        assert!(is_test_file(Path::new("foo_test.py")));
        assert!(!is_test_file(Path::new("src/foo.py")));
    }

    #[test]
    fn test_directory_detection() {
        assert!(is_in_test_directory(Path::new("tests/helpers/util.py")));
        assert!(!is_in_test_directory(Path::new("src/helpers/util.py")));
    }

    #[test]
    fn pytest_nodeid_source_files_exclude_conftest_and_helpers() {
        assert!(is_pytest_nodeid_source_file(Path::new("tests/test_foo.py")));
        assert!(is_pytest_nodeid_source_file(Path::new("foo_test.py")));
        assert!(!is_pytest_nodeid_source_file(Path::new(
            "tests/conftest.py"
        )));
        assert!(!is_pytest_nodeid_source_file(Path::new(
            "tests/fast/run_one_test_helpers.py"
        )));
        assert!(!is_pytest_nodeid_source_file(Path::new(
            "tests/fast/cogneato/gp.py"
        )));
    }
}
