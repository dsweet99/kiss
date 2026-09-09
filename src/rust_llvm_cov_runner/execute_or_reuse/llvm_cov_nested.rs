use std::fs::{File, OpenOptions};
use std::io;

use crate::rust_llvm_cov_runner::plan::llvm_cov_active::llvm_cov_batch_already_active;

pub(crate) fn apply_nested_llvm_cov_argv(argv: &mut [String]) {
    if !llvm_cov_batch_already_active() {
        return;
    }
    set_flag_value(argv, "--build-jobs", "1");
    set_flag_value(argv, "--test-threads", "1");
}

fn set_flag_value(argv: &mut [String], flag: &str, value: &str) {
    let mut index = 0;
    while index + 1 < argv.len() {
        if argv[index] == flag {
            argv[index + 1] = value.to_string();
        }
        index += 1;
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
mod tests {
    use super::{apply_nested_llvm_cov_argv, set_flag_value};
    use crate::rust_llvm_cov_runner::plan::llvm_cov_active::LlvmCovActiveEnvGuard;

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
    fn set_flag_value_replaces_every_matching_flag() {
        let mut argv = vec![
            "--build-jobs".to_string(),
            "8".to_string(),
            "--build-jobs".to_string(),
            "4".to_string(),
        ];
        set_flag_value(&mut argv, "--build-jobs", "1");
        assert_eq!(argv[1], "1");
        assert_eq!(argv[3], "1");
    }
}
