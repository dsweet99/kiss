use std::path::Path;

#[test]
fn detection_helpers_are_directly_callable_from_sibling_test_module() {
    for path in [
        Path::new("test_widget.py"),
        Path::new("widget_test.py"),
        Path::new("CONFTEST.PY"),
    ] {
        let recognized = super::detection::is_test_file(path);
        assert!(
            recognized,
            "{} should be a Python test file",
            path.display()
        );
    }
    for path in [Path::new("test_widget.rs"), Path::new("contest.py")] {
        let recognized = super::detection::is_test_file(path);
        assert!(
            !recognized,
            "{} should not be a Python test file",
            path.display()
        );
    }
    for path in [
        Path::new("pkg/tests/test_widget.py"),
        Path::new("pkg/test/helpers.py"),
    ] {
        let in_test_dir = super::detection::is_in_test_directory(path);
        assert!(
            in_test_dir,
            "{} should be in a test directory",
            path.display()
        );
    }

    let in_latest_dir = super::detection::is_in_test_directory(Path::new("pkg/latest/helpers.py"));
    assert!(!in_latest_dir);
}

#[test]
fn detection_test_names_require_python_extension_except_conftest_case() {
    let test_prefix = super::detection::has_python_test_naming(Path::new("tests/test_widget.py"));
    let test_suffix = super::detection::has_python_test_naming(Path::new("tests/widget_test.py"));
    let conftest = super::detection::has_python_test_naming(Path::new("tests/CONFTEST.PY"));
    assert!(test_prefix);
    assert!(test_suffix);
    assert!(conftest);

    for path in [
        Path::new("tests/test_widget.rs"),
        Path::new("tests/widget_tests.py"),
        Path::new("tests/conftest.txt"),
    ] {
        let recognized = super::detection::has_python_test_naming(path);
        assert!(
            !recognized,
            "{} should not match pytest file naming",
            path.display()
        );
    }
}

#[test]
fn detection_test_directories_match_whole_components_only() {
    let tests_dir = super::detection::is_in_test_directory(Path::new("pkg/tests/test_widget.py"));
    let test_dir = super::detection::is_in_test_directory(Path::new("pkg/test/helpers.py"));
    assert!(tests_dir);
    assert!(test_dir);

    for path in [
        Path::new("pkg/latest/helpers.py"),
        Path::new("pkg/mytests/helpers.py"),
        Path::new("pkg/testdata/helpers.py"),
    ] {
        let in_test_dir = super::detection::is_in_test_directory(path);
        assert!(
            !in_test_dir,
            "{} should not be in a test directory",
            path.display()
        );
    }
}
