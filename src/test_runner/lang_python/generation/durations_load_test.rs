use super::*;
use super::super::paths::{generation_dir, pointer_path};
use super::super::publish::{
    GENERATION_DURATIONS_SCHEMA, GenerationDurationsFile, PathMaxDuration,
};
use super::super::types::{POINTER_SCHEMA_VERSION, PopulationPointer, SelectorTimingRecord};
use crate::test_runner::python_coverage_index::storage::python_coverage_cache_root;
use std::time::Duration;

fn write_generation_fixture(repo: &Path, with_path_maxes: bool) -> PathBuf {
    clear_generation_durations_memo();
    let cache_root = python_coverage_cache_root(repo).expect("cache root");
    let gen_id = format!(
        "gen-durations-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let gen_dir = generation_dir(&cache_root, &gen_id);
    fs::create_dir_all(&gen_dir).unwrap();
    let pointer = PopulationPointer {
        schema_version: POINTER_SCHEMA_VERSION.to_string(),
        generation_id: gen_id.clone(),
        manifest_sha256: "abc".into(),
    };
    fs::write(
        pointer_path(&cache_root),
        serde_json::to_vec_pretty(&pointer).unwrap(),
    )
    .unwrap();
    let path_maxes = if with_path_maxes {
        vec![PathMaxDuration {
            path: "tests/test_a.py".into(),
            max_duration_ns: 5_000_000,
            example_selector: "tests/test_a.py::test_one".into(),
        }]
    } else {
        vec![]
    };
    let durations = GenerationDurationsFile {
        schema_version: GENERATION_DURATIONS_SCHEMA.to_string(),
        durations_ns: vec![Some(1_000_000), Some(5_000_000)],
        max_duration_ns: 5_000_000,
        path_maxes,
    };
    fs::write(
        gen_dir.join("durations.json"),
        serde_json::to_vec_pretty(&durations).unwrap(),
    )
    .unwrap();
    let manifest = serde_json::json!({
        "plan": {
            "selectors": [
                "tests/test_a.py::test_one",
                "tests/test_b.py::test_two"
            ]
        }
    });
    fs::write(
        gen_dir.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    gen_dir
}

#[test]
fn load_pairs_and_max_from_fixture_and_memoize() {
    let tmp = tempfile::tempdir().unwrap();
    write_generation_fixture(tmp.path(), true);
    let pairs = try_load_generation_durations_pairs(tmp.path()).expect("pairs");
    assert_eq!(pairs.len(), 2);
    assert_eq!(pairs[0].1, Duration::from_nanos(1_000_000));
    assert_eq!(
        try_load_generation_max_duration(tmp.path()),
        Some(Duration::from_nanos(5_000_000))
    );
    let maxes = try_load_generation_path_maxes(tmp.path()).expect("path maxes");
    assert!(!maxes.is_empty());
    assert_eq!(
        try_load_generation_durations_pairs(tmp.path())
            .expect("memo pairs")
            .len(),
        2
    );
    clear_generation_durations_memo();
}

#[test]
fn path_maxes_only_reads_durations_without_pairs_poison() {
    let tmp = tempfile::tempdir().unwrap();
    write_generation_fixture(tmp.path(), true);
    clear_generation_durations_memo();
    let maxes = try_load_generation_path_maxes_only(tmp.path()).expect("path maxes only");
    assert_eq!(maxes[0].path, "tests/test_a.py");
    assert_eq!(
        try_load_generation_path_maxes_only(tmp.path())
            .expect("memo")
            .len(),
        1
    );
}

#[test]
fn legacy_durations_without_path_maxes_are_backfilled() {
    let tmp = tempfile::tempdir().unwrap();
    let gen_dir = write_generation_fixture(tmp.path(), false);
    clear_generation_durations_memo();
    let maxes = try_load_generation_path_maxes(tmp.path()).expect("backfill");
    assert!(!maxes.is_empty());
    let on_disk: GenerationDurationsFile =
        serde_json::from_slice(&fs::read(gen_dir.join("durations.json")).unwrap()).unwrap();
    assert!(!on_disk.path_maxes.is_empty());
}

#[test]
fn missing_or_mismatched_artifacts_return_none() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(try_load_generation_durations_pairs(tmp.path()).is_none());
    assert!(try_load_generation_max_duration(tmp.path()).is_none());
    assert!(try_load_generation_path_maxes_only(tmp.path()).is_none());
    let gen_dir = write_generation_fixture(tmp.path(), true);
    let bad = GenerationDurationsFile {
        schema_version: GENERATION_DURATIONS_SCHEMA.to_string(),
        durations_ns: vec![Some(1)],
        max_duration_ns: 1,
        path_maxes: vec![],
    };
    fs::write(
        gen_dir.join("durations.json"),
        serde_json::to_vec_pretty(&bad).unwrap(),
    )
    .unwrap();
    clear_generation_durations_memo();
    assert!(try_load_generation_durations_pairs(tmp.path()).is_none());
}

#[test]
fn unresolved_durations_are_not_collapsed_to_zero_in_sidecars() {
    use super::super::types::TimingCacheDisposition;
    let timings = vec![
        SelectorTimingRecord {
            selector: "tests/a.py::known".into(),
            raw_status: "passed".into(),
            effective_status: "passed".into(),
            duration_ns: Some(4_000_000),
            cache_disposition: TimingCacheDisposition::MissStored,
            reason: None,
        },
        SelectorTimingRecord {
            selector: "tests/a.py::unresolved".into(),
            raw_status: "unresolved".into(),
            effective_status: "unresolved".into(),
            duration_ns: None,
            cache_disposition: TimingCacheDisposition::Unknown,
            reason: Some("missing outcome".into()),
        },
        SelectorTimingRecord {
            selector: "tests/b.py::only_unresolved".into(),
            raw_status: "unresolved".into(),
            effective_status: "unresolved".into(),
            duration_ns: None,
            cache_disposition: TimingCacheDisposition::Unknown,
            reason: Some("missing outcome".into()),
        },
    ];
    let file = super::super::publish::generation_durations_file(&timings);
    assert_eq!(
        file.durations_ns,
        vec![Some(4_000_000), None, None],
        "absence must remain None, not 0"
    );
    assert_eq!(file.max_duration_ns, 4_000_000);
    let paths: Vec<_> = file.path_maxes.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(paths, vec!["tests/a.py"]);
    assert_eq!(file.path_maxes[0].max_duration_ns, 4_000_000);
    assert!(
        !file
            .path_maxes
            .iter()
            .any(|p| p.path == "tests/b.py" || p.max_duration_ns == 0),
        "path with only unresolved timings must not invent a 0 ns max"
    );
}
