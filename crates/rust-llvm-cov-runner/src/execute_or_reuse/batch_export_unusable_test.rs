use super::unusable_profile_stderr;
use crate::execute_or_reuse::batch_export::{
    InstanceExportRequest, SubprocessInstanceExporter, write_fake_profile,
};
use crate::execute_or_reuse::batch_export_tools::ExportTools;
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
    assert!(unusable_profile_stderr("warning: x.profraw: truncated profile data"));
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
