use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use crate::database::write_database_atomic;
use crate::discovery::{config_fingerprints, discover_repo_files};
use crate::{Database, RSLIP_VERSION, SCHEMA_VERSION};

fn write(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
}

#[test]
fn query_covering_tests_is_directly_callable_from_tests() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("app.py"), "def app():\n    return 1\n");
    write(&tmp.path().join("other.py"), "def other():\n    return 2\n");
    write(
        &tmp.path().join("test_app.py"),
        "from app import app\n\ndef test_app():\n    assert app() == 1\n",
    );

    let covering = crate::refresh::query_covering_tests(
        tmp.path(),
        &[tmp.path().join("app.py"), tmp.path().join("other.py")],
        1,
    )
    .unwrap();
    for (path, name) in &covering {
        assert!(path.ends_with("test_app.py"));
        assert_eq!(name, "test_app");
    }

    assert_eq!(
        covering,
        vec![(tmp.path().join("test_app.py"), "test_app".to_string())]
    );
    let missing =
        crate::refresh::query_covering_tests(tmp.path(), &[tmp.path().join("missing.py")], 1)
            .unwrap();
    assert!(missing.is_empty());
}

#[test]
fn query_covering_tests_accepts_relative_sources_and_deduplicates_selectors() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("app.py"), "def app():\n    return 1\n");
    write(
        &tmp.path().join("test_app.py"),
        "def test_app():\n    assert 1\n",
    );
    let file_records = discover_repo_files(tmp.path()).unwrap();
    let files = file_records
        .iter()
        .map(|file| (file.path.clone(), file.clone()))
        .collect();
    let db = Database {
        schema_version: SCHEMA_VERSION,
        rslip_version: RSLIP_VERSION.to_string(),
        config_fingerprints: config_fingerprints(&file_records),
        files,
        tests: std::collections::BTreeMap::new(),
        source_to_covering_tests: std::collections::BTreeMap::from([(
            "app.py".to_string(),
            vec![
                "test_app.py::test_app".to_string(),
                "test_app.py::test_app".to_string(),
                "test_app.py::TestApp::test_method".to_string(),
            ],
        )]),
    };
    write_database_atomic(tmp.path(), &db).unwrap();

    let covering =
        crate::refresh::query_covering_tests(tmp.path(), &[PathBuf::from("app.py")], 1).unwrap();
    assert_eq!(
        covering,
        vec![
            (
                tmp.path().join("test_app.py"),
                "TestApp::test_method".to_string()
            ),
            (tmp.path().join("test_app.py"), "test_app".to_string()),
        ]
    );
}

#[test]
fn changed_files_uses_mtime_fast_path() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("pkg.py"), "x = 1\n");
    let records = discover_repo_files(tmp.path()).unwrap();
    let pkg = records
        .iter()
        .find(|file| file.path == "pkg.py")
        .expect("pkg.py record");
    let mut stale = pkg.clone();
    stale.mtime_ns = pkg.mtime_ns.saturating_sub(1);
    assert_eq!(pkg.content_digest, stale.content_digest);
    let db = Database {
        schema_version: SCHEMA_VERSION,
        rslip_version: RSLIP_VERSION.to_string(),
        config_fingerprints: config_fingerprints(&records),
        files: std::collections::BTreeMap::from([("pkg.py".to_string(), stale)]),
        tests: std::collections::BTreeMap::new(),
        source_to_covering_tests: std::collections::BTreeMap::new(),
    };
    let changed = crate::refresh::changed_files(tmp.path(), &db).unwrap();
    assert!(
        changed.contains(&"pkg.py".to_string()),
        "mtime mismatch must mark file dirty even when digest unchanged: {changed:?}"
    );
}
