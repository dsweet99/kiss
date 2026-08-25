use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub(crate) fn target_runner_shim_program() -> String {
    resolve_target_runner_shim_program(
        std::env::var_os("KISS_RUST_LLVM_COV_TARGET_RUNNER_SHIM"),
        std::env::current_exe().ok(),
    )
}

pub(crate) fn resolve_target_runner_shim_program(
    env_override: Option<OsString>,
    current_exe: Option<PathBuf>,
) -> String {
    if let Some(path) = env_override {
        return path.to_string_lossy().to_string();
    }
    if let Some(exe) = current_exe {
        if let Some(cli) = prefer_nonhashed_kiss_cli(&exe) {
            return cli.to_string_lossy().to_string();
        }
        return exe.to_string_lossy().to_string();
    }
    "kiss".to_string()
}

fn prefer_nonhashed_kiss_cli(exe: &Path) -> Option<PathBuf> {
    let name = exe.file_name()?.to_str()?;
    let parent = exe.parent()?;
    if parent.file_name()?.to_str()? != "deps" {
        return None;
    }
    if !is_cargo_hashed_kiss_artifact(name) {
        return None;
    }
    let candidate = parent.parent()?.join(kiss_cli_file_name());
    candidate.is_file().then_some(candidate)
}

fn kiss_cli_file_name() -> &'static str {
    if cfg!(windows) { "kiss.exe" } else { "kiss" }
}

fn is_cargo_hashed_kiss_artifact(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("kiss-") else {
        return false;
    };
    let rest = rest.strip_suffix(".exe").unwrap_or(rest);
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::{
        is_cargo_hashed_kiss_artifact, prefer_nonhashed_kiss_cli,
        resolve_target_runner_shim_program, target_runner_shim_program,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn env_override_wins_over_current_exe() {
        let resolved = resolve_target_runner_shim_program(
            Some("/tmp/override-kiss".into()),
            Some(PathBuf::from(
                "/tmp/target/debug/deps/kiss-abcdef0123456789",
            )),
        );
        assert_eq!(resolved, "/tmp/override-kiss");
    }

    #[test]
    fn missing_current_exe_falls_back_to_kiss_name() {
        assert_eq!(resolve_target_runner_shim_program(None, None), "kiss");
    }

    #[test]
    fn non_deps_current_exe_is_kept() {
        let exe = PathBuf::from("/opt/kiss");
        assert_eq!(
            resolve_target_runner_shim_program(None, Some(exe.clone())),
            exe.to_string_lossy()
        );
    }

    #[test]
    fn hashed_deps_artifact_prefers_adjacent_cli_when_present() {
        let root = tempfile::tempdir().unwrap();
        let deps = root.path().join("deps");
        fs::create_dir_all(&deps).unwrap();
        let cli = root.path().join("kiss");
        fs::write(&cli, b"cli").unwrap();
        let harness = deps.join("kiss-fe206e86e67a977d");
        fs::write(&harness, b"harness").unwrap();

        let preferred = prefer_nonhashed_kiss_cli(&harness).unwrap();
        assert_eq!(preferred, cli);
        assert_eq!(
            resolve_target_runner_shim_program(None, Some(harness)),
            cli.to_string_lossy()
        );
    }

    #[test]
    fn hashed_deps_artifact_keeps_harness_when_cli_missing() {
        let root = tempfile::tempdir().unwrap();
        let deps = root.path().join("deps");
        fs::create_dir_all(&deps).unwrap();
        let harness = deps.join("kiss-aabbccddeeff0011");
        fs::write(&harness, b"harness").unwrap();

        assert!(prefer_nonhashed_kiss_cli(&harness).is_none());
        assert_eq!(
            resolve_target_runner_shim_program(None, Some(harness.clone())),
            harness.to_string_lossy()
        );
    }

    #[test]
    fn metamorphic_hashed_suffix_resolution_is_suffix_invariant() {
        let root = tempfile::tempdir().unwrap();
        let deps = root.path().join("deps");
        fs::create_dir_all(&deps).unwrap();
        let cli = root.path().join("kiss");
        fs::write(&cli, b"cli").unwrap();

        let suffixes = ["0123456789abcdef", "deadbeefcafebabe", "0000000000000001"];
        let resolved: Vec<_> = suffixes
            .iter()
            .map(|suffix| {
                let harness = deps.join(format!("kiss-{suffix}"));
                fs::write(&harness, b"harness").unwrap();
                resolve_target_runner_shim_program(None, Some(harness))
            })
            .collect();

        assert!(resolved.iter().all(|path| path == &cli.to_string_lossy()));
    }

    #[test]
    fn fuzz_hashed_artifact_detector_accepts_hex_rejects_noise() {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        println!("fuzz_hashed_artifact_detector seed={seed}");
        let mut state = seed;
        for _ in 0..64 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let hex_len = 8 + (state % 9) as usize;
            let hex: String = (0..hex_len)
                .map(|i| {
                    const HEX: &[u8] = b"0123456789abcdef";
                    HEX[((state >> (i * 4)) as usize) % 16] as char
                })
                .collect();
            assert!(is_cargo_hashed_kiss_artifact(&format!("kiss-{hex}")));
            assert!(!is_cargo_hashed_kiss_artifact(&format!("kiss-{hex}z")));
            assert!(!is_cargo_hashed_kiss_artifact(&format!("demo-{hex}")));
            assert!(!is_cargo_hashed_kiss_artifact("kiss"));
        }
    }

    #[test]
    fn target_runner_shim_program_honors_test_override() {
        unsafe {
            std::env::set_var("KISS_RUST_LLVM_COV_TARGET_RUNNER_SHIM", "/tmp/kiss-test");
        }
        assert_eq!(target_runner_shim_program(), "/tmp/kiss-test");

        unsafe {
            std::env::remove_var("KISS_RUST_LLVM_COV_TARGET_RUNNER_SHIM");
        }
    }
}
