use super::*;
use crate::plan::batch_fingerprint::RustCoverageBatchIdentity;
use crate::plan::batch_plan::RustCoverageBatchRequest;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

#[test]
fn strict_current_rejects_stale_identity_but_reusable_prior_accepts_it() {
    let fixture = AggregateFixture::new();
    publish_check_aggregate(&fixture.req, &fixture.aggregate).unwrap();
    let stale_identity = RustCoverageBatchIdentity {
        input_digest: "new-input".to_string(),
        generation_fingerprint: "new-generation".to_string(),
        selection_context_fingerprint: fixture.identity.selection_context_fingerprint.clone(),
        ordinary_source_digests: fixture.identity.ordinary_source_digests.clone(),
    };

    assert!(
        load_current_check_aggregate_snapshot(
            &fixture.req.cache_root,
            &fixture.req.source_root,
            &stale_identity,
            Some(&fixture.aggregate.selectors),
        )
        .is_none()
    );
    assert!(
        load_reusable_prior_check_aggregate(
            &fixture.req.cache_root,
            &fixture.req.source_root,
            &fixture.aggregate.selectors,
            &stale_identity.selection_context_fingerprint,
        )
        .is_some()
    );
}

#[test]
fn loader_rejects_incomplete_selector_mapping() {
    let fixture = AggregateFixture::new();
    publish_check_aggregate(&fixture.req, &fixture.aggregate).unwrap();
    mutate_raw(&fixture.req.cache_root, |raw| {
        raw["selector_binary_ids"]
            .as_object_mut()
            .unwrap()
            .remove("pkg::bin$alpha");
    });

    assert!(
        load_current_check_aggregate_snapshot(
            &fixture.req.cache_root,
            &fixture.req.source_root,
            &fixture.identity,
            Some(&fixture.aggregate.selectors),
        )
        .is_none()
    );
}

#[test]
fn loader_rejects_aggregate_union_corruption() {
    let fixture = AggregateFixture::new();
    publish_check_aggregate(&fixture.req, &fixture.aggregate).unwrap();
    mutate_raw(&fixture.req.cache_root, |raw| {
        raw["aggregate_covered_lines"]["src/lib.rs"] = serde_json::json!([1]);
    });

    assert!(
        load_current_check_aggregate_snapshot(
            &fixture.req.cache_root,
            &fixture.req.source_root,
            &fixture.identity,
            Some(&fixture.aggregate.selectors),
        )
        .is_none()
    );
}

#[test]
fn loader_rejects_integrity_fingerprint_corruption() {
    let fixture = AggregateFixture::new();
    publish_check_aggregate(&fixture.req, &fixture.aggregate).unwrap();
    mutate_raw(&fixture.req.cache_root, |raw| {
        raw["binaries"][0]["digest"] = serde_json::json!("tampered");
    });

    assert!(
        load_current_check_aggregate_snapshot(
            &fixture.req.cache_root,
            &fixture.req.source_root,
            &fixture.identity,
            Some(&fixture.aggregate.selectors),
        )
        .is_none()
    );
}

#[test]
fn loader_rejects_identity_and_metadata_corruption() {
    for mutate in [
        |raw: &mut serde_json::Value| {
            raw["schema_version"] = serde_json::json!("future");
        },
        |raw: &mut serde_json::Value| {
            raw["cache_schema_version"] = serde_json::json!("future");
        },
        |raw: &mut serde_json::Value| {
            raw["source_root"] = serde_json::json!("/outside/repo");
        },
        |raw: &mut serde_json::Value| {
            raw["selection_context_fingerprint"] = serde_json::json!("different");
        },
        |raw: &mut serde_json::Value| {
            raw["ordinary_source_digests"] = serde_json::json!({"../lib.rs": "digest"});
        },
    ] {
        assert_loader_rejects_mutation(mutate);
    }
}

#[test]
fn loader_rejects_selector_population_corruption() {
    for mutate in [
        |raw: &mut serde_json::Value| {
            raw["selectors"] = serde_json::json!(["pkg::bin$alpha", "pkg::bin$alpha"]);
        },
        |raw: &mut serde_json::Value| {
            raw["selectors"] = serde_json::json!(["pkg::bin$zeta", "pkg::bin$alpha"]);
        },
        |raw: &mut serde_json::Value| {
            raw["selector_binary_ids"]["pkg::bin$alpha"] = serde_json::json!([]);
        },
        |raw: &mut serde_json::Value| {
            raw["selector_binary_ids"]["pkg::bin$alpha"] = serde_json::json!(["bin-a", "bin-a"]);
        },
    ] {
        assert_loader_rejects_mutation(mutate);
    }
}

#[test]
fn loader_rejects_binary_record_corruption() {
    for mutate in [
        |raw: &mut serde_json::Value| {
            raw["binaries"][0]["id"] = serde_json::json!("");
        },
        |raw: &mut serde_json::Value| {
            raw["binaries"][0]["executable"] = serde_json::json!("/outside/repo/bin-a");
        },
        |raw: &mut serde_json::Value| {
            raw["binaries"][0]["digest"] = serde_json::json!("");
        },
        |raw: &mut serde_json::Value| {
            raw["binaries"][0]["line_map"] = serde_json::json!({"../lib.rs": [1]});
        },
        |raw: &mut serde_json::Value| {
            raw["binaries"][0]["line_map"] = serde_json::json!({"src/lib.rs": [0]});
        },
    ] {
        assert_loader_rejects_mutation(mutate);
    }
}

#[test]
fn loader_ignores_partial_temporary_publication_file() {
    let fixture = AggregateFixture::new();
    publish_check_aggregate(&fixture.req, &fixture.aggregate).unwrap();
    fs::write(
        fixture.req.cache_root.join(".check_aggregate.partial.tmp"),
        b"{",
    )
    .unwrap();

    assert!(
        load_current_check_aggregate_snapshot(
            &fixture.req.cache_root,
            &fixture.req.source_root,
            &fixture.identity,
            Some(&fixture.aggregate.selectors),
        )
        .is_some()
    );
}

#[test]
fn build_check_aggregate_reports_missing_binary_identity_and_line_map() {
    let fixture = AggregateFixture::new();
    let selectors = vec!["pkg::bin$alpha".to_string()];
    let selector_binary_ids = BTreeMap::from([(selectors[0].clone(), vec!["missing".to_string()])]);

    let missing_identity = build_check_aggregate(
        &fixture.req,
        &fixture.identity,
        &selectors,
        selector_binary_ids.clone(),
        &[],
        BTreeMap::new(),
    )
    .unwrap_err();
    assert!(matches!(
        missing_identity,
        RustLlvmCovError::InvalidRequest(message)
            if message.contains("missing test-binary identity")
    ));

    let missing_line_map = build_check_aggregate(
        &fixture.req,
        &fixture.identity,
        &selectors,
        selector_binary_ids,
        &[fixture_binary(&fixture.req.source_root, "missing")],
        BTreeMap::new(),
    )
    .unwrap_err();
    assert!(matches!(
        missing_line_map,
        RustLlvmCovError::InvalidRequest(message) if message.contains("missing line map")
    ));
}

struct AggregateFixture {
    _tmp: tempfile::TempDir,
    req: RustCoverageBatchRequest,
    identity: RustCoverageBatchIdentity,
    aggregate: ValidatedCheckAggregate,
}

impl AggregateFixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        write_fixture_files(&root);
        let req = fixture_request(&root);
        let identity = fixture_identity();
        let selectors = vec!["pkg::bin$alpha".to_string()];
        let binary_id = "bin-a".to_string();
        let aggregate = build_check_aggregate(
            &req,
            &identity,
            &selectors,
            BTreeMap::from([(selectors[0].clone(), vec![binary_id.clone()])]),
            &[fixture_binary(&root, &binary_id)],
            BTreeMap::from([(
                binary_id,
                RustLineCoverage {
                    files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1, 2]))]),
                },
            )]),
        )
        .unwrap();
        Self {
            _tmp: tmp,
            req,
            identity,
            aggregate,
        }
    }
}

fn write_fixture_files(root: &Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("target")).unwrap();
    fs::write(root.join("src").join("lib.rs"), "pub fn covered() {}\n").unwrap();
    fs::write(root.join("target").join("bin-a"), "binary").unwrap();
}

fn fixture_request(root: &Path) -> RustCoverageBatchRequest {
    let mut req = RustCoverageBatchRequest::witness();
    req.cwd = root.to_path_buf();
    req.source_root = root.to_path_buf();
    req.cache_root = root.join(".kiss").join("rust_llvm_cov_cache");
    req.generated_config = req
        .cache_root
        .join("runs")
        .join("run-a")
        .join("nextest.toml");
    req
}

fn fixture_identity() -> RustCoverageBatchIdentity {
    RustCoverageBatchIdentity {
        input_digest: "input".to_string(),
        generation_fingerprint: "generation".to_string(),
        selection_context_fingerprint: "selection".to_string(),
        ordinary_source_digests: BTreeMap::from([("src/lib.rs".to_string(), "digest".to_string())]),
    }
}

fn fixture_binary(root: &Path, binary_id: &str) -> RustTestBinaryIdentity {
    RustTestBinaryIdentity {
        id: binary_id.to_string(),
        executable: root
            .join("target")
            .join("bin-a")
            .to_string_lossy()
            .to_string(),
        digest: "binary-digest".to_string(),
    }
}

fn mutate_raw(cache_root: &Path, mutate: impl FnOnce(&mut serde_json::Value)) {
    let path = check_aggregate_path(cache_root);
    let mut raw: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    mutate(&mut raw);
    fs::write(path, serde_json::to_vec_pretty(&raw).unwrap()).unwrap();
}

fn assert_loader_rejects_mutation(mutate: fn(&mut serde_json::Value)) {
    let fixture = AggregateFixture::new();
    publish_check_aggregate(&fixture.req, &fixture.aggregate).unwrap();
    mutate_raw(&fixture.req.cache_root, mutate);

    assert!(
        load_current_check_aggregate_snapshot(
            &fixture.req.cache_root,
            &fixture.req.source_root,
            &fixture.identity,
            Some(&fixture.aggregate.selectors),
        )
        .is_none()
    );
}
