use crate::config::{ConfigError, check_unknown_keys};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct TestSectionConfig {
    pub main_branch: Option<String>,
    pub num_jobs: usize,
}

impl Default for TestSectionConfig {
    fn default() -> Self {
        Self {
            main_branch: None,
            num_jobs: 4,
        }
    }
}

impl TestSectionConfig {
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
        if let Some(t) = value.get("test").and_then(|v| v.as_table()) {
            if let Err(e) = check_unknown_keys(t, &["main_branch", "num_jobs"], "test") {
                eprintln!("Error: {e}");
                return;
            }
            if let Some(v) = t.get("main_branch") {
                if let Some(s) = v.as_str() {
                    self.main_branch = Some(s.to_string());
                } else {
                    eprintln!("Warning: Config key 'main_branch' expected string");
                }
            }
            if let Some(v) = t.get("num_jobs") {
                if let Some(n) = v.as_integer().and_then(|n| usize::try_from(n).ok())
                    && n > 0
                {
                    self.num_jobs = n;
                } else {
                    eprintln!("Warning: Config key 'num_jobs' expected a positive integer");
                }
            }
        }
    }

    fn try_merge_from_toml(&mut self, toml_str: &str) -> Result<(), ConfigError> {
        let value = toml_str
            .parse::<toml::Table>()
            .map_err(|e| ConfigError::ParseError {
                message: e.to_string(),
            })?;
        if let Some(t) = value.get("test").and_then(|v| v.as_table()) {
            check_unknown_keys(t, &["main_branch", "num_jobs"], "test")?;
            if let Some(v) = t.get("main_branch") {
                let s = v.as_str().ok_or_else(|| ConfigError::InvalidValue {
                    key: "main_branch".into(),
                    message: "expected string".into(),
                })?;
                self.main_branch = Some(s.to_string());
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
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::TestSectionConfig;

    #[test]
    fn test_section_config_defaults_num_jobs_to_four() {
        assert_eq!(TestSectionConfig::default().num_jobs, 4);
    }

    #[test]
    fn test_section_config_reads_positive_num_jobs() {
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
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "[test]\nnum_jobs = 0\n").unwrap();

        assert!(TestSectionConfig::try_load_from(tmp.path()).is_err());
    }

    #[test]
    fn test_section_config_try_load_rejects_local_nonpositive_num_jobs() {
        let _cwd_guard = crate::cwd_test_lock::lock();
        let tmp = tempfile::TempDir::new().unwrap();
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        std::fs::write(".kissconfig", "[test]\nnum_jobs = 0\n").unwrap();

        assert!(TestSectionConfig::try_load().is_err());

        std::env::set_current_dir(original).unwrap();
    }
}
