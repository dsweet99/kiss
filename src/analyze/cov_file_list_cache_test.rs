use super::{CovFileListKey, store_cov_file_list, try_load_cov_file_list};
use crate::analyze::cov_cache_test_support::write_python_population_for_cache_tests;
use std::path::PathBuf;

#[test]
fn cov_file_list_cache_round_trip_and_population_invalidation() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    write_python_population_for_cache_tests(repo, "abc");
    let py = PathBuf::from("pkg/a.py");
    let key = CovFileListKey {
        repo_root: repo,
        lang_filter: Some(kiss::Language::Python),
        ignore: &[],
    };
    assert!(try_load_cov_file_list(&key).is_none());
    store_cov_file_list(&key, std::slice::from_ref(&py), &[]);
    let (loaded_py, loaded_rs) = try_load_cov_file_list(&key).expect("warm hit");
    assert_eq!(loaded_py, vec![py]);
    assert!(loaded_rs.is_empty());

    write_python_population_for_cache_tests(repo, "changed");
    assert!(try_load_cov_file_list(&key).is_none());
}

#[test]
fn cov_file_list_cache_misses_when_cargo_toml_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    write_python_population_for_cache_tests(repo, "abc");
    std::fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    let py = PathBuf::from("pkg/a.py");
    let key = CovFileListKey {
        repo_root: repo,
        lang_filter: Some(kiss::Language::Python),
        ignore: &[],
    };
    store_cov_file_list(&key, std::slice::from_ref(&py), &[]);
    assert!(try_load_cov_file_list(&key).is_some());
    std::fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[[bin]]\nname = \"tool\"\npath = \"src/lib.rs\"\n",
    )
    .unwrap();
    assert!(
        try_load_cov_file_list(&key).is_none(),
        "Cargo.toml target metadata must miss the file-list cache before parse"
    );
}
