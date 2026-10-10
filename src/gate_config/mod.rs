mod unit_test_seconds;

pub(crate) use unit_test_seconds::parse_max_unit_test_seconds;
pub use unit_test_seconds::{
    MatchedUnitTestSecondsRule, catch_all_limit, default_max_unit_test_seconds, exceeds_limit,
    format_nested_toml_table, limit_for_selector, matched_rule_for_selector,
    time_gate_uses_path_prefixes, validate_rules,
};

use crate::Language;
use crate::config::{ConfigError, check_unknown_keys};
use crate::defaults;
use std::path::Path;

const GLOBAL_KEYS: &[&str] = &[
    "min_similarity",
    "duplication_enabled",
    "comment_removal_enabled",
    "docs_allowed",
    "orphan_allowed",
];

const GATE_RENAMED_MSG: &str = "\
[gate] was renamed: put min_similarity/duplication_enabled/\
comment_removal_enabled/docs_allowed/orphan_allowed under [global], \
orphan_detection/max_unit_test_seconds under [test], and \
max_num_tests under [python] and [rust]";

#[derive(Debug, Clone)]
pub struct GateConfig {
    pub max_unit_test_seconds: Vec<(String, f64)>,
    pub max_num_tests_python: usize,
    pub max_num_tests_rust: usize,
    pub min_similarity: f64,
    pub duplication_enabled: bool,
    pub orphan_detection: bool,
    pub comment_removal_enabled: bool,
    pub docs_allowed: Vec<String>,
    pub orphan_allowed: Vec<String>,
}

impl Default for GateConfig {
    fn default() -> Self {
        Self {
            max_unit_test_seconds: default_max_unit_test_seconds(),
            max_num_tests_python: defaults::python::MAX_NUM_TESTS,
            max_num_tests_rust: defaults::rust::MAX_NUM_TESTS,
            min_similarity: defaults::duplication::MIN_SIMILARITY,
            duplication_enabled: true,
            orphan_detection: false,
            comment_removal_enabled: false,
            docs_allowed: Vec::new(),
            orphan_allowed: Vec::new(),
        }
    }
}

impl GateConfig {
    pub fn unit_test_seconds_limit(&self, selector: &str) -> f64 {
        limit_for_selector(&self.max_unit_test_seconds, selector)
    }

    pub fn catch_all_unit_test_seconds(&self) -> f64 {
        catch_all_limit(&self.max_unit_test_seconds)
            .unwrap_or(defaults::gate::MAX_UNIT_TEST_SECONDS)
    }

    pub fn unit_test_time_gate_disabled(&self) -> bool {
        self.max_unit_test_seconds.is_empty()
    }
}

pub fn max_num_tests_for(gate: &GateConfig, language: Language) -> usize {
    match language {
        Language::Python => gate.max_num_tests_python,
        Language::Rust => gate.max_num_tests_rust,
    }
}

fn load_from_file(path: &Path) -> GateConfig {
    let mut config = GateConfig::default();
    if let Ok(c) = std::fs::read_to_string(path) {
        config.merge_from_toml(&c);
    }
    config
}

impl GateConfig {
    pub fn load() -> Self {
        load_from_file(&crate::config::kissconfig_path_from_cwd())
    }

    pub fn load_for_repo(repo_root: &Path) -> Self {
        load_from_file(&crate::config::kissconfig_path_for_repo(repo_root))
    }

    pub fn load_from(path: &Path) -> Self {
        load_from_file(path)
    }

    pub fn try_load_from(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|e| ConfigError::IoError {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        Self::try_load_from_content(&content)
    }

    pub fn try_load_from_content(content: &str) -> Result<Self, ConfigError> {
        let mut config = Self::default();
        config.try_merge_from_toml(content)?;
        Ok(config)
    }

    fn merge_from_toml(&mut self, toml_str: &str) {
        let Ok(value) = toml_str.parse::<toml::Table>() else {
            return;
        };
        if value.get("gate").is_some() {
            eprintln!("Error: {GATE_RENAMED_MSG}");
            return;
        }
        if let Some(global) = value.get("global").and_then(|v| v.as_table()) {
            if check_unknown_keys(global, GLOBAL_KEYS, "global").is_err() {
                return;
            }
            merge_global_lenient(self, global);
        }
        if let Some(test) = value.get("test").and_then(|v| v.as_table()) {
            crate::test_toml::merge_test_table_lenient(test, Some(self), None, None);
        }
        merge_language_caps_lenient(self, &value);
    }

    fn try_merge_from_toml(&mut self, toml_str: &str) -> Result<(), ConfigError> {
        let value = toml_str
            .parse::<toml::Table>()
            .map_err(|e| ConfigError::ParseError {
                message: e.to_string(),
            })?;
        if value.get("gate").is_some() {
            return Err(ConfigError::InvalidValue {
                key: "gate".into(),
                message: GATE_RENAMED_MSG.into(),
            });
        }
        if let Some(global) = value.get("global").and_then(|v| v.as_table()) {
            check_unknown_keys(global, GLOBAL_KEYS, "global")?;
            merge_global_strict(self, global)?;
        }
        if let Some(test) = value.get("test").and_then(|v| v.as_table()) {
            crate::test_toml::merge_test_table_strict(test, Some(self), None, None)?;
        }
        merge_language_caps_strict(self, &value)?;
        Ok(())
    }
}

fn merge_language_caps_lenient(config: &mut GateConfig, root: &toml::Table) {
    for language in Language::ALL {
        let Some(table) = root.get(language.label()).and_then(|v| v.as_table()) else {
            continue;
        };
        match crate::test_toml::try_get_max_num_tests(table) {
            Ok(Some(n)) => set_max_num_tests(config, language, n),
            Ok(None) => {}
            Err(err) => eprintln!("Error: {err}"),
        }
    }
}

fn merge_language_caps_strict(
    config: &mut GateConfig,
    root: &toml::Table,
) -> Result<(), ConfigError> {
    for language in Language::ALL {
        let Some(table) = root.get(language.label()).and_then(|v| v.as_table()) else {
            continue;
        };
        if let Some(n) = crate::test_toml::try_get_max_num_tests(table)? {
            set_max_num_tests(config, language, n);
        }
    }
    Ok(())
}

fn set_max_num_tests(config: &mut GateConfig, language: Language, n: usize) {
    match language {
        Language::Python => config.max_num_tests_python = n,
        Language::Rust => config.max_num_tests_rust = n,
    }
}

mod toml_merge;
use toml_merge::*;

#[cfg(test)]
#[path = "gate_rename_test.rs"]
mod rename_tests;
#[cfg(test)]
#[path = "gate_config_test.rs"]
mod tests;
