//! Single integration-test crate: links once against `kiss` instead of one binary per `tests/*.rs` file.
#[path = "common/mod.rs"]
mod common;

#[path = "cases/c2_break_orphans.rs"]
mod break_c2_orphans;
#[path = "cases/c2_break_test_coverage.rs"]
mod break_c2_test_coverage;
#[path = "cases/cache_integration.rs"]
mod cache_integration;
#[path = "cases/cli_integration.rs"]
mod cli_integration;
#[path = "cases/config_tests.rs"]
mod config_tests;
#[path = "cases/fix_h1_error_nodes.rs"]
mod fix_h1_error_nodes;
#[path = "cases/fix_h5_phantom_orphans.rs"]
mod fix_h5_phantom_orphans;
#[path = "cases/journal_hypotheses.rs"]
mod journal_hypotheses;
#[path = "cases/kpop_definitions.rs"]
mod kpop_definitions;
#[path = "cases/kpop_python_function_metrics.rs"]
mod kpop_python_function_metrics;
#[path = "cases/kpop_python_graph_metrics.rs"]
mod kpop_python_graph_metrics;
#[path = "cases/kpop_python_none.rs"]
mod kpop_python_none;
#[path = "cases/kpop_python_none_graph_and_gates.rs"]
mod kpop_python_none_graph_and_gates;
#[path = "cases/kpop_rust_counts_metrics.rs"]
mod kpop_rust_counts_metrics;
#[path = "cases/kpop_rust_file_metrics_plan.rs"]
mod kpop_rust_file_metrics_plan;
#[path = "cases/kpop_rust_function_metrics.rs"]
mod kpop_rust_function_metrics;
#[path = "cases/kpop_rust_graph_metrics.rs"]
mod kpop_rust_graph_metrics;
#[path = "cases/kpop_rust_none.rs"]
mod kpop_rust_none;
#[path = "cases/kpop_rust_none_graph_and_gates.rs"]
mod kpop_rust_none_graph_and_gates;
#[path = "cases/kpop_show_tests_bug.rs"]
mod kpop_show_tests_bug;
#[path = "cases/lib_integration.rs"]
mod lib_integration;
#[path = "cases/main_integration.rs"]
mod main_integration;
#[path = "cases/py_metrics_tests.rs"]
mod py_metrics_tests;
#[path = "cases/python_counts_violations.rs"]
mod python_counts_violations;
#[path = "cases/regression_check_perf.rs"]
mod regression_check_perf;
#[path = "cases/regression_stats_all_metric_registry.rs"]
mod regression_stats_all_metric_registry;
#[path = "cases/rules_config_integration.rs"]
mod rules_config_integration;
#[path = "cases/rust_counts_violations.rs"]
mod rust_counts_violations;
#[path = "cases/stress_break_kiss.rs"]
mod stress_break_kiss;
#[path = "cases/symbol_mv_regressions.rs"]
mod symbol_mv_regressions;
