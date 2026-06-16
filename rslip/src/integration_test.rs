use super::*;
use crate::discovery::{classify_python, is_in_test_directory, is_test_file};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

struct FakeCollector {
    runs: Vec<TestCoverageRun>,
    fail: bool,
}

impl FakeCollector {
    fn collect(
        &self,
        _repo_root: &Path,
        selectors: &[String],
        _j: usize,
    ) -> Result<Vec<TestCoverageRun>, String> {
        if self.fail {
            return Err("collector failed".to_string());
        }
        let wanted: HashSet<_> = selectors.iter().collect();
        Ok(self
            .runs
            .iter()
            .filter(|run| wanted.contains(&run.selector))
            .cloned()
            .collect())
    }
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn fake_run(selector: &str, test_path: &str, hits: &[(&str, &[usize])]) -> TestCoverageRun {
    TestCoverageRun {
        selector: selector.to_string(),
        test_path: PathBuf::from(test_path),
        hits: hits
            .iter()
            .map(|(path, lines)| (PathBuf::from(path), lines.iter().copied().collect()))
            .collect(),
    }
}

#[test]
fn content_digest_is_stable_and_sensitive() {
    assert_eq!(content_digest(b"same"), content_digest(b"same"));
    assert_ne!(content_digest(b"same"), content_digest(b"some"));
}

#[test]
fn python_file_classification_identifies_tests_and_sources() {
    assert!(is_test_file(Path::new("test_api.py")));
    assert!(is_test_file(Path::new("api_test.py")));
    assert!(is_test_file(Path::new("conftest.py")));
    assert!(!is_test_file(Path::new("api.py")));
    assert!(is_in_test_directory(Path::new("pkg/tests/api.py")));
    assert_eq!(
        classify_python(Path::new("pkg/tests/api.py")),
        FileRole::Test
    );
    assert_eq!(classify_python(Path::new("pkg/api.py")), FileRole::Source);
}

#[test]
fn line_accounting_reports_missing_body_lines() {
    let executable = executable_lines_from_source("def f():\n    return 1\n\n# comment\n");
    let covered = line_coverage(&executable, &BTreeSet::from([1]));
    assert_eq!(covered.executable_lines, vec![1, 2]);
    assert_eq!(covered.executed_lines, vec![1]);
    assert_eq!(covered.missing_lines, vec![2]);
    assert_eq!(covered.percent_covered, 50);
}

#[test]
fn empty_file_is_fully_covered() {
    let covered = line_coverage(&[], &BTreeSet::new());
    assert_eq!(covered.percent_covered, 100);
    assert!(covered.missing_lines.is_empty());
}

#[test]
fn module_docstrings_are_not_executable_lines() {
    let executable = executable_lines_from_source(concat!(
        "\"\"\"Package documentation.\"\"\"\n",
        "\n",
        "from .thing import run\n",
    ));
    assert_eq!(executable, vec![3]);

    let docstring_only = executable_lines_from_source("\"\"\"Package documentation.\"\"\"\n");
    assert!(docstring_only.is_empty());
}

#[test]
fn parenthesized_import_continuations_are_not_executable_lines() {
    let executable = executable_lines_from_source(concat!(
        "from pkg import (\n",
        "    alpha,\n",
        "    beta,\n",
        ")\n",
        "value = alpha\n",
    ));
    assert_eq!(executable, vec![1, 5]);
}

#[test]
fn multiline_prompt_string_body_is_not_executable_lines() {
    let executable = executable_lines_from_source(concat!(
        "def prompt():\n",
        "    return f\"\"\"Title\n",
        "\n",
        "Body line\n",
        "\"\"\"\n",
        "x = 1\n",
    ));
    assert_eq!(executable, vec![1, 2, 6]);
}

#[test]
fn database_round_trips_and_old_schema_is_ignored() {
    let tmp = TempDir::new().unwrap();
    let db = Database {
        schema_version: SCHEMA_VERSION,
        rslip_version: RSLIP_VERSION.to_string(),
        config_fingerprints: BTreeMap::new(),
        files: BTreeMap::new(),
        tests: BTreeMap::new(),
        source_to_covering_tests: BTreeMap::new(),
    };
    write_database_atomic(tmp.path(), &db).unwrap();
    assert_eq!(load_database(tmp.path()).unwrap(), Some(db));
    let mut old = load_database(tmp.path()).unwrap().unwrap();
    old.schema_version += 1;
    fs::write(db_path(tmp.path()), serde_json::to_vec(&old).unwrap()).unwrap();
    assert!(load_database(tmp.path()).unwrap().is_none());
    fs::write(
        db_path(tmp.path()),
        r#"{"schema_version":1,"tests":{"t":{"selector":"t"}}}"#,
    )
    .unwrap();
    assert!(load_database(tmp.path()).unwrap().is_none());
}

#[test]
fn database_path_is_repo_local() {
    let repo = Path::new("/tmp/repo");
    assert_eq!(db_path(repo), PathBuf::from("/tmp/repo/.kiss/rslip.json"));
}

#[test]
fn discover_tests_resets_class_context_for_top_level_tests() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("pkg.py"), "def f():\n    return 1\n");
    write(
        &tmp.path().join("test_pkg.py"),
        concat!(
            "class TestThing:\n",
            "    def test_method(self):\n",
            "        pass\n\n",
            "def test_top_level():\n",
            "    pass\n",
        ),
    );
    let files = discover_repo_files(tmp.path()).unwrap();
    let tests = discover_tests(tmp.path(), &files).unwrap();
    let selectors: BTreeSet<_> = tests.into_iter().map(|(selector, _)| selector).collect();
    assert!(selectors.contains("test_pkg.py::TestThing::test_method"));
    assert!(selectors.contains("test_pkg.py::test_top_level"));
    assert!(!selectors.contains("test_pkg.py::TestThing::test_top_level"));
}

#[test]
fn dirty_detection_catches_same_size_change_delete_and_config() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("pkg.py"), "x = 1\n");
    write(
        &tmp.path().join("test_pkg.py"),
        "def test_x():\n    assert 1\n",
    );
    write(&tmp.path().join(".kissconfig"), "[gate]\n");
    let collector = FakeCollector {
        runs: vec![fake_run(
            "test_pkg.py::test_x",
            "test_pkg.py",
            &[("pkg.py", &[1])],
        )],
        fail: false,
    };
    let db = refresh_with_collector(tmp.path(), &|repo, selectors, _j| {
        collector.collect(repo, selectors, _j)
    }, 1)
    .unwrap();
    write(&tmp.path().join("pkg.py"), "x = 2\n");
    assert!(
        changed_files(tmp.path(), &db)
            .unwrap()
            .contains(&"pkg.py".to_string())
    );
    write(&tmp.path().join("pkg.py"), "x = 1\n");
    fs::remove_file(tmp.path().join("test_pkg.py")).unwrap();
    assert!(
        changed_files(tmp.path(), &db)
            .unwrap()
            .contains(&"test_pkg.py".to_string())
    );
    write(
        &tmp.path().join("test_pkg.py"),
        "def test_x():\n    assert 1\n",
    );
    write(&tmp.path().join(".kissconfig"), "[gate]\n# changed\n");
    assert!(
        changed_files(tmp.path(), &db)
            .unwrap()
            .contains(&".kissconfig".to_string())
    );
}

#[test]
fn dirty_detection_catches_implementation_fingerprint_change() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("pkg.py"), "value = 1\n");
    write(
        &tmp.path().join("test_pkg.py"),
        "def test_value():\n    assert 1\n",
    );
    let collector = FakeCollector {
        runs: vec![fake_run(
            "test_pkg.py::test_value",
            "test_pkg.py",
            &[("pkg.py", &[1])],
        )],
        fail: false,
    };
    let mut db = refresh_with_collector(tmp.path(), &|repo, selectors, _j| {
        collector.collect(repo, selectors, _j)
    }, 1)
    .unwrap();
    db.config_fingerprints
        .insert("rslip_version".to_string(), "stale".to_string());
    let changed = changed_files(tmp.path(), &db).unwrap();
    assert!(changed.contains(&"rslip_version".to_string()));
}

#[test]
fn refresh_records_one_test_covering_multiple_sources_and_inverse() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("a.py"), "def a():\n    return 1\n");
    write(&tmp.path().join("b.py"), "def b():\n    return 2\n");
    write(&tmp.path().join("test_ab.py"), "def test_ab():\n    pass\n");
    write(&tmp.path().join("test_a.py"), "def test_a():\n    pass\n");
    let collector = FakeCollector {
        runs: vec![
            fake_run(
                "test_ab.py::test_ab",
                "test_ab.py",
                &[("a.py", &[1, 2]), ("b.py", &[1, 2])],
            ),
            fake_run("test_a.py::test_a", "test_a.py", &[("a.py", &[1, 2])]),
        ],
        fail: false,
    };
    let db = refresh_with_collector(tmp.path(), &|repo, selectors, _j| {
        collector.collect(repo, selectors, _j)
    }, 1)
    .unwrap();
    assert_eq!(
        db.source_to_covering_tests["b.py"],
        vec!["test_ab.py::test_ab"]
    );
    assert_eq!(
        db.source_to_covering_tests["a.py"],
        vec!["test_a.py::test_a", "test_ab.py::test_ab"]
    );
}

#[test]
fn changed_test_refresh_only_collects_tests_in_changed_file() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("a.py"), "def a():\n    return 1\n");
    write(&tmp.path().join("test_a.py"), "def test_a():\n    pass\n");
    write(
        &tmp.path().join("test_other.py"),
        "def test_other():\n    pass\n",
    );
    let initial = FakeCollector {
        runs: vec![fake_run(
            "test_other.py::test_other",
            "test_other.py",
            &[("a.py", &[1])],
        )],
        fail: false,
    };
    let db = refresh_with_collector(tmp.path(), &|repo, selectors, _j| {
        initial.collect(repo, selectors, _j)
    }, 1)
    .unwrap();
    let changed = FakeCollector {
        runs: vec![fake_run(
            "test_a.py::test_a",
            "test_a.py",
            &[("a.py", &[2])],
        )],
        fail: false,
    };
    let next = refresh_changed_tests_with_collector(
        tmp.path(),
        &db,
        &[tmp.path().join("test_a.py")],
        &|repo, selectors, _j| changed.collect(repo, selectors, _j),
        1,
    )
    .unwrap();
    assert!(next.tests.contains_key("test_other.py::test_other"));
    assert!(next.tests.contains_key("test_a.py::test_a"));
    let coverage = next.files["a.py"].coverage.as_ref().unwrap();
    assert_eq!(coverage.executed_lines, vec![1, 2]);
    assert!(coverage.missing_lines.is_empty());
}

#[test]
fn atomic_refresh_preserves_previous_database_on_failure() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("a.py"), "def a():\n    return 1\n");
    write(&tmp.path().join("test_a.py"), "def test_a():\n    pass\n");
    let ok = FakeCollector {
        runs: vec![fake_run(
            "test_a.py::test_a",
            "test_a.py",
            &[("a.py", &[1])],
        )],
        fail: false,
    };
    let db = refresh_and_store(tmp.path(), &|repo, selectors, _j| ok.collect(repo, selectors, _j), 1).unwrap();
    let failing = FakeCollector {
        runs: Vec::new(),
        fail: true,
    };
    assert!(
        refresh_and_store(tmp.path(), &|repo, selectors, _j| failing
            .collect(repo, selectors, _j), 1)
        .is_err()
    );
    assert_eq!(load_database(tmp.path()).unwrap(), Some(db));
}

#[test]
fn refresh_stores_parametrized_nodeids_as_keys() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("sample.py"), "def is_even(v):\n    return v % 2 == 0\n");
    write(
        &tmp.path().join("test_sample.py"),
        concat!(
            "import pytest\n",
            "from sample import is_even\n\n",
            "@pytest.mark.parametrize('value', [2, 4])\n",
            "def test_is_even(value):\n",
            "    assert is_even(value)\n",
        ),
    );
    let collector = FakeCollector {
        runs: vec![
            fake_run(
                "test_sample.py::test_is_even[2]",
                "test_sample.py",
                &[("sample.py", &[1, 2])],
            ),
            fake_run(
                "test_sample.py::test_is_even[4]",
                "test_sample.py",
                &[("sample.py", &[1, 2])],
            ),
        ],
        fail: false,
    };
    let db = refresh_with_collector(
        tmp.path(),
        &|repo, selectors, j| collector.collect(repo, selectors, j),
        1,
    )
    .unwrap();
    assert!(db.tests.contains_key("test_sample.py::test_is_even[2]"));
    assert!(db.tests.contains_key("test_sample.py::test_is_even[4]"));
    assert!(!db.tests.contains_key("test_sample.py::test_is_even"));
}
