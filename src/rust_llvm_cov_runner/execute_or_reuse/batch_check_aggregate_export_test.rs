use super::*;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_shim::SHIM_LIST_SCHEMA;
use std::time::Duration;

fn fake_exporter(fail_binary_id: Option<String>) -> CheckAggregateExportFn {
    Arc::new(move |request, _source_root, _catalog| {
        if fail_binary_id.as_deref() == Some(request.binary_id.as_str()) {
            return Err(RustLlvmCovError::InvalidRequest(format!(
                "fake export failure for {}",
                request.binary_id
            )));
        }
        Ok((
            request.binary_id.clone(),
            RustLineCoverage {
                files: BTreeMap::from([(
                    format!("src/{}.rs", request.binary_id),
                    BTreeSet::from([request.objects.len() as u32]),
                )]),
            },
        ))
    })
}

fn instance(name: &str, binary_id: &str, passed: bool) -> InstanceResult {
    InstanceResult {
        full_name: name.to_string(),
        test_binary_id: binary_id.to_string(),
        passed,
        timed_out: false,
        exit_code: Some(if passed { 0 } else { 1 }),
        duration: Duration::from_millis(1),
        stdout: None,
        stderr: None,
        coverage: RustLineCoverage {
            files: BTreeMap::new(),
        },
    }
}

fn shim(name: &str, exe: &str, profile: &str) -> BatchShimMetadata {
    BatchShimMetadata {
        schema_version: SHIM_LIST_SCHEMA.to_string(),
        id: name.to_string(),
        full_name: name.to_string(),
        profile_path: PathBuf::from(profile),
        cwd: PathBuf::from("."),
        argv: vec![exe.to_string()],
        exit_code: Some(0),
        spawn_error: None,
        shim_identity: None,
        delegated_identity: None,
        stdout: None,
        stderr: None,
        output_frame_count: None,
    }
}

fn export_request(binary_id: &str, object_count: usize) -> CheckAggregateExportRequest {
    CheckAggregateExportRequest {
        binary_id: binary_id.to_string(),
        instance_names: vec![format!("{binary_id}::test")],
        profile_paths: vec![PathBuf::from(format!("/tmp/{binary_id}.profraw"))],
        objects: (0..object_count)
            .map(|index| PathBuf::from(format!("/tmp/{binary_id}-{index}.o")))
            .collect(),
    }
}

#[test]
fn builds_requests_from_passed_instances_and_deduplicates_paths() {
    let instances = vec![
        instance("bin::test_b", "bin", true),
        instance("bin::test_a", "bin", true),
        instance("bin::failed", "bin", false),
    ];
    let shim_metadata = vec![
        shim("bin::test_b", "/tmp/test-bin", "/tmp/profiles/b.profraw"),
        shim("bin::test_a", "/tmp/test-bin", "/tmp/profiles/a.profraw"),
        shim(
            "bin::failed",
            "/tmp/test-bin",
            "/tmp/profiles/failed.profraw",
        ),
    ];
    let artifacts = vec![BatchCompilerArtifact {
        executable: Some("/tmp/test-bin".to_string()),
        filenames: vec!["/tmp/test-bin.o".to_string(), "/tmp/test-bin.o".to_string()],
        nextest_binary_id: None,
        libtest_binary_prefix: None,
        src_path: None,
        is_test_harness: false,
    }];

    let requests =
        build_check_aggregate_export_requests(&instances, &shim_metadata, &artifacts, None)
            .unwrap();

    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].binary_id, "bin");
    assert_eq!(
        requests[0].instance_names,
        vec!["bin::test_a".to_string(), "bin::test_b".to_string()]
    );
    assert_eq!(
        requests[0].profile_paths,
        vec![
            PathBuf::from("/tmp/profiles/a.profraw"),
            PathBuf::from("/tmp/profiles/b.profraw")
        ]
    );
    assert_eq!(requests[0].objects, vec![PathBuf::from("/tmp/test-bin.o")]);
}

#[test]
fn publication_filter_reports_missing_binary_ids() {
    let instances = vec![instance("bin::test", "bin", true)];
    let shim_metadata = vec![shim(
        "bin::test",
        "/tmp/test-bin",
        "/tmp/profiles/a.profraw",
    )];
    let artifacts = vec![BatchCompilerArtifact {
        executable: Some("/tmp/test-bin".to_string()),
        filenames: vec!["/tmp/test-bin.o".to_string()],
        nextest_binary_id: None,
        libtest_binary_prefix: None,
        src_path: None,
        is_test_harness: false,
    }];
    let publication = BTreeSet::from(["other".to_string()]);

    let err = build_check_aggregate_export_requests(
        &instances,
        &shim_metadata,
        &artifacts,
        Some(&publication),
    )
    .unwrap_err();

    assert!(format!("{err:?}").contains("aggregate repair did not produce"));
}

#[test]
fn publication_filter_accepts_observed_binary_id() {
    let instances = vec![
        instance("bin::test", "bin", true),
        instance("other::test", "other", true),
    ];
    let shim_metadata = vec![
        shim("bin::test", "/tmp/test-bin", "/tmp/profiles/a.profraw"),
        shim("other::test", "/tmp/other-bin", "/tmp/profiles/b.profraw"),
    ];
    let artifacts = vec![
        BatchCompilerArtifact {
            executable: Some("/tmp/test-bin".to_string()),
            filenames: vec!["/tmp/test-bin.o".to_string()],
            nextest_binary_id: None,
            libtest_binary_prefix: None,
            src_path: None,
            is_test_harness: false,
        },
        BatchCompilerArtifact {
            executable: Some("/tmp/other-bin".to_string()),
            filenames: vec!["/tmp/other-bin.o".to_string()],
            nextest_binary_id: None,
            libtest_binary_prefix: None,
            src_path: None,
            is_test_harness: false,
        },
    ];
    let publication = BTreeSet::from(["bin".to_string()]);

    let requests = build_check_aggregate_export_requests(
        &instances,
        &shim_metadata,
        &artifacts,
        Some(&publication),
    )
    .unwrap();

    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].binary_id, "bin");
}

#[test]
fn request_building_reports_missing_argv_and_missing_objects() {
    let instances = vec![instance("bin::test", "bin", true)];
    let mut missing_argv = shim("bin::test", "/tmp/test-bin", "/tmp/profiles/a.profraw");
    missing_argv.argv.clear();
    let err =
        build_check_aggregate_export_requests(&instances, &[missing_argv], &[], None).unwrap_err();
    assert!(format!("{err:?}").contains("missing test binary argv"));

    let no_objects = vec![shim(
        "bin::test",
        "/tmp/test-bin",
        "/tmp/profiles/a.profraw",
    )];
    let err =
        build_check_aggregate_export_requests(&instances, &no_objects, &[], None).unwrap_err();
    assert!(format!("{err:?}").contains("has no profiles or objects"));
}

#[test]
fn bounded_export_returns_empty_for_empty_requests() {
    let tmp = tempfile::tempdir().unwrap();
    let (coverage, counters) =
        export_check_aggregates_bounded(1, tmp.path(), &[], Vec::new()).unwrap();

    assert!(coverage.is_empty());
    assert_eq!(counters.export_jobs, 0);
}

#[test]
fn stable_name_is_deterministic_and_hex_encoded() {
    let first = stable_name("target/debug/deps/demo");
    let second = stable_name("target/debug/deps/demo");
    let other = stable_name("target/debug/deps/other");

    assert_eq!(first, second);
    assert_ne!(first, other);
    assert_eq!(first.len(), 16);
    assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
}

#[test]
fn bounded_export_with_fake_exporter_reports_coverage_and_counters() {
    let tmp = tempfile::tempdir().unwrap();
    let requests = vec![
        export_request("alpha", 1),
        export_request("beta", 3),
        export_request("gamma", 2),
    ];
    let exporter = fake_exporter(None);

    let (coverage, counters) =
        export_check_aggregates_bounded_with(2, tmp.path(), &[], requests, exporter).unwrap();

    assert_eq!(coverage.len(), 3);
    assert_eq!(coverage["alpha"].files["src/alpha.rs"], BTreeSet::from([1]));
    assert_eq!(coverage["beta"].files["src/beta.rs"], BTreeSet::from([3]));
    assert_eq!(counters.export_jobs, 3);
    assert_eq!(counters.max_objects_per_export, 3);
    assert!(counters.max_active_exports <= 2);
}

#[test]
fn bounded_export_propagates_worker_error() {
    let tmp = tempfile::tempdir().unwrap();
    let exporter = fake_exporter(Some("beta".to_string()));

    let err = export_check_aggregates_bounded_with(
        2,
        tmp.path(),
        &[],
        vec![export_request("alpha", 1), export_request("beta", 1)],
        exporter,
    )
    .unwrap_err();

    assert!(
        matches!(err, RustLlvmCovError::InvalidRequest(message) if message.contains("fake export failure for beta"))
    );
}
