use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use kiss::rust_llvm_cov_runner::RustCoverageToolIdentity;

use crate::test_runner::runners::command_stdout;

fn system_time_to_nanos(ts: SystemTime) -> Option<u64> {
    ts.duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
}

fn version_with_meta_tag(version: String, program: &Path) -> String {
    let resolved = if program.is_absolute() {
        program.to_path_buf()
    } else {
        resolve_on_path(program).unwrap_or_else(|| program.to_path_buf())
    };
    let Ok(meta) = fs::metadata(&resolved) else {
        return version;
    };
    let mtime = meta
        .modified()
        .ok()
        .and_then(system_time_to_nanos)
        .unwrap_or(0);
    #[cfg(unix)]
    let inode = {
        use std::os::unix::fs::MetadataExt;
        meta.ino()
    };
    #[cfg(not(unix))]
    let inode = 0u64;
    format!("{version}#{:x}-{:x}-{:x}", meta.len(), mtime, inode)
}

fn resolve_on_path(program: &Path) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(program);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    })
}

pub(super) fn detect_live_rust_coverage_tool_identity(
    repo_root: &Path,
) -> Result<RustCoverageToolIdentity, String> {
    let cargo = PathBuf::from("cargo");
    let rustc = PathBuf::from("rustc");
    let repo = repo_root.to_path_buf();
    std::thread::scope(|scope| {
        let cargo_v = scope.spawn(|| -> Result<String, String> {
            Ok(version_with_meta_tag(
                command_stdout(&PathBuf::from("cargo"), &["--version"], &repo)?,
                &cargo,
            ))
        });
        let llvm_v = scope.spawn(|| -> Result<String, String> {
            Ok(version_with_meta_tag(
                command_stdout(
                    &PathBuf::from("cargo"),
                    &["llvm-cov", "--version"],
                    &repo,
                )?,
                Path::new("cargo-llvm-cov"),
            ))
        });
        let rustc_v = scope.spawn(|| -> Result<String, String> {
            Ok(version_with_meta_tag(
                command_stdout(&PathBuf::from("rustc"), &["-Vv"], &repo)?,
                &rustc,
            ))
        });
        let nextest_v = scope.spawn(|| -> Result<String, String> {
            Ok(version_with_meta_tag(
                command_stdout(
                    &PathBuf::from("cargo"),
                    &["nextest", "--version"],
                    &repo,
                )?,
                Path::new("cargo-nextest"),
            ))
        });
        Ok(RustCoverageToolIdentity {
            cargo_version: cargo_v
                .join()
                .unwrap_or_else(|_| Err("cargo version probe panicked".into()))?,
            llvm_cov_version: llvm_v
                .join()
                .unwrap_or_else(|_| Err("llvm-cov version probe panicked".into()))?,
            rustc_version: rustc_v
                .join()
                .unwrap_or_else(|_| Err("rustc version probe panicked".into()))?,
            cargo_nextest_version: nextest_v
                .join()
                .unwrap_or_else(|_| Err("nextest version probe panicked".into()))?,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    #[test]
    fn meta_tag_is_stable_for_unchanged_file() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("tool");
        fs::write(&bin, b"abc").unwrap();
        let a = version_with_meta_tag("v1".into(), &bin);
        let b = version_with_meta_tag("v1".into(), &bin);
        assert_eq!(a, b);
        assert!(a.starts_with("v1#"));
    }

    #[test]
    fn meta_tag_returns_version_when_file_missing() {
        let missing = PathBuf::from("/tmp/kiss-missing-tool-identity-binary-xyz");
        let tagged = version_with_meta_tag("plain".into(), &missing);
        assert_eq!(tagged, "plain");
    }

    #[test]
    fn resolve_on_path_finds_executable() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("kiss-tool-id-probe");
        fs::write(&bin, b"#!/bin/sh\necho ok\n").unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
        let mut path = tmp.path().display().to_string();
        if let Ok(existing) = std::env::var("PATH") {
            path.push(':');
            path.push_str(&existing);
        }
        let _guard = crate::test_runner::TestEnvVarGuard::set("PATH", &path);
        let resolved = resolve_on_path(Path::new("kiss-tool-id-probe"));
        assert_eq!(resolved.as_deref(), Some(bin.as_path()));
        let tagged = version_with_meta_tag("v".into(), Path::new("kiss-tool-id-probe"));
        assert!(tagged.starts_with("v#"));
        assert!(tagged.contains('#'));
    }

    #[test]
    fn detect_live_probes_real_toolchain() {
        let tmp = tempfile::tempdir().unwrap();
        let tools = detect_live_rust_coverage_tool_identity(tmp.path()).expect("live tools");
        assert!(tools.cargo_version.contains('#'));
        assert!(tools.llvm_cov_version.contains('#'));
        assert!(tools.rustc_version.contains('#'));
        assert!(tools.cargo_nextest_version.contains('#'));
    }

    #[test]
    fn system_time_to_nanos_rejects_before_epoch() {
        let before = UNIX_EPOCH.checked_sub(Duration::from_secs(1)).unwrap();
        assert!(system_time_to_nanos(before).is_none());
        assert!(system_time_to_nanos(UNIX_EPOCH).is_some());
    }
}
