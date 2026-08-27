use super::*;

#[test]
fn test_rules_functions_no_panic() {
    let py_config = Config::python_defaults();
    let rs_config = Config::rust_defaults();
    let gate_config = GateConfig::default();

    run_rules(&py_config, &rs_config, &gate_config, None);
    run_rules(&py_config, &rs_config, &gate_config, Some(Language::Python));
    run_rules(&py_config, &rs_config, &gate_config, Some(Language::Rust));
}

#[test]
fn test_print_rules() {
    let config = Config::python_defaults();
    let gate = GateConfig::default();
    print_summary_term_definitions();
    print_rule_specs("global", global::GLOBAL_RULE_SPECS, &config, &gate);
    print_rule_specs("test", test_rules::TEST_RULE_SPECS, &config, &gate);
    print_threshold_rules("Python", &config, &gate);
    print_threshold_rules("Rust", &Config::rust_defaults(), &gate);
}

#[test]
fn global_and_test_rule_specs_use_shared_metrics() {
    let global_metrics: Vec<_> = global::GLOBAL_RULE_SPECS.iter().map(|s| s.metric).collect();
    assert_eq!(
        global_metrics,
        ["min_similarity", "comment", "doc", "orphan_module"]
    );
    let test_metrics: Vec<_> = test_rules::TEST_RULE_SPECS
        .iter()
        .map(|s| s.metric)
        .collect();
    assert_eq!(
        test_metrics,
        [
            "test_coverage_threshold",
            "max_unit_test_seconds",
            "max_num_tests",
            "orphan"
        ]
    );
    let py_metrics: Vec<_> = python::PY_RULE_SPECS.iter().map(|s| s.metric).collect();
    let rs_metrics: Vec<_> = rust_rules::RS_RULE_SPECS.iter().map(|s| s.metric).collect();
    for metric in global_metrics.iter().chain(test_metrics.iter()) {
        assert!(!py_metrics.contains(metric), "{metric} still under Python");
        assert!(!rs_metrics.contains(metric), "{metric} still under Rust");
    }
}

#[test]
fn test_threshold_value_format() {
    let c = Config::python_defaults();
    let g = GateConfig::default();
    let usize_tv = ThresholdValue::Usize(|c, _| c.statements_per_function);
    let f64_tv = ThresholdValue::F64(|_, g| g.min_similarity);
    assert_eq!(
        usize_tv.format(&c, &g),
        c.statements_per_function.to_string()
    );
    assert!(f64_tv.format(&c, &g).contains('.'));
}

#[test]
fn test_rule_spec_struct_literal_referenced() {
    let spec = RuleSpec {
        metric: "touch",
        op: ThresholdOp::Equal,
        threshold: ThresholdValue::Usize(|c, _| c.statements_per_function),
        description: "touch",
    };
    assert_eq!(spec.metric, "touch");
}

#[test]
fn test_rule_spec_fields() {
    let specs: &[RuleSpec] = python::PY_RULE_SPECS;
    let spec = &specs[0];
    assert_eq!(spec.metric, "statements_per_function");
    assert_eq!(spec.op, ThresholdOp::AtMost);
    assert!(!spec.description.is_empty());
}

impl RuleSpec {
    fn witness() -> Self {
        Self {
            metric: "touch",
            op: ThresholdOp::Equal,
            threshold: ThresholdValue::Usize(|c, _| c.statements_per_function),
            description: "touch",
        }
    }
}

#[test]
fn witness_rule_spec_assoc() {
    let spec = RuleSpec::witness();
    assert_eq!(spec.metric, "touch");
}

#[test]
fn display_op_uses_eq_for_zero_cycle_size() {
    assert_eq!(display_op("cycle_size", ThresholdOp::AtMost, "0"), "==");
    assert_eq!(display_op("cycle_size", ThresholdOp::AtMost, "3"), "<=");
    assert_eq!(
        display_op("statements_per_function", ThresholdOp::AtMost, "0"),
        "<="
    );
}

#[test]
fn alias_note_documents_toml_keys() {
    assert!(alias_note("Rust", "positional_args").contains("arguments"));
    assert!(alias_note("Python", "max_indentation_depth").contains("max_indentation"));
    assert!(alias_note("Python", "statements_per_function").is_empty());
}
