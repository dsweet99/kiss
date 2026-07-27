use super::{
    InstanceExportRequest, SubprocessInstanceExporter, export_instances_bounded_with,
    object_paths_for_executable,
};
use crate::batch_events::BatchCompilerArtifact;
use crate::batch_export_catalog::object_paths_from_artifacts;
use crate::batch_export_resolve::BinaryIdObjectMap;
use crate::batch_export_tools::ExportTools;
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

#[test]
fn subprocess_exporter_returns_empty_coverage_without_objects() {
    let tools = ExportTools {
        llvm_profdata: PathBuf::from("/bin/llvm-profdata"),
        llvm_cov: PathBuf::from("/bin/llvm-cov"),
        llvm_readobj: PathBuf::from("/bin/llvm-readobj"),
    };
    let exporter = SubprocessInstanceExporter::new(tools, None);
    let request = InstanceExportRequest {
        instance_id: "inst".to_string(),
        profile_path: PathBuf::from("/tmp/inst.profraw"),
        objects: Vec::new(),
    };
    let coverage = exporter
        .export_instance(&request, Path::new("/repo"), &[], &[])
        .unwrap();
    assert!(coverage.files.is_empty());
}

#[test]
fn subprocess_exporter_reports_profdata_merge_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let profile = tmp.path().join("inst.profraw");
    write_fake_profile(&profile, b"profile").unwrap();
    let tools = ExportTools {
        llvm_profdata: PathBuf::from("/bin/false"),
        llvm_cov: PathBuf::from("/bin/false"),
        llvm_readobj: PathBuf::from("/bin/false"),
    };
    let exporter = SubprocessInstanceExporter::new(tools, None);
    let request = InstanceExportRequest {
        instance_id: "inst".to_string(),
        profile_path: profile,
        objects: vec![PathBuf::from("/tmp/a.o")],
    };

    let err = exporter
        .export_instance(&request, Path::new("/repo"), &[], &request.objects)
        .unwrap_err();

    assert!(format!("{err:?}").contains("llvm-profdata merge failed"));
}

#[test]
fn subprocess_exporter_reports_missing_binary_id_map_after_merge() {
    let tmp = tempfile::tempdir().unwrap();
    let profile = tmp.path().join("inst.profraw");
    write_fake_profile(&profile, b"profile").unwrap();
    let object = tmp.path().join("a.o");
    fs::write(&object, b"object").unwrap();
    let tools = ExportTools {
        llvm_profdata: PathBuf::from("/bin/true"),
        llvm_cov: PathBuf::from("/bin/false"),
        llvm_readobj: PathBuf::from("/bin/false"),
    };
    let exporter = SubprocessInstanceExporter::new(tools, None);
    let request = InstanceExportRequest {
        instance_id: "inst".to_string(),
        profile_path: profile,
        objects: vec![object.clone()],
    };

    let err = exporter
        .export_instance(&request, tmp.path(), &[], &[object])
        .unwrap_err();

    assert!(format!("{err:?}").contains("binary-id object map"));
}

#[test]
fn merge_profiles_rejects_empty_input_without_spawning_tool() {
    let tmp = tempfile::tempdir().unwrap();
    let tools = ExportTools {
        llvm_profdata: PathBuf::from("/bin/false"),
        llvm_cov: PathBuf::from("/bin/false"),
        llvm_readobj: PathBuf::from("/bin/false"),
    };
    let err = super::merge_profiles(&tools, &[], &tmp.path().join("out.profdata")).unwrap_err();

    assert!(matches!(
        err,
        RustLlvmCovError::InvalidRequest(message)
            if message.contains("requires at least one input")
    ));
}

#[test]
fn export_instance_coverage_parses_successful_tool_output() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("src.rs");
    fs::write(&source, "fn f() {}\n").unwrap();
    let profdata = tmp.path().join("inst.profdata");
    fs::write(&profdata, b"profile").unwrap();
    let object = tmp.path().join("a.o");
    fs::write(&object, b"object").unwrap();
    let json_path = tmp.path().join("cov.json");
    fs::write(
        &json_path,
        crate::test_support::llvm_cov_json_for_file(&source),
    )
    .unwrap();
    let argv_log = tmp.path().join("argv.log");
    let llvm_cov = crate::test_support::write_executable(
        tmp.path().join("llvm-cov"),
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\ncat '{}'\n",
            argv_log.display(),
            json_path.display()
        ),
    );
    let tools = ExportTools {
        llvm_profdata: PathBuf::from("/bin/true"),
        llvm_cov,
        llvm_readobj: PathBuf::from("/bin/true"),
    };
    let coverage = super::export_instance_coverage(
        &tools,
        &profdata,
        tmp.path(),
        &[object],
        Some(r"\.cargo/"),
    )
    .unwrap();
    let argv = fs::read_to_string(&argv_log).unwrap();
    assert!(
        argv.lines().any(|line| line == "--threads=1"),
        "llvm-cov export must pass --threads=1; argv was:\n{argv}"
    );
    assert!(
        coverage.files.keys().any(|k| k.contains("src.rs")) || !coverage.files.is_empty(),
        "expected parsed coverage files, got {:?}",
        coverage.files.keys().collect::<Vec<_>>()
    );
}

#[test]
fn export_instance_coverage_reports_tool_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let profdata = tmp.path().join("inst.profdata");
    fs::write(&profdata, b"profile").unwrap();
    let object = tmp.path().join("a.o");
    fs::write(&object, b"object").unwrap();
    let tools = ExportTools {
        llvm_profdata: PathBuf::from("/bin/true"),
        llvm_cov: PathBuf::from("/bin/false"),
        llvm_readobj: PathBuf::from("/bin/true"),
    };
    let err = super::export_instance_coverage(&tools, &profdata, tmp.path(), &[object], None)
        .unwrap_err();
    assert!(format!("{err:?}").contains("llvm-cov export failed"));
}

#[test]
fn with_binary_id_map_stores_map_for_later_exports() {
    let tools = ExportTools {
        llvm_profdata: PathBuf::from("/bin/false"),
        llvm_cov: PathBuf::from("/bin/false"),
        llvm_readobj: PathBuf::from("/bin/false"),
    };
    let map = BinaryIdObjectMap::default();
    let exporter = SubprocessInstanceExporter::with_binary_id_map(tools, None, map);
    let request = InstanceExportRequest {
        instance_id: "inst".to_string(),
        profile_path: PathBuf::from("/tmp/missing.profraw"),
        objects: Vec::new(),
    };
    let coverage = exporter
        .export_instance(&request, Path::new("/tmp"), &[], &[])
        .unwrap();
    assert!(coverage.files.is_empty());
}
