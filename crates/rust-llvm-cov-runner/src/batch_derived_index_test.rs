use std::collections::{BTreeMap, BTreeSet};

use crate::RustPopulationState;
use crate::batch_derived::{INDEX_SCHEMA_VERSION, POPULATION_SCHEMA_VERSION};
use crate::batch_derived_index::{
    OnDiskIndex, OnDiskIndexWithFiles, PopulationManifestOnDisk, RustSnapshotDelta,
    load_current_generation_coverage_snapshot, load_current_generation_line_index,
    load_current_population_state, read_coverage_index, read_population_generation,
    read_population_manifest, reusable_snapshot_delta,
};
use crate::test_support::{published_alpha_derived_fixture, tamper_json_file};

fn read_on_disk_index_with_files(cache_root: &std::path::Path) -> Option<OnDiskIndexWithFiles> {
    let bytes = std::fs::read(cache_root.join("index.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[test]
fn load_current_population_state_validates_selectors_and_returns_index() {
    let fixture = published_alpha_derived_fixture();
    let state = load_current_population_state(
        &fixture.req.cache_root,
        fixture.repo.path(),
        &fixture.identity,
        Some(&["alpha".to_string()]),
    )
    .expect("population state");
    assert!(state.line_index.contains_key("src/lib.rs"));
    assert_eq!(state.selectors, vec!["alpha".to_string()]);
    assert!(
        load_current_population_state(
            &fixture.req.cache_root,
            fixture.repo.path(),
            &fixture.identity,
            Some(&["beta".to_string()]),
        )
        .is_none()
    );
    assert!(
        load_current_population_state(
            &fixture.req.cache_root,
            fixture.repo.path(),
            &fixture.identity,
            None,
        )
        .expect("index without selector check")
        .line_index
        .contains_key("src/lib.rs")
    );
    let expected = RustPopulationState {
        input_fingerprint: state.input_fingerprint.clone(),
        generation_fingerprint: state.generation_fingerprint.clone(),
        selection_context_fingerprint: state.selection_context_fingerprint.clone(),
        entries_fingerprint: state.entries_fingerprint.clone(),
        selectors: state.selectors.clone(),
        line_index: state.line_index.clone(),
        ordinary_source_digests: state.ordinary_source_digests.clone(),
        test_binaries: state.test_binaries.clone(),
    };
    assert_eq!(state, expected);
    let on_disk: OnDiskIndexWithFiles =
        read_on_disk_index_with_files(&fixture.req.cache_root).expect("on-disk index with files");
    assert_eq!(
        on_disk.generation_fingerprint,
        fixture.identity.generation_fingerprint
    );
    assert_eq!(on_disk.entries_fingerprint, state.entries_fingerprint);
    assert_eq!(
        on_disk.source_root,
        fixture
            .repo
            .path()
            .canonicalize()
            .unwrap()
            .to_string_lossy()
    );
    assert!(on_disk.files.contains_key("src/lib.rs"));
}

#[test]
fn load_current_generation_line_index_returns_published_files() {
    let fixture = published_alpha_derived_fixture();
    let index = load_current_generation_line_index(&fixture.req.cache_root, fixture.repo.path())
        .expect("current generation index");
    assert!(index.contains_key("src/lib.rs"));
}

#[test]
fn load_current_generation_coverage_snapshot_returns_published_lines() {
    let fixture = published_alpha_derived_fixture();
    let snapshot = load_current_generation_coverage_snapshot(
        &fixture.req.cache_root,
        fixture.repo.path(),
        &fixture.identity,
        Some(&["alpha".to_string()]),
    )
    .expect("current generation coverage snapshot");

    assert_eq!(snapshot.population.selectors, vec!["alpha".to_string()]);
    assert_eq!(
        snapshot.covered_lines["src/lib.rs"],
        BTreeSet::from([1_u32])
    );
    assert!(!snapshot.identity.is_empty());
}

#[test]
fn load_current_generation_line_index_rejects_mismatched_manifest() {
    let fixture = published_alpha_derived_fixture();
    tamper_json_file(&fixture.req.cache_root, "population.json", |value| {
        value["entries_fingerprint"] = serde_json::Value::String("wrong".to_string());
    });
    assert!(
        load_current_generation_line_index(&fixture.req.cache_root, fixture.repo.path()).is_none()
    );
}

#[test]
fn load_current_generation_line_index_rejects_wrong_manifest_schema() {
    let fixture = published_alpha_derived_fixture();
    tamper_json_file(&fixture.req.cache_root, "population.json", |value| {
        value["schema_version"] = serde_json::Value::String("wrong".to_string());
    });
    assert!(
        load_current_generation_line_index(&fixture.req.cache_root, fixture.repo.path()).is_none()
    );
}

#[test]
fn load_current_generation_line_index_rejects_generation_fingerprint_mismatch() {
    let fixture = published_alpha_derived_fixture();
    tamper_json_file(&fixture.req.cache_root, "population.json", |value| {
        value["generation_fingerprint"] = serde_json::Value::String("wrong".to_string());
    });
    assert!(
        load_current_generation_line_index(&fixture.req.cache_root, fixture.repo.path()).is_none()
    );
}

#[test]
fn load_current_generation_line_index_rejects_tampered_index_files() {
    let fixture = published_alpha_derived_fixture();
    tamper_json_file(&fixture.req.cache_root, "index.json", |value| {
        if let Some(files) = value.get_mut("files") {
            *files = serde_json::json!({});
        }
    });
    assert!(
        load_current_generation_line_index(&fixture.req.cache_root, fixture.repo.path()).is_none()
    );
}

#[test]
fn read_population_and_index_loaders_handle_missing_and_invalid_json() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(read_population_manifest(tmp.path()).is_none());
    assert!(read_coverage_index(tmp.path()).is_none());
    assert!(read_population_generation(tmp.path()).is_none());

    std::fs::write(tmp.path().join("population.json"), b"{").unwrap();
    std::fs::write(tmp.path().join("index.json"), b"[").unwrap();
    assert!(read_population_manifest(tmp.path()).is_none());
    assert!(read_coverage_index(tmp.path()).is_none());
    assert!(read_population_generation(tmp.path()).is_none());

    let index = OnDiskIndex {
        schema_version: INDEX_SCHEMA_VERSION.to_string(),
        generation_fingerprint: "gen".to_string(),
        entries_fingerprint: "entries".to_string(),
    };
    let manifest = PopulationManifestOnDisk {
        schema_version: POPULATION_SCHEMA_VERSION.to_string(),
        generation_fingerprint: "gen".to_string(),
        input_fingerprint: "input".to_string(),
        selection_context_fingerprint: "context".to_string(),
        entries_fingerprint: "entries".to_string(),
        selectors: vec!["alpha".to_string()],
        ordinary_source_digests: BTreeMap::new(),
        test_binaries: BTreeMap::new(),
    };
    assert_eq!(
        index.generation_fingerprint,
        manifest.generation_fingerprint
    );
    let _files: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
}

#[test]
fn read_population_generation_reads_generation_fingerprint() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("population.json"),
        br#"{"generation_fingerprint":"gen-42"}"#,
    )
    .unwrap();
    assert_eq!(
        read_population_generation(tmp.path()).as_deref(),
        Some("gen-42")
    );
}

#[test]
fn rust_population_state_exposes_generation_and_index_fields() {
    let fixture = published_alpha_derived_fixture();
    let state = load_current_population_state(
        &fixture.req.cache_root,
        fixture.repo.path(),
        &fixture.identity,
        Some(&["alpha".to_string()]),
    )
    .expect("population state");
    assert_eq!(
        state.generation_fingerprint,
        fixture.identity.generation_fingerprint
    );
    assert!(!state.entries_fingerprint.is_empty());
    assert_eq!(state.selectors, vec!["alpha".to_string()]);
    assert!(state.line_index.contains_key("src/lib.rs"));
    let cloned = state.clone();
    assert_eq!(format!("{cloned:?}"), format!("{state:?}"));
    assert_eq!(state, cloned);
    let on_disk = read_on_disk_index_with_files(&fixture.req.cache_root).expect("on-disk index");
    assert_eq!(
        on_disk.generation_fingerprint,
        fixture.identity.generation_fingerprint
    );
    assert_eq!(on_disk.entries_fingerprint, state.entries_fingerprint);
    assert!(on_disk.files.contains_key("src/lib.rs"));
    let literal = RustPopulationState {
        input_fingerprint: state.input_fingerprint.clone(),
        generation_fingerprint: state.generation_fingerprint.clone(),
        selection_context_fingerprint: state.selection_context_fingerprint.clone(),
        entries_fingerprint: state.entries_fingerprint.clone(),
        selectors: state.selectors.clone(),
        line_index: state.line_index.clone(),
        ordinary_source_digests: state.ordinary_source_digests.clone(),
        test_binaries: state.test_binaries.clone(),
    };
    assert_eq!(state, literal);
}

#[test]
fn manifest_generation_entries_complete_rejects_duplicate_selectors() {
    let fixture = published_alpha_derived_fixture();
    let duplicate = fixture.req.cache_root.join("entries/duplicate.json");
    let entry = std::fs::read_to_string(
        fixture
            .req
            .cache_root
            .join("entries")
            .read_dir()
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
            .expect("entry")
            .path(),
    )
    .unwrap();
    std::fs::write(&duplicate, entry).unwrap();
    assert!(
        load_current_population_state(
            &fixture.req.cache_root,
            fixture.repo.path(),
            &fixture.identity,
            Some(&["alpha".to_string()]),
        )
        .is_none()
    );
}

#[test]
fn on_disk_index_with_files_round_trips_through_loader() {
    let fixture = published_alpha_derived_fixture();
    let index = read_on_disk_index_with_files(&fixture.req.cache_root).expect("index");
    assert_eq!(index.schema_version, INDEX_SCHEMA_VERSION);
    assert_eq!(
        index.source_root,
        fixture
            .repo
            .path()
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string()
    );
    assert!(!index.generation_fingerprint.is_empty());
    assert!(!index.entries_fingerprint.is_empty());
    assert!(index.files.contains_key("src/lib.rs"));
    let disk = read_coverage_index(&fixture.req.cache_root).expect("disk index");
    assert_eq!(disk.generation_fingerprint, index.generation_fingerprint);
}

#[test]
fn malformed_entry_files_do_not_invalidate_complete_population() {
    let fixture = published_alpha_derived_fixture();
    std::fs::write(
        fixture.req.cache_root.join("entries/bad.json"),
        b"{not-json",
    )
    .unwrap();
    assert!(
        load_current_population_state(
            &fixture.req.cache_root,
            fixture.repo.path(),
            &fixture.identity,
            Some(&["alpha".to_string()]),
        )
        .is_some()
    );
}

#[test]
fn reusable_snapshot_delta_reports_unchanged_modified_and_structural_cases() {
    let root = std::path::Path::new("/repo");
    let prior = BTreeMap::from([
        ("src/a.rs".to_string(), "aaaaaaaaaaaaaaaa".to_string()),
        ("src/b.rs".to_string(), "bbbbbbbbbbbbbbbb".to_string()),
    ]);
    assert_eq!(
        reusable_snapshot_delta(root, &prior, &prior),
        RustSnapshotDelta::Unchanged
    );

    let current = BTreeMap::from([
        ("src/a.rs".to_string(), "cccccccccccccccc".to_string()),
        ("src/b.rs".to_string(), "dddddddddddddddd".to_string()),
    ]);
    assert_eq!(
        reusable_snapshot_delta(root, &prior, &current),
        RustSnapshotDelta::Modified(vec![root.join("src/a.rs"), root.join("src/b.rs")])
    );

    let added = BTreeMap::from([
        ("src/a.rs".to_string(), "aaaaaaaaaaaaaaaa".to_string()),
        ("src/b.rs".to_string(), "bbbbbbbbbbbbbbbb".to_string()),
        ("src/c.rs".to_string(), "cccccccccccccccc".to_string()),
    ]);
    assert_eq!(
        reusable_snapshot_delta(root, &prior, &added),
        RustSnapshotDelta::StructuralChange
    );

    let renamed = BTreeMap::from([
        ("src/a.rs".to_string(), "aaaaaaaaaaaaaaaa".to_string()),
        ("src/c.rs".to_string(), "bbbbbbbbbbbbbbbb".to_string()),
    ]);
    assert_eq!(
        reusable_snapshot_delta(root, &prior, &renamed),
        RustSnapshotDelta::StructuralChange
    );
}
