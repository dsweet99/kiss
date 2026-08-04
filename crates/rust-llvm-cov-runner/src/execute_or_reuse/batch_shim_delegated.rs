use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::execute_or_reuse::batch_process_tree::ProcessGroupIdentity;

pub(crate) const DELEGATED_GO_ENV: &str = "KISS_RUST_LLVM_COV_DELEGATED_GO";
/// Optional QA/debug hold (milliseconds) after writing start identities and before
/// releasing the delegated handshake. Unset/0 keeps production paths fast.
///
/// Intentionally omitted from [`COVERAGE_BUILD_ENV_KEYS`]: batch subprocess env
/// scrubbing must not strip this from the shim host (which reads it via
/// `std::env`). Delegated children may inherit it; that is harmless.
pub(crate) const HOLD_BEFORE_GO_MS_ENV: &str = "KISS_RUST_LLVM_COV_HOLD_BEFORE_GO_MS";

const COVERAGE_BUILD_ENV_KEYS: &[&str] = &[
    "LLVM_PROFILE_FILE",
    "LLVM_PROFILE_FILE_NAME",
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
    "RUSTFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "RUSTDOCFLAGS",
    "CARGO_TARGET_DIR",
    "CARGO_LLVM_COV_TARGET_DIR",
    "CARGO_LLVM_COV_BUILD_DIR",
    "KISS_COVERAGE_RUNTIME_REFRESH_ACTIVE",
    "KISS_RUST_COVERAGE_PROFILE_POOL",
];

#[cfg(test)]
pub(crate) const COVERAGE_BUILD_ENV_KEYS_FOR_TEST: &[&str] = COVERAGE_BUILD_ENV_KEYS;

pub(crate) fn spawn_delegated_piped_child(
    delegated: &[String],
    command: &[OsString],
    profile_path: &Path,
    go_path: &Path,
) -> io::Result<(std::process::Child, Option<ProcessGroupIdentity>)> {
    let mut command = build_handshake_wrapped_command(delegated, command, go_path);
    scrub_coverage_build_env(&mut command);
    command.env("LLVM_PROFILE_FILE", profile_path);
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command.stdin(Stdio::null());
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = command.spawn()?;
    let delegated_identity = record_spawned_child_identity(&child);
    Ok((child, delegated_identity))
}

pub(crate) fn scrub_coverage_build_env(command: &mut Command) {
    for key in COVERAGE_BUILD_ENV_KEYS {
        command.env_remove(key);
    }
}

pub(crate) fn release_delegated_child_handshake(go_path: &Path) -> io::Result<()> {
    hold_before_delegated_go_release();
    fs::write(go_path, b"go\n")
}

fn hold_before_delegated_go_release() {
    let Ok(raw) = std::env::var(HOLD_BEFORE_GO_MS_ENV) else {
        return;
    };
    let Ok(ms) = raw.parse::<u64>() else {
        return;
    };
    if ms == 0 {
        return;
    }
    std::thread::sleep(std::time::Duration::from_millis(ms.min(5_000)));
}

fn build_handshake_wrapped_command(
    delegated: &[String],
    command: &[OsString],
    go_path: &Path,
) -> Command {
    let mut wrapper = Command::new("/bin/sh");
    wrapper.arg("-c").arg(
        "while [ ! -f \"$KISS_RUST_LLVM_COV_DELEGATED_GO\" ]; do sleep 0.001; done; cmd=$1; shift; exec \"$cmd\" \"$@\"",
    );
    wrapper.arg("sh");
    if delegated.is_empty() {
        wrapper.arg(&command[0]);
        wrapper.args(&command[1..]);
    } else {
        wrapper.arg(&delegated[0]);
        wrapper.args(&delegated[1..]);
        wrapper.args(command);
    }
    wrapper.env(DELEGATED_GO_ENV, go_path);
    wrapper
}

fn record_spawned_child_identity(child: &std::process::Child) -> Option<ProcessGroupIdentity> {
    let pid = child.id();
    if pid == 0 {
        return None;
    }
    let pgid = unsafe { libc::getpgid(pid as i32) };
    if pgid <= 0 {
        return None;
    }
    Some(ProcessGroupIdentity {
        pid,
        pgid: pgid as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvVarGuard {
        key: &'static str,
        prior: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let prior = std::env::var_os(key);
            unsafe { std::env::set_var(key, value) };
            Self { key, prior }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(prior) = &self.prior {
                unsafe { std::env::set_var(self.key, prior) };
            } else {
                unsafe { std::env::remove_var(self.key) };
            }
        }
    }

    fn env_has_key(env: &str, key: &str) -> bool {
        let prefix = format!("{key}=");
        env.lines().any(|line| line.starts_with(&prefix))
    }

    fn env_has_binding(env: &str, key: &str, value: &str) -> bool {
        let expected = format!("{key}={value}");
        env.lines().any(|line| line == expected)
    }

    #[test]
    fn hold_before_go_release_honors_env_milliseconds() {
        let _lock = crate::test_support::shim_test_env_lock();
        let _hold = EnvVarGuard::set(HOLD_BEFORE_GO_MS_ENV, "40");
        let started = std::time::Instant::now();
        hold_before_delegated_go_release();
        assert!(
            started.elapsed() >= std::time::Duration::from_millis(35),
            "elapsed={:?}",
            started.elapsed()
        );
    }

    #[test]
    fn hold_before_go_release_skips_when_unset() {
        let _lock = crate::test_support::shim_test_env_lock();
        let _hold = EnvVarGuard::set(HOLD_BEFORE_GO_MS_ENV, "0");
        let started = std::time::Instant::now();
        hold_before_delegated_go_release();
        assert!(started.elapsed() < std::time::Duration::from_millis(20));
    }

    #[test]
    fn delegated_child_scrubs_coverage_build_environment() {
        let _lock = crate::test_support::shim_test_env_lock();
        let _rustflags = EnvVarGuard::set("RUSTFLAGS", "-Cinstrument-coverage");
        let _target_dir = EnvVarGuard::set("CARGO_TARGET_DIR", "/outer-target");
        let _check_refresh = EnvVarGuard::set("KISS_COVERAGE_RUNTIME_REFRESH_ACTIVE", "1");
        let tmp = tempfile::tempdir().unwrap();
        let go_path = tmp.path().join("go");
        let profile_path = tmp.path().join("fresh.profraw");
        let command = vec![OsString::from("/usr/bin/env")];

        let (child, _) =
            spawn_delegated_piped_child(&[], &command, &profile_path, &go_path).unwrap();
        release_delegated_child_handshake(&go_path).unwrap();
        let output = child.wait_with_output().unwrap();
        let env = String::from_utf8(output.stdout).unwrap();

        assert!(output.status.success());
        assert!(!env_has_key(&env, "RUSTFLAGS"));
        assert!(!env_has_key(&env, "CARGO_ENCODED_RUSTFLAGS"));
        assert!(!env_has_key(&env, "CARGO_TARGET_DIR"));
        assert!(!env_has_key(&env, "KISS_COVERAGE_RUNTIME_REFRESH_ACTIVE"));
        assert!(env_has_binding(
            &env,
            "LLVM_PROFILE_FILE",
            &profile_path.display().to_string()
        ));
    }

    #[test]
    fn delegated_child_runs_command_through_nonempty_delegated_runner() {
        let _lock = crate::test_support::shim_test_env_lock();
        let tmp = tempfile::tempdir().unwrap();
        let go_path = tmp.path().join("go");
        let profile_path = tmp.path().join("fresh.profraw");
        let delegated = vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "exec \"$@\"".to_string(),
            "delegated-sh".to_string(),
        ];
        let command = vec![
            OsString::from("/bin/sh"),
            OsString::from("-c"),
            OsString::from("printf delegated-ok"),
        ];

        let (child, identity) =
            spawn_delegated_piped_child(&delegated, &command, &profile_path, &go_path).unwrap();
        assert!(identity.is_some());
        release_delegated_child_handshake(&go_path).unwrap();
        let output = child.wait_with_output().unwrap();

        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "delegated-ok");
    }
}
