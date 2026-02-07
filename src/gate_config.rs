use crate::config::{check_unknown_keys, get_usize, ConfigError};
use crate::defaults;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct GateConfig {
    pub test_coverage_threshold: usize,
    pub min_similarity: f64,
    pub duplication_enabled: bool,
}

impl Default for GateConfig {
    fn default() -> Self {
        Self {
            test_coverage_threshold: defaults::gate::TEST_COVERAGE_THRESHOLD,
            min_similarity: defaults::duplication::MIN_SIMILARITY,
            duplication_enabled: true,
        }
    }
}

impl GateConfig {
    pub fn load() -> Self {
        let mut config = Self::default();
        if let Some(home) = std::env::var_os("HOME")
            && let Ok(c) = std::fs::read_to_string(Path::new(&home).join(".kissconfig"))
        {
            config.merge_from_toml(&c);
        }
        if let Ok(c) = std::fs::read_to_string(".kissconfig") {
            config.merge_from_toml(&c);
        }
        config
    }

    pub fn load_from(path: &Path) -> Self {
        let mut config = Self::default();
        // Chain from ~/.kissconfig first, matching Config::load_from behavior
        if let Some(home) = std::env::var_os("HOME")
            && let Ok(c) = std::fs::read_to_string(Path::new(&home).join(".kissconfig"))
        {
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
            if let Err(e) = check_unknown_keys(
                gate,
                &[
                    "test_coverage_threshold",
                    "min_similarity",
                    "duplication_enabled",
                ],
                "gate",
            ) {
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
            if let Some(s) = get_f64(gate, "min_similarity") {
                if !(0.0..=1.0).contains(&s) {
                    eprintln!("Error: min_similarity must be 0.0-1.0, got {s}");
                    return;
                }
                self.min_similarity = s;
            }
            if let Some(v) = gate
                .get("duplication_enabled")
                .and_then(toml::Value::as_bool)
            {
                self.duplication_enabled = v;
            } else if gate.contains_key("duplication_enabled") {
                eprintln!("Warning: Config key 'duplication_enabled' expected bool");
            }
        }
    }

    /// Result-based merge that returns errors instead of printing to stderr.
    fn try_merge_from_toml(&mut self, toml_str: &str) -> Result<(), ConfigError> {
        let value = toml_str.parse::<toml::Table>().map_err(|e| ConfigError::ParseError {
            message: e.to_string(),
        })?;
        if let Some(gate) = value.get("gate").and_then(|v| v.as_table()) {
            check_unknown_keys(
                gate,
                &["test_coverage_threshold", "min_similarity", "duplication_enabled"],
                "gate",
            )?;
            if let Some(t) = get_usize(gate, "test_coverage_threshold") {
                if t > 100 {
                    return Err(ConfigError::InvalidValue {
                        key: "test_coverage_threshold".into(),
                        message: format!("must be 0-100, got {t}"),
                    });
                }
                self.test_coverage_threshold = t;
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
            if let Some(v) = gate.get("duplication_enabled").and_then(toml::Value::as_bool) {
                self.duplication_enabled = v;
            } else if gate.contains_key("duplication_enabled") {
                return Err(ConfigError::InvalidValue {
                    key: "duplication_enabled".into(),
                    message: "expected bool".into(),
                });
            }
        }
        Ok(())
    }
}

fn try_get_f64(table: &toml::Table, key: &str) -> Result<Option<f64>, ConfigError> {
    let Some(value) = table.get(key) else {
        return Ok(None);
    };
    value
        .as_float()
        .or_else(|| value.as_integer().map(|i| i as f64))
        .map(Some)
        .ok_or_else(|| ConfigError::InvalidValue {
            key: key.into(),
            message: format!("expected float, got {}", value.type_str()),
        })
}

fn get_f64(table: &toml::Table, key: &str) -> Option<f64> {
    let value = table.get(key)?;
    value
        .as_float()
        .or_else(|| value.as_integer().map(|i| i as f64))
        .or_else(|| {
            eprintln!(
                "Warning: Config key '{key}' expected float, got {}",
                value.type_str()
            );
            None
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gate_config_merge_from_toml() {
        let mut gate = GateConfig::default();
        gate.merge_from_toml("[gate]\ntest_coverage_threshold = 50\nmin_similarity = 0.8\nduplication_enabled = false");
        assert_eq!(gate.test_coverage_threshold, 50);
        assert!((gate.min_similarity - 0.8).abs() < 0.01);
        assert!(!gate.duplication_enabled);
    }

    #[test]
    fn test_get_usize() {
        let mut table = toml::Table::new();
        table.insert("valid".into(), toml::Value::Integer(42));
        assert_eq!(get_usize(&table, "valid"), Some(42));
        assert_eq!(get_usize(&table, "missing"), None);
        table.insert("negative".into(), toml::Value::Integer(-1));
        assert_eq!(get_usize(&table, "negative"), None);
    }

    // === Bug-hunting tests ===

    #[test]
    fn test_min_similarity_integer_accepted() {
        // TOML treats `min_similarity = 1` as an integer, not float.
        // The config should accept integer values and coerce to float.
        let mut gate = GateConfig::default();
        gate.merge_from_toml("[gate]\nmin_similarity = 1");
        assert!(
            (gate.min_similarity - 1.0).abs() < f64::EPSILON,
            "min_similarity = 1 (integer) should be treated as 1.0 (got {})",
            gate.min_similarity
        );
    }

    #[test]
    fn test_get_f64() {
        let mut table = toml::Table::new();
        table.insert("valid".into(), toml::Value::Float(0.5));
        assert_eq!(get_f64(&table, "valid"), Some(0.5));
        assert_eq!(get_f64(&table, "missing"), None);
    }
}
