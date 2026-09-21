use super::unusable_profile_stderr;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_export::{
    InstanceExportRequest, SubprocessInstanceExporter, export_instances_bounded, write_fake_profile,
};
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_export_tools::ExportTools;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::{fs, io};

fn write_exec(path: &Path, body: &str) -> io::Result<()> {
    fs::write(path, body)?;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)
}

#[test]
fn unusable_profile_stderr_detects_known_messages() {
    assert!(unusable_profile_stderr("error: no profile can be merged"));
    assert!(unusable_profile_stderr(
        "warning: x.profraw: malformed instrumentation profile data: symbol name is empty"
    ));
    assert!(unusable_profile_stderr(
        "warning: x.profraw: truncated profile data"
    ));
    assert!(!unusable_profile_stderr("some other llvm failure"));
}

#[test]
fn export_instance_returns_empty_coverage_for_unusable_profraw() {
    let tmp = tempfile::tempdir().unwrap();
    let profile = tmp.path().join("inst.profraw");
    write_fake_profile(&profile, b"not-a-real-profile").unwrap();
    let fake_merge = tmp.path().join("llvm-profdata");
    write_exec(
        &fake_merge,
        "#!/bin/sh\necho 'warning: inst.profraw: malformed instrumentation profile data: symbol name is empty' >&2\necho 'error: no profile can be merged' >&2\nexit 1\n",
    )
    .unwrap();
    let tools = ExportTools {
        llvm_profdata: fake_merge,
        llvm_cov: PathBuf::from("/bin/false"),
        llvm_readobj: PathBuf::from("/bin/false"),
    };
    let exporter = SubprocessInstanceExporter::new(tools, None);
    let request = InstanceExportRequest {
        instance_id: "inst".to_string(),
        profile_path: profile,
        objects: vec![PathBuf::from("/tmp/a.o")],
    };
    let coverage = exporter
        .export_instance(&request, Path::new("/repo"), &[], &request.objects)
        .unwrap();
    assert!(coverage.files.is_empty());
}

#[test]
fn export_instance_returns_empty_coverage_when_profile_binary_id_missing_from_catalog() {
    let tmp = tempfile::tempdir().unwrap();
    let profile = tmp.path().join("inst.profraw");
    write_fake_profile(&profile, b"profile").unwrap();
    let llvm_profdata = tmp.path().join("llvm-profdata");
    write_exec(
        &llvm_profdata,
        "#!/bin/sh\nif [ \"$1\" = merge ]; then : > \"$6\"; exit 0; fi\nif [ \"$1\" = show ]; then printf 'Binary IDs:\\ndeadbeef\\n'; exit 0; fi\nexit 1\n",
    )
    .unwrap();
    let llvm_readobj = tmp.path().join("llvm-readobj");
    write_exec(
        &llvm_readobj,
        "#!/bin/sh\nprintf 'Build ID: cafebabe\\n'\nexit 0\n",
    )
    .unwrap();
    let seed = tmp.path().join("seed-object");
    std::fs::write(&seed, b"object").unwrap();
    let catalog = vec![seed.clone()];
    let tools = ExportTools {
        llvm_profdata,
        llvm_cov: PathBuf::from("/bin/false"),
        llvm_readobj,
    };
    let exporter = SubprocessInstanceExporter::new(tools, None)
        .with_catalog_map(&catalog, 1)
        .expect("catalog map");
    let request = InstanceExportRequest {
        instance_id: "remote_run::localhost_conda_env_lock$test_show_config".to_string(),
        profile_path: profile,
        objects: catalog.clone(),
    };
    let coverage = exporter
        .export_instance(&request, Path::new("/repo"), &catalog, &catalog)
        .expect("stale profile id must not fail the instance export");
    assert!(coverage.files.is_empty());
}

#[test]
fn export_instances_bounded_keeps_sibling_when_one_profile_id_is_stale() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("src.rs");
    std::fs::write(&source, "fn f() {}\n").unwrap();
    let json_path = tmp.path().join("cov.json");
    std::fs::write(
        &json_path,
        crate::rust_llvm_cov_runner::test_support::llvm_cov_json_for_file(&source),
    )
    .unwrap();
    let stale_raw = tmp.path().join("stale.profraw");
    let good_raw = tmp.path().join("good.profraw");
    write_fake_profile(&stale_raw, b"profile").unwrap();
    write_fake_profile(&good_raw, b"profile").unwrap();
    let llvm_profdata = tmp.path().join("llvm-profdata");
    write_exec(
        &llvm_profdata,
        "#!/bin/sh\nif [ \"$1\" = merge ]; then : > \"$6\"; exit 0; fi\nif [ \"$1\" = show ]; then\n  case \"$3\" in *stale*) printf 'Binary IDs:\\ndeadbeef\\n' ;; *) printf 'Binary IDs:\\ncafebabe\\n' ;; esac\n  exit 0\nfi\nexit 1\n",
    )
    .unwrap();
    let llvm_readobj = tmp.path().join("llvm-readobj");
    write_exec(
        &llvm_readobj,
        "#!/bin/sh\nprintf 'Build ID: cafebabe\\n'\nexit 0\n",
    )
    .unwrap();
    let llvm_cov = tmp.path().join("llvm-cov");
    write_exec(
        &llvm_cov,
        &format!("#!/bin/sh\ncat '{}'\n", json_path.display()),
    )
    .unwrap();
    let seed = tmp.path().join("seed-object");
    std::fs::write(&seed, b"object").unwrap();
    let catalog = vec![seed.clone()];
    let tools = ExportTools {
        llvm_profdata,
        llvm_cov,
        llvm_readobj,
    };
    let exporter = SubprocessInstanceExporter::new(tools, None)
        .with_catalog_map(&catalog, 1)
        .expect("catalog map");
    let (results, _) = export_instances_bounded(
        2,
        exporter,
        tmp.path(),
        &catalog,
        vec![
            InstanceExportRequest {
                instance_id: "stale".to_string(),
                profile_path: stale_raw,
                objects: catalog.clone(),
            },
            InstanceExportRequest {
                instance_id: "good".to_string(),
                profile_path: good_raw,
                objects: catalog.clone(),
            },
        ],
    )
    .expect("stale sibling must not abort the batch");
    let by_id: std::collections::BTreeMap<_, _> = results.into_iter().collect();
    assert!(by_id["stale"].files.is_empty());
    assert!(!by_id["good"].files.is_empty());
}

#[test]
fn export_instance_returns_empty_coverage_when_profraw_missing() {
    let tools = ExportTools {
        llvm_profdata: PathBuf::from("/bin/false"),
        llvm_cov: PathBuf::from("/bin/false"),
        llvm_readobj: PathBuf::from("/bin/false"),
    };
    let exporter = SubprocessInstanceExporter::new(tools, None);
    let request = InstanceExportRequest {
        instance_id: "inst".to_string(),
        profile_path: PathBuf::from("/tmp/definitely-missing-kiss-profraw.profraw"),
        objects: vec![PathBuf::from("/tmp/a.o")],
    };
    let coverage = exporter
        .export_instance(&request, Path::new("/repo"), &[], &request.objects)
        .unwrap();
    assert!(coverage.files.is_empty());
}
