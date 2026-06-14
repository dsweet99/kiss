use std::fs;
use std::path::Path;

use crate::FileRole;
use crate::discovery::classify_python;
use tempfile::TempDir;

fn write(path: &Path, contents: &str) {
    let parent = path.parent().expect("test file path should have a parent");
    fs::create_dir_all(parent).expect("test directory should be creatable");
    fs::write(path, contents).expect("test file should be writable");
}

#[test]
fn discovery_helpers_are_directly_callable_from_tests() {
    for path in [
        Path::new("test_api.py"),
        Path::new("api_test.py"),
        Path::new("conftest.py"),
    ] {
        let recognized = crate::discovery::is_test_file(path);
        assert!(recognized, "{} should be a test file", path.display());
    }
    for path in [Path::new("api.py"), Path::new("test_api.rs")] {
        let recognized = crate::discovery::is_test_file(path);
        assert!(
            !recognized,
            "{} should not be a Python test file",
            path.display()
        );
    }
    for path in [
        Path::new("pkg/tests/api.py"),
        Path::new("pkg/test/helpers.py"),
    ] {
        let in_test_dir = crate::discovery::is_in_test_directory(path);
        assert!(
            in_test_dir,
            "{} should be in a test directory",
            path.display()
        );
    }
    let source_role = classify_python(Path::new("pkg/api.py"));
    let test_role = classify_python(Path::new("pkg/tests/api.py"));

    assert_eq!(source_role, FileRole::Source);
    assert_eq!(test_role, FileRole::Test);
}

#[test]
fn discovery_test_names_are_case_insensitive_but_extension_checked() {
    for path in [
        Path::new("TEST_API.PY"),
        Path::new("api_TEST.Py"),
        Path::new("CONFTEST.PY"),
    ] {
        let recognized = crate::discovery::is_test_file(path);
        assert!(
            recognized,
            "{} should be a Python test file",
            path.display()
        );
    }

    for path in [Path::new("test_api.txt"), Path::new("contest.py")] {
        let recognized = crate::discovery::is_test_file(path);
        assert!(
            !recognized,
            "{} should not be a Python test file",
            path.display()
        );
    }
}

#[test]
fn discovery_test_directories_match_whole_components() {
    let nested_tests = crate::discovery::is_in_test_directory(Path::new("pkg/tests/unit/api.py"));
    let nested_test = crate::discovery::is_in_test_directory(Path::new("pkg/test/helpers.py"));
    assert!(nested_tests);
    assert!(nested_test);

    for path in [
        Path::new("pkg/latest/api.py"),
        Path::new("pkg/mytests/api.py"),
        Path::new("pkg/testdata/api.py"),
    ] {
        let in_test_dir = crate::discovery::is_in_test_directory(path);
        assert!(
            !in_test_dir,
            "{} should not be considered inside a test directory",
            path.display()
        );
    }
}

#[test]
fn discovery_does_not_globally_ignore_fake_or_fixtures_names() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("fake_app.py"), "VALUE = 1\n");
    write(&tmp.path().join("fixtures/helpers.py"), "HELPER = 1\n");

    let files = crate::discovery::discover_repo_files(tmp.path()).unwrap();
    let paths: Vec<_> = files.iter().map(|file| file.path.as_str()).collect();

    assert!(paths.contains(&"fake_app.py"));
    assert!(paths.contains(&"fixtures/helpers.py"));
}

#[test]
fn discovery_uses_kissignore_for_fixture_boundaries() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join(".kissignore"), "tests/fake_*/\n");
    write(&tmp.path().join("tests/fake_python/bad.py"), "BAD = 1\n");
    write(
        &tmp.path().join("tests/fixtures/python/app.py"),
        "APP = 1\n",
    );

    let files = crate::discovery::discover_repo_files(tmp.path()).unwrap();
    let paths: Vec<_> = files.iter().map(|file| file.path.as_str()).collect();

    assert!(paths.contains(&".kissignore"));
    assert!(!paths.contains(&"tests/fake_python/bad.py"));
    assert!(paths.contains(&"tests/fixtures/python/app.py"));
}

#[test]
fn top_level_discovery_excludes_embedded_fixture_projects() {
    let tmp = TempDir::new().unwrap();
    write(
        &tmp.path().join(".kissignore"),
        "tests/fixtures/\ntests/fake_python/\n",
    );
    write(&tmp.path().join("src/app.py"), "def app():\n    return 1\n");
    write(
        &tmp.path().join("tests/test_app.py"),
        "from src.app import app\n\ndef test_app():\n    assert app() == 1\n",
    );
    write(
        &tmp.path()
            .join("tests/fixtures/mv/python/simple_package/pkg/caller.py"),
        "def run():\n    return 1\n",
    );
    write(
        &tmp.path()
            .join("tests/fixtures/mv/python/simple_package/tests/test_pkg.py"),
        "from pkg.caller import run\n\ndef test_run():\n    assert run() == 1\n",
    );
    write(
        &tmp.path().join("tests/fake_python/api_handler_test.py"),
        "def test_fake_fixture():\n    assert True\n",
    );

    let top_level = crate::discovery::discover_repo_files(tmp.path()).unwrap();
    let top_level_paths: Vec<_> = top_level.iter().map(|file| file.path.as_str()).collect();

    assert!(top_level_paths.contains(&"src/app.py"));
    assert!(top_level_paths.contains(&"tests/test_app.py"));
    assert!(top_level_paths.contains(&".kissignore"));
    assert!(!top_level_paths.contains(&"tests/fixtures/mv/python/simple_package/pkg/caller.py"));
    assert!(!top_level_paths.contains(&"tests/fixtures/mv/python/simple_package/tests/test_pkg.py"));
    assert!(!top_level_paths.contains(&"tests/fake_python/api_handler_test.py"));

    let fixture_root = tmp.path().join("tests/fixtures/mv/python/simple_package");
    let fixture_project = crate::discovery::discover_repo_files(&fixture_root).unwrap();
    let fixture_paths: Vec<_> = fixture_project.iter().map(|file| file.path.as_str()).collect();

    assert!(fixture_paths.contains(&"pkg/caller.py"));
    assert!(fixture_paths.contains(&"tests/test_pkg.py"));
}
