use crate::publish_derived::batch_derived::publish_conservative_derived_state_from_check_aggregate;
use crate::publish_derived::batch_derived_index::{
    load_current_generation_line_index, load_current_population_state,
};
use crate::test_support::{derived_fixture_request, tamper_json_file, witness_batch_tools};
use crate::{RustLineCoverage, RustTestBinaryIdentity};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[test]
fn load_current_population_state_rejects_tampered_check_aggregate_backing() {
    let fixture = aggregate_backed_population_fixture();
    tamper_json_file(&fixture.req.cache_root, "check_aggregate.json", |value| {
        value["aggregate_covered_lines"]["src/a.rs"] = serde_json::json!([99]);
    });

    assert!(
        load_current_population_state(
            &fixture.req.cache_root,
            fixture.repo.path(),
            &fixture.identity,
            Some(&fixture.selectors),
        )
        .is_none()
    );
    assert!(
        load_current_generation_line_index(&fixture.req.cache_root, fixture.repo.path()).is_none()
    );
}

#[test]
fn load_current_population_state_accepts_empty_check_aggregate_coverage() {
    let fixture = aggregate_backed_population_fixture_with(empty_binary_line_maps());

    let state = load_current_population_state(
        &fixture.req.cache_root,
        fixture.repo.path(),
        &fixture.identity,
        Some(&fixture.selectors),
    )
    .expect("empty aggregate-backed population state");

    assert!(state.line_index.is_empty());
    assert_eq!(state.selectors, fixture.selectors);
    assert_eq!(state.test_binaries.len(), 2);
    assert!(state.entries_fingerprint.starts_with("check-aggregate:"));
    assert!(crate::is_check_aggregate_population(&state));
    assert_eq!(
        load_current_generation_line_index(&fixture.req.cache_root, fixture.repo.path()),
        Some(BTreeMap::new())
    );
}

#[test]
fn load_check_aggregate_population_skips_index_json_body() {
    let fixture = aggregate_backed_population_fixture();
    let index_path = fixture.req.cache_root.join("index.json");
    let original = std::fs::read(&index_path).expect("index.json");
    // Corrupt the huge files map while leaving a parseable stub that would fail
    // full conservative equality if it were consulted.
    std::fs::write(
        &index_path,
        format!(
            "{{\n  \"schema_version\": \"{}\",\n  \"source_root\": \"{}\",\n  \"generation_fingerprint\": \"tampered\",\n  \"entries_fingerprint\": \"tampered\",\n  \"files\": {{}}\n}}\n",
            crate::publish_derived::batch_derived::INDEX_SCHEMA_VERSION,
            fixture.repo.path().canonicalize().unwrap().display()
        ),
    )
    .unwrap();

    let state = load_current_population_state(
        &fixture.req.cache_root,
        fixture.repo.path(),
        &fixture.identity,
        Some(&fixture.selectors),
    )
    .expect("check-aggregate load must not require index.json body");
    assert!(state.line_index.contains_key("src/a.rs"));
    assert!(state.line_index["src/a.rs"].is_empty());
    assert_eq!(state.selectors, fixture.selectors);

    std::fs::write(&index_path, original).unwrap();
}

#[test]
fn load_check_aggregate_population_rejects_malformed_index_json() {
    let fixture = aggregate_backed_population_fixture();
    let index_path = fixture.req.cache_root.join("index.json");
    std::fs::write(&index_path, "{ deliberately broken").unwrap();
    assert!(
        load_current_population_state(
            &fixture.req.cache_root,
            fixture.repo.path(),
            &fixture.identity,
            Some(&fixture.selectors),
        )
        .is_none()
    );
}

#[test]
fn load_check_aggregate_population_accepts_large_index_with_object_prefix() {
    let fixture = aggregate_backed_population_fixture();
    let index_path = fixture.req.cache_root.join("index.json");
    // Larger than the full-parse byte limit: must not DOM-parse; object prefix is enough.
    let mut huge = Vec::with_capacity(4 * 1024 * 1024 + 64);
    huge.extend_from_slice(b"{\n  \"schema_version\": \"x\",\n  \"pad\": \"");
    huge.extend(std::iter::repeat_n(b'a', 4 * 1024 * 1024));
    huge.extend_from_slice(b"\"\n}\n");
    std::fs::write(&index_path, huge).unwrap();

    let state = load_current_population_state(
        &fixture.req.cache_root,
        fixture.repo.path(),
        &fixture.identity,
        Some(&fixture.selectors),
    )
    .expect("large parseable-prefix index must not block check-aggregate load");
    assert!(crate::is_check_aggregate_population(&state));
}

#[test]
fn load_check_aggregate_population_rejects_large_index_without_object_prefix() {
    let fixture = aggregate_backed_population_fixture();
    let index_path = fixture.req.cache_root.join("index.json");
    let mut huge = Vec::with_capacity(4 * 1024 * 1024 + 8);
    huge.extend_from_slice(b"[");
    huge.extend(std::iter::repeat_n(b'a', 4 * 1024 * 1024));
    huge.extend_from_slice(b"]");
    std::fs::write(&index_path, huge).unwrap();
    assert!(
        load_current_population_state(
            &fixture.req.cache_root,
            fixture.repo.path(),
            &fixture.identity,
            Some(&fixture.selectors),
        )
        .is_none()
    );
}

#[test]
fn conservative_check_aggregate_publish_writes_valid_population_index() {
    let fixture = aggregate_backed_population_fixture();
    let state = load_current_population_state(
        &fixture.req.cache_root,
        fixture.repo.path(),
        &fixture.identity,
        Some(&fixture.selectors),
    )
    .expect("aggregate-backed population state");
    assert!(state.line_index.contains_key("src/a.rs"));
    assert!(state.line_index.contains_key("src/b.rs"));
    // In-memory and on-disk check-aggregate indexes are compact (keys only).
    assert!(state.line_index["src/a.rs"].is_empty());
    assert!(state.line_index["src/b.rs"].is_empty());
    assert_eq!(state.selectors, fixture.selectors);
    assert!(crate::is_check_aggregate_population(&state));
    let index: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture.req.cache_root.join("index.json")).unwrap())
            .unwrap();
    assert_eq!(
        index["files"]["src/a.rs"],
        serde_json::json!([]),
        "on-disk check-aggregate index must not duplicate the selector universe per file"
    );
    let population: serde_json::Value = serde_json::from_slice(
        &std::fs::read(fixture.req.cache_root.join("population.json")).unwrap(),
    )
    .unwrap();
    assert!(
        population.get("reverse_line_index").is_none(),
        "CheckAggregate must not claim line-precise reverse ownership"
    );
    assert_eq!(
        state.test_binaries.keys().cloned().collect::<Vec<_>>(),
        vec!["bin-a".to_string(), "bin-b".to_string()]
    );
}

struct AggregateBackedPopulationFixture {
    repo: tempfile::TempDir,
    req: crate::RustCoverageBatchRequest,
    identity: crate::plan::batch_fingerprint::RustCoverageBatchIdentity,
    selectors: Vec<String>,
}

fn aggregate_backed_population_fixture() -> AggregateBackedPopulationFixture {
    aggregate_backed_population_fixture_with(binary_line_maps())
}

fn aggregate_backed_population_fixture_with(
    binary_line_maps: BTreeMap<String, RustLineCoverage>,
) -> AggregateBackedPopulationFixture {
    let repo = tempfile::tempdir().unwrap();
    write_aggregate_repo(repo.path());
    let mut req = derived_fixture_request(repo.path());
    req.logical_selectors = aggregate_selectors();
    req.population_publication_selectors = Some(req.logical_selectors.clone());
    let tools = witness_batch_tools();
    let identity = crate::plan::batch_fingerprint::batch_identity(&req, &tools).unwrap();
    let selectors = aggregate_selectors();
    let aggregate = crate::build_check_aggregate(
        &req,
        &identity,
        &selectors,
        selector_binary_ids(),
        &aggregate_binaries(repo.path()),
        binary_line_maps,
    )
    .unwrap();
    crate::publish_check_aggregate(&req, &aggregate).unwrap();
    publish_conservative_derived_state_from_check_aggregate(&req, &tools, &identity, &aggregate)
        .unwrap();
    AggregateBackedPopulationFixture {
        repo,
        req,
        identity,
        selectors,
    }
}

fn write_aggregate_repo(repo: &Path) {
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::create_dir_all(repo.join("target")).unwrap();
    std::fs::write(repo.join("Cargo.toml"), "[package]\n").unwrap();
    std::fs::write(repo.join("src").join("a.rs"), "pub fn a() {}\n").unwrap();
    std::fs::write(repo.join("src").join("b.rs"), "pub fn b() {}\n").unwrap();
    std::fs::write(repo.join("target").join("bin-a"), "binary-a").unwrap();
    std::fs::write(repo.join("target").join("bin-b"), "binary-b").unwrap();
}

fn aggregate_selectors() -> Vec<String> {
    vec!["alpha".to_string(), "beta".to_string()]
}

fn selector_binary_ids() -> BTreeMap<String, Vec<String>> {
    BTreeMap::from([
        ("alpha".to_string(), vec!["bin-a".to_string()]),
        ("beta".to_string(), vec!["bin-b".to_string()]),
    ])
}

fn aggregate_binaries(repo: &Path) -> [RustTestBinaryIdentity; 2] {
    [
        aggregate_binary(repo, "bin-a", "aaaaaaaaaaaaaaaa"),
        aggregate_binary(repo, "bin-b", "bbbbbbbbbbbbbbbb"),
    ]
}

fn aggregate_binary(repo: &Path, id: &str, digest: &str) -> RustTestBinaryIdentity {
    RustTestBinaryIdentity {
        id: id.to_string(),
        executable: repo.join("target").join(id).to_string_lossy().to_string(),
        digest: digest.to_string(),
    }
}

fn binary_line_maps() -> BTreeMap<String, RustLineCoverage> {
    BTreeMap::from([
        ("bin-a".to_string(), coverage_for("src/a.rs", 1)),
        ("bin-b".to_string(), coverage_for("src/b.rs", 2)),
    ])
}

fn empty_binary_line_maps() -> BTreeMap<String, RustLineCoverage> {
    BTreeMap::from([
        (
            "bin-a".to_string(),
            RustLineCoverage {
                files: BTreeMap::new(),
            },
        ),
        (
            "bin-b".to_string(),
            RustLineCoverage {
                files: BTreeMap::new(),
            },
        ),
    ])
}

fn coverage_for(file: &str, line: u32) -> RustLineCoverage {
    RustLineCoverage {
        files: BTreeMap::from([(file.to_string(), BTreeSet::from([line]))]),
    }
}
