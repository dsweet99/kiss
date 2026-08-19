use crate::publish_derived::batch_reverse_build::REVERSE_LINE_INDEX_SCHEMA;
use crate::publish_derived::batch_reverse_publish::snapshot_path;
use crate::publish_derived::batch_reverse_test_support::{
    publish_bound_reverse, write_passed_entry, write_population_with_reverse,
};
use crate::rust_cov_cache::{RustCovCacheEntry, repo_relative_coverage_file};
use rpytest_runner::TestStatus;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use tempfile::tempdir;

/// Authoritative forward-entry oracle used to check reverse answers.
fn forward_oracle_selectors(
    cache: &Path,
    source: &Path,
    generation: &str,
    wanted: &BTreeMap<String, BTreeSet<u32>>,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let entries = cache.join("entries");
    if !entries.is_dir() {
        return out;
    }
    for entry in fs::read_dir(&entries).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let parsed: RustCovCacheEntry =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        if parsed.generation_fingerprint != generation || parsed.status != TestStatus::Passed {
            continue;
        }
        for (file, covered) in &parsed.coverage.files {
            let Some(rel) = repo_relative_coverage_file(source, file) else {
                continue;
            };
            let Some(wanted_lines) = wanted.get(&rel) else {
                continue;
            };
            if !wanted_lines.is_disjoint(covered) {
                out.entry(rel).or_default().insert(parsed.selector.clone());
            }
        }
    }
    out
}

fn assert_reverse_matches_forward_oracle(
    cache: &Path,
    source: &Path,
    generation: &str,
    wanted: &BTreeMap<String, BTreeSet<u32>>,
) {
    let reverse = crate::query_reverse_line_index(cache, generation, wanted).unwrap();
    let oracle = forward_oracle_selectors(cache, source, generation, wanted);
    assert_eq!(reverse, oracle, "reverse must equal forward-entry oracle");
}

#[test]
fn reverse_index_answers_overlapping_symbol_lines() {
    let tmp = tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let source = tmp.path().join("src");
    fs::create_dir_all(source.join("src")).unwrap();
    fs::write(source.join("src/lib.rs"), "fn a() {}\nfn b() {}\n").unwrap();
    write_passed_entry(
        &cache,
        "gen1",
        "test_a",
        BTreeMap::from([(
            source.join("src/lib.rs").to_string_lossy().into_owned(),
            BTreeSet::from([1_u32]),
        )]),
    );
    write_passed_entry(
        &cache,
        "gen1",
        "test_b",
        BTreeMap::from([(
            source.join("src/lib.rs").to_string_lossy().into_owned(),
            BTreeSet::from([2_u32]),
        )]),
    );
    publish_bound_reverse(&cache, &source, "gen1", "entries-fp");
    let hit = crate::query_reverse_line_index(
        &cache,
        "gen1",
        &BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([1_u32]))]),
    )
    .unwrap();
    assert_eq!(
        hit.get("src/lib.rs").unwrap(),
        &BTreeSet::from(["test_a".to_string()])
    );
    let miss = crate::query_reverse_line_index(
        &cache,
        "gen1",
        &BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([9_u32]))]),
    )
    .unwrap();
    assert!(miss.is_empty());
    assert_reverse_matches_forward_oracle(
        &cache,
        &source,
        "gen1",
        &BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([1_u32]))]),
    );
    assert_reverse_matches_forward_oracle(
        &cache,
        &source,
        "gen1",
        &BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([9_u32]))]),
    );
}

#[test]
fn reverse_index_answers_disjoint_line_requests() {
    let tmp = tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let source = tmp.path().join("src");
    fs::create_dir_all(source.join("src")).unwrap();
    fs::write(source.join("src/lib.rs"), "fn a() {}\nfn b() {}\n").unwrap();
    let lib = source.join("src/lib.rs").to_string_lossy().into_owned();
    write_passed_entry(
        &cache,
        "gen1",
        "test_a",
        BTreeMap::from([(lib.clone(), BTreeSet::from([1_u32]))]),
    );
    write_passed_entry(
        &cache,
        "gen1",
        "test_b",
        BTreeMap::from([(lib, BTreeSet::from([2_u32]))]),
    );
    publish_bound_reverse(&cache, &source, "gen1", "entries-fp");

    let wanted = BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([1_u32, 2_u32]))]);
    assert_reverse_matches_forward_oracle(&cache, &source, "gen1", &wanted);
    let hit = crate::query_reverse_line_index(&cache, "gen1", &wanted).unwrap();
    assert_eq!(
        hit.get("src/lib.rs").unwrap(),
        &BTreeSet::from(["test_a".to_string(), "test_b".to_string()])
    );
}

#[test]
fn reverse_index_answers_multi_range_selector_coverage() {
    let tmp = tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let source = tmp.path().join("src");
    fs::create_dir_all(source.join("src")).unwrap();
    fs::write(
        source.join("src/lib.rs"),
        "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n",
    )
    .unwrap();

    write_passed_entry(
        &cache,
        "gen1",
        "test_multi",
        BTreeMap::from([(
            source.join("src/lib.rs").to_string_lossy().into_owned(),
            BTreeSet::from([1_u32, 2_u32, 4_u32]),
        )]),
    );
    publish_bound_reverse(&cache, &source, "gen1", "entries-fp");
    for wanted in [
        BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([1_u32]))]),
        BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([4_u32]))]),
        BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([1_u32, 4_u32]))]),
        BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([3_u32]))]),
    ] {
        assert_reverse_matches_forward_oracle(&cache, &source, "gen1", &wanted);
    }
    let both = crate::query_reverse_line_index(
        &cache,
        "gen1",
        &BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([1_u32, 4_u32]))]),
    )
    .unwrap();
    assert_eq!(
        both.get("src/lib.rs").unwrap(),
        &BTreeSet::from(["test_multi".to_string()])
    );
    let gap = crate::query_reverse_line_index(
        &cache,
        "gen1",
        &BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([3_u32]))]),
    )
    .unwrap();
    assert!(gap.is_empty());
}

#[test]
fn reverse_index_rejects_other_generation() {
    let tmp = tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let source = tmp.path();
    publish_bound_reverse(&cache, source, "gen1", "fp");
    assert!(
        crate::query_reverse_line_index(
            &cache,
            "gen2",
            &BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([1_u32]))]),
        )
        .is_none()
    );
}

#[test]
fn reverse_index_rejects_entries_fingerprint_mismatch() {
    let tmp = tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let source = tmp.path();
    let info = publish_bound_reverse(&cache, source, "gen1", "fp-a");
    write_population_with_reverse(&cache, "gen1", "fp-b", &info);
    assert!(
        crate::query_reverse_line_index(
            &cache,
            "gen1",
            &BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([1_u32]))]),
        )
        .is_none()
    );
}

#[test]
fn absent_file_from_meta_is_trusted_empty() {
    let tmp = tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let source = tmp.path().join("src");
    fs::create_dir_all(source.join("src")).unwrap();
    fs::write(source.join("src/lib.rs"), "fn a() {}\n").unwrap();
    write_passed_entry(
        &cache,
        "gen1",
        "test_a",
        BTreeMap::from([(
            source.join("src/lib.rs").to_string_lossy().into_owned(),
            BTreeSet::from([1_u32]),
        )]),
    );
    publish_bound_reverse(&cache, &source, "gen1", "fp");
    let hit = crate::query_reverse_line_index(
        &cache,
        "gen1",
        &BTreeMap::from([("src/other.rs".into(), BTreeSet::from([1_u32]))]),
    )
    .unwrap();
    assert!(hit.is_empty());
}

#[test]
fn declared_missing_file_record_makes_query_unavailable() {
    let tmp = tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let source = tmp.path().join("src");
    fs::create_dir_all(source.join("src")).unwrap();
    fs::write(source.join("src/lib.rs"), "fn a() {}\n").unwrap();
    write_passed_entry(
        &cache,
        "gen1",
        "test_a",
        BTreeMap::from([(
            source.join("src/lib.rs").to_string_lossy().into_owned(),
            BTreeSet::from([1_u32]),
        )]),
    );
    let info = publish_bound_reverse(&cache, &source, "gen1", "fp");
    let root = snapshot_path(&cache, &info.snapshot_id);
    let meta_path = root.join("meta.json");
    let mut meta: serde_json::Value =
        serde_json::from_slice(&fs::read(&meta_path).unwrap()).unwrap();
    let files = meta.get_mut("files").unwrap().as_object_mut().unwrap();
    let first_key = files.keys().next().unwrap().clone();
    let record = files.get(&first_key).unwrap().clone();
    let record_name = record.get("record").unwrap().as_str().unwrap();
    fs::remove_file(root.join("files").join(record_name)).unwrap();
    assert_eq!(info.schema_version, REVERSE_LINE_INDEX_SCHEMA);
    assert!(
        crate::query_reverse_line_index(
            &cache,
            "gen1",
            &BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([1_u32]))]),
        )
        .is_none()
    );
}

#[test]
fn snapshot_lives_under_immutable_snapshots_dir() {
    let tmp = tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let source = tmp.path();
    let info = publish_bound_reverse(&cache, source, "gen1", "fp");
    assert!(snapshot_path(&cache, &info.snapshot_id).join("meta.json").is_file());
    assert!(!cache.join("reverse_line_index").join("meta.json").exists());
}
