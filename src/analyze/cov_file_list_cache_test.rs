use super::{CovFileListKey, store_cov_file_list, try_load_cov_file_list};
use std::fs;
use std::path::PathBuf;

fn write_python_population(repo: &std::path::Path) {
    let host = repo.join(".kiss/rslip_cache/hosts/testhost");
    fs::create_dir_all(&host).unwrap();
    fs::write(
        host.join("population.json"),
        r#"{
            "schema_version":"rslip-python-population-v1",
            "input_fingerprint":"abc",
            "entries_fingerprint":"def",
            "selectors":["t::one"]
        }"#,
    )
    .unwrap();
}

#[test]
fn cov_file_list_cache_round_trip_and_population_invalidation() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    write_python_population(repo);
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

    fs::write(
        repo.join(".kiss/rslip_cache/hosts/testhost/population.json"),
        r#"{
            "schema_version":"rslip-python-population-v1",
            "input_fingerprint":"changed",
            "entries_fingerprint":"def",
            "selectors":["t::one"]
        }"#,
    )
    .unwrap();
    assert!(try_load_cov_file_list(&key).is_none());
}
