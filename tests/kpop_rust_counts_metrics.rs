use kiss::config::Config;
use kiss::rust_counts::analyze_rust_file;
use kiss::rust_parsing::parse_rust_file;
use std::path::Path;

#[test]
fn bug_rust_methods_per_class_violation_metric_id_mismatch() {
    // RULE: [Rust] [methods_per_class] is the maximum number of methods in an impl block.
    //
    // Hypothesis: the violation metric id does not match the rule name.
    // Prediction: when methods exceed the threshold, the violation metric should be "methods_per_class",
    // but the current implementation emits "methods_per_type".
    let parsed = parse_rust_file(Path::new("tests/fake_rust/too_many_methods.rs")).expect("parse");
    let cfg = Config {
        methods_per_class: 1,
        ..Config::rust_defaults()
    };
    let viols = analyze_rust_file(&parsed, &cfg);
    let method_viols: Vec<_> = viols
        .iter()
        .filter(|v| v.value >= 2)
        .filter(|v| v.metric.contains("methods"))
        .collect();
    assert!(
        method_viols.iter().any(|v| v.metric == "methods_per_class"),
        "expected methods_per_class metric id, got: {method_viols:#?}"
    );
}

