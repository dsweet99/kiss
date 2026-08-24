use super::*;
use tempfile::tempdir;

#[test]
fn publish_advances_revision_and_matches() {
    let tmp = tempdir().unwrap();
    let cache = tmp.path();
    let r1 = publish_next_entry_state(cache, "gen", "fp-a").unwrap();
    assert_eq!(r1, 1);
    let r2 = publish_next_entry_state(cache, "gen", "fp-b").unwrap();
    assert_eq!(r2, 2);
    let state = read_entry_state(cache).unwrap();
    assert!(entry_state_matches(&state, "gen", "fp-b", 2));
    assert!(!entry_state_matches(&state, "gen", "fp-a", 2));
}

#[test]
fn invalidate_removes_token() {
    let tmp = tempdir().unwrap();
    let cache = tmp.path();
    publish_next_entry_state(cache, "gen", "fp").unwrap();
    invalidate_entry_state(cache);
    assert!(read_entry_state(cache).is_none());
}

#[test]
fn invalidate_leaves_population_manifest_bytes_alone() {
    let tmp = tempdir().unwrap();
    let cache = tmp.path();
    let population = br#"{"schema_version":"rust-llvm-cov-population-v6","reverse_line_index":{"snapshot_id":"s"}}"#;
    fs::write(cache.join("population.json"), population).unwrap();
    publish_next_entry_state(cache, "gen", "fp").unwrap();
    invalidate_entry_state(cache);
    assert_eq!(fs::read(cache.join("population.json")).unwrap(), population);
    assert!(read_entry_state(cache).is_none());
}
