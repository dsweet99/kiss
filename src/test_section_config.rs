use crate::config::{
    ConfigError, apply_lenient_string_list, check_unknown_keys, parse_string_list_key,
};
use std::path::Path;

const TEST_SECTION_KEYS: &[&str] = &[
    "main_branch",
    "num_jobs",
    "watch_settle_seconds",
    "pytest_plugins",
    "ignore",
    "test_coverage_threshold",
    "test_coverage_scope",
    "max_unit_test_seconds",
    "max_num_tests",
];

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
            num_jobs: 4,
            watch_settle_seconds: 1.0,
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
        if let Ok(s) = std::fs::read_to_string(crate::config::kissconfig_path_from_cwd()) {
            c.merge_from_toml(&s);
        }
        c
    }

    pub fn try_load() -> Result<Self, ConfigError> {
        let mut c = Self::default();
        if let Ok(s) = std::fs::read_to_string(crate::config::kissconfig_path_from_cwd()) {
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
        apply_strict_test_table(self, t)
    }
}

fn apply_strict_test_table(
    config: &mut TestSectionConfig,
    table: &toml::Table,
) -> Result<(), ConfigError> {
    if let Some(v) = table.get("main_branch") {
        config.main_branch = Some(
            v.as_str()
                .ok_or_else(|| ConfigError::InvalidValue {
                    key: "main_branch".into(),
                    message: "expected string".into(),
                })?
                .to_string(),
        );
    }
    if let Some(v) = table.get("num_jobs") {
        config.num_jobs = parse_positive_usize(v, "num_jobs")?;
    }
    if let Some(v) = table.get("watch_settle_seconds") {
        config.watch_settle_seconds = parse_positive_f64(v, "watch_settle_seconds")?;
    }
    if let Some(v) = table.get("pytest_plugins") {
        config.pytest_plugins = parse_string_list_key(v, "pytest_plugins", "plugin names")?;
    }
    if let Some(v) = table.get("ignore") {
        config.ignore = parse_string_list_key(v, "ignore", "ignore patterns")?;
    }
    Ok(())
}

fn parse_positive_usize(value: &toml::Value, key: &str) -> Result<usize, ConfigError> {
    let n = value
        .as_integer()
        .ok_or_else(|| ConfigError::InvalidValue {
            key: key.into(),
            message: "expected a positive integer".into(),
        })?;
    usize::try_from(n)
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| ConfigError::InvalidValue {
            key: key.into(),
            message: "expected a positive integer".into(),
        })
}

fn parse_positive_f64(value: &toml::Value, key: &str) -> Result<f64, ConfigError> {
    let n = value
        .as_float()
        .or_else(|| value.as_integer().map(|i| i as f64))
        .ok_or_else(|| ConfigError::InvalidValue {
            key: key.into(),
            message: "expected a finite number greater than zero".into(),
        })?;
    if n.is_finite() && n > 0.0 {
        Ok(n)
    } else {
        Err(ConfigError::InvalidValue {
            key: key.into(),
            message: "expected a finite number greater than zero".into(),
        })
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
        match parse_positive_usize(v, "num_jobs") {
            Ok(n) => config.num_jobs = n,
            Err(_) => eprintln!("Warning: Config key 'num_jobs' expected a positive integer"),
        }
    }
    if let Some(v) = table.get("watch_settle_seconds") {
        match parse_positive_f64(v, "watch_settle_seconds") {
            Ok(n) => config.watch_settle_seconds = n,
            Err(_) => eprintln!(
                "Warning: Config key 'watch_settle_seconds' expected a finite number greater than zero"
            ),
        }
    }
    apply_lenient_string_list(table, "pytest_plugins", "plugin names", |v| {
        config.pytest_plugins = v;
    });
    apply_lenient_string_list(table, "ignore", "ignore patterns", |v| {
        config.ignore = v;
    });
}

#[cfg(test)]
#[path = "test_section_config_test.rs"]
mod tests;
