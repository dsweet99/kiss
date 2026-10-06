use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(crate) const KISS_LLVM_COV_ACTIVE_ENV: &str = "KISS_LLVM_COV_ACTIVE";

pub(crate) fn llvm_cov_batch_already_active() -> bool {
    std::env::var_os(KISS_LLVM_COV_ACTIVE_ENV).is_some()
}

pub(crate) fn mark_llvm_cov_active(env: &mut BTreeMap<String, String>, nested_lock: &Path) {
    env.insert(
        KISS_LLVM_COV_ACTIVE_ENV.to_string(),
        nested_lock.to_string_lossy().into_owned(),
    );
}

pub(crate) fn nested_llvm_cov_lock_path() -> PathBuf {
    std::env::var_os(KISS_LLVM_COV_ACTIVE_ENV)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| {
            crate::test_state_dir(&std::env::current_dir().unwrap_or_default())
                .join("locks")
                .join("llvm_cov_nested.lock")
        })
}

#[cfg(test)]
pub(crate) struct LlvmCovActiveEnvGuard {
    previous: Option<std::ffi::OsString>,
}

#[cfg(test)]
impl LlvmCovActiveEnvGuard {
    pub(crate) fn enter() -> Self {
        let previous = std::env::var_os(KISS_LLVM_COV_ACTIVE_ENV);
        unsafe {
            std::env::set_var(KISS_LLVM_COV_ACTIVE_ENV, "1");
        }
        Self { previous }
    }

    pub(crate) fn clear() -> Self {
        let previous = std::env::var_os(KISS_LLVM_COV_ACTIVE_ENV);
        unsafe {
            std::env::remove_var(KISS_LLVM_COV_ACTIVE_ENV);
        }
        Self { previous }
    }
}

#[cfg(test)]
impl Drop for LlvmCovActiveEnvGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(value) => unsafe {
                std::env::set_var(KISS_LLVM_COV_ACTIVE_ENV, value);
            },
            None => unsafe {
                std::env::remove_var(KISS_LLVM_COV_ACTIVE_ENV);
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        KISS_LLVM_COV_ACTIVE_ENV, LlvmCovActiveEnvGuard, llvm_cov_batch_already_active,
        mark_llvm_cov_active,
    };
    use std::collections::BTreeMap;

    #[test]
    fn mark_llvm_cov_active_sets_child_env() {
        let mut env = BTreeMap::new();
        mark_llvm_cov_active(&mut env, std::path::Path::new("/repo/locks/n.lock"));
        assert_eq!(
            env.get(KISS_LLVM_COV_ACTIVE_ENV).map(String::as_str),
            Some("/repo/locks/n.lock")
        );
    }

    #[test]
    fn llvm_cov_batch_already_active_reads_process_env() {
        let _clear = LlvmCovActiveEnvGuard::clear();
        assert!(!llvm_cov_batch_already_active());
        let _guard = LlvmCovActiveEnvGuard::enter();
        assert!(llvm_cov_batch_already_active());
    }
}
