use super::{TestSectionConfig, effective_python_pytest_args};
use std::path::PathBuf;

struct CwdGuard {
    original: PathBuf,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl CwdGuard {
    fn enter(path: &std::path::Path) -> Self {
        let lock = crate::cwd_test_lock::lock();
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(path).unwrap();
        Self {
            original,
            _lock: lock,
        }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.original).unwrap();
    }
}

#[test]
fn test_section_config_defaults_num_jobs_to_four() {
    assert_eq!(TestSectionConfig::default().num_jobs, 4);
}

#[test]
fn test_section_config_reads_positive_num_jobs() {
    let cwd = tempfile::TempDir::new().unwrap();
    let _cwd_guard = CwdGuard::enter(cwd.path());
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), "[test]\nnum_jobs = 7\n").unwrap();
    assert_eq!(
        TestSectionConfig::try_load_from(tmp.path())
            .unwrap()
            .num_jobs,
        7
    );
}

#[test]
fn test_section_config_rejects_nonpositive_num_jobs() {
    let cwd = tempfile::TempDir::new().unwrap();
    let _cwd_guard = CwdGuard::enter(cwd.path());
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), "[test]\nnum_jobs = 0\n").unwrap();
    assert!(TestSectionConfig::try_load_from(tmp.path()).is_err());
}

#[test]
fn test_section_config_try_load_rejects_local_nonpositive_num_jobs() {
    let tmp = tempfile::TempDir::new().unwrap();
    let _cwd_guard = CwdGuard::enter(tmp.path());
    std::fs::write(".kissconfig", "[test]\nnum_jobs = 0\n").unwrap();
    assert!(TestSectionConfig::try_load().is_err());
}

#[test]
fn test_section_config_defaults_watch_settle_to_one() {
    assert!((TestSectionConfig::default().watch_settle_seconds - 1.0).abs() < f64::EPSILON);
}

#[test]
fn test_section_config_reads_watch_settle_seconds() {
    let cwd = tempfile::TempDir::new().unwrap();
    let _cwd_guard = CwdGuard::enter(cwd.path());
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), "[test]\nwatch_settle_seconds = 2.5\n").unwrap();
    assert!(
        (TestSectionConfig::try_load_from(tmp.path())
            .unwrap()
            .watch_settle_seconds
            - 2.5)
            .abs()
            < f64::EPSILON
    );
}

#[test]
fn test_section_config_rejects_nonpositive_watch_settle() {
    let cwd = tempfile::TempDir::new().unwrap();
    let _cwd_guard = CwdGuard::enter(cwd.path());
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), "[test]\nwatch_settle_seconds = 0\n").unwrap();
    assert!(TestSectionConfig::try_load_from(tmp.path()).is_err());
}

#[test]
fn test_section_config_reads_pytest_plugins() {
    let cwd = tempfile::TempDir::new().unwrap();
    let _cwd_guard = CwdGuard::enter(cwd.path());
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        tmp.path(),
        "[test]\npytest_plugins = [\"pytest_asyncio.plugin\", \"random_order.plugin\"]\n",
    )
    .unwrap();
    let cfg = TestSectionConfig::try_load_from(tmp.path()).unwrap();
    assert_eq!(
        cfg.pytest_plugins,
        vec![
            "pytest_asyncio.plugin".to_string(),
            "random_order.plugin".to_string()
        ]
    );
    assert_eq!(
        cfg.pytest_plugin_cli_args(),
        vec![
            "-p".to_string(),
            "pytest_asyncio.plugin".to_string(),
            "-p".to_string(),
            "random_order.plugin".to_string()
        ]
    );
}

#[test]
fn test_section_config_rejects_invalid_pytest_plugins() {
    let cwd = tempfile::TempDir::new().unwrap();
    let _cwd_guard = CwdGuard::enter(cwd.path());
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), "[test]\npytest_plugins = \"asyncio\"\n").unwrap();
    assert!(TestSectionConfig::try_load_from(tmp.path()).is_err());
}

#[test]
fn test_section_config_defaults_and_reads_ignore() {
    assert!(TestSectionConfig::default().ignore.is_empty());
    let cwd = tempfile::TempDir::new().unwrap();
    let _cwd_guard = CwdGuard::enter(cwd.path());
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        tmp.path(),
        "[test]\nignore = [\"ignored_tests/\", \"vendor\"]\n",
    )
    .unwrap();
    let cfg = TestSectionConfig::try_load_from(tmp.path()).unwrap();
    assert_eq!(
        cfg.ignore,
        vec!["ignored_tests/".to_string(), "vendor".to_string()]
    );
    assert_eq!(
        cfg.merged_ignore(&["cli_extra".to_string()]),
        vec![
            "ignored_tests".to_string(),
            "vendor".to_string(),
            "cli_extra".to_string()
        ]
    );
}

#[test]
fn test_section_config_rejects_invalid_ignore() {
    let cwd = tempfile::TempDir::new().unwrap();
    let _cwd_guard = CwdGuard::enter(cwd.path());
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), "[test]\nignore = \"vendor\"\n").unwrap();
    assert!(TestSectionConfig::try_load_from(tmp.path()).is_err());
}

#[test]
fn effective_python_pytest_args_prefixes_plugins() {
    let plugins = vec!["pytest_asyncio.plugin".to_string()];
    let extra = vec!["-q".to_string()];
    assert_eq!(
        effective_python_pytest_args(&plugins, &extra),
        vec![
            "-p".to_string(),
            "pytest_asyncio.plugin".to_string(),
            "-q".to_string()
        ]
    );
}
