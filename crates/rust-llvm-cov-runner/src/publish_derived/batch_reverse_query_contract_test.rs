
use crate::publish_derived::batch_entry_state::publish_next_entry_state;
use crate::publish_derived::batch_reverse_build::{hex_digest, BuiltReverseIndex, FileReverseRecord};
use crate::publish_derived::batch_reverse_publish::{
    prune_unreferenced_snapshots, snapshot_path, write_reverse_snapshot,
};
use crate::publish_derived::batch_reverse_test_support::{
    publish_bound_reverse, write_passed_entry, write_population_with_reverse,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use tempfile::tempdir;

fn seed(cache: &Path, source: &Path) {
    fs::create_dir_all(source.join("src")).unwrap();
    fs::write(source.join("src/lib.rs"), "fn a() {}\n").unwrap();
    write_passed_entry(
        cache,
        "gen1",
        "test_a",
        BTreeMap::from([(
            source.join("src/lib.rs").to_string_lossy().into_owned(),
            BTreeSet::from([1_u32]),
        )]),
    );
}

fn wanted() -> BTreeMap<String, BTreeSet<u32>> {
    BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([1_u32]))])
}

fn rewrite_population_meta_digest(cache: &Path, meta_bytes: &[u8]) {
    let mut pop: serde_json::Value =
        serde_json::from_slice(&fs::read(cache.join("population.json")).unwrap()).unwrap();
    pop["reverse_line_index"]["meta_digest"] = serde_json::json!(hex_digest(meta_bytes));
    fs::write(
        cache.join("population.json"),
        serde_json::to_vec_pretty(&pop).unwrap(),
    )
    .unwrap();
}

fn first_file_record(root: &Path) -> std::path::PathBuf {
    fs::read_dir(root.join("files"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
}

fn write_meta_with_newline(root: &Path, meta: &serde_json::Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(meta).unwrap();
    bytes.push(b'\n');
    fs::write(root.join("meta.json"), &bytes).unwrap();
    bytes
}

#[test]
fn reverse_rejects_wrong_schema_and_snapshot_id() {
    let tmp = tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let source = tmp.path().join("src");
    seed(&cache, &source);
    let info = publish_bound_reverse(&cache, &source, "gen1", "fp");
    let mut bad: serde_json::Value =
        serde_json::from_slice(&fs::read(cache.join("population.json")).unwrap()).unwrap();
    bad["reverse_line_index"]["schema_version"] = serde_json::json!("wrong");
    fs::write(
        cache.join("population.json"),
        serde_json::to_vec_pretty(&bad).unwrap(),
    )
    .unwrap();
    assert!(crate::query_reverse_line_index(&cache, "gen1", &wanted()).is_none());
    write_population_with_reverse(&cache, "gen1", "fp", &info);
    bad = serde_json::from_slice(&fs::read(cache.join("population.json")).unwrap()).unwrap();
    bad["reverse_line_index"]["snapshot_id"] = serde_json::json!("no-such");
    fs::write(
        cache.join("population.json"),
        serde_json::to_vec_pretty(&bad).unwrap(),
    )
    .unwrap();
    assert!(crate::query_reverse_line_index(&cache, "gen1", &wanted()).is_none());
}

#[test]
fn reverse_rejects_tampered_selectors_digest() {
    let tmp = tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let source = tmp.path().join("src");
    seed(&cache, &source);
    let info = publish_bound_reverse(&cache, &source, "gen1", "fp");
    let root = snapshot_path(&cache, &info.snapshot_id);
    let mut meta: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("meta.json")).unwrap()).unwrap();
    meta["selectors_digest"] = serde_json::json!("0".repeat(64));
    let meta_bytes = write_meta_with_newline(&root, &meta);
    rewrite_population_meta_digest(&cache, &meta_bytes);
    assert!(crate::query_reverse_line_index(&cache, "gen1", &wanted()).is_none());
}

#[test]
fn reverse_rejects_malformed_meta_json() {
    let tmp = tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let source = tmp.path().join("src");
    seed(&cache, &source);
    let info = publish_bound_reverse(&cache, &source, "gen1", "fp");
    fs::write(snapshot_path(&cache, &info.snapshot_id).join("meta.json"), b"{").unwrap();
    assert!(crate::query_reverse_line_index(&cache, "gen1", &wanted()).is_none());
}

#[test]
fn reverse_rejects_unknown_selector_id() {
    let tmp = tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let source = tmp.path().join("src");
    seed(&cache, &source);
    let info = publish_bound_reverse(&cache, &source, "gen1", "fp");
    let root = snapshot_path(&cache, &info.snapshot_id);
    let record_path = first_file_record(&root);
    let bad_record = FileReverseRecord {
        file: "src/lib.rs".into(),
        ranges: vec![(1, 1, vec![99])],
    };
    let mut record_bytes = serde_json::to_vec(&bad_record).unwrap();
    record_bytes.push(b'\n');
    fs::write(&record_path, &record_bytes).unwrap();
    let mut meta: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("meta.json")).unwrap()).unwrap();
    let key = meta["files"].as_object().unwrap().keys().next().unwrap().clone();
    meta["files"][&key]["digest"] = serde_json::json!(hex_digest(&record_bytes));
    let meta_bytes = write_meta_with_newline(&root, &meta);
    rewrite_population_meta_digest(&cache, &meta_bytes);
    assert!(crate::query_reverse_line_index(&cache, "gen1", &wanted()).is_none());
}

#[test]
fn reverse_rejects_malformed_file_record_bytes() {
    let tmp = tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let source = tmp.path().join("src");
    seed(&cache, &source);
    let info = publish_bound_reverse(&cache, &source, "gen1", "fp");
    let root = snapshot_path(&cache, &info.snapshot_id);
    fs::write(first_file_record(&root), b"not-json").unwrap();
    assert!(crate::query_reverse_line_index(&cache, "gen1", &wanted()).is_none());
}

#[test]
fn reverse_rejects_file_digest_mismatch() {
    let tmp = tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let source = tmp.path().join("src");
    seed(&cache, &source);
    let info = publish_bound_reverse(&cache, &source, "gen1", "fp");
    let root = snapshot_path(&cache, &info.snapshot_id);

    let mut meta: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("meta.json")).unwrap()).unwrap();
    let key = meta["files"].as_object().unwrap().keys().next().unwrap().clone();
    meta["files"][&key]["digest"] = serde_json::json!("0".repeat(64));
    let meta_bytes = write_meta_with_newline(&root, &meta);
    rewrite_population_meta_digest(&cache, &meta_bytes);
    assert!(
        crate::query_reverse_line_index(&cache, "gen1", &wanted()).is_none(),
        "digest-mismatched declared file record must make reverse unavailable"
    );
}

#[test]
fn revision_advance_and_reverse_hit_avoid_entry_reads() {
    let tmp = tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let source = tmp.path().join("src");
    seed(&cache, &source);
    publish_bound_reverse(&cache, &source, "gen1", "fp-a");
    let entries = cache.join("entries");
    let mode = entries.metadata().unwrap().permissions().mode();
    fs::set_permissions(&entries, fs::Permissions::from_mode(0o000)).unwrap();
    let hit_ok = crate::query_reverse_line_index(&cache, "gen1", &wanted());
    publish_next_entry_state(&cache, "gen1", "fp-b").unwrap();
    let hit_stale = crate::query_reverse_line_index(&cache, "gen1", &wanted());
    fs::set_permissions(&entries, fs::Permissions::from_mode(mode)).unwrap();
    assert_eq!(
        hit_ok.unwrap().get("src/lib.rs").unwrap(),
        &BTreeSet::from(["test_a".to_string()])
    );
    assert!(hit_stale.is_none());
}

#[test]
fn abandoned_staging_reclaimed_without_activation() {
    let tmp = tempdir().unwrap();
    let cache = tmp.path();
    let built = BuiltReverseIndex {
        selectors: vec!["a".into()],
        files: BTreeMap::new(),
    };
    let r1 = publish_next_entry_state(cache, "gen", "fp").unwrap();
    let active = write_reverse_snapshot(cache, "gen", "fp", r1, &built).unwrap();
    let staging = cache.join("reverse_line_index").join("snapshots").join(format!(
        ".staging.{}",
        kiss_publication_barrier::unique_process_suffix()
    ));
    fs::create_dir_all(staging.join("files")).unwrap();
    fs::write(staging.join("meta.json"), b"{}").unwrap();
    assert!(!cache.join("population.json").exists());
    let removed = prune_unreferenced_snapshots(cache, &active.snapshot_id, None).unwrap();
    assert!(removed >= 1);
    assert!(!staging.exists());
    assert!(snapshot_path(cache, &active.snapshot_id).is_dir());
}
