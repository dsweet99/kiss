use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::{Mutex, OnceLock};

use crate::kiss_publication_barrier::{BARRIER_DIR_ENV, BARRIER_TARGET_ENV, unique_nanos};

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub(crate) struct EnvGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    dir: Option<String>,
    target: Option<String>,
}

impl EnvGuard {
    pub(crate) fn set(dir: Option<&Path>, target: Option<&str>) -> Self {
        let lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let guard = Self {
            _lock: lock,
            dir: std::env::var(BARRIER_DIR_ENV).ok(),
            target: std::env::var(BARRIER_TARGET_ENV).ok(),
        };
        unsafe {
            if let Some(dir) = dir {
                std::env::set_var(BARRIER_DIR_ENV, dir);
            } else {
                std::env::remove_var(BARRIER_DIR_ENV);
            }
            if let Some(target) = target {
                std::env::set_var(BARRIER_TARGET_ENV, target);
            } else {
                std::env::remove_var(BARRIER_TARGET_ENV);
            }
        }
        guard
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        unsafe {
            if let Some(value) = &self.dir {
                std::env::set_var(BARRIER_DIR_ENV, value);
            } else {
                std::env::remove_var(BARRIER_DIR_ENV);
            }
            if let Some(value) = &self.target {
                std::env::set_var(BARRIER_TARGET_ENV, value);
            } else {
                std::env::remove_var(BARRIER_TARGET_ENV);
            }
        }
    }
}

pub(crate) fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "kiss-publication-barrier-test-{}-{}",
        process::id(),
        unique_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    dir
}
