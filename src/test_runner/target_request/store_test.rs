#[test]
fn cold_cache_still_has_a_runner_identity() {
    let tmp = tempfile::TempDir::new().unwrap();
    assert!(!super::report::runner_identity(tmp.path()).is_empty());
}
