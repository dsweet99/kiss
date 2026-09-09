use std::fs::{File, OpenOptions};
use std::io;
#[cfg(test)]
use std::process::{Command, Stdio};

#[cfg(test)]
use crate::rust_llvm_cov_runner::execute_or_reuse::llvm_cov_process_budget::{
    ProcessBudgetBreach, check_llvm_cov_nextest_budget,
};
use crate::rust_llvm_cov_runner::plan::llvm_cov_active::llvm_cov_batch_already_active;

pub(crate) fn apply_nested_llvm_cov_argv(argv: &mut Vec<String>) {
    if !llvm_cov_batch_already_active() {
        return;
    }
    force_serial_llvm_cov_width(argv);
}

pub(crate) fn force_serial_llvm_cov_width(argv: &mut Vec<String>) {
    if argv_has_token(argv, "nextest") {
        ensure_flag_value(argv, "--build-jobs", "1");
        ensure_flag_value(argv, "--test-threads", "1");
        return;
    }
    ensure_flag_value(argv, "--jobs", "1");
}

fn argv_has_token(argv: &[String], token: &str) -> bool {
    let stop = argv.iter().position(|arg| arg == "--").unwrap_or(argv.len());
    argv[..stop].iter().any(|arg| arg == token)
}

fn ensure_flag_value(argv: &mut Vec<String>, flag: &str, value: &str) {
    let stop = argv.iter().position(|arg| arg == "--").unwrap_or(argv.len());
    let mut index = 0;
    let mut found = false;
    while index + 1 < stop {
        if argv[index] == flag {
            argv[index + 1] = value.to_string();
            found = true;
        }
        index += 1;
    }
    if !found {
        argv.insert(stop, flag.to_string());
        argv.insert(stop + 1, value.to_string());
    }
}

pub(crate) struct NestedLlvmCovLock {
    _file: Option<File>,
}

impl NestedLlvmCovLock {
    pub(crate) fn acquire() -> io::Result<Self> {
        if !llvm_cov_batch_already_active() {
            return Ok(Self { _file: None });
        }
        Self::acquire_exclusive()
    }

    pub(crate) fn acquire_exclusive() -> io::Result<Self> {
        let path = nested_lock_path();
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)?;
        fs2::FileExt::lock_exclusive(&file)?;
        Ok(Self { _file: Some(file) })
    }
}

fn nested_lock_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "kiss-llvm-cov-nested-{}.lock",
        crate::rust_llvm_cov_runner::execute_or_reuse::llvm_cov_process_budget::current_uid()
    ))
}

#[cfg(test)]
pub(crate) fn run_fixture_cargo_llvm_cov(
    mut argv: Vec<String>,
    configure: impl FnOnce(&mut Command),
) -> io::Result<std::process::Output> {
    budget_to_io(check_llvm_cov_nextest_budget())?;
    let _lock = NestedLlvmCovLock::acquire_exclusive()?;
    budget_to_io(check_llvm_cov_nextest_budget())?;
    force_serial_llvm_cov_width(&mut argv);
    let program = argv.first().cloned().unwrap_or_else(|| "cargo".to_string());
    let mut command = Command::new(program);
    if argv.len() > 1 {
        command.args(&argv[1..]);
    }
    command.stdin(Stdio::null());
    scrub_fixture_env(&mut command);
    configure(&mut command);
    command.output()
}

#[cfg(test)]
fn budget_to_io(result: Result<(), ProcessBudgetBreach>) -> io::Result<()> {
    result.map_err(|err| {
        io::Error::new(
            io::ErrorKind::ResourceBusy,
            format!(
                "{} cargo-llvm-cov processes live (cap {})",
                err.live, err.cap
            ),
        )
    })
}

#[cfg(test)]
fn scrub_fixture_env(command: &mut Command) {
    #[cfg(unix)]
    {
        crate::rust_llvm_cov_runner::execute_or_reuse::batch_shim_delegated::scrub_coverage_build_env(
            command,
        );
    }
    #[cfg(not(unix))]
    {
        let _ = command;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_nested_llvm_cov_argv, ensure_flag_value, force_serial_llvm_cov_width, scrub_fixture_env,
    };
    use crate::rust_llvm_cov_runner::plan::llvm_cov_active::LlvmCovActiveEnvGuard;
    use std::process::Command;

    #[test]
    fn nested_env_forces_serial_llvm_cov_width() {
        let mut argv = vec![
            "cargo".to_string(),
            "llvm-cov".to_string(),
            "nextest".to_string(),
            "--build-jobs".to_string(),
            "32".to_string(),
            "--test-threads".to_string(),
            "16".to_string(),
        ];
        let _clear = LlvmCovActiveEnvGuard::clear();
        apply_nested_llvm_cov_argv(&mut argv);
        assert_eq!(argv[4], "32");
        assert_eq!(argv[6], "16");
        drop(_clear);
        let _guard = LlvmCovActiveEnvGuard::enter();
        apply_nested_llvm_cov_argv(&mut argv);
        assert_eq!(argv[4], "1");
        assert_eq!(argv[6], "1");
    }

    #[test]
    fn ensure_flag_value_replaces_every_matching_flag() {
        let mut argv = vec![
            "--build-jobs".to_string(),
            "8".to_string(),
            "--build-jobs".to_string(),
            "4".to_string(),
        ];
        ensure_flag_value(&mut argv, "--build-jobs", "1");
        assert_eq!(argv[1], "1");
        assert_eq!(argv[3], "1");
    }

    #[test]
    fn force_serial_inserts_jobs_for_cargo_llvm_cov_test() {
        let mut argv = vec![
            "cargo".to_string(),
            "llvm-cov".to_string(),
            "test".to_string(),
            "--".to_string(),
            "--test-threads=1".to_string(),
        ];
        force_serial_llvm_cov_width(&mut argv);
        assert_eq!(
            argv,
            [
                "cargo",
                "llvm-cov",
                "test",
                "--jobs",
                "1",
                "--",
                "--test-threads=1",
            ]
        );
    }

    #[test]
    fn force_serial_inserts_build_jobs_for_nextest() {
        let mut argv = vec![
            "cargo".to_string(),
            "llvm-cov".to_string(),
            "nextest".to_string(),
        ];
        force_serial_llvm_cov_width(&mut argv);
        assert_eq!(
            argv,
            [
                "cargo",
                "llvm-cov",
                "nextest",
                "--build-jobs",
                "1",
                "--test-threads",
                "1",
            ]
        );
    }

    #[test]
    fn fixture_env_scrub_removes_rustc_wrapper() {
        let mut command = Command::new("cargo");
        scrub_fixture_env(&mut command);
        let wrapper = command
            .get_envs()
            .find(|(key, _)| *key == "RUSTC_WRAPPER")
            .map(|(_, value)| value);
        assert_eq!(wrapper, Some(None));
    }
}
