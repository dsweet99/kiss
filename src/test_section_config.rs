use crate::config::ConfigError;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct TestSectionConfig {
    pub main_branch: Option<String>,
    pub num_jobs: usize,
    pub watch_settle_seconds: f64,
    pub pytest_plugins: Vec<String>,
    pub ignore: Vec<String>,
}

impl Default for TestSectionConfig {
    fn default() -> Self {
        Self {
            main_branch: None,
            num_jobs: crate::defaults::gate::NUM_JOBS,
            watch_settle_seconds: crate::defaults::gate::WATCH_SETTLE_SECONDS,
            pytest_plugins: Vec::new(),
            ignore: Vec::new(),
        }
    }
}

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

pub fn effective_python_pytest_args(plugins: &[String], extra: &[String]) -> Vec<String> {
    let mut args = pytest_plugin_cli_args(plugins);
    args.extend(extra.iter().cloned());
    args
}

impl TestSectionConfig {
    pub fn pytest_plugin_cli_args(&self) -> Vec<String> {
        pytest_plugin_cli_args(&self.pytest_plugins)
    }

    #[must_use]
    pub fn merged_ignore(&self, cli_ignore: &[String]) -> Vec<String> {
        let mut ignore = self.ignore.clone();
        ignore.extend(cli_ignore.iter().cloned());
        crate::discovery::merge_check_ignore_prefixes(&ignore)
    }

    pub fn load() -> Self {
        let mut c = Self::default();
        if let Ok(s) = std::fs::read_to_string(crate::config::active_kissconfig_path()) {
            c.merge_from_toml(&s);
        }
        c
    }

    pub fn try_load() -> Result<Self, ConfigError> {
        let mut c = Self::default();
        if let Ok(s) = std::fs::read_to_string(crate::config::active_kissconfig_path()) {
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

    pub fn try_load_path_only(path: &Path) -> Result<Self, ConfigError> {
        let mut c = Self::default();
        if !path.exists() {
            return Ok(c);
        }
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
        crate::test_toml::merge_test_table_lenient(t, None, Some(self));
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
        crate::test_toml::merge_test_table_strict(t, None, Some(self))
    }
}

#[cfg(test)]
#[path = "test_section_config_test.rs"]
mod tests;
