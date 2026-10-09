use std::path::{Path, PathBuf};

use super::config::configured_python_filename_patterns;
use super::{is_collect_candidate, python_files_under, workspace_python_collect_paths};

#[test]
fn pytest_ini_python_files_replaces_the_default_names() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("pytest.ini"),
        "[pytest]\npython_files =\n    check_*.py\n    extra_?.py\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("pyproject.toml"),
        "[tool.pytest.ini_options]\npython_files = [\"other_*.py\"]\n",
    )
    .unwrap();
    let patterns = configured_python_filename_patterns(tmp.path()).unwrap();
    assert_eq!(
        patterns,
        vec!["check_*.py".to_string(), "extra_?.py".to_string()]
    );
    assert!(is_collect_candidate(
        Path::new("check_bad.py"),
        Some(&patterns)
    ));
    assert!(is_collect_candidate(
        Path::new("extra_a.py"),
        Some(&patterns)
    ));
    assert!(!is_collect_candidate(
        Path::new("test_hidden.py"),
        Some(&patterns)
    ));
    assert!(!is_collect_candidate(
        Path::new("other_no.py"),
        Some(&patterns)
    ));
}

#[test]
fn python_files_character_class_matches_fnmatch() {
    let patterns = ["check_[ab].py".to_string()];
    assert!(is_collect_candidate(
        Path::new("check_a.py"),
        Some(&patterns)
    ));
    assert!(is_collect_candidate(
        Path::new("check_b.py"),
        Some(&patterns)
    ));
    assert!(!is_collect_candidate(
        Path::new("check_c.py"),
        Some(&patterns)
    ));
    let negated = ["check_[!c].py".to_string()];
    assert!(is_collect_candidate(
        Path::new("check_a.py"),
        Some(&negated)
    ));
    assert!(!is_collect_candidate(
        Path::new("check_c.py"),
        Some(&negated)
    ));
    let ranged = ["check_[a-c].py".to_string()];
    assert!(is_collect_candidate(Path::new("check_b.py"), Some(&ranged)));
    assert!(!is_collect_candidate(
        Path::new("check_d.py"),
        Some(&ranged)
    ));
}

#[test]
fn missing_python_files_keeps_the_default_candidate_rule() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("pytest.ini"), "[pytest]\naddopts = -q\n").unwrap();
    assert!(configured_python_filename_patterns(tmp.path()).is_none());
    assert!(is_collect_candidate(Path::new("test_a.py"), None));
    assert!(is_collect_candidate(Path::new("a_test.py"), None));
    assert!(!is_collect_candidate(Path::new("check_bad.py"), None));
}

#[test]
fn python_files_path_pattern_matches_the_whole_path() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("pytest.ini"),
        "[pytest]\npython_files = nested/check_*.py\n",
    )
    .unwrap();
    let nested = tmp.path().join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(
        nested.join("check_a.py"),
        "def test_a():\n    assert False\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("check_b.py"),
        "def test_b():\n    assert False\n",
    )
    .unwrap();
    let names = collect_file_names(tmp.path());
    assert_eq!(names, vec!["check_a.py".to_string()]);
    let absolute = nested.join("check_a.py");
    assert!(is_collect_candidate(
        &absolute,
        Some(&["nested/check_*.py".to_string()])
    ));
    assert!(!is_collect_candidate(
        &tmp.path().join("check_b.py"),
        Some(&["nested/check_*.py".to_string()])
    ));
}

#[test]
fn norecursedirs_setting_drops_nested_files_and_replaces_defaults() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("pytest.ini"),
        "[pytest]\nnorecursedirs = hidden\n",
    )
    .unwrap();
    let hidden = tmp.path().join("hidden");
    let build = tmp.path().join("build");
    std::fs::create_dir_all(&hidden).unwrap();
    std::fs::create_dir_all(&build).unwrap();
    std::fs::write(
        hidden.join("test_bad.py"),
        "def test_a():\n    assert False\n",
    )
    .unwrap();
    std::fs::write(
        build.join("test_built.py"),
        "def test_b():\n    assert False\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("test_ok.py"),
        "def test_c():\n    assert True\n",
    )
    .unwrap();
    let names = collect_file_names(tmp.path());
    assert!(names.contains(&"test_ok.py".to_string()));
    assert!(names.contains(&"test_built.py".to_string()));
    assert!(!names.iter().any(|name| name == "test_bad.py"));
}

#[test]
fn norecursedirs_path_pattern_drops_the_nested_directory() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("pytest.ini"),
        "[pytest]\nnorecursedirs = nested/hidden\n",
    )
    .unwrap();
    let hidden = tmp.path().join("nested").join("hidden");
    std::fs::create_dir_all(&hidden).unwrap();
    std::fs::write(
        hidden.join("test_bad.py"),
        "def test_a():\n    assert False\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("nested").join("test_ok.py"),
        "def test_b():\n    assert True\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("test_ok.py"),
        "def test_c():\n    assert True\n",
    )
    .unwrap();
    let names = collect_file_names(tmp.path());
    assert!(names.contains(&"test_ok.py".to_string()));
    assert!(!names.iter().any(|name| name == "test_bad.py"));
}

#[test]
fn collect_ignore_drops_a_flat_file_and_a_nested_glob() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("conftest.py"),
        "collect_ignore = [\"test_bad.py\"]\ncollect_ignore_glob = [\"fixtures/**\"]\n",
    )
    .unwrap();
    let fixtures = tmp.path().join("fixtures");
    std::fs::create_dir_all(&fixtures).unwrap();
    std::fs::write(
        tmp.path().join("test_bad.py"),
        "def test_a():\n    assert False\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("test_ok.py"),
        "def test_b():\n    assert True\n",
    )
    .unwrap();
    std::fs::write(
        fixtures.join("test_hidden.py"),
        "def test_c():\n    assert False\n",
    )
    .unwrap();
    let names = collect_file_names(tmp.path());
    assert_eq!(names, vec!["test_ok.py".to_string()]);
}

#[test]
fn commented_collect_ignore_keeps_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("conftest.py"),
        "# collect_ignore = [\"test_bad.py\"]\nnote = \"collect_ignore = [\\\"test_bad.py\\\"]\"\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("test_bad.py"),
        "def test_a():\n    assert False\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("test_ok.py"),
        "def test_b():\n    assert True\n",
    )
    .unwrap();
    let names = collect_file_names(tmp.path());
    assert!(names.contains(&"test_bad.py".to_string()));
    assert!(names.contains(&"test_ok.py".to_string()));
}

#[test]
fn collect_ignore_directory_drops_children_only_under_that_conftest() {
    let tmp = tempfile::tempdir().unwrap();
    let skipped = tmp.path().join("skipped");
    let other = tmp.path().join("other");
    std::fs::create_dir_all(&skipped).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(
        tmp.path().join("conftest.py"),
        "collect_ignore = [\"skipped\"]\n",
    )
    .unwrap();
    std::fs::write(
        other.join("conftest.py"),
        "collect_ignore = [\"test_local.py\"]\n",
    )
    .unwrap();
    std::fs::write(
        skipped.join("test_bad.py"),
        "def test_a():\n    assert False\n",
    )
    .unwrap();
    std::fs::write(
        other.join("test_local.py"),
        "def test_b():\n    assert False\n",
    )
    .unwrap();
    std::fs::write(other.join("test_ok.py"), "def test_c():\n    assert True\n").unwrap();
    let names = collect_file_names(tmp.path());
    assert_eq!(names, vec!["test_ok.py".to_string()]);
}

#[test]
fn symlink_test_file_keeps_the_link_name() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("impl.py"),
        "def test_ok():\n    assert False\n",
    )
    .unwrap();
    std::os::unix::fs::symlink("impl.py", tmp.path().join("test_ok.py")).unwrap();
    let names = collect_file_names(tmp.path());
    assert!(
        names.iter().any(|name| name == "test_ok.py"),
        "symlink test file must stay collectable: {names:?}"
    );
}

#[test]
fn default_norecursedirs_skips_node_modules() {
    let tmp = tempfile::tempdir().unwrap();
    let nested = tmp.path().join("node_modules");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(
        nested.join("test_bad.py"),
        "def test_a():\n    assert False\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("test_ok.py"),
        "def test_b():\n    assert True\n",
    )
    .unwrap();
    let names = collect_file_names(tmp.path());
    assert_eq!(names, vec!["test_ok.py".to_string()]);
}

#[test]
fn directory_collection_uses_nested_ini_and_workspace_keeps_the_root() {
    let tmp = tempfile::tempdir().unwrap();
    let sub = tmp.path().join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(
        sub.join("pytest.ini"),
        "[pytest]\npython_files = check_*.py\n",
    )
    .unwrap();
    std::fs::write(
        sub.join("test_bad.py"),
        "def test_bad():\n    assert False\n",
    )
    .unwrap();
    std::fs::write(sub.join("check_ok.py"), "def test_ok():\n    assert True\n").unwrap();
    let under = file_names(&python_files_under(tmp.path(), &sub, &[]));
    assert_eq!(under, vec!["check_ok.py".to_string()]);
    assert_eq!(
        collect_file_names(tmp.path()),
        vec!["test_bad.py".to_string()]
    );
}

fn file_names(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .filter_map(|path| path.file_name().and_then(|name| name.to_str()))
        .map(str::to_string)
        .collect()
}

fn collect_file_names(root: &Path) -> Vec<String> {
    workspace_python_collect_paths(root, &[])
        .iter()
        .filter_map(|path| path.file_name().and_then(|name| name.to_str()))
        .map(str::to_string)
        .collect()
}
