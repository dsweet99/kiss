use super::*;
use crate::{RustLineCoverage, RustLlvmCovOutcome};
use rpytest_runner::TestStatus;
use std::time::Duration;
use tempfile::tempdir;

fn write_entry(
    cache_root: &Path,
    generation: &str,
    selector: &str,
    files: BTreeMap<String, BTreeSet<u32>>,
) {
    let entry = RustCovCacheEntry::from_outcome(
        &RustLlvmCovOutcome {
            selector: selector.to_string(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            coverage: RustLineCoverage { files },
            test_binary_ids: vec!["bin".into()],
            cache_status: crate::RustCovCacheStatus::Hit,
            stdout: None,
            stderr: None,
        },
        generation,
    );
    let dir = cache_root.join("entries");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{selector}.json"));
    fs::write(path, serde_json::to_vec_pretty(&entry).unwrap()).unwrap();
}

#[test]
fn reverse_index_answers_overlapping_symbol_lines() {
    let tmp = tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let source = tmp.path().join("src");
    fs::create_dir_all(source.join("src")).unwrap();
    fs::write(source.join("src/lib.rs"), "fn a() {}\nfn b() {}\n").unwrap();
    write_entry(
        &cache,
        "gen1",
        "test_a",
        BTreeMap::from([(
            source.join("src/lib.rs").to_string_lossy().into_owned(),
            BTreeSet::from([1_u32]),
        )]),
    );
    write_entry(
        &cache,
        "gen1",
        "test_b",
        BTreeMap::from([(
            source.join("src/lib.rs").to_string_lossy().into_owned(),
            BTreeSet::from([2_u32]),
        )]),
    );
    publish_reverse_line_index(&cache, &source, "gen1", "entries-fp").unwrap();
    let hit = query_reverse_line_index(
        &cache,
        "gen1",
        &BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([1_u32]))]),
    )
    .unwrap();
    assert_eq!(
        hit.get("src/lib.rs").unwrap(),
        &BTreeSet::from(["test_a".to_string()])
    );
    let miss = query_reverse_line_index(
        &cache,
        "gen1",
        &BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([9_u32]))]),
    )
    .unwrap();
    assert!(miss.is_empty());
}

#[test]
fn reverse_index_rejects_other_generation() {
    let tmp = tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let source = tmp.path();
    publish_reverse_line_index(&cache, source, "gen1", "fp").unwrap();
    assert!(
        query_reverse_line_index(
            &cache,
            "gen2",
            &BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([1_u32]))]),
        )
        .is_none()
    );
}
