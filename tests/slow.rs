#[path = "common/mod.rs"]
mod common;
#[path = "support/mod.rs"]
mod support;

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

#[path = "slow/bug_indirect_dependencies_check.rs"]
mod bug_indirect_dependencies_check;
#[path = "slow/bug_nested_non_member_coverage_universe.rs"]
mod bug_nested_non_member_coverage_universe;
#[path = "slow/bug_test_coverage_violation_message.rs"]
mod bug_test_coverage_violation_message;
#[path = "slow/cache_integration.rs"]
mod cache_integration;
#[path = "slow/clamp_then_check.rs"]
mod clamp_then_check;
#[path = "slow/cli_integration.rs"]
mod cli_integration;
#[path = "slow/cli_integration_2.rs"]
mod cli_integration_2;
#[path = "slow/cli_kiss_test_smoke.rs"]
mod cli_kiss_test_smoke;
#[path = "slow/cli_kiss_test_wait.rs"]
mod cli_kiss_test_wait;
#[path = "slow/comment_removal_check.rs"]
mod comment_removal_check;
#[path = "slow/coverage_corpus.rs"]
mod coverage_corpus;
#[path = "slow/docs_allowed_check.rs"]
mod docs_allowed_check;
#[path = "slow/oneshot_args.rs"]
mod oneshot_args;
#[path = "slow/oneshot_ctrl_c_resume.rs"]
mod oneshot_ctrl_c_resume;
#[path = "slow/oneshot_kissconfig_ignore.rs"]
mod oneshot_kissconfig_ignore;
#[path = "slow/oneshot_python_fail_fix.rs"]
mod oneshot_python_fail_fix;
#[path = "slow/oneshot_retry_bad.rs"]
mod oneshot_retry_bad;
#[path = "slow/oneshot_two_runs.rs"]
mod oneshot_two_runs;
#[path = "slow/regression_check_cache_uncached_default.rs"]
mod regression_check_cache_uncached_default;
#[path = "slow/regression_check_default_warm_gate.rs"]
mod regression_check_default_warm_gate;
#[path = "slow/regression_check_default_writes_cache.rs"]
mod regression_check_default_writes_cache;
#[path = "slow/regression_check_focus_empty_dir.rs"]
mod regression_check_focus_empty_dir;
#[path = "slow/regression_check_ignore_filename.rs"]
mod regression_check_ignore_filename;
#[path = "slow/regression_check_mixed_runtime_line_coverage.rs"]
mod regression_check_mixed_runtime_line_coverage;
#[path = "slow/regression_check_rust_incremental_runtime_coverage.rs"]
mod regression_check_rust_incremental_runtime_coverage;
#[path = "slow/regression_check_stats_share_relative.rs"]
mod regression_check_stats_share_relative;
#[path = "slow/regression_check_synthetic_python_coverage_paths.rs"]
mod regression_check_synthetic_python_coverage_paths;
#[path = "slow/regression_init_py_imports_sync.rs"]
mod regression_init_py_imports_sync;
#[path = "slow/regression_stats_all_metric_registry.rs"]
mod regression_stats_all_metric_registry;
#[path = "slow/regression_stats_check_same_ignore.rs"]
mod regression_stats_check_same_ignore;
#[path = "slow/regression_stats_cold_eq_warm.rs"]
mod regression_stats_cold_eq_warm;
#[path = "slow/regression_stats_grouped_unit_test_runtime.rs"]
mod regression_stats_grouped_unit_test_runtime;
#[path = "slow/regression_stats_summary_headers_and_coverage.rs"]
mod regression_stats_summary_headers_and_coverage;
#[path = "slow/regression_stats_summary_uses_cache.rs"]
mod regression_stats_summary_uses_cache;
#[path = "slow/regression_test_sigint_caching.rs"]
mod regression_test_sigint_caching;
#[path = "slow/rpytest_runner_behavior.rs"]
mod rpytest_runner_behavior;
#[path = "slow/rules_config_integration.rs"]
mod rules_config_integration;
#[path = "slow/rust_duplicate_test_names.rs"]
mod rust_duplicate_test_names;
#[path = "slow/sync_stats_check.rs"]
mod sync_stats_check;
