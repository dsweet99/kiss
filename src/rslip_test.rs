use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn seed_query_db(tmp: &Path) {
    std::fs::write(tmp.join("app.py"), "def app():\n    return 1\n").unwrap();
    std::fs::write(tmp.join("test_app.py"), "def test_app():\n    assert 1\n").unwrap();
    let file_records = rslip::discover_repo_files(tmp).unwrap();
    let files = file_records
        .iter()
        .map(|file| (file.path.clone(), file.clone()))
        .collect();
    let db = rslip::Database {
        schema_version: rslip::SCHEMA_VERSION,
        rslip_version: rslip::RSLIP_VERSION.to_string(),
        config_fingerprints: rslip::config_fingerprints(&file_records),
        files,
        tests: BTreeMap::new(),
        source_to_covering_tests: BTreeMap::from([(
            "app.py".to_string(),
            vec!["test_app.py::test_app".to_string()],
        )]),
    };
    rslip::write_database_atomic(tmp, &db).unwrap();
}

#[test]
fn rslip_query_covering_tests_is_directly_callable_from_sibling_test_module() {
    let tmp = tempfile::TempDir::new().unwrap();
    seed_query_db(tmp.path());
    let covering =
        crate::rslip::query_covering_tests(tmp.path(), &[tmp.path().join("app.py")]).unwrap();
    assert_eq!(covering.len(), 1);
    assert!(covering[0].0.ends_with("test_app.py"));
    assert_eq!(covering[0].1, "test_app");
}

#[test]
fn rslip_query_covering_tests_accepts_empty_changed_sources() {
    let tmp = tempfile::TempDir::new().unwrap();
    seed_query_db(tmp.path());
    let covering = crate::rslip::query_covering_tests(tmp.path(), &[] as &[PathBuf]).unwrap();
    assert!(covering.is_empty());
}
