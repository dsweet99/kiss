use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use kiss::rpytest_runner::TestStatus;
use kiss::rust_llvm_cov_runner::RustLineCoverage;

use super::test_support::{test_entries_fingerprint, write_test_entry, write_test_entry_with_args};
use super::*;

#[test]
fn write_rust_population_manifest_for_args_marks_population_current() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    fs::write(&lib, "pub fn lib() {}\n").unwrap();
    let _ = super::current_rust_coverage_batch_identity(tmp.path(), &[]);
    write_test_entry(
        tmp.path(),
        "a",
        "test_lib",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );
    write_rust_population_manifest_for_args(tmp.path(), &["test_lib".to_string()], &[]).unwrap();
    assert!(rust_population_manifest_is_current_for_args(
        tmp.path(),
        &["test_lib".to_string()],
        &[],
    ));
    assert!(!rust_population_manifest_is_current_for_args(
        tmp.path(),
        &["missing_selector".to_string()],
        &[],
    ));
    let grown = ["test_lib".to_string(), "test_new".to_string()];
    assert_eq!(
        rust_selective_rebuild_publication_selectors(tmp.path(), &grown, &[]),
        Some(grown.as_slice())
    );
    assert_eq!(
        rust_selective_rebuild_publication_selectors(
            tmp.path(),
            &["test_lib".to_string()],
            &[],
        ),
        None
    );
}

#[test]
fn population_manifest_requires_matching_generation_and_selectors() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    fs::write(&lib, "pub fn lib() {}\n").unwrap();
    let exact_args = vec!["--exact".to_string()];
    write_test_entry_with_args(
        tmp.path(),
        "a",
        "test_lib",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
        &exact_args,
    );

    write_rust_population_manifest_for_args(tmp.path(), &["test_lib".to_string()], &exact_args)
        .unwrap();

    assert!(rust_population_manifest_is_current_for_args(
        tmp.path(),
        &["test_lib".to_string()],
        &exact_args,
    ));

    let mut with_nocapture = exact_args.clone();
    with_nocapture.push("--nocapture".to_string());
    assert!(
        rust_population_manifest_is_current_for_args(
            tmp.path(),
            &["test_lib".to_string()],
            &with_nocapture,
        ),
        "output-only --nocapture must not invalidate population freshness"
    );
    assert!(
        !rust_population_manifest_is_current_for_args(tmp.path(), &["test_lib".to_string()], &[],),
        "coverage-affecting --exact invalidates population freshness"
    );

    assert!(
        !rust_population_manifest_is_current_for_args(
            tmp.path(),
            &["other_selector".to_string()],
            &exact_args,
        ),
        "selector population mismatch invalidates population freshness"
    );
}

#[test]
fn empty_sources_and_empty_cache_have_empty_selection_contracts() {
    let tmp = tempfile::tempdir().unwrap();

    let index = rebuild_rust_coverage_index(tmp.path()).unwrap();
    let selected = select_rust_source_selectors_from_index(tmp.path(), &[], &[]).unwrap();

    assert!(index.is_empty());
    assert!(selected.is_empty());
    assert!(rust_coverage_cache_root(tmp.path()).ends_with(".kiss/rust_llvm_cov_cache"));
    assert!(rust_coverage_index_path(tmp.path()).ends_with(".kiss/rust_llvm_cov_cache/index.json"));
}

#[test]
fn bad_index_files_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let index_path = rust_coverage_index_path(tmp.path());
    fs::create_dir_all(index_path.parent().unwrap()).unwrap();
    fs::write(&index_path, "{not json").unwrap();
    assert!(load_current_rust_coverage_index(tmp.path(), &[]).is_none());

    fs::write(
        &index_path,
        serde_json::json!({
            "schema_version": "old",
            "source_root": normalized_repo_root(tmp.path()),
            "entries_fingerprint": test_entries_fingerprint(tmp.path(), &[]),
            "files": {}
        })
        .to_string(),
    )
    .unwrap();
    assert!(load_current_rust_coverage_index(tmp.path(), &[]).is_none());

    fs::write(
        &index_path,
        serde_json::json!({
            "schema_version": LEGACY_INDEX_SCHEMA_VERSION,
            "source_root": "/not/this/repo",
            "entries_fingerprint": test_entries_fingerprint(tmp.path(), &[]),
            "files": {}
        })
        .to_string(),
    )
    .unwrap();
    assert!(load_current_rust_coverage_index(tmp.path(), &[]).is_none());
}

#[test]
fn path_fingerprint_and_temp_file_helpers_have_contracts() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    fs::write(&lib, "pub fn lib() {}\n").unwrap();
    let outside = tempfile::tempdir().unwrap();
    let outside_file = outside.path().join("other.rs");
    fs::write(&outside_file, "pub fn other() {}\n").unwrap();

    assert_eq!(
        repo_relative_coverage_file(tmp.path(), &lib.to_string_lossy()),
        Some("src/lib.rs".to_string())
    );
    assert_eq!(
        repo_relative_path(tmp.path(), Path::new("src/lib.rs")),
        Some("src/lib.rs".to_string())
    );
    assert!(repo_relative_path(tmp.path(), &outside_file).is_none());

    let first = test_entries_fingerprint(tmp.path(), &[]);
    write_test_entry(
        tmp.path(),
        "a",
        "test_lib",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );
    let second = test_entries_fingerprint(tmp.path(), &[]);
    assert_ne!(first, second);

    let temp_path = tmp.path().join("created-once.tmp");
    let _file = create_new_file(&temp_path).unwrap();
    assert!(create_new_file(&temp_path).is_err());
    assert_ne!(unique_suffix(), "");
}

#[test]
fn facade_helpers_have_contracts() {
    let tmp = tempfile::tempdir().unwrap();
    let err = command_stdout(Path::new("/definitely/not/a/command"), &[], tmp.path()).unwrap_err();
    assert!(err.contains("failed to spawn"));
    assert!(is_cargo_config_input_path(Path::new(".cargo/config")));
    assert!(is_cargo_config_input_path(Path::new(".cargo/config.toml")));
    assert!(!is_cargo_config_input_path(Path::new("config.toml")));
    assert!(rust_population_manifest_path(tmp.path()).ends_with("population.json"));
}

#[test]
fn derived_publish_without_selectors_does_not_write_empty_population() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::write(tmp.path().join("src").join("lib.rs"), "pub fn lib() {}\n").unwrap();
    write_rust_population_manifest_for_args(tmp.path(), &["test_lib".to_string()], &[]).unwrap();
    let before = fs::read(rust_population_manifest_path(tmp.path())).unwrap();
    publish_rust_derived_state_with_filter(tmp.path(), Some(&[]), &[], |_, _| true).unwrap();
    let after = fs::read(rust_population_manifest_path(tmp.path())).unwrap();
    assert_eq!(before, after);
}
