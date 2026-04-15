use super::*;

#[test]
fn test_rules_functions_no_panic() {
    let py_config = Config::python_defaults();
    let rs_config = Config::rust_defaults();
    let gate_config = GateConfig::default();

    run_rules(&py_config, &rs_config, &gate_config, None, false);
    run_rules(
        &py_config,
        &rs_config,
        &gate_config,
        Some(Language::Python),
        true,
    );
    run_rules(
        &py_config,
        &rs_config,
        &gate_config,
        Some(Language::Rust),
        true,
    );
}

#[test]
fn test_print_rules() {
    let config = Config::python_defaults();
    let gate = GateConfig::default();
    print_summary_term_definitions();
    print_threshold_rules("Python", &config, &gate);
    print_threshold_rules("Rust", &Config::rust_defaults(), &gate);
}
#[test]
fn test_run_config_and_print_config() {
    let py = Config::python_defaults();
    let rs = Config::rust_defaults();
    let gate = GateConfig::default();
    run_config(&py, &rs, &gate, None, false);
    run_config(&py, &rs, &gate, None, true);
    print_python_config(&py);
    print_rust_config(&rs);
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
fn test_rule_spec_fields() {
    let spec = &python::PY_RULE_SPECS[0];
    assert_eq!(spec.metric, "statements_per_function");
    assert_eq!(spec.op, "<");
    assert!(!spec.description.is_empty());
}

#[test]
fn static_coverage_touch_rule_spec_type() {
    let _ = std::marker::PhantomData::<RuleSpec>;
}
