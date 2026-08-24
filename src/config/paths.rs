use std::cell::RefCell;
use std::path::{Path, PathBuf};

thread_local! {
    static CONFIG_PATH_OVERRIDE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

pub fn set_config_path_override(path: Option<&Path>) {
    CONFIG_PATH_OVERRIDE.with(|slot| {
        *slot.borrow_mut() = path.map(Path::to_path_buf);
    });
}

#[must_use]
pub fn active_kissconfig_path() -> PathBuf {
    CONFIG_PATH_OVERRIDE.with(|slot| {
        slot.borrow()
            .clone()
            .unwrap_or_else(kissconfig_path_from_cwd)
    })
}

pub struct ConfigPathOverrideGuard {
    previous: Option<PathBuf>,
}

impl ConfigPathOverrideGuard {
    #[must_use]
    pub fn enter(path: Option<&Path>) -> Self {
        let previous = CONFIG_PATH_OVERRIDE.with(|slot| slot.borrow().clone());
        set_config_path_override(path);
        Self { previous }
    }
}

impl Drop for ConfigPathOverrideGuard {
    fn drop(&mut self) {
        CONFIG_PATH_OVERRIDE.with(|slot| {
            *slot.borrow_mut() = self.previous.take();
        });
    }
}

pub fn find_repo_root(start: &Path) -> PathBuf {
    let start = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
    let start_dir = if start.is_file() {
        start.parent().unwrap_or(&start).to_path_buf()
    } else {
        start
    };
    let mut cursor = start_dir.as_path();
    loop {
        if cursor.join(".git").exists() {
            return cursor.to_path_buf();
        }
        let Some(parent) = cursor.parent() else {
            return start_dir;
        };
        cursor = parent;
    }
}

pub fn kissconfig_path_from_cwd() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    find_repo_root(&cwd).join(".kissconfig")
}

pub fn kissconfig_path_for_repo(repo_root: &Path) -> PathBuf {
    repo_root.join(".kissconfig")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_repo_root_walks_up_to_git() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        let nested = tmp.path().join("src").join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(find_repo_root(&nested), tmp.path());
    }

    #[test]
    fn find_repo_root_without_git_stays_at_start_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert_eq!(find_repo_root(tmp.path()), tmp.path());
    }

    #[test]
    fn kissconfig_path_for_repo_is_root_only() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert_eq!(
            kissconfig_path_for_repo(tmp.path()),
            tmp.path().join(".kissconfig")
        );
    }

    #[test]
    fn config_path_override_guard_restores_previous() {
        let tmp = tempfile::TempDir::new().unwrap();
        let custom = tmp.path().join("custom.toml");
        {
            let _guard = ConfigPathOverrideGuard::enter(Some(&custom));
            assert_eq!(active_kissconfig_path(), custom);
        }
        assert_ne!(active_kissconfig_path(), custom);
    }

    #[test]
    fn load_for_repo_reads_root_kissconfig_not_nested() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join(".kissconfig"),
            "[test]\ntest_coverage_threshold = 41\n",
        )
        .unwrap();
        std::fs::create_dir_all(tmp.path().join("nested")).unwrap();
        std::fs::write(
            tmp.path().join("nested").join(".kissconfig"),
            "[test]\ntest_coverage_threshold = 7\n",
        )
        .unwrap();
        let gate = crate::gate_config::GateConfig::load_for_repo(tmp.path());
        assert_eq!(gate.test_coverage_threshold, 41);
    }
}
