use super::*;
use crate::config::{ConfigError, get_usize};

#[test]
fn test_gate_config_merge_from_toml() {
    let mut gate = GateConfig::default();
    gate.merge_from_toml(
        "[gate]\ntest_coverage_threshold = 50\nmin_similarity = 0.8\nduplication_enabled = false",
    );
    assert_eq!(gate.test_coverage_threshold, 50);
    assert!((gate.min_similarity - 0.8).abs() < 0.01);
    assert!(!gate.duplication_enabled);
}

#[test]
fn default_gate_config_scope_is_codebase() {
    assert_eq!(
        GateConfig::default().test_coverage_scope,
        TestCoverageScope::Codebase
    );
}

#[test]
fn parse_test_coverage_scope_values() {
    let by_file =
        GateConfig::try_load_from_content("[gate]\ntest_coverage_scope = \"by_file\"").unwrap();
    assert_eq!(by_file.test_coverage_scope, TestCoverageScope::ByFile);
    let codebase =
        GateConfig::try_load_from_content("[gate]\ntest_coverage_scope = \"codebase\"").unwrap();
    assert_eq!(codebase.test_coverage_scope, TestCoverageScope::Codebase);
}

#[test]
fn missing_test_coverage_scope_keeps_codebase() {
    let gate = GateConfig::try_load_from_content("[gate]\ntest_coverage_threshold = 80\n").unwrap();
    assert_eq!(gate.test_coverage_scope, TestCoverageScope::Codebase);
}

#[test]
fn try_load_rejects_unknown_test_coverage_scope() {
    let err =
        GateConfig::try_load_from_content("[gate]\ntest_coverage_scope = \"mean\"").unwrap_err();
    assert!(matches!(
        err,
        ConfigError::InvalidValue { ref key, .. } if key == "test_coverage_scope"
    ));
    let wrong_type =
        GateConfig::try_load_from_content("[gate]\ntest_coverage_scope = 1").unwrap_err();
    assert!(matches!(
        wrong_type,
        ConfigError::InvalidValue { ref key, .. } if key == "test_coverage_scope"
    ));
}

#[test]
fn merge_from_toml_keeps_prior_scope_on_invalid() {
    let mut gate = GateConfig {
        test_coverage_scope: TestCoverageScope::Codebase,
        ..Default::default()
    };
    gate.merge_from_toml("[gate]\ntest_coverage_scope = \"mean\"");
    assert_eq!(gate.test_coverage_scope, TestCoverageScope::Codebase);
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
fn try_load_from_content_rejects_non_numeric_min_similarity() {
    let err = GateConfig::try_load_from_content("[gate]\nmin_similarity = \"bad\"").unwrap_err();
    assert!(matches!(err, ConfigError::InvalidValue { .. }));
}

#[test]
fn try_load_from_content_rejects_non_bool_gate_flag() {
    let err = GateConfig::try_load_from_content("[gate]\nduplication_enabled = 1").unwrap_err();
    assert!(matches!(err, ConfigError::InvalidValue { .. }));
}

#[test]
fn try_load_from_content_accepts_integer_min_similarity() {
    let gate = GateConfig::try_load_from_content("[gate]\nmin_similarity = 1").unwrap();
    assert!((gate.min_similarity - 1.0).abs() < f64::EPSILON);
}

#[test]
fn merge_from_toml_ignores_non_bool_gate_flag() {
    let mut gate = GateConfig::default();
    gate.merge_from_toml("[gate]\nduplication_enabled = \"yes\"");
    assert!(gate.duplication_enabled);
}

#[test]
fn try_get_f64_parses_float_and_integer() {
    let mut table = toml::Table::new();
    table.insert("f".into(), toml::Value::Float(0.5));
    assert_eq!(try_get_f64(&table, "f").unwrap(), Some(0.5));
    table.insert("i".into(), toml::Value::Integer(2));
    assert_eq!(try_get_f64(&table, "i").unwrap(), Some(2.0));
    assert_eq!(try_get_f64(&table, "missing").unwrap(), None);
    table.insert("bad".into(), toml::Value::String("nope".into()));
    assert!(try_get_f64(&table, "bad").is_err());
}

#[test]
fn get_bool_reads_bool_and_ignores_invalid() {
    let mut table = toml::Table::new();
    table.insert("flag".into(), toml::Value::Boolean(true));
    assert_eq!(get_bool(&table, "flag"), Some(true));
    table.insert("bad".into(), toml::Value::String("nope".into()));
    assert_eq!(get_bool(&table, "bad"), None);
    assert_eq!(get_bool(&table, "missing"), None);
}

#[test]
fn try_get_bool_uses_default_and_rejects_invalid() {
    let table = toml::Table::new();
    assert!(try_get_bool(&table, "flag", true).unwrap());
    let mut table = table;
    table.insert("flag".into(), toml::Value::Boolean(false));
    assert!(!try_get_bool(&table, "flag", true).unwrap());
    table.insert("bad".into(), toml::Value::Integer(1));
    assert!(try_get_bool(&table, "bad", false).is_err());
}

#[test]
fn test_get_f64() {
    let mut table = toml::Table::new();
    table.insert("valid".into(), toml::Value::Float(0.5));
    assert_eq!(get_f64(&table, "valid"), Some(0.5));
    assert_eq!(get_f64(&table, "missing"), None);
}

#[test]
fn witness_try_merge_from_toml_and_int_to_f64() {
    let mut gate = GateConfig::default();
    gate.try_merge_from_toml("[gate]\ntest_coverage_threshold = 75\n")
        .unwrap();
    assert_eq!(gate.test_coverage_threshold, 75);
    assert_eq!(int_to_f64(3), 3.0);
}

#[test]
fn try_load_from_content_accepts_all_gate_fields() {
    let gate = GateConfig::try_load_from_content(
        "\
[gate]
test_coverage_threshold = 91
test_coverage_scope = \"codebase\"
max_unit_test_seconds = 1.5
min_similarity = 0.75
duplication_enabled = false
orphan_module_enabled = false
",
    )
    .unwrap();

    assert_eq!(gate.test_coverage_threshold, 91);
    assert_eq!(gate.test_coverage_scope, TestCoverageScope::Codebase);
    assert!((gate.max_unit_test_seconds - 1.5).abs() < f64::EPSILON);
    assert!((gate.min_similarity - 0.75).abs() < f64::EPSILON);
    assert!(!gate.duplication_enabled);
    assert!(!gate.orphan_module_enabled);
}

#[test]
fn default_max_unit_test_seconds_is_two() {
    assert!((GateConfig::default().max_unit_test_seconds - 2.0).abs() < f64::EPSILON);
}

#[test]
fn try_load_rejects_negative_and_nonfinite_max_unit_test_seconds() {
    for raw in ["-1", "nan", "inf", "-inf"] {
        let err = GateConfig::try_load_from_content(&format!(
            "[gate]\nmax_unit_test_seconds = {raw}"
        ))
        .unwrap_err();
        assert!(
            matches!(
                err,
                ConfigError::InvalidValue { ref key, .. } if key == "max_unit_test_seconds"
            ),
            "raw={raw} err={err:?}"
        );
    }
}

#[test]
fn merge_keeps_prior_max_unit_test_seconds_on_invalid() {
    let mut gate = GateConfig {
        max_unit_test_seconds: 1.25,
        ..GateConfig::default()
    };
    gate.merge_from_toml("[gate]\nmax_unit_test_seconds = -3");
    assert!((gate.max_unit_test_seconds - 1.25).abs() < f64::EPSILON);
}

#[test]
fn max_unit_test_seconds_zero_disables() {
    let gate =
        GateConfig::try_load_from_content("[gate]\nmax_unit_test_seconds = 0").unwrap();
    assert_eq!(gate.max_unit_test_seconds, 0.0);
}

#[test]
fn try_load_from_reads_file_and_reports_missing_file() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), "[gate]\ntest_coverage_threshold = 44\n").unwrap();
    let gate = GateConfig::try_load_from(tmp.path()).unwrap();
    assert_eq!(gate.test_coverage_threshold, 44);

    let missing = tempfile::NamedTempFile::new().unwrap();
    let missing_path = missing.path().to_path_buf();
    drop(missing);
    let err = GateConfig::try_load_from(&missing_path).unwrap_err();
    assert!(matches!(err, ConfigError::IoError { .. }));
}

#[test]
fn try_load_from_content_rejects_out_of_range_gate_values() {
    let coverage =
        GateConfig::try_load_from_content("[gate]\ntest_coverage_threshold = 101").unwrap_err();
    assert!(matches!(
        coverage,
        ConfigError::InvalidValue { ref key, .. } if key == "test_coverage_threshold"
    ));

    let similarity = GateConfig::try_load_from_content("[gate]\nmin_similarity = 1.5").unwrap_err();
    assert!(matches!(
        similarity,
        ConfigError::InvalidValue { ref key, .. } if key == "min_similarity"
    ));
}

#[test]
fn merge_from_toml_ignores_out_of_range_and_unknown_gate_values() {
    let mut gate = GateConfig::default();
    gate.merge_from_toml("[gate]\ntest_coverage_threshold = 101");
    assert_eq!(
        gate.test_coverage_threshold,
        defaults::gate::TEST_COVERAGE_THRESHOLD
    );

    gate.merge_from_toml("[gate]\nmin_similarity = 1.5");
    assert_eq!(gate.min_similarity, defaults::duplication::MIN_SIMILARITY);

    gate.merge_from_toml("[gate]\nunknown = 1\ntest_coverage_threshold = 1");
    assert_eq!(
        gate.test_coverage_threshold,
        defaults::gate::TEST_COVERAGE_THRESHOLD
    );
}
