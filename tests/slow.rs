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
#[path = "slow/bug_test_coverage_aggregate_gate_masking.rs"]
mod bug_test_coverage_aggregate_gate_masking;
#[path = "slow/bug_test_coverage_violation_message.rs"]
mod bug_test_coverage_violation_message;
#[path = "slow/cache_integration.rs"]
mod cache_integration;
#[path = "slow/clamp_then_check.rs"]
mod clamp_then_check;
#[path = "slow/cli_check_hint.rs"]
mod cli_check_hint;
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
#[path = "slow/sync_stats_check.rs"]
mod sync_stats_check;
#[path = "slow/watch_bilingual_counts.rs"]
mod watch_bilingual_counts;
#[path = "slow/watch_client.rs"]
mod watch_client;
#[path = "slow/watch_client_violations.rs"]
mod watch_client_violations;
#[path = "slow/watch_dry_run_rejected.rs"]
mod watch_dry_run_rejected;
#[path = "slow/watch_paths.rs"]
mod watch_paths;
#[path = "slow/watch_retry_bad.rs"]
mod watch_retry_bad;
#[path = "slow/watch_s01_no_edits.rs"]
mod watch_s01_no_edits;
#[path = "slow/watch_s02_edit_cycle.rs"]
mod watch_s02_edit_cycle;
#[path = "slow/watch_s02_python_fail_fix.rs"]
mod watch_s02_python_fail_fix;
#[path = "slow/watch_s02_rust_library_edit.rs"]
mod watch_s02_rust_library_edit;
#[path = "slow/watch_s03_target_scope.rs"]
mod watch_s03_target_scope;
#[path = "slow/watch_s05_s06_lang.rs"]
mod watch_s05_s06_lang;
#[path = "slow/watch_s07_second_watch.rs"]
mod watch_s07_second_watch;
#[path = "slow/watch_s08_two_oneshots.rs"]
mod watch_s08_two_oneshots;
#[path = "slow/watch_s09_client_during_startup.rs"]
mod watch_s09_client_during_startup;
#[path = "slow/watch_s10_stale_watcher.rs"]
mod watch_s10_stale_watcher;
#[path = "slow/watch_s11_watch_during_oneshot.rs"]
mod watch_s11_watch_during_oneshot;
#[path = "slow/watch_s13_git_targets.rs"]
mod watch_s13_git_targets;
#[path = "slow/watch_s14_multi_targets.rs"]
mod watch_s14_multi_targets;
#[path = "slow/watch_s15_watch_takes_no_options.rs"]
mod watch_s15_watch_takes_no_options;
#[path = "slow/watch_s16_doctests.rs"]
mod watch_s16_doctests;
#[path = "slow/watch_s17_ctrl_c_plain_run.rs"]
mod watch_s17_ctrl_c_plain_run;
#[path = "slow/watch_s18_ctrl_c_client.rs"]
mod watch_s18_ctrl_c_client;
#[path = "slow/watch_s19_edits_during_cycle.rs"]
mod watch_s19_edits_during_cycle;
#[path = "slow/watch_s20_subdir.rs"]
mod watch_s20_subdir;
#[path = "slow/watch_s21_outdated_config.rs"]
mod watch_s21_outdated_config;
#[path = "slow/watch_s22_retry_bad_no_watcher.rs"]
mod watch_s22_retry_bad_no_watcher;
#[path = "slow/watch_s23_kissconfig_ignore.rs"]
mod watch_s23_kissconfig_ignore;
#[path = "slow/watch_s24_second_worktree.rs"]
mod watch_s24_second_worktree;
#[path = "slow/watch_sigint.rs"]
mod watch_sigint;
#[path = "slow/watch_startup_order.rs"]
mod watch_startup_order;
