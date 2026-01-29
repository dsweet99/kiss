use crate::config::{check_unknown_keys, get_usize};
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
        if let Ok(c) = std::fs::read_to_string(path) {
            config.merge_from_toml(&c);
        }
        config
    }

    fn merge_from_toml(&mut self, toml_str: &str) {
        let Ok(value) = toml_str.parse::<toml::Table>() else { return };
        if let Some(gate) = value.get("gate").and_then(|v| v.as_table()) {
            check_unknown_keys(gate, &["test_coverage_threshold", "min_similarity", "duplication_enabled"], "gate");
            if let Some(t) = get_usize(gate, "test_coverage_threshold") {
                if t > 100 {
                    eprintln!("Error: test_coverage_threshold must be 0-100, got {t}");
                    std::process::exit(1);
                }
                self.test_coverage_threshold = t;
            }
            if let Some(s) = get_f64(gate, "min_similarity") {
                if !(0.0..=1.0).contains(&s) {
                    eprintln!("Error: min_similarity must be 0.0-1.0, got {s}");
                    std::process::exit(1);
                }
                self.min_similarity = s;
            }
            if let Some(v) = gate.get("duplication_enabled").and_then(toml::Value::as_bool) {
                self.duplication_enabled = v;
            } else if gate.contains_key("duplication_enabled") {
                eprintln!("Warning: Config key 'duplication_enabled' expected bool");
            }
        }
    }
}

fn get_f64(table: &toml::Table, key: &str) -> Option<f64> {
    let value = table.get(key)?;
    value.as_float().or_else(|| {
        eprintln!("Warning: Config key '{key}' expected float, got {}", value.type_str());
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

    #[test]
    fn test_get_f64() {
        let mut table = toml::Table::new();
        table.insert("valid".into(), toml::Value::Float(0.5));
        assert_eq!(get_f64(&table, "valid"), Some(0.5));
        assert_eq!(get_f64(&table, "missing"), None);
    }
}

