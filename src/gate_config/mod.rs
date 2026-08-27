mod unit_test_seconds;

pub use unit_test_seconds::{
    MatchedUnitTestSecondsRule, catch_all_limit, default_max_unit_test_seconds, exceeds_limit,
    format_nested_toml_table, limit_for_selector, matched_rule_for_selector, validate_rules,
};

use crate::config::{ConfigError, check_unknown_keys};
use crate::defaults;
use std::fmt;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TestCoverageScope {
    ByFile,
    #[default]
    Codebase,
}

impl TestCoverageScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ByFile => "by_file",
            Self::Codebase => "codebase",
        }
    }

    fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "by_file" => Ok(Self::ByFile),
            "codebase" => Ok(Self::Codebase),
            other => Err(format!(
                "must be \"by_file\" or \"codebase\", got \"{other}\""
            )),
        }
    }
}

impl fmt::Display for TestCoverageScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

const GLOBAL_KEYS: &[&str] = &[
    "min_similarity",
    "duplication_enabled",
    "orphan_module_enabled",
    "orphan_unit_enabled",
    "comment_removal_enabled",
    "docs_allowed",
    "orphan_allowed",
];

const GATE_RENAMED_MSG: &str = "\
[gate] was renamed: put min_similarity/duplication_enabled/orphan_module_enabled/\
orphan_unit_enabled/comment_removal_enabled/docs_allowed/orphan_allowed under [global], and test_coverage_threshold/\
test_coverage_scope/max_unit_test_seconds/max_num_tests under [test]";

#[derive(Debug, Clone)]
pub struct GateConfig {
    pub test_coverage_threshold: usize,
    pub test_coverage_scope: TestCoverageScope,
    pub max_unit_test_seconds: Vec<(String, f64)>,
    pub max_num_tests: usize,
    pub min_similarity: f64,
    pub duplication_enabled: bool,
    pub orphan_module_enabled: bool,
    pub orphan_unit_enabled: bool,
    pub comment_removal_enabled: bool,
    pub docs_allowed: Vec<String>,
    pub orphan_allowed: Vec<String>,
}

impl Default for GateConfig {
    fn default() -> Self {
        Self {
            test_coverage_threshold: defaults::gate::TEST_COVERAGE_THRESHOLD,
            test_coverage_scope: TestCoverageScope::Codebase,
            max_unit_test_seconds: default_max_unit_test_seconds(),
            max_num_tests: defaults::gate::MAX_NUM_TESTS,
            min_similarity: defaults::duplication::MIN_SIMILARITY,
            duplication_enabled: true,
            orphan_module_enabled: true,
            orphan_unit_enabled: false,
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
            if let Err(e) = check_unknown_keys(global, GLOBAL_KEYS, "global") {
                eprintln!("Error: {e}");
                return;
            }
            merge_global_lenient(self, global);
        }
        if let Some(test) = value.get("test").and_then(|v| v.as_table()) {
            merge_test_gates_lenient(self, test);
        }
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
            merge_test_gates_strict(self, test)?;
        }
        Ok(())
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
