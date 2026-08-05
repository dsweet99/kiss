use crate::config::{ConfigError, check_unknown_keys};
use std::path::Path;

const TEST_SECTION_KEYS: &[&str] = &[
    "main_branch",
    "num_jobs",
    "watch_settle_seconds",
    "pytest_plugins",
];

#[derive(Debug, Clone)]
pub struct TestSectionConfig {
    pub main_branch: Option<String>,
    pub num_jobs: usize,
    pub watch_settle_seconds: f64,
    /// Explicit pytest plugin modules loaded via `-p` while plugin autoload is disabled.
    pub pytest_plugins: Vec<String>,
}

impl Default for TestSectionConfig {
    fn default() -> Self {
        Self {
            main_branch: None,
            num_jobs: 4,
            watch_settle_seconds: 1.0,
            pytest_plugins: Vec::new(),
        }
    }
}

/// Expand plugin module names into pytest `-p` CLI pairs.
pub fn pytest_plugin_cli_args(plugins: &[String]) -> Vec<String> {
    let mut args = Vec::with_capacity(plugins.len().saturating_mul(2));
    for plugin in plugins {
        let name = plugin.trim();
        if name.is_empty() {
            continue;
        }
        args.push("-p".to_string());
        args.push(name.to_string());
    }
    args
}

/// Prefixed plugin `-p` args followed by caller `extra` args (Python-only).
pub fn effective_python_pytest_args(plugins: &[String], extra: &[String]) -> Vec<String> {
    let mut args = pytest_plugin_cli_args(plugins);
    args.extend(extra.iter().cloned());
    args
}

impl TestSectionConfig {
    /// Expand configured plugin modules into pytest `-p` CLI pairs.
    pub fn pytest_plugin_cli_args(&self) -> Vec<String> {
        pytest_plugin_cli_args(&self.pytest_plugins)
    }

    pub fn load() -> Self {
        let mut c = Self::default();
        if let Ok(s) = std::fs::read_to_string(".kissconfig") {
            c.merge_from_toml(&s);
        }
        c
    }

    pub fn try_load() -> Result<Self, ConfigError> {
        let mut c = Self::default();
        if let Ok(s) = std::fs::read_to_string(".kissconfig") {
            c.try_merge_from_toml(&s)?;
        }
        Ok(c)
    }

    pub fn load_from(path: &Path) -> Self {
        let mut c = Self::load();
        if let Ok(s) = std::fs::read_to_string(path) {
            c.merge_from_toml(&s);
        }
        c
    }

    pub fn try_load_from(path: &Path) -> Result<Self, ConfigError> {
        let mut c = Self::try_load()?;
        let s = std::fs::read_to_string(path).map_err(|e| ConfigError::IoError {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        c.try_merge_from_toml(&s)?;
        Ok(c)
    }

    fn merge_from_toml(&mut self, toml_str: &str) {
        let Ok(value) = toml_str.parse::<toml::Table>() else {
            return;
        };
        let Some(t) = value.get("test").and_then(|v| v.as_table()) else {
            return;
        };
        if let Err(e) = check_unknown_keys(t, TEST_SECTION_KEYS, "test") {
            eprintln!("Error: {e}");
            return;
        }
        apply_lenient_test_table(self, t);
    }

    fn try_merge_from_toml(&mut self, toml_str: &str) -> Result<(), ConfigError> {
        let value = toml_str
            .parse::<toml::Table>()
            .map_err(|e| ConfigError::ParseError {
                message: e.to_string(),
            })?;
        let Some(t) = value.get("test").and_then(|v| v.as_table()) else {
            return Ok(());
        };
        check_unknown_keys(t, TEST_SECTION_KEYS, "test")?;
        if let Some(v) = t.get("main_branch") {
            self.main_branch = Some(v.as_str().ok_or_else(|| ConfigError::InvalidValue {
                key: "main_branch".into(),
                message: "expected string".into(),
            })?.to_string());
        }
        if let Some(v) = t.get("num_jobs") {
            let n = v.as_integer().ok_or_else(|| ConfigError::InvalidValue {
                key: "num_jobs".into(),
                message: "expected a positive integer".into(),
            })?;
            self.num_jobs = usize::try_from(n).ok().filter(|n| *n > 0).ok_or_else(|| {
                ConfigError::InvalidValue {
                    key: "num_jobs".into(),
                    message: "expected a positive integer".into(),
                }
            })?;
        }
        if let Some(v) = t.get("watch_settle_seconds") {
            let n = v
                .as_float()
                .or_else(|| v.as_integer().map(|i| i as f64))
                .ok_or_else(|| ConfigError::InvalidValue {
                    key: "watch_settle_seconds".into(),
                    message: "expected a finite number greater than zero".into(),
                })?;
            if !n.is_finite() || n <= 0.0 {
                return Err(ConfigError::InvalidValue {
                    key: "watch_settle_seconds".into(),
                    message: "expected a finite number greater than zero".into(),
                });
            }
            self.watch_settle_seconds = n;
        }
        if let Some(v) = t.get("pytest_plugins") {
            self.pytest_plugins =
                parse_pytest_plugins(v).map_err(|message| ConfigError::InvalidValue {
                    key: "pytest_plugins".into(),
                    message,
                })?;
        }
        Ok(())
    }
}

fn apply_lenient_test_table(config: &mut TestSectionConfig, table: &toml::Table) {
    if let Some(v) = table.get("main_branch") {
        if let Some(s) = v.as_str() {
            config.main_branch = Some(s.to_string());
        } else {
            eprintln!("Warning: Config key 'main_branch' expected string");
        }
    }
    if let Some(v) = table.get("num_jobs") {
        if let Some(n) = v.as_integer().and_then(|n| usize::try_from(n).ok()).filter(|n| *n > 0)
        {
            config.num_jobs = n;
        } else {
            eprintln!("Warning: Config key 'num_jobs' expected a positive integer");
        }
    }
    if let Some(v) = table.get("watch_settle_seconds") {
        if let Some(n) = v
            .as_float()
            .or_else(|| v.as_integer().map(|i| i as f64))
            .filter(|n| n.is_finite() && *n > 0.0)
        {
            config.watch_settle_seconds = n;
        } else {
            eprintln!(
                "Warning: Config key 'watch_settle_seconds' expected a finite number greater than zero"
            );
        }
    }
    if let Some(v) = table.get("pytest_plugins") {
        match parse_pytest_plugins(v) {
            Ok(plugins) => config.pytest_plugins = plugins,
            Err(message) => eprintln!("Warning: Config key 'pytest_plugins' {message}"),
        }
    }
}

fn parse_pytest_plugins(value: &toml::Value) -> Result<Vec<String>, String> {
    let arr = value
        .as_array()
        .ok_or_else(|| "expected an array of strings".to_string())?;
    let mut plugins = Vec::with_capacity(arr.len());
    for item in arr {
        let s = item
            .as_str()
            .ok_or_else(|| "expected an array of strings".to_string())?;
        let name = s.trim();
        if name.is_empty() {
            return Err("plugin names must be non-empty".to_string());
        }
        plugins.push(name.to_string());
    }
    Ok(plugins)
}

#[cfg(test)]
mod tests {
    use super::TestSectionConfig;
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
    fn effective_python_pytest_args_prefixes_plugins() {
        let plugins = vec!["pytest_asyncio.plugin".to_string()];
        let extra = vec!["-q".to_string()];
        assert_eq!(
            super::effective_python_pytest_args(&plugins, &extra),
            vec![
                "-p".to_string(),
                "pytest_asyncio.plugin".to_string(),
                "-q".to_string()
            ]
        );
    }
}
