use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use crate::RustCoverageToolIdentity;

pub(crate) const FIXTURE_ROOT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/export_contract"
);
pub(crate) const HELPER_BIN_ENV: &str = "EXPORT_CONTRACT_HELPER_BIN";
pub(crate) const TARGET_RUNNER_SHIM_ENV: &str = "KISS_RUST_LLVM_COV_TARGET_RUNNER_SHIM";
pub(crate) static TARGET_RUNNER_ENV_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn real_tool_tests_enabled() -> bool {
    std::env::var_os("KISS_REAL_TOOL_TESTS").is_some_and(|value| !value.is_empty())
}

pub(crate) fn fixture_manifest() -> PathBuf {
    Path::new(FIXTURE_ROOT).join("Cargo.toml")
}

pub(crate) fn workspace_manifest() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("runner crate lives under workspace/crates")
        .join("Cargo.toml")
}

pub(crate) fn workspace_debug_binary(name: &str) -> PathBuf {
    let exe = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    workspace_manifest()
        .parent()
        .expect("workspace manifest parent")
        .join("target")
        .join("debug")
        .join(exe)
}

pub(crate) fn build_kiss_binary() -> PathBuf {
    let manifest = workspace_manifest();
    let output = Command::new("cargo")
        .args(["build", "--bin", "kiss", "--manifest-path"])
        .arg(&manifest)
        .output()
        .expect("cargo build --bin kiss");
    assert!(
        output.status.success(),
        "cargo build --bin kiss failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    workspace_debug_binary("kiss")
}

pub(crate) fn build_helper_bin(target_dir: &Path) {
    let output = Command::new("cargo")
        .args([
            "build",
            "-p",
            "export-contract-helper",
            "--bin",
            "helper-bin",
            "--manifest-path",
            &fixture_manifest().to_string_lossy(),
        ])
        .env("CARGO_TARGET_DIR", target_dir)
        .current_dir(FIXTURE_ROOT)
        .output()
        .expect("cargo build helper-bin");
    assert!(
        output.status.success(),
        "cargo build helper-bin failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(crate) fn helper_bin_path(target_dir: &Path) -> PathBuf {
    let exe = if cfg!(windows) {
        "helper-bin.exe"
    } else {
        "helper-bin"
    };
    target_dir.join("debug").join(exe)
}

pub(crate) fn fixture_cargo_args() -> Vec<String> {
    vec![
        "-p".to_string(),
        "export-contract-runner".to_string(),
        "--manifest-path".to_string(),
        fixture_manifest().to_string_lossy().to_string(),
    ]
}

pub(crate) fn command_stdout(program: &str, args: &[&str], cwd: &Path) -> String {
    let output = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|err| panic!("failed to run {program}: {err}"));
    assert!(
        output.status.success(),
        "{program} {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

pub(crate) fn real_tool_identity(cwd: &Path) -> RustCoverageToolIdentity {
    RustCoverageToolIdentity {
        cargo_version: command_stdout("cargo", &["--version"], cwd),
        llvm_cov_version: command_stdout("cargo", &["llvm-cov", "--version"], cwd),
        rustc_version: command_stdout("rustc", &["-Vv"], cwd),
        cargo_nextest_version: command_stdout("cargo", &["nextest", "--version"], cwd),
    }
}

pub(crate) fn run_cargo_llvm_cov_json(target_dir: &Path) -> Vec<u8> {
    build_helper_bin(target_dir);
    let output = Command::new("cargo")
        .args([
            "llvm-cov",
            "test",
            "-p",
            "export-contract-runner",
            "--manifest-path",
            &fixture_manifest().to_string_lossy(),
            "--json",
            "--",
            "--test-threads=1",
            "spawns_instrumented_helper_binary",
        ])
        .env("CARGO_TARGET_DIR", target_dir)
        .current_dir(FIXTURE_ROOT)
        .output()
        .expect("cargo llvm-cov test");
    assert!(
        output.status.success(),
        "cargo llvm-cov test failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    extract_json_payload(&output.stdout)
}

pub(crate) fn extract_json_payload(stdout: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(stdout);
    let start = text
        .find('{')
        .unwrap_or_else(|| panic!("no JSON object in cargo llvm-cov stdout: {text}"));
    text[start..].as_bytes().to_vec()
}

pub(crate) fn discover_profraw_files(target_dir: &Path) -> Vec<PathBuf> {
    let mut profraws = Vec::new();
    collect_profraws(target_dir, &mut profraws);
    profraws
}

pub(crate) fn merge_profraws_for_test(
    tools: &crate::execute_or_reuse::batch_export_tools::ExportTools,
    profraws: &[PathBuf],
    profdata_output: &Path,
) -> Result<(), crate::RustLlvmCovError> {
    let mut command = std::process::Command::new(&tools.llvm_profdata);
    command.arg("merge").arg("-sparse").arg("--num-threads=1");
    for profraw in profraws {
        command.arg(profraw);
    }
    command.arg("-o").arg(profdata_output);
    let status = command.status().map_err(crate::RustLlvmCovError::Io)?;
    if !status.success() {
        return Err(crate::RustLlvmCovError::InvalidRequest(
            "llvm-profdata merge failed for fixture profraws".into(),
        ));
    }
    Ok(())
}

pub(crate) fn export_merged_profile(
    tools: &crate::execute_or_reuse::batch_export_tools::ExportTools,
    profdata: &Path,
    source_root: &Path,
    objects: &[PathBuf],
) -> Result<crate::RustLineCoverage, crate::RustLlvmCovError> {
    let mut command = std::process::Command::new(&tools.llvm_cov);
    command
        .arg("export")
        .arg("-format=text")
        .arg("--threads=1")
        .arg("-instr-profile")
        .arg(profdata);
    for object in objects {
        command.arg("-object").arg(object);
    }
    let output = command.output().map_err(crate::RustLlvmCovError::Io)?;
    if !output.status.success() {
        return Err(crate::RustLlvmCovError::InvalidRequest(format!(
            "llvm-cov export failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    crate::execute_or_reuse::llvm_cov_json::parse_llvm_cov_json(&output.stdout, source_root)
}

pub(crate) fn collect_profraws(dir: &Path, out: &mut Vec<PathBuf>) {
    if !dir.is_dir() {
        return;
    }
    for entry in fs::read_dir(dir).expect("read dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() {
            collect_profraws(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("profraw") {
            out.push(path);
        }
    }
}

pub(crate) fn discover_integration_test_executable(target_dir: &Path) -> PathBuf {
    let mut candidates = Vec::new();
    collect_integration_executables(target_dir, &mut candidates);
    candidates
        .into_iter()
        .max_by_key(|path| path.metadata().ok().map(|meta| meta.len()).unwrap_or(0))
        .unwrap_or_else(|| {
            panic!(
                "no integration test executable under {}",
                target_dir.display()
            )
        })
}

pub(crate) fn collect_integration_executables(dir: &Path, out: &mut Vec<PathBuf>) {
    if !dir.is_dir() {
        return;
    }
    for entry in fs::read_dir(dir).expect("read dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|name| name.to_str()) == Some("deps") {
                for dep in fs::read_dir(&path).expect("read deps") {
                    let dep_path = dep.expect("dep entry").path();
                    if !dep_path.is_file() {
                        continue;
                    }
                    let Some(name) = dep_path.file_name().and_then(|value| value.to_str()) else {
                        continue;
                    };
                    if name.starts_with("integration-")
                        && !name.contains(".d")
                        && dep_path.extension().is_none()
                    {
                        out.push(dep_path);
                    }
                }
            }
            collect_integration_executables(&path, out);
        }
    }
}

pub(crate) fn discover_seed_objects(_target_dir: &Path, executable: &Path) -> Vec<PathBuf> {
    let deps_dir = executable.parent().expect("executable parent");
    let mut seeds = vec![executable.to_path_buf()];
    for entry in fs::read_dir(deps_dir).expect("read deps") {
        let path = entry.expect("dep entry").path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if (name.contains("export_contract_helper") || name.contains("export_contract_runner"))
            && path.extension().and_then(|ext| ext.to_str()) == Some("rlib")
        {
            seeds.push(path);
        }
    }
    seeds.sort();
    seeds.dedup();
    seeds
}
