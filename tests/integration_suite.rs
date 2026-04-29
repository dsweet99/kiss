//! Single integration-test crate: links once against `kiss` instead of one binary per `tests/*.rs` file.
#[path = "common/mod.rs"]
mod common;
#[path = "support/mod.rs"]
mod support;

#[path = "cases/c2_break_orphans.rs"]
mod break_c2_orphans;
#[path = "cases/c2_break_test_coverage.rs"]
mod break_c2_test_coverage;
#[path = "cases/cache_integration.rs"]
mod cache_integration;
#[path = "cases/cli_integration.rs"]
mod cli_integration;
#[path = "cases/cli_integration_2.rs"]
mod cli_integration_2;
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
#[path = "cases/kpop_definitions_2.rs"]
mod kpop_definitions_2;
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
#[path = "cases/regression_stats_summary_headers_and_coverage.rs"]
mod regression_stats_summary_headers_and_coverage;
#[path = "cases/review_findings.rs"]
mod review_findings;
#[path = "cases/review_findings_cache.rs"]
mod review_findings_cache;
#[path = "cases/review_findings_python.rs"]
mod review_findings_python;
#[path = "cases/review_findings_python_2.rs"]
mod review_findings_python_2;
#[path = "cases/review_findings_python_3.rs"]
mod review_findings_python_3;
#[path = "cases/review_findings_rust.rs"]
mod review_findings_rust;
#[path = "cases/review_findings_rust_2.rs"]
mod review_findings_rust_2;
#[path = "cases/review_findings_rust_3.rs"]
mod review_findings_rust_3;
#[path = "cases/rules_config_integration.rs"]
mod rules_config_integration;
#[path = "cases/rust_counts_violations.rs"]
mod rust_counts_violations;
#[path = "cases/stress_break_kiss.rs"]
mod stress_break_kiss;
#[path = "cases/stress_break_kiss_2.rs"]
mod stress_break_kiss_2;
#[path = "cases/symbol_mv_corpus.rs"]
mod symbol_mv_corpus;
#[path = "cases/symbol_mv_internal_coverage.rs"]
mod symbol_mv_internal_coverage;
#[path = "cases/symbol_mv_matrix.rs"]
mod symbol_mv_matrix;
#[path = "cases/symbol_mv_metamorphic.rs"]
mod symbol_mv_metamorphic;
#[path = "cases/symbol_mv_regressions.rs"]
mod symbol_mv_regressions;
#[path = "cases/symbol_mv_regressions_2.rs"]
mod symbol_mv_regressions_2;
#[path = "cases/symbol_mv_regressions_3.rs"]
mod symbol_mv_regressions_3;
#[path = "cases/symbol_mv_regressions_4.rs"]
mod symbol_mv_regressions_4;
#[path = "cases/symbol_mv_regressions_5.rs"]
mod symbol_mv_regressions_5;
#[path = "cases/symbol_mv_regressions_6.rs"]
mod symbol_mv_regressions_6;
#[path = "cases/symbol_mv_regressions_7.rs"]
mod symbol_mv_regressions_7;
#[path = "cases/symbol_mv_regressions_8.rs"]
mod symbol_mv_regressions_8;
#[path = "cases/symbol_mv_regressions_9.rs"]
mod symbol_mv_regressions_9;
#[path = "cases/symbol_mv_regressions_10.rs"]
mod symbol_mv_regressions_10;
#[path = "cases/symbol_mv_regressions_11.rs"]
mod symbol_mv_regressions_11;
#[path = "cases/symbol_mv_regressions_12.rs"]
mod symbol_mv_regressions_12;
#[path = "cases/symbol_mv_regressions_13.rs"]
mod symbol_mv_regressions_13;
#[path = "cases/symbol_mv_regressions_14.rs"]
mod symbol_mv_regressions_14;
#[path = "cases/sync_stats_check.rs"]
mod sync_stats_check;
