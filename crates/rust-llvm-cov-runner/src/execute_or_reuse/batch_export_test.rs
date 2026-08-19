use super::{
    InstanceExportRequest, SubprocessInstanceExporter, export_instances_bounded_with,
    object_paths_for_executable,
};
use crate::execute_or_reuse::batch_events::BatchCompilerArtifact;
use crate::execute_or_reuse::batch_export_catalog::object_paths_from_artifacts;
use crate::execute_or_reuse::batch_export_resolve::BinaryIdObjectMap;
use crate::execute_or_reuse::batch_export_tools::ExportTools;
use crate::{RustLineCoverage, RustLlvmCovError};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct FakeInstanceExporter {
    coverage_by_id: BTreeMap<String, RustLineCoverage>,
}

impl FakeInstanceExporter {
    pub fn new(coverage_by_id: BTreeMap<String, RustLineCoverage>) -> Self {
        Self { coverage_by_id }
    }

    pub fn export_instance(
        &self,
        request: &InstanceExportRequest,
        _source_root: &Path,
        _catalog: &[PathBuf],
        _seed_objects: &[PathBuf],
    ) -> Result<RustLineCoverage, RustLlvmCovError> {
        Ok(self
            .coverage_by_id
            .get(&request.instance_id)
            .cloned()
            .unwrap_or_else(|| RustLineCoverage {
                files: BTreeMap::new(),
            }))
    }
}

pub fn write_fake_profile(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)
}

fn export_argv_for_test() -> Vec<String> {
    vec![
        "llvm-profdata".to_string(),
        "merge".to_string(),
        "-sparse".to_string(),
        "--num-threads=1".to_string(),
        "llvm-cov".to_string(),
        "export".to_string(),
        "-format=text".to_string(),
        "--threads=1".to_string(),
        "-skip-expansions".to_string(),
        "-skip-functions".to_string(),
    ]
}

#[test]
fn object_paths_collect_unique_object_files_from_artifacts() {
    let artifacts = vec![BatchCompilerArtifact {
        executable: Some("/tmp/bin".into()),
        filenames: vec![
            "/tmp/a.o".into(),
            "/tmp/a.o".into(),
            "/tmp/b.rlib".into(),
            "/tmp/c.txt".into(),
        ],
        nextest_binary_id: None,
    libtest_binary_prefix: None,
    src_path: None,
    is_test_harness: false,
    }];
    let objects = object_paths_from_artifacts(&artifacts);
    assert_eq!(
        objects,
        vec![PathBuf::from("/tmp/a.o"), PathBuf::from("/tmp/b.rlib")]
    );
}

#[test]
fn object_paths_for_executable_selects_only_matching_artifact_objects() {
    let artifacts = vec![
        BatchCompilerArtifact {
            executable: Some("/tmp/bin-a".into()),
            filenames: vec!["/tmp/a.o".into()],
            nextest_binary_id: None,
        libtest_binary_prefix: None,
        src_path: None,
        is_test_harness: false,
        },
        BatchCompilerArtifact {
            executable: Some("/tmp/bin-b".into()),
            filenames: vec!["/tmp/b.o".into()],
            nextest_binary_id: None,
        libtest_binary_prefix: None,
        src_path: None,
        is_test_harness: false,
        },
    ];
    let objects = object_paths_for_executable(&artifacts, Path::new("/tmp/bin-a"));
    assert_eq!(objects, vec![PathBuf::from("/tmp/a.o")]);
}

#[test]
fn object_paths_for_executable_matches_basename_and_suffix_forms() {
    let artifacts = vec![
        BatchCompilerArtifact {
            executable: Some("/tmp/target/debug/deps/demo-abc".into()),
            filenames: vec!["/tmp/demo.o".into()],
            nextest_binary_id: None,
        libtest_binary_prefix: None,
        src_path: None,
        is_test_harness: false,
        },
        BatchCompilerArtifact {
            executable: Some("relative/bin-two".into()),
            filenames: vec!["/tmp/two.o".into()],
            nextest_binary_id: None,
        libtest_binary_prefix: None,
        src_path: None,
        is_test_harness: false,
        },
    ];

    assert_eq!(
        object_paths_for_executable(&artifacts, Path::new("demo-abc")),
        vec![PathBuf::from("/tmp/demo.o")]
    );
    assert_eq!(
        object_paths_for_executable(&artifacts, Path::new("/repo/relative/bin-two")),
        vec![PathBuf::from("/tmp/two.o")]
    );
}

#[test]
fn bounded_export_pool_never_exceeds_jobs() {
    let tmp = tempfile::tempdir().unwrap();
    let profile = tmp.path().join("inst.profraw");
    write_fake_profile(&profile, b"profile").unwrap();
    let requests = (0..6)
        .map(|index| InstanceExportRequest {
            instance_id: format!("inst-{index}"),
            profile_path: profile.clone(),
            objects: vec![PathBuf::from("/tmp/a.o")],
        })
        .collect::<Vec<_>>();
    let mut coverage = BTreeMap::new();
    coverage.insert(
        "inst-0".to_string(),
        RustLineCoverage {
            files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
        },
    );
    let fake = Arc::new(FakeInstanceExporter::new(coverage));
    let (results, counters) = export_instances_bounded_with(
        2,
        tmp.path(),
        &[PathBuf::from("/tmp/a.o")],
        requests,
        Arc::new(move |request, source_root, _catalog, _seed_objects| {
            fake.export_instance(request, source_root, &[], &[])
        }),
    )
    .unwrap();
    assert_eq!(results.len(), 6);
    assert_eq!(counters.export_jobs, 6);
    assert!(counters.max_active_exports <= 2);
}

#[test]
fn bounded_export_preserves_request_order_and_propagates_worker_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let profile = tmp.path().join("inst.profraw");
    write_fake_profile(&profile, b"profile").unwrap();
    let requests = ["slow", "fast"]
        .into_iter()
        .map(|id| InstanceExportRequest {
            instance_id: id.to_string(),
            profile_path: profile.clone(),
            objects: vec![PathBuf::from("/tmp/a.o")],
        })
        .collect::<Vec<_>>();
    let (results, counters) = export_instances_bounded_with(
        2,
        tmp.path(),
        &[PathBuf::from("/tmp/a.o")],
        requests,
        Arc::new(|request, _source_root, _catalog, _seed_objects| {
            if request.instance_id == "slow" {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Ok(RustLineCoverage {
                files: BTreeMap::from([(request.instance_id.clone(), BTreeSet::from([1]))]),
            })
        }),
    )
    .unwrap();
    assert_eq!(counters.max_objects_per_export, 1);
    assert_eq!(results[0].0, "slow");
    assert_eq!(results[1].0, "fast");

    let err = export_instances_bounded_with(
        1,
        tmp.path(),
        &[PathBuf::from("/tmp/a.o")],
        vec![InstanceExportRequest {
            instance_id: "bad".to_string(),
            profile_path: profile,
            objects: vec![PathBuf::from("/tmp/a.o")],
        }],
        Arc::new(|request, _source_root, _catalog, _seed_objects| {
            Err(RustLlvmCovError::InvalidRequest(format!(
                "failed {}",
                request.instance_id
            )))
        }),
    )
    .unwrap_err();
    assert!(
        matches!(err, RustLlvmCovError::InvalidRequest(message) if message.contains("failed bad"))
    );
}

#[test]
fn subprocess_exporter_argv_keeps_sparse_merge_and_text_export() {
    let argv = export_argv_for_test();
    assert!(argv.contains(&"--num-threads=1".to_string()));
    assert!(argv.contains(&"-format=text".to_string()));
    assert!(argv.contains(&"--threads=1".to_string()));
    assert!(argv.contains(&"-skip-expansions".to_string()));
    assert!(argv.contains(&"-skip-functions".to_string()));
}

#[test]
fn export_instances_bounded_handles_empty_requests() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = Arc::new(FakeInstanceExporter::new(BTreeMap::new()));
    let (results, counters) = export_instances_bounded_with(
        2,
        tmp.path(),
        &[],
        Vec::new(),
        Arc::new(move |request, source_root, _catalog, _seed_objects| {
            fake.export_instance(request, source_root, &[], &[])
        }),
    )
    .unwrap();
    assert!(results.is_empty());
    assert_eq!(counters.export_jobs, 0);
}
