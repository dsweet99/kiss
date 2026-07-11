use std::collections::{BTreeMap, BTreeSet};

use crate::RustPopulationState;
use crate::batch_derived::{INDEX_SCHEMA_VERSION, POPULATION_SCHEMA_VERSION};
use crate::batch_derived_index::{
    OnDiskIndex, OnDiskIndexWithFiles, PopulationManifestOnDisk,
    load_current_generation_line_index, load_current_population_state, read_coverage_index,
    read_population_generation, read_population_manifest,
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
        generation_fingerprint: state.generation_fingerprint.clone(),
        entries_fingerprint: state.entries_fingerprint.clone(),
        selectors: state.selectors.clone(),
        line_index: state.line_index.clone(),
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
        entries_fingerprint: "entries".to_string(),
        selectors: vec!["alpha".to_string()],
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
        generation_fingerprint: state.generation_fingerprint.clone(),
        entries_fingerprint: state.entries_fingerprint.clone(),
        selectors: state.selectors.clone(),
        line_index: state.line_index.clone(),
    };
    assert_eq!(state, literal);
}
