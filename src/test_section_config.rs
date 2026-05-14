use crate::config::{ConfigError, check_unknown_keys};
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct TestSectionConfig {
    pub main_branch: Option<String>,
}

impl TestSectionConfig {
    pub fn load() -> Self {
        let mut c = Self::default();
        if let Some(home) = std::env::var_os("HOME")
            && let Ok(s) = std::fs::read_to_string(Path::new(&home).join(".kissconfig"))
        {
            c.merge_from_toml(&s);
        }
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
