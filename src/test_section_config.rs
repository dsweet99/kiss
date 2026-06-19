use crate::config::{ConfigError, check_unknown_keys};
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct TestSectionConfig {
    pub main_branch: Option<String>,
}

impl TestSectionConfig {
    pub fn load() -> Self {
        let mut c = Self::default();
        if let Ok(s) = std::fs::read_to_string(".kissconfig") {
            c.merge_from_toml(&s);
        }
        c
    }

    pub fn load_from(path: &Path) -> Self {
        let mut c = Self::load();
        if let Ok(s) = std::fs::read_to_string(path) {
            c.merge_from_toml(&s);
        }
        c
    }

    pub fn try_load_from(path: &Path) -> Result<Self, ConfigError> {
        let mut c = Self::load();
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
            if let Err(e) = check_unknown_keys(t, &["main_branch"], "test") {
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
        }
    }

    fn try_merge_from_toml(&mut self, toml_str: &str) -> Result<(), ConfigError> {
        let value = toml_str
            .parse::<toml::Table>()
            .map_err(|e| ConfigError::ParseError {
                message: e.to_string(),
            })?;
        if let Some(t) = value.get("test").and_then(|v| v.as_table()) {
            check_unknown_keys(t, &["main_branch"], "test")?;
            if let Some(v) = t.get("main_branch") {
                let s = v.as_str().ok_or_else(|| ConfigError::InvalidValue {
                    key: "main_branch".into(),
                    message: "expected string".into(),
                })?;
                self.main_branch = Some(s.to_string());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static CWD_LOCK: Mutex<()> = Mutex::new(());

    fn with_temp_cwd<T>(f: impl FnOnce(&Path) -> T) -> T {
        let _guard = CWD_LOCK.lock().unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        let result = f(tmp.path());
        std::env::set_current_dir(original).unwrap();
        result
    }

    #[test]
    fn load_reads_default_and_override_test_section() {
        with_temp_cwd(|root| {
            std::fs::write(root.join(".kissconfig"), "[test]\nmain_branch = 'main'\n").unwrap();
            let override_path = root.join("override.toml");
            std::fs::write(&override_path, "[test]\nmain_branch = 'develop'\n").unwrap();

            assert_eq!(
                TestSectionConfig::load().main_branch.as_deref(),
                Some("main")
            );
            assert_eq!(
                TestSectionConfig::load_from(&override_path)
                    .main_branch
                    .as_deref(),
                Some("develop")
            );
        });
    }

    #[test]
    fn merge_from_toml_ignores_invalid_input_without_clearing_existing_value() {
        let mut config = TestSectionConfig {
            main_branch: Some("main".to_string()),
        };

        config.merge_from_toml("not toml");
        config.merge_from_toml("[test]\nmain_branch = 123\n");
        config.merge_from_toml("[test]\nunknown = 'value'\n");

        assert_eq!(config.main_branch.as_deref(), Some("main"));
    }

    #[test]
    fn try_load_from_reports_io_parse_unknown_and_invalid_value_errors() {
        with_temp_cwd(|root| {
            let missing = root.join("missing.toml");
            assert!(matches!(
                TestSectionConfig::try_load_from(&missing),
                Err(ConfigError::IoError { .. })
            ));

            let invalid_toml = root.join("invalid.toml");
            std::fs::write(&invalid_toml, "[test\n").unwrap();
            assert!(matches!(
                TestSectionConfig::try_load_from(&invalid_toml),
                Err(ConfigError::ParseError { .. })
            ));

            let unknown = root.join("unknown.toml");
            std::fs::write(&unknown, "[test]\nextra = 'x'\n").unwrap();
            assert!(matches!(
                TestSectionConfig::try_load_from(&unknown),
                Err(ConfigError::UnknownKey { .. })
            ));

            let invalid_value = root.join("invalid_value.toml");
            std::fs::write(&invalid_value, "[test]\nmain_branch = 123\n").unwrap();
            assert!(matches!(
                TestSectionConfig::try_load_from(&invalid_value),
                Err(ConfigError::InvalidValue { .. })
            ));
        });
    }

    #[test]
    fn try_load_from_merges_valid_test_section() {
        with_temp_cwd(|root| {
            let config_path = root.join("kiss.toml");
            std::fs::write(&config_path, "[test]\nmain_branch = 'trunk'\n").unwrap();

            let config = TestSectionConfig::try_load_from(&config_path).unwrap();

            assert_eq!(config.main_branch.as_deref(), Some("trunk"));
        });
    }
}
