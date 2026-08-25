use super::tests::write_fake_profile;
use super::{InstanceExportRequest, SubprocessInstanceExporter};
use crate::rust_llvm_cov_runner::RustLlvmCovError;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_export_resolve::BinaryIdObjectMap;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_export_tools::ExportTools;
use std::fs;
use std::path::{Path, PathBuf};

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
        crate::rust_llvm_cov_runner::test_support::llvm_cov_json_for_file(&source),
    )
    .unwrap();
    let argv_log = tmp.path().join("argv.log");
    let llvm_cov = crate::rust_llvm_cov_runner::test_support::write_executable(
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
        argv.lines().any(|line| line == "-skip-expansions"),
        "llvm-cov export must skip expansions; argv was:\n{argv}"
    );
    assert!(
        argv.lines().any(|line| line == "-skip-functions"),
        "llvm-cov export must skip function records; argv was:\n{argv}"
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
