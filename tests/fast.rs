#[path = "common/mod.rs"]
mod common;

#[cfg(target_os = "linux")]
#[used]
#[allow(non_upper_case_globals)]
#[unsafe(link_section = ".init_array")]
static prefer_tmpfs_tmpdir_init: extern "C" fn() = {
    extern "C" fn init() {
        common::prefer_tmpfs_tmpdir();
    }
    init
};

#[path = "fast/c2_break_orphans.rs"]
mod c2_break_orphans;
#[path = "fast/clamp_graph_keys.rs"]
mod clamp_graph_keys;
#[path = "fast/config_tests.rs"]
mod config_tests;
#[path = "fast/fix_h1_error_nodes.rs"]
mod fix_h1_error_nodes;
#[path = "fast/fix_h5_phantom_orphans.rs"]
mod fix_h5_phantom_orphans;
#[path = "fast/journal_hypotheses.rs"]
mod journal_hypotheses;
#[path = "fast/kpop_definitions.rs"]
mod kpop_definitions;
#[path = "fast/kpop_definitions_2.rs"]
mod kpop_definitions_2;
#[path = "fast/kpop_include_rollup_regressions.rs"]
mod kpop_include_rollup_regressions;
#[path = "fast/kpop_python_function_metrics.rs"]
mod kpop_python_function_metrics;
#[path = "fast/kpop_python_graph_metrics.rs"]
mod kpop_python_graph_metrics;
#[path = "fast/kpop_python_none.rs"]
mod kpop_python_none;
#[path = "fast/kpop_python_none_graph_and_gates.rs"]
mod kpop_python_none_graph_and_gates;
#[path = "fast/kpop_rust_counts_metrics.rs"]
mod kpop_rust_counts_metrics;
#[path = "fast/kpop_rust_file_metrics_plan.rs"]
mod kpop_rust_file_metrics_plan;
#[path = "fast/kpop_rust_function_metrics.rs"]
mod kpop_rust_function_metrics;
#[path = "fast/kpop_rust_graph_metrics.rs"]
mod kpop_rust_graph_metrics;
#[path = "fast/kpop_rust_none.rs"]
mod kpop_rust_none;
#[path = "fast/kpop_rust_none_graph_and_gates.rs"]
mod kpop_rust_none_graph_and_gates;
#[path = "fast/lib_integration.rs"]
mod lib_integration;
#[path = "fast/main_integration.rs"]
mod main_integration;
#[path = "fast/py_metrics_tests.rs"]
mod py_metrics_tests;
#[path = "fast/python_counts_violations.rs"]
mod python_counts_violations;
#[path = "fast/regression_check_all_ignores_test_file_sentinel.rs"]
mod regression_check_all_ignores_test_file_sentinel;
#[path = "fast/regression_check_perf.rs"]
mod regression_check_perf;
#[path = "fast/rust_counts_violations.rs"]
mod rust_counts_violations;
#[path = "fast/stress_break_kiss.rs"]
mod stress_break_kiss;
#[path = "fast/stress_break_kiss_2.rs"]
mod stress_break_kiss_2;
