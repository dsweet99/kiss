use std::path::{Path, PathBuf};

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
