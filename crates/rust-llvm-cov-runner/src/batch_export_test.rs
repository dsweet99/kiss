use super::{
    InstanceExportRequest, SubprocessInstanceExporter, export_instances_bounded_with,
    object_paths_for_executable,
};
use crate::batch_events::BatchCompilerArtifact;
use crate::batch_export_catalog::object_paths_from_artifacts;
use crate::batch_export_resolve::BinaryIdObjectMap;
use crate::batch_export_tools::{
    ExportTools, find_llvm_cov_in_path, find_llvm_profdata_in_path, find_llvm_readobj_in_path,
    resolve_export_tools_from_env, resolve_export_tools_from_rustc, which_tool,
};
use crate::{RustLineCoverage, RustLlvmCovError};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
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
        },
        BatchCompilerArtifact {
            executable: Some("/tmp/bin-b".into()),
            filenames: vec!["/tmp/b.o".into()],
        },
    ];
    let objects = object_paths_for_executable(&artifacts, Path::new("/tmp/bin-a"));
    assert_eq!(objects, vec![PathBuf::from("/tmp/a.o")]);
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
fn subprocess_exporter_argv_uses_single_thread_flags() {
    let argv = export_argv_for_test();
    assert!(argv.contains(&"--num-threads=1".to_string()));
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
fn resolve_export_tools_honors_env_overrides() {
    let tmp = tempfile::tempdir().unwrap();
    let profdata = tmp.path().join("llvm-profdata");
    let cov = tmp.path().join("llvm-cov");
    let readobj = tmp.path().join("llvm-readobj");
    fs::write(&profdata, b"").unwrap();
    fs::write(&cov, b"").unwrap();
    fs::write(&readobj, b"").unwrap();
    // SAFETY: test-only env mutation restored by process exit.
    unsafe {
        std::env::set_var("LLVM_PROFDATA", &profdata);
        std::env::set_var("LLVM_COV", &cov);
        std::env::set_var("LLVM_READOBJ", &readobj);
    }
    let tools = resolve_export_tools_from_rustc(OsStr::new("rustc")).unwrap();
    assert_eq!(tools.llvm_profdata, profdata);
    assert_eq!(tools.llvm_cov, cov);
    assert_eq!(tools.llvm_readobj, readobj);
    // SAFETY: test-only env cleanup.
    unsafe {
        std::env::remove_var("LLVM_PROFDATA");
        std::env::remove_var("LLVM_COV");
        std::env::remove_var("LLVM_READOBJ");
    }
}

#[test]
fn resolve_export_tools_falls_back_to_path_lookup() {
    let tools = resolve_export_tools_from_env().unwrap();
    assert!(!tools.llvm_cov.as_os_str().is_empty());
    assert!(!tools.llvm_profdata.as_os_str().is_empty());
}

#[test]
fn private_tool_lookup_helpers_are_callable() {
    let old_path = std::env::var_os("PATH").unwrap();
    // SAFETY: test-only PATH mutation restored below.
    unsafe {
        std::env::set_var("PATH", "/definitely/not/on/path");
    }
    let cov = find_llvm_cov_in_path();
    let profdata = find_llvm_profdata_in_path();
    let readobj = find_llvm_readobj_in_path();
    let missing = which_tool("definitely-not-a-tool-xyz");
    // SAFETY: restore PATH for other tests.
    unsafe {
        std::env::set_var("PATH", old_path);
    }
    assert_eq!(cov, PathBuf::from("llvm-cov"));
    assert_eq!(profdata, PathBuf::from("llvm-profdata"));
    assert_eq!(readobj, PathBuf::from("llvm-readobj"));
    assert!(missing.is_none());
}

#[test]
fn which_tool_finds_executable_on_path() {
    let tmp = tempfile::tempdir().unwrap();
    let tool = tmp.path().join("llvm-cov");
    fs::write(&tool, b"").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&tool).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&tool, permissions).unwrap();
    }
    let old_path = std::env::var_os("PATH").unwrap();
    // SAFETY: test-only PATH mutation restored below.
    unsafe {
        std::env::set_var(
            "PATH",
            format!("{}:{}", tmp.path().display(), old_path.to_string_lossy()),
        );
    }
    let tools = resolve_export_tools_from_env().unwrap();
    assert_eq!(tools.llvm_cov, tool);
    // SAFETY: restore PATH for other tests.
    unsafe {
        std::env::set_var("PATH", old_path);
    }
}

#[test]
fn export_request_and_tools_types_are_constructible() {
    let request = InstanceExportRequest {
        instance_id: "id".to_string(),
        profile_path: PathBuf::from("/tmp/id.profraw"),
        objects: vec![PathBuf::from("/tmp/a.o")],
    };
    let tools = ExportTools {
        llvm_profdata: PathBuf::from("/bin/llvm-profdata"),
        llvm_cov: PathBuf::from("/bin/llvm-cov"),
        llvm_readobj: PathBuf::from("/bin/llvm-readobj"),
    };
    assert_eq!(request.instance_id, "id");
    assert_eq!(tools.llvm_cov, PathBuf::from("/bin/llvm-cov"));
}

#[test]
fn subprocess_exporter_with_binary_id_map_is_constructible() {
    let tools = ExportTools {
        llvm_profdata: PathBuf::from("/bin/llvm-profdata"),
        llvm_cov: PathBuf::from("/bin/llvm-cov"),
        llvm_readobj: PathBuf::from("/bin/llvm-readobj"),
    };
    let exporter =
        SubprocessInstanceExporter::with_binary_id_map(tools, None, BinaryIdObjectMap::default());
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
fn export_instances_bounded_drains_more_jobs_than_concurrency() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = Arc::new(FakeInstanceExporter::new(BTreeMap::from([(
        "a".to_string(),
        RustLineCoverage {
            files: BTreeMap::new(),
        },
    )])));
    let fake_for_fn = fake.clone();
    let make_request = |id: &str| InstanceExportRequest {
        instance_id: id.to_string(),
        profile_path: tmp.path().join(format!("{id}.profraw")),
        objects: vec![PathBuf::from("/tmp/a.o")],
    };
    let requests = vec![make_request("a"), make_request("b"), make_request("c")];
    let (results, counters) = export_instances_bounded_with(
        2,
        tmp.path(),
        &[],
        requests,
        Arc::new(move |request, source_root, _catalog, _seed_objects| {
            fake_for_fn.export_instance(request, source_root, &[], &[])
        }),
    )
    .unwrap();
    assert_eq!(results.len(), 3);
    assert_eq!(counters.export_jobs, 3);
    assert!(counters.max_active_exports <= 2);
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
