use crate::config::{
    ConfigError, apply_lenient_string_list, check_unknown_keys, parse_string_list_key,
};
use crate::gate_config::{GateConfig, parse_max_unit_test_seconds};
use crate::test_section_config::TestSectionConfig;

const TEST_SECTION_KEYS: &[&str] = &[
    "main_branch",
    "num_jobs",
    "num_jobs_pytest",
    "num_jobs_nextest",
    "num_jobs_llvm_cov",
    "watch_settle_seconds",
    "test_coverage_threshold",
    "test_coverage_scope",
    "pytest_plugins",
    "ignore",
    "orphan_detection",
    "max_unit_test_seconds",
    "max_num_tests",
    "cache",
];

pub(crate) fn merge_test_table_lenient(
    table: &toml::Table,
    gate: Option<&mut GateConfig>,
    runtime: Option<&mut TestSectionConfig>,
    repo_root: Option<&std::path::Path>,
) {
    if let Err(e) = check_unknown_keys(table, TEST_SECTION_KEYS, "test") {
        eprintln!("Error: {e}");
        return;
    }
    if let Some(gate) = gate {
        merge_test_gates_lenient(gate, table);
    }
    if let Some(runtime) = runtime {
        apply_lenient_runtime(runtime, table, repo_root);
    }
}

pub(crate) fn merge_test_table_strict(
    table: &toml::Table,
    gate: Option<&mut GateConfig>,
    runtime: Option<&mut TestSectionConfig>,
    repo_root: Option<&std::path::Path>,
) -> Result<(), ConfigError> {
    check_unknown_keys(table, TEST_SECTION_KEYS, "test")?;
    if let Some(gate) = gate {
        merge_test_gates_strict(gate, table)?;
    }
    if let Some(runtime) = runtime {
        apply_strict_runtime(runtime, table, repo_root)?;
    }
    Ok(())
}

fn merge_test_gates_lenient(config: &mut GateConfig, test: &toml::Table) {
    merge_orphan_detection_lenient(config, test);
    if let Some(value) = test.get("max_unit_test_seconds") {
        match parse_max_unit_test_seconds(value) {
            Ok(rules) => config.max_unit_test_seconds = rules,
            Err(err) => eprintln!("Error: {err}"),
        }
    }
    match try_get_max_num_tests(test) {
        Ok(Some(n)) => config.max_num_tests = n,
        Ok(None) => {}
        Err(err) => eprintln!("Error: {err}"),
    }
}

fn merge_test_gates_strict(config: &mut GateConfig, test: &toml::Table) -> Result<(), ConfigError> {
    if let Some(enabled) = try_get_orphan_detection(test)? {
        config.orphan_detection = enabled;
    }
    if let Some(value) = test.get("max_unit_test_seconds") {
        config.max_unit_test_seconds = parse_max_unit_test_seconds(value)?;
    }
    if let Some(n) = try_get_max_num_tests(test)? {
        config.max_num_tests = n;
    }
    Ok(())
}

fn merge_orphan_detection_lenient(config: &mut GateConfig, test: &toml::Table) {
    if let Some(value) = test.get("orphan_detection") {
        if let Some(b) = value.as_bool() {
            config.orphan_detection = b;
        } else {
            eprintln!("Warning: Config key 'orphan_detection' expected bool");
        }
    }
}

fn try_get_orphan_detection(table: &toml::Table) -> Result<Option<bool>, ConfigError> {
    let Some(value) = table.get("orphan_detection") else {
        return Ok(None);
    };
    value
        .as_bool()
        .map(Some)
        .ok_or_else(|| ConfigError::InvalidValue {
            key: "orphan_detection".into(),
            message: "expected bool".into(),
        })
}

fn try_get_max_num_tests(table: &toml::Table) -> Result<Option<usize>, ConfigError> {
    let Some(value) = table.get("max_num_tests") else {
        return Ok(None);
    };
    let Some(n) = value.as_integer() else {
        return Err(ConfigError::InvalidValue {
            key: "max_num_tests".into(),
            message: format!("expected nonnegative integer, got {}", value.type_str()),
        });
    };
    usize::try_from(n)
        .map(Some)
        .map_err(|_| ConfigError::InvalidValue {
            key: "max_num_tests".into(),
            message: format!("expected nonnegative integer, got {n}"),
        })
}

fn apply_strict_runtime(
    config: &mut TestSectionConfig,
    table: &toml::Table,
    repo_root: Option<&std::path::Path>,
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
    if let Some(v) = table.get("num_jobs_pytest") {
        let val = parse_positive_usize(v, "num_jobs_pytest")?;
        config.num_jobs_pytest = val;
        config.num_jobs_pytest_explicit = Some(val);
    }
    if let Some(key) = nextest_jobs_key(table) {
        let val = parse_positive_usize(&table[key], key)?;
        config.num_jobs_nextest = val;
        config.num_jobs_nextest_explicit = Some(val);
    }
    if let Some(v) = table.get("pytest_plugins") {
        config.pytest_plugins = parse_string_list_key(v, "pytest_plugins", "plugin names")?;
    }
    if let Some(v) = table.get("ignore") {
        config.ignore = parse_string_list_key(v, "ignore", "ignore patterns")?;
    }
    if let Some(v) = table.get("cache").and_then(|v| v.as_table()) {
        config.cache_policy = crate::test_cache_policy::TestCachePolicy::parse_table(v, repo_root)?;
    }
    Ok(())
}

fn nextest_jobs_key(table: &toml::Table) -> Option<&'static str> {
    ["num_jobs_nextest", "num_jobs_llvm_cov"]
        .into_iter()
        .find(|key| table.contains_key(*key))
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

fn apply_lenient_positive_usize(table: &toml::Table, key: &str, dest: &mut usize) {
    if let Some(v) = table.get(key) {
        match parse_positive_usize(v, key) {
            Ok(n) => *dest = n,
            Err(_) => eprintln!("Warning: Config key '{key}' expected a positive integer"),
        }
    }
}

fn apply_lenient_runtime(
    config: &mut TestSectionConfig,
    table: &toml::Table,
    repo_root: Option<&std::path::Path>,
) {
    if let Some(v) = table.get("main_branch") {
        if let Some(s) = v.as_str() {
            config.main_branch = Some(s.to_string());
        } else {
            eprintln!("Warning: Config key 'main_branch' expected string");
        }
    }
    apply_lenient_positive_usize(table, "num_jobs", &mut config.num_jobs);
    apply_lenient_positive_usize(table, "num_jobs_pytest", &mut config.num_jobs_pytest);
    if let Some(key) = nextest_jobs_key(table) {
        apply_lenient_positive_usize(table, key, &mut config.num_jobs_nextest);
    }
    if table.contains_key("num_jobs_pytest") {
        config.num_jobs_pytest_explicit = Some(config.num_jobs_pytest);
    }
    if nextest_jobs_key(table).is_some() {
        config.num_jobs_nextest_explicit = Some(config.num_jobs_nextest);
    }
    apply_lenient_string_list(table, "pytest_plugins", "plugin names", |v| {
        config.pytest_plugins = v;
    });
    apply_lenient_string_list(table, "ignore", "ignore patterns", |v| {
        config.ignore = v;
    });
    apply_lenient_cache(config, table, repo_root);
}

fn apply_lenient_cache(
    config: &mut TestSectionConfig,
    table: &toml::Table,
    repo_root: Option<&std::path::Path>,
) {
    if let Some(v) = table.get("cache").and_then(|v| v.as_table()) {
        match crate::test_cache_policy::TestCachePolicy::parse_table(v, repo_root) {
            Ok(policy) => config.cache_policy = policy,
            Err(err) => eprintln!("Error: {err}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TEST_SECTION_KEYS;

    #[test]
    fn test_section_keys_cover_gate_and_runtime() {
        assert!(TEST_SECTION_KEYS.contains(&"test_coverage_threshold"));
        assert!(TEST_SECTION_KEYS.contains(&"orphan_detection"));
        assert!(TEST_SECTION_KEYS.contains(&"num_jobs"));
        assert!(TEST_SECTION_KEYS.contains(&"num_jobs_pytest"));
        assert!(TEST_SECTION_KEYS.contains(&"num_jobs_nextest"));
        assert!(TEST_SECTION_KEYS.contains(&"num_jobs_llvm_cov"));
        assert!(TEST_SECTION_KEYS.contains(&"max_unit_test_seconds"));
        assert!(TEST_SECTION_KEYS.contains(&"cache"));
    }
}
