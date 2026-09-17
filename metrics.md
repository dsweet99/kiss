# Eval metrics

Source: `ops/evaluate.py run-all`

| Field | Value |
| --- | --- |
| When | 2026-09-17 14:26 (partial remeasure after kiss-test speed work) |
| Eval count | 28 (summary mixes prior run-all with remeasured long/short timings) |
| Notes | Sympy/anydoc/short timings remeasured this session; other shorts from 09:57 run-all |

## Summary

| Eval | elapsed_s | peak_rss_kib | Other metrics |
| --- | ---: | ---: | --- |
| `long/timing_kiss_test` | 9.7169 | 28368 | kiss_test_cold_elapsed_s=SMALLER(3.1694); kiss_test_warm_elapsed_s=SMALLER(3.1878) |
| `long/timing_kiss_test_anydoc` | 12.3926 | 57272 | kiss_test_cold_elapsed_s=SMALLER(6.5951); kiss_test_warm_elapsed_s=SMALLER(0.2839); cargo_nextest_elapsed_s=SMALLER(5.3760); kiss_test_to_nextest_ratio=SMALLER(1.2268) |
| `long/timing_kiss_test_sympy` | 27.6826 | 175600 | kiss_test_cold_elapsed_s=SMALLER(24.7765); kiss_test_warm_elapsed_s=SMALLER(2.7412) |
| `short/aggregate_coverage` | 5.6778 | 121548 | — |
| `short/concurrent_cache_recovery` | 4.3448 | 30756 | — |
| `short/coverage_cache_witness` | 7.8139 | 123228 | — |
| `short/coverage_no_xdg_hydrate` | 7.9154 | 122464 | — |
| `short/coverage_publication_crash_recovery` | 4.7557 | 30800 | — |
| `short/coverage_stress` | 3.3354 | 30772 | — |
| `short/path_isolation` | 0.0819 | 17676 | — |
| `short/profraw_discard_sink` | 0.0001 | 17716 | — |
| `short/reverse_index_concurrency_stress` | 4.7689 | 122644 | — |
| `short/rust_batch_e2e` | 10.2058 | 123048 | — |
| `short/rust_distinct_groups_interrupt` | 8.0180 | 122680 | — |
| `short/rust_full_repo_observer` | 4.9202 | 123028 | rust_peak_rss_kib=SMALLER(170224); rust_peak_processes=SMALLER(13) |
| `short/rust_phase_interrupt` | 7.6338 | 121828 | — |
| `short/rust_retained_cache_audit` | 4.5103 | 121640 | rust_retained_cache_bytes=SMALLER(19031883) |
| `short/timing_aggregate_parallel` | 4.8043 | 122216 | rust_serial_elapsed_s=SMALLER(4.2841); rust_parallel_elapsed_s=SMALLER(0.3451) |
| `short/timing_kiss_check` | 1.0185 | 27244 | kiss_check_elapsed_s=SMALLER(0.7843) |
| `short/timing_kiss_test` | 7.3585 | 125076 | kiss_test_cold_elapsed_s=SMALLER(4.0427); kiss_test_wipe_elapsed_s=SMALLER(2.9799); kiss_test_warm_elapsed_s=SMALLER(0.1919) |
| `short/timing_kiss_test_progress_cpu` | 4.3896 | 123508 | kiss_test_max_progress_gap_s=SMALLER(0.7131); kiss_test_cpu_utilization=LARGER(0.1890) |
| `short/timing_kiss_test_retry_bad` | 4.4708 | 30824 | kiss_test_retry_bad_elapsed_s=SMALLER(1.8497) |
| `short/timing_kiss_test_sigint_restart` | 6.1017 | 126228 | kiss_test_sigint_exit_latency_s=SMALLER(0.0638); kiss_test_post_interrupt_elapsed_s=SMALLER(4.8664) |
| `short/timing_kiss_test_sigint_work_reuse` | 10.3923 | 30832 | kiss_test_sigint_restart_cached_pass_lines=LARGER(2); kiss_test_sigint_restart_reuse_elapsed_s=SMALLER(0.6827) |
| `short/timing_kiss_test_watch` | 4.2995 | 124716 | kiss_test_elapsed_s=SMALLER(4.1227); kiss_test_peak_rss_kib=SMALLER(8532); watcher_peak_rss_kib=SMALLER(224896) |
| `short/timing_kiss_test_watch_cache_hit` | 8.8611 | 123432 | kiss_test_watch_cache_hit_elapsed_s=SMALLER(0.1151); kiss_test_watch_cache_hit_peak_rss_kib=SMALLER(8808) |
| `short/timing_kiss_test_watch_dirty` | 7.6636 | 30904 | kiss_test_watch_dirty_elapsed_s=SMALLER(2.7616); kiss_test_watch_dirty_peak_rss_kib=SMALLER(8120) |
| `short/timing_rust_throughput` | 4.1808 | 122652 | rust_cold_elapsed_s=SMALLER(3.8304); rust_warm_elapsed_s=SMALLER(0.2044) |

## Per-eval metrics

Each metric uses the VISION.md `EVAL:` convention (`LARGER` / `SMALLER`).

### `long/timing_kiss_test`

| Metric | Direction | Value |
| --- | --- | ---: |
| `kiss_test_cold_elapsed_s` | SMALLER | 3.1694 |
| `kiss_test_warm_elapsed_s` | SMALLER | 3.1878 |
| `elapsed_s` | SMALLER | 9.7169 |
| `peak_rss_kib` | SMALLER | 28368 |

### `long/timing_kiss_test_anydoc`

| Metric | Direction | Value |
| --- | --- | ---: |
| `kiss_test_cold_elapsed_s` | SMALLER | 6.5951 |
| `kiss_test_warm_elapsed_s` | SMALLER | 0.2839 |
| `cargo_nextest_elapsed_s` | SMALLER | 5.3760 |
| `kiss_test_to_nextest_ratio` | SMALLER | 1.2268 |
| `elapsed_s` | SMALLER | 12.3926 |
| `peak_rss_kib` | SMALLER | 57272 |

### `long/timing_kiss_test_sympy`

| Metric | Direction | Value |
| --- | --- | ---: |
| `kiss_test_cold_elapsed_s` | SMALLER | 24.7765 |
| `kiss_test_warm_elapsed_s` | SMALLER | 2.7412 |
| `elapsed_s` | SMALLER | 27.6826 |
| `peak_rss_kib` | SMALLER | 175600 |

### `short/aggregate_coverage`

| Metric | Direction | Value |
| --- | --- | ---: |
| `elapsed_s` | SMALLER | 5.6778 |
| `peak_rss_kib` | SMALLER | 121548 |

### `short/concurrent_cache_recovery`

| Metric | Direction | Value |
| --- | --- | ---: |
| `elapsed_s` | SMALLER | 4.3448 |
| `peak_rss_kib` | SMALLER | 30756 |

### `short/coverage_cache_witness`

| Metric | Direction | Value |
| --- | --- | ---: |
| `elapsed_s` | SMALLER | 7.8139 |
| `peak_rss_kib` | SMALLER | 123228 |

### `short/coverage_no_xdg_hydrate`

| Metric | Direction | Value |
| --- | --- | ---: |
| `elapsed_s` | SMALLER | 7.9154 |
| `peak_rss_kib` | SMALLER | 122464 |

### `short/coverage_publication_crash_recovery`

| Metric | Direction | Value |
| --- | --- | ---: |
| `elapsed_s` | SMALLER | 4.7557 |
| `peak_rss_kib` | SMALLER | 30800 |

### `short/coverage_stress`

| Metric | Direction | Value |
| --- | --- | ---: |
| `elapsed_s` | SMALLER | 3.3354 |
| `peak_rss_kib` | SMALLER | 30772 |

### `short/path_isolation`

| Metric | Direction | Value |
| --- | --- | ---: |
| `elapsed_s` | SMALLER | 0.0819 |
| `peak_rss_kib` | SMALLER | 17676 |

### `short/profraw_discard_sink`

| Metric | Direction | Value |
| --- | --- | ---: |
| `elapsed_s` | SMALLER | 0.0001 |
| `peak_rss_kib` | SMALLER | 17716 |

### `short/reverse_index_concurrency_stress`

| Metric | Direction | Value |
| --- | --- | ---: |
| `elapsed_s` | SMALLER | 4.7689 |
| `peak_rss_kib` | SMALLER | 122644 |

### `short/rust_batch_e2e`

| Metric | Direction | Value |
| --- | --- | ---: |
| `elapsed_s` | SMALLER | 10.2058 |
| `peak_rss_kib` | SMALLER | 123048 |

### `short/rust_distinct_groups_interrupt`

| Metric | Direction | Value |
| --- | --- | ---: |
| `elapsed_s` | SMALLER | 8.0180 |
| `peak_rss_kib` | SMALLER | 122680 |

### `short/rust_full_repo_observer`

| Metric | Direction | Value |
| --- | --- | ---: |
| `rust_peak_rss_kib` | SMALLER | 170224 |
| `rust_peak_processes` | SMALLER | 13 |
| `elapsed_s` | SMALLER | 4.9202 |
| `peak_rss_kib` | SMALLER | 123028 |

### `short/rust_phase_interrupt`

| Metric | Direction | Value |
| --- | --- | ---: |
| `elapsed_s` | SMALLER | 7.6338 |
| `peak_rss_kib` | SMALLER | 121828 |

### `short/rust_retained_cache_audit`

| Metric | Direction | Value |
| --- | --- | ---: |
| `rust_retained_cache_bytes` | SMALLER | 19031883 |
| `elapsed_s` | SMALLER | 4.5103 |
| `peak_rss_kib` | SMALLER | 121640 |

### `short/timing_aggregate_parallel`

| Metric | Direction | Value |
| --- | --- | ---: |
| `rust_serial_elapsed_s` | SMALLER | 4.2841 |
| `rust_parallel_elapsed_s` | SMALLER | 0.3451 |
| `elapsed_s` | SMALLER | 4.8043 |
| `peak_rss_kib` | SMALLER | 122216 |

### `short/timing_kiss_check`

| Metric | Direction | Value |
| --- | --- | ---: |
| `kiss_check_elapsed_s` | SMALLER | 0.7843 |
| `elapsed_s` | SMALLER | 1.0185 |
| `peak_rss_kib` | SMALLER | 27244 |

### `short/timing_kiss_test`

| Metric | Direction | Value |
| --- | --- | ---: |
| `kiss_test_cold_elapsed_s` | SMALLER | 4.0427 |
| `kiss_test_wipe_elapsed_s` | SMALLER | 2.9799 |
| `kiss_test_warm_elapsed_s` | SMALLER | 0.1919 |
| `elapsed_s` | SMALLER | 7.3585 |
| `peak_rss_kib` | SMALLER | 125076 |

### `short/timing_kiss_test_progress_cpu`

| Metric | Direction | Value |
| --- | --- | ---: |
| `kiss_test_max_progress_gap_s` | SMALLER | 0.7131 |
| `kiss_test_cpu_utilization` | LARGER | 0.1890 |
| `elapsed_s` | SMALLER | 4.3896 |
| `peak_rss_kib` | SMALLER | 123508 |

### `short/timing_kiss_test_retry_bad`

| Metric | Direction | Value |
| --- | --- | ---: |
| `kiss_test_retry_bad_elapsed_s` | SMALLER | 1.8497 |
| `elapsed_s` | SMALLER | 4.4708 |
| `peak_rss_kib` | SMALLER | 30824 |

### `short/timing_kiss_test_sigint_restart`

| Metric | Direction | Value |
| --- | --- | ---: |
| `kiss_test_sigint_exit_latency_s` | SMALLER | 0.0638 |
| `kiss_test_post_interrupt_elapsed_s` | SMALLER | 4.8664 |
| `elapsed_s` | SMALLER | 6.1017 |
| `peak_rss_kib` | SMALLER | 126228 |

### `short/timing_kiss_test_sigint_work_reuse`

| Metric | Direction | Value |
| --- | --- | ---: |
| `kiss_test_sigint_restart_cached_pass_lines` | LARGER | 2 |
| `kiss_test_sigint_restart_reuse_elapsed_s` | SMALLER | 0.6827 |
| `elapsed_s` | SMALLER | 10.3923 |
| `peak_rss_kib` | SMALLER | 30832 |

### `short/timing_kiss_test_watch`

| Metric | Direction | Value |
| --- | --- | ---: |
| `kiss_test_elapsed_s` | SMALLER | 4.1227 |
| `kiss_test_peak_rss_kib` | SMALLER | 8532 |
| `watcher_peak_rss_kib` | SMALLER | 224896 |
| `elapsed_s` | SMALLER | 4.2995 |
| `peak_rss_kib` | SMALLER | 124716 |

### `short/timing_kiss_test_watch_cache_hit`

| Metric | Direction | Value |
| --- | --- | ---: |
| `kiss_test_watch_cache_hit_elapsed_s` | SMALLER | 0.1151 |
| `kiss_test_watch_cache_hit_peak_rss_kib` | SMALLER | 8808 |
| `elapsed_s` | SMALLER | 8.8611 |
| `peak_rss_kib` | SMALLER | 123432 |

### `short/timing_kiss_test_watch_dirty`

| Metric | Direction | Value |
| --- | --- | ---: |
| `kiss_test_watch_dirty_elapsed_s` | SMALLER | 2.7616 |
| `kiss_test_watch_dirty_peak_rss_kib` | SMALLER | 8120 |
| `elapsed_s` | SMALLER | 7.6636 |
| `peak_rss_kib` | SMALLER | 30904 |

### `short/timing_rust_throughput`

| Metric | Direction | Value |
| --- | --- | ---: |
| `rust_cold_elapsed_s` | SMALLER | 3.8304 |
| `rust_warm_elapsed_s` | SMALLER | 0.2044 |
| `elapsed_s` | SMALLER | 4.1808 |
| `peak_rss_kib` | SMALLER | 122652 |

