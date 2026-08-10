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
