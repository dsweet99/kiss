use super::*;

#[test]
fn storage_paths_hashes_and_input_filters_have_contracts() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".kiss").join("rslip_cache")).unwrap();
    std::fs::create_dir(tmp.path().join(".rslip_cache")).unwrap();
    std::fs::write(tmp.path().join("app.py"), "VALUE = 1\n").unwrap();
    std::fs::write(
        tmp.path().join("pyproject.toml"),
        "[tool.pytest.ini_options]\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path()
            .join(".kiss")
            .join("rslip_cache")
            .join("ignored.py"),
        "VALUE = 2\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join(".rslip_cache").join("ignored.py"),
        "VALUE = 3\n",
    )
    .unwrap();
    let cache_root = python_coverage_cache_root(tmp.path()).unwrap();

    assert_eq!(
        cache_root,
        tmp.path()
            .join(".kiss")
            .join("rslip_cache")
            .join("hosts")
            .join(cache_root.file_name().unwrap())
    );
    assert_eq!(
        python_coverage_index_path(tmp.path()).unwrap(),
        cache_root.join("index.json")
    );
    assert_eq!(
        python_population_manifest_path(tmp.path()).unwrap(),
        cache_root.join("population.json")
    );
    assert!(is_kiss_rslip_cache_dir(
        &tmp.path().join(".kiss").join("rslip_cache")
    ));
    assert!(should_skip_python_source_input_dir(
        &tmp.path().join(".rslip_cache")
    ));
    assert!(is_python_source_input_path(&tmp.path().join("app.py")));
    assert!(is_python_source_input_path(
        &tmp.path().join("pyproject.toml")
    ));
    assert!(python_source_input_fingerprint(tmp.path()).is_ok());
    assert!(
        python_source_input_paths(tmp.path())
            .unwrap()
            .iter()
            .any(|path| path.ends_with("app.py"))
    );
    assert_eq!(
        python_repo_relative_coverage_file(
            tmp.path(),
            &tmp.path().join("app.py").to_string_lossy()
        ),
        Some("app.py".to_string())
    );
    assert!(python_repo_relative_path(tmp.path(), &tmp.path().join("app.py")).is_some());
    assert!(python_repo_relative_path(tmp.path(), std::path::Path::new("/outside.py")).is_none());
    assert_eq!(
        normalized_python_repo_root(tmp.path()),
        tmp.path().canonicalize().unwrap().display().to_string()
    );
    assert_eq!(python_coverage_entry_paths(&cache_root).len(), 0);
    assert!(python_entries_fingerprint(&cache_root).is_ok());
    let created = tmp.path().join("created.txt");
    create_new_python_file(&created).unwrap();
    assert!(create_new_python_file(&created).is_err());
    assert_ne!(python_unique_suffix(), "");
    assert_ne!(
        python_fnv1a64(0xcbf2_9ce4_8422_2325, b"a"),
        python_fnv1a64(0xcbf2_9ce4_8422_2325, b"b")
    );
}

#[test]
fn stale_entries_fingerprint_makes_python_index_fail_closed() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = python_coverage_cache_root(tmp.path()).unwrap();
    let stale_fingerprint = python_entries_fingerprint(&cache_root).unwrap();
    let entry = cache_root.join("entries").join("new.json");
    std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
    std::fs::write(
        entry,
        serde_json::json!({
            "schema_version": kiss::rslip::CACHE_SCHEMA_VERSION,
            "nodeid": "tests/test_app.py::test_value",
            "status": "passed",
            "exit_code": 0,
            "duration": {"secs": 0, "nanos": 1},
            "coverage": {"files": {}},
        })
        .to_string(),
    )
    .unwrap();

    write_python_coverage_index_with_entries_fingerprint(
        tmp.path(),
        &PythonCoverageIndex::new(),
        &stale_fingerprint,
    )
    .unwrap();

    assert!(load_current_python_coverage_index(tmp.path()).is_none());
}
