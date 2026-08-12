mod unit_test_seconds;

pub use unit_test_seconds::{
    MatchedUnitTestSecondsRule, catch_all_limit, default_max_unit_test_seconds, exceeds_limit,
    format_nested_toml_table, limit_for_selector, matched_rule_for_selector, validate_rules,
};

use crate::config::{ConfigError, check_unknown_keys, get_usize};
use crate::defaults;
use std::fmt;
use std::path::Path;

/// Which coverage surface `kiss cov` compares to `test_coverage_threshold`.
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

const GATE_KEYS: &[&str] = &[
    "test_coverage_threshold",
    "test_coverage_scope",
    "max_unit_test_seconds",
    "min_similarity",
    "duplication_enabled",
    "orphan_module_enabled",
];

#[derive(Debug, Clone)]
pub struct GateConfig {
    pub test_coverage_threshold: usize,
    pub test_coverage_scope: TestCoverageScope,
    /// Ordered path-pattern → seconds limits; last entry must be `"*"`.
    pub max_unit_test_seconds: Vec<(String, f64)>,
    pub min_similarity: f64,
    pub duplication_enabled: bool,
    pub orphan_module_enabled: bool,
}

impl Default for GateConfig {
    fn default() -> Self {
        Self {
            test_coverage_threshold: defaults::gate::TEST_COVERAGE_THRESHOLD,
            test_coverage_scope: TestCoverageScope::Codebase,
            max_unit_test_seconds: default_max_unit_test_seconds(),
            min_similarity: defaults::duplication::MIN_SIMILARITY,
            duplication_enabled: true,
            orphan_module_enabled: true,
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

impl GateConfig {
    pub fn load() -> Self {
        let mut config = Self::default();
        if let Ok(c) = std::fs::read_to_string(".kissconfig") {
            config.merge_from_toml(&c);
        }
        config
    }

    pub fn load_from(path: &Path) -> Self {
        let mut config = Self::default();
        if let Ok(c) = std::fs::read_to_string(".kissconfig") {
            config.merge_from_toml(&c);
        }
        if let Ok(c) = std::fs::read_to_string(path) {
            config.merge_from_toml(&c);
        }
        config
    }

    /// Try to load gate config from a file, returning an error on failure.
    ///
    /// This is the Result-based API for library embedding. Unlike `load_from`,
    /// this function returns errors instead of printing to stderr.
    pub fn try_load_from(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|e| ConfigError::IoError {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        Self::try_load_from_content(&content)
    }

    /// Try to load gate config from TOML content, returning an error on failure.
    ///
    /// This is the Result-based API for library embedding. Unlike the internal merge,
    /// this function returns errors instead of printing to stderr.
    pub fn try_load_from_content(content: &str) -> Result<Self, ConfigError> {
        let mut config = Self::default();
        config.try_merge_from_toml(content)?;
        Ok(config)
    }

    fn merge_from_toml(&mut self, toml_str: &str) {
        let Ok(value) = toml_str.parse::<toml::Table>() else {
            return;
        };
        if let Some(gate) = value.get("gate").and_then(|v| v.as_table()) {
            if let Err(e) = check_unknown_keys(gate, GATE_KEYS, "gate") {
                eprintln!("Error: {e}");
                return;
            }
            if let Some(t) = get_usize(gate, "test_coverage_threshold") {
                if t > 100 {
                    eprintln!("Error: test_coverage_threshold must be 0-100, got {t}");
                    return;
                }
                self.test_coverage_threshold = t;
            }
            if let Err(msg) = merge_scope_lenient(gate, &mut self.test_coverage_scope) {
                eprintln!("Error: {msg}");
            }
            merge_max_unit_test_seconds_lenient(gate, &mut self.max_unit_test_seconds);
            if let Some(s) = get_f64(gate, "min_similarity") {
                if !(0.0..=1.0).contains(&s) {
                    eprintln!("Error: min_similarity must be 0.0-1.0, got {s}");
                    return;
                }
                self.min_similarity = s;
            }
            if let Some(v) = get_bool(gate, "duplication_enabled") {
                self.duplication_enabled = v;
            }
            if let Some(v) = get_bool(gate, "orphan_module_enabled") {
                self.orphan_module_enabled = v;
            }
        }
    }

    /// Result-based merge that returns errors instead of printing to stderr.
    fn try_merge_from_toml(&mut self, toml_str: &str) -> Result<(), ConfigError> {
        let value = toml_str
            .parse::<toml::Table>()
            .map_err(|e| ConfigError::ParseError {
                message: e.to_string(),
            })?;
        if let Some(gate) = value.get("gate").and_then(|v| v.as_table()) {
            check_unknown_keys(gate, GATE_KEYS, "gate")?;
            if let Some(t) = get_usize(gate, "test_coverage_threshold") {
                if t > 100 {
                    return Err(ConfigError::InvalidValue {
                        key: "test_coverage_threshold".into(),
                        message: format!("must be 0-100, got {t}"),
                    });
                }
                self.test_coverage_threshold = t;
            }
            if let Some(scope) = try_get_scope(gate)? {
                self.test_coverage_scope = scope;
            }
            if let Some(value) = gate.get("max_unit_test_seconds") {
                self.max_unit_test_seconds =
                    unit_test_seconds::parse_max_unit_test_seconds(value)?;
            }
            if let Some(s) = try_get_f64(gate, "min_similarity")? {
                if !(0.0..=1.0).contains(&s) {
                    return Err(ConfigError::InvalidValue {
                        key: "min_similarity".into(),
                        message: format!("must be 0.0-1.0, got {s}"),
                    });
                }
                self.min_similarity = s;
            }
            self.duplication_enabled =
                try_get_bool(gate, "duplication_enabled", self.duplication_enabled)?;
            self.orphan_module_enabled =
                try_get_bool(gate, "orphan_module_enabled", self.orphan_module_enabled)?;
        }
        Ok(())
    }
}

fn merge_max_unit_test_seconds_lenient(
    gate: &toml::Table,
    current: &mut Vec<(String, f64)>,
) {
    let Some(value) = gate.get("max_unit_test_seconds") else {
        return;
    };
    match unit_test_seconds::parse_max_unit_test_seconds(value) {
        Ok(rules) => *current = rules,
        Err(err) => eprintln!("Error: {err}"),
    }
}

fn merge_scope_lenient(
    gate: &toml::Table,
    current: &mut TestCoverageScope,
) -> Result<(), String> {
    let Some(value) = gate.get("test_coverage_scope") else {
        return Ok(());
    };
    let Some(raw) = value.as_str() else {
        return Err(format!(
            "test_coverage_scope must be \"by_file\" or \"codebase\", got {}",
            value.type_str()
        ));
    };
    match TestCoverageScope::parse(raw) {
        Ok(scope) => {
            *current = scope;
            Ok(())
        }
        Err(message) => Err(format!("test_coverage_scope {message}")),
    }
}

fn try_get_scope(gate: &toml::Table) -> Result<Option<TestCoverageScope>, ConfigError> {
    let Some(value) = gate.get("test_coverage_scope") else {
        return Ok(None);
    };
    let Some(raw) = value.as_str() else {
        return Err(ConfigError::InvalidValue {
            key: "test_coverage_scope".into(),
            message: format!(
                "must be \"by_file\" or \"codebase\", got {}",
                value.type_str()
            ),
        });
    };
    TestCoverageScope::parse(raw)
        .map(Some)
        .map_err(|message| ConfigError::InvalidValue {
            key: "test_coverage_scope".into(),
            message,
        })
}

#[allow(clippy::cast_precision_loss)]
const fn int_to_f64(i: i64) -> f64 {
    i as f64
}

fn try_get_f64(table: &toml::Table, key: &str) -> Result<Option<f64>, ConfigError> {
    let Some(value) = table.get(key) else {
        return Ok(None);
    };
    value
        .as_float()
        .or_else(|| value.as_integer().map(int_to_f64))
        .map(Some)
        .ok_or_else(|| ConfigError::InvalidValue {
            key: key.into(),
            message: format!("expected float, got {}", value.type_str()),
        })
}

fn get_bool(table: &toml::Table, key: &str) -> Option<bool> {
    if let Some(v) = table.get(key) {
        if let Some(b) = v.as_bool() {
            return Some(b);
        }
        eprintln!("Warning: Config key '{key}' expected bool");
    }
    None
}

fn try_get_bool(table: &toml::Table, key: &str, default: bool) -> Result<bool, ConfigError> {
    let Some(value) = table.get(key) else {
        return Ok(default);
    };
    value.as_bool().ok_or_else(|| ConfigError::InvalidValue {
        key: key.into(),
        message: "expected bool".into(),
    })
}

fn get_f64(table: &toml::Table, key: &str) -> Option<f64> {
    let value = table.get(key)?;
    value
        .as_float()
        .or_else(|| value.as_integer().map(int_to_f64))
        .or_else(|| {
            eprintln!(
                "Warning: Config key '{key}' expected float, got {}",
                value.type_str()
            );
            None
        })
}

#[cfg(test)]
#[path = "gate_config_test.rs"]
mod tests;
