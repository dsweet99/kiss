#![allow(unused_imports)]
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[test]
fn selectors_for_source_paths_skips_uncovered_siblings() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    let missing = tmp.path().join("src").join("missing.rs");
    fs::write(&lib, "pub fn lib() {}\n").unwrap();
    fs::write(&missing, "pub fn missing() {}\n").unwrap();
    let index = BTreeMap::from([(
        "src/lib.rs".to_string(),
        BTreeSet::from(["test_lib".to_string()]),
    )]);

    assert_eq!(
        selectors_for_source_paths(tmp.path(), std::slice::from_ref(&lib), &index).unwrap(),
        BTreeSet::from(["test_lib".to_string()])
    );
    assert_eq!(
        selectors_for_source_paths(tmp.path(), &[lib, missing], &index).unwrap(),
        BTreeSet::from(["test_lib".to_string()]),
        "uncovered source files contribute no selectors but do not abort siblings"
    );
}

#[test]
fn selectors_for_source_paths_fuzz_union_is_monotone_in_index() {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    println!("selectors_for_source_paths_fuzz seed={seed}");
    let mut rng = seed;
    let next = |rng: &mut u64| {
        *rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        *rng
    };
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    let mut paths = Vec::new();
    let mut index = BTreeMap::new();
    for i in 0..8 {
        let path = tmp.path().join("src").join(format!("f{i}.rs"));
        fs::write(&path, format!("pub fn f{i}() {{}}\n")).unwrap();
        if next(&mut rng) % 2 == 0 {
            index.insert(
                format!("src/f{i}.rs"),
                BTreeSet::from([format!("test_{i}")]),
            );
        }
        paths.push(path);
    }
    let selected = selectors_for_source_paths(tmp.path(), &paths, &index).unwrap();
    let mut grown = index.clone();
    grown
        .entry("src/f0.rs".to_string())
        .or_default()
        .insert("test_extra".to_string());
    let selected_grown = selectors_for_source_paths(tmp.path(), &paths, &grown).unwrap();
    assert!(
        selected.is_subset(&selected_grown),
        "adding index entries must not drop selectors: {selected:?} vs {selected_grown:?}"
    );
}

#[test]
fn selectors_for_changed_lines_use_line_intersection() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    fs::write(&lib, "pub fn a() {}\npub fn b() {}\n").unwrap();
    write_test_entry(
        tmp.path(),
        "a",
        "test_line_1",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );
    write_test_entry(
        tmp.path(),
        "b",
        "test_line_2",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([2]))]),
        },
    );

    let selected = select_rust_source_selectors_for_changed_lines(
        tmp.path(),
        &BTreeMap::from([(lib, BTreeSet::from([2]))]),
    )
    .unwrap();

    assert_eq!(selected, BTreeSet::from(["test_line_2".to_string()]));
}

#[test]
fn selectors_for_changed_lines_require_every_changed_file_to_match() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    fs::write(&lib, "pub fn a() {}\npub fn b() {}\n").unwrap();
    write_test_entry(
        tmp.path(),
        "a",
        "test_line_1",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );

    assert!(
        select_rust_source_selectors_for_changed_lines(
            tmp.path(),
            &BTreeMap::from([(lib, BTreeSet::from([2]))]),
        )
        .is_none(),
        "missing changed-line coverage falls back to file-level selection"
    );
}

#[test]
fn hybrid_selection_falls_back_per_file() {
    let (tmp, precise, fallback) = hybrid_selection_fixture();

    let selected = select_rust_source_selectors_hybrid(
        tmp.path(),
        &[precise.clone(), fallback.clone()],
        &BTreeMap::from([
            (precise, BTreeSet::from([2])),
            (fallback, BTreeSet::from([99])),
        ]),
        &[],
    )
    .unwrap();

    assert_eq!(
        selected,
        BTreeSet::from([
            "test_precise_2".to_string(),
            "test_fallback_file".to_string()
        ])
    );
}

fn hybrid_selection_fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    let precise = tmp.path().join("src").join("precise.rs");
    let fallback = tmp.path().join("src").join("fallback.rs");
    fs::write(&precise, "pub fn a() {}\npub fn b() {}\n").unwrap();
    fs::write(&fallback, "pub fn c() {}\npub fn d() {}\n").unwrap();
    write_test_entry(
        tmp.path(),
        "precise_line_1",
        "test_precise_1",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(precise.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );
    write_test_entry(
        tmp.path(),
        "precise_line_2",
        "test_precise_2",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(precise.to_string_lossy().to_string(), BTreeSet::from([2]))]),
        },
    );
    write_test_entry(
        tmp.path(),
        "fallback_file",
        "test_fallback_file",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(fallback.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );
    rebuild_rust_coverage_index(tmp.path()).unwrap();
    write_rust_population_manifest_for_args(
        tmp.path(),
        &[
            "test_precise_1".to_string(),
            "test_precise_2".to_string(),
            "test_fallback_file".to_string(),
        ],
        &[],
    )
    .unwrap();
    (tmp, precise, fallback)
}

#[test]
fn stale_index_is_not_loaded() {
    let tmp = tempfile::tempdir().unwrap();
    let index_path = rust_coverage_index_path(tmp.path());
    fs::create_dir_all(index_path.parent().unwrap()).unwrap();
    fs::write(
        index_path,
        serde_json::json!({
            "schema_version": LEGACY_INDEX_SCHEMA_VERSION,
            "source_root": normalized_repo_root(tmp.path()),
            "entries_fingerprint": "stale",
            "files": {}
        })
        .to_string(),
    )
    .unwrap();

    assert!(load_current_rust_coverage_index(tmp.path(), &[]).is_none());
}


#[test]
fn select_rust_source_selectors_hybrid_empty_sources_returns_empty_set() {
    let tmp = tempfile::tempdir().unwrap();
    let out = select_rust_source_selectors_hybrid(
        tmp.path(),
        &[],
        &BTreeMap::new(),
        &[],
    );
    assert_eq!(out, Some(BTreeSet::new()));
}
