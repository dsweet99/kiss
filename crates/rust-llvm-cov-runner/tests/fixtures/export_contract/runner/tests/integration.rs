use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn invokes_helper_in_process() {
    assert_eq!(export_contract_runner::run_helper(), 42);
}

#[test]
fn spawns_instrumented_helper_binary() {
    assert_eq!(export_contract_runner::run_helper(), 42);
    let helper = locate_helper_bin_near_test_executable();
    let output = Command::new(&helper)
        .output()
        .unwrap_or_else(|err| panic!("failed to spawn {}: {err}", helper.display()));
    assert!(
        output.status.success(),
        "helper-bin failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn locate_helper_bin_near_test_executable() -> PathBuf {
    if let Some(helper) = std::env::var_os("EXPORT_CONTRACT_HELPER_BIN") {
        let helper = PathBuf::from(helper);
        if helper.is_file() {
            return helper;
        }
    }
    if let Some(target_dir) = std::env::var_os("CARGO_TARGET_DIR") {
        let base = PathBuf::from(target_dir);
        for profile in ["debug", "release"] {
            let candidate = base.join(profile).join("helper-bin");
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    let exe = std::env::current_exe().expect("current test executable");
    let mut dir = exe
        .parent()
        .map(Path::to_path_buf)
        .expect("test executable parent");
    for _ in 0..12 {
        let candidate = dir.join("helper-bin");
        if candidate.is_file() {
            return candidate;
        }
        let Some(parent) = dir.parent() else {
            break;
        };
        dir = parent.to_path_buf();
    }
    panic!(
        "helper-bin not found near test executable {}",
        exe.display()
    );
}
