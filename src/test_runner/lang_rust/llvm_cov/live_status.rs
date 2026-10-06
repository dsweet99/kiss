use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use kiss::rpytest_runner::TestStatus;
use kiss::rust_llvm_cov_runner::RustLlvmCovOutcome;

static LIVE_REMAINING: AtomicUsize = AtomicUsize::new(0);

struct LiveHookShared {
    report_ids: BTreeMap<String, String>,
    gate: kiss::GateConfig,
    seen: std::sync::Arc<Mutex<HashSet<String>>>,
    remaining: std::sync::Arc<Mutex<usize>>,
}

fn install_live_test_event_hook(shared: LiveHookShared) {
    let LiveHookShared {
        report_ids,
        gate,
        seen,
        remaining,
    } = shared;
    kiss::rust_llvm_cov_runner::install_live_rust_test_hook(move |name, event, exec_time| {
        let Ok(mut remaining_guard) = remaining.lock() else {
            return;
        };
        let Ok(mut seen_guard) = seen.lock() else {
            return;
        };
        emit_one_live_status(
            &report_ids,
            &gate,
            &mut LiveEmitState {
                remaining: &mut remaining_guard,
                seen: &mut seen_guard,
            },
            name,
            event,
            exec_time,
        );
    });
}

fn install_prepared_cache_hits_hook(shared: LiveHookShared) {
    let LiveHookShared {
        report_ids,
        gate,
        seen,
        remaining,
    } = shared;
    kiss::rust_llvm_cov_runner::install_prepared_rust_cache_hits_hook(move |outcomes| {
        let Ok(mut remaining_guard) = remaining.lock() else {
            return;
        };
        let Ok(mut seen_guard) = seen.lock() else {
            return;
        };
        emit_prepared_cache_hit_statuses(
            &report_ids,
            &gate,
            &mut LiveEmitState {
                remaining: &mut remaining_guard,
                seen: &mut seen_guard,
            },
            outcomes,
        );
    });
}

#[allow(clippy::too_many_arguments)]
pub(super) fn install_live_rust_status_hook(
    repo_root: &Path,
    selectors: &[String],
    gate: &kiss::GateConfig,
    selector_timeout_millis: &BTreeMap<String, u64>,
) -> Result<(), String> {
    let report_ids =
        crate::test_runner::rust_report_id_cache::rust_logical_to_kiss_test_ids_cached(
            repo_root,
            &[],
        )?;
    let banned_count = selectors
        .iter()
        .filter(|selector| selector_timeout_millis.get(*selector) == Some(&0))
        .count();
    let remaining = selectors.len().saturating_sub(banned_count);
    LIVE_REMAINING.store(remaining, Ordering::SeqCst);
    let seen = std::sync::Arc::new(Mutex::new(HashSet::new()));
    let remaining_slot = std::sync::Arc::new(Mutex::new(remaining));
    let shared = LiveHookShared {
        report_ids: report_ids.clone(),
        gate: gate.clone(),
        seen: std::sync::Arc::clone(&seen),
        remaining: std::sync::Arc::clone(&remaining_slot),
    };
    install_live_test_event_hook(LiveHookShared {
        report_ids: shared.report_ids.clone(),
        gate: shared.gate.clone(),
        seen: std::sync::Arc::clone(&shared.seen),
        remaining: std::sync::Arc::clone(&shared.remaining),
    });
    install_prepared_cache_hits_hook(shared);
    let banned =
        emit_zero_sla_rust_timeouts_before_batch(selectors, selector_timeout_millis, &report_ids);
    debug_assert_eq!(banned, banned_count);
    crate::test_runner::tests_remaining::set_language_remaining(
        kiss::Language::Rust,
        LIVE_REMAINING.load(Ordering::SeqCst),
    );
    Ok(())
}

fn emit_zero_sla_rust_timeouts_before_batch(
    selectors: &[String],
    selector_timeout_millis: &BTreeMap<String, u64>,
    report_ids: &BTreeMap<String, String>,
) -> usize {
    let banned: Vec<&String> = selectors
        .iter()
        .filter(|selector| selector_timeout_millis.get(*selector) == Some(&0))
        .collect();
    if banned.is_empty() {
        return 0;
    }
    for logical in &banned {
        let report = report_ids
            .get(logical.as_str())
            .cloned()
            .unwrap_or_else(|| (*logical).clone());
        let status = TestStatus::TimedOut;
        kiss::rust_llvm_cov_runner::mark_live_rust_printed(&report);
        crate::test_runner::status_labels::print_classified_status_line(
            status,
            &report,
            Duration::ZERO,
            None,
            false,
        );
    }
    banned.len()
}

pub(super) fn finish_live_rust_remaining() {
    LIVE_REMAINING.store(0, Ordering::SeqCst);
    crate::test_runner::tests_remaining::set_language_remaining(kiss::Language::Rust, 0);
}

struct LiveEmitState<'a> {
    remaining: &'a mut usize,
    seen: &'a mut HashSet<String>,
}

fn emit_prepared_cache_hit_statuses(
    report_ids: &BTreeMap<String, String>,
    gate: &kiss::GateConfig,
    state: &mut LiveEmitState<'_>,
    outcomes: &[RustLlvmCovOutcome],
) {
    for outcome in outcomes {
        if !matches!(
            outcome.cache_status,
            kiss::rust_llvm_cov_runner::RustCovCacheStatus::Hit
        ) {
            continue;
        }
        let report = report_ids
            .get(&outcome.selector)
            .cloned()
            .unwrap_or_else(|| outcome.selector.clone());
        if !state.seen.insert(report.clone()) {
            continue;
        }
        kiss::rust_llvm_cov_runner::mark_live_rust_printed(&report);
        let status = crate::test_runner::status_labels::apply_unit_test_time_limit(
            outcome.status,
            &report,
            outcome.duration,
            gate,
        );
        crate::test_runner::status_labels::print_classified_status_line(
            status,
            &report,
            outcome.duration,
            Some("cached"),
            false,
        );
        *state.remaining = state.remaining.saturating_sub(1);
        LIVE_REMAINING.store(*state.remaining, Ordering::SeqCst);
    }
    if !outcomes.is_empty() {
        crate::test_runner::tests_remaining::set_language_remaining(
            kiss::Language::Rust,
            *state.remaining,
        );
    }
}

fn emit_one_live_status(
    report_ids: &BTreeMap<String, String>,
    gate: &kiss::GateConfig,
    state: &mut LiveEmitState<'_>,
    name: &str,
    event: &str,
    exec_time: f64,
) {
    let Some(report) = kiss_id_for_libtest(report_ids, name) else {
        return;
    };
    let Some(raw) = status_from_libtest_event(event) else {
        return;
    };
    if !state.seen.insert(report.clone()) {
        return;
    }
    kiss::rust_llvm_cov_runner::mark_live_rust_printed(&report);
    let duration = Duration::from_secs_f64(exec_time.max(0.0));
    let status =
        crate::test_runner::status_labels::apply_unit_test_time_limit(raw, &report, duration, gate);
    crate::test_runner::status_labels::print_classified_status_line(
        status, &report, duration, None, true,
    );
    *state.remaining = state.remaining.saturating_sub(1);
    LIVE_REMAINING.store(*state.remaining, Ordering::SeqCst);
    crate::test_runner::tests_remaining::set_language_remaining(
        kiss::Language::Rust,
        *state.remaining,
    );
}

fn kiss_id_for_libtest(report_ids: &BTreeMap<String, String>, name: &str) -> Option<String> {
    if let Some(id) = report_ids.get(name) {
        return Some(id.clone());
    }
    if let Some(index) = name.rfind('$')
        && let Some(id) = report_ids.get(&name[index..])
    {
        return Some(id.clone());
    }
    let logical = name.rsplit_once('$').map_or(name, |(_, test)| test);
    if let Some(id) = report_ids.get(logical) {
        return Some(id.clone());
    }
    let suffix = format!("::{logical}");
    let mut candidates: Vec<(&String, &String)> = report_ids
        .iter()
        .filter(|(key, _)| key.ends_with(&suffix) || logical.ends_with(&format!("::{key}")))
        .collect();

    if candidates.is_empty() {
        return None;
    }
    if candidates.len() == 1 {
        return Some(candidates[0].1.clone());
    }

    let max_len = candidates
        .iter()
        .map(|(key, _)| key.len())
        .max()
        .unwrap_or(0);
    candidates.retain(|(key, _)| key.len() == max_len);
    if candidates.len() == 1 {
        return Some(candidates[0].1.clone());
    }

    candidates.retain(|(_, id)| source_path_matches_logical(id, logical));
    if candidates.len() == 1 {
        return Some(candidates[0].1.clone());
    }

    None
}

fn source_path_matches_logical(id: &str, logical: &str) -> bool {
    let file_path = id.split_once("::").map_or(id, |(path, _)| path);
    let path = Path::new(file_path);
    let name = if path.file_name().and_then(|s| s.to_str()) == Some("mod.rs") {
        path.parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
    } else {
        path.file_stem().and_then(|s| s.to_str())
    };
    if let Some(mod_name) = name
        && mod_name != "lib"
        && mod_name != "main"
    {
        return logical.split("::").any(|seg| seg == mod_name);
    }
    false
}

fn status_from_libtest_event(event: &str) -> Option<TestStatus> {
    match event {
        "ok" => Some(TestStatus::Passed),
        "failed" => Some(TestStatus::Failed),
        "timeout" | "timed_out" => Some(TestStatus::TimedOut),
        _ => None,
    }
}

#[cfg(test)]
mod live_status_test {
    use super::{
        LIVE_REMAINING, LiveEmitState, emit_one_live_status, emit_prepared_cache_hit_statuses,
        install_live_rust_status_hook, kiss_id_for_libtest, status_from_libtest_event,
    };
    use kiss::rpytest_runner::TestStatus;
    use std::collections::{BTreeMap, HashSet};
    use std::sync::atomic::Ordering;
    use std::sync::{Mutex, MutexGuard};
    use std::time::Duration;

    fn live_status_serial_guard() -> MutexGuard<'static, ()> {
        static GUARD: Mutex<()> = Mutex::new(());
        GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn begin_live_status_serial() -> (std::sync::MutexGuard<'static, ()>, MutexGuard<'static, ()>) {
        let remaining = crate::test_runner::tests_remaining::remaining_test_guard();
        let guard = live_status_serial_guard();
        kiss::rust_llvm_cov_runner::clear_live_rust_test_hook();
        LIVE_REMAINING.store(0, Ordering::SeqCst);
        crate::test_runner::tests_remaining::reset_tests_remaining();
        (remaining, guard)
    }

    fn emit(
        ids: &BTreeMap<String, String>,
        gate: &kiss::GateConfig,
        remaining: &mut usize,
        seen: &mut HashSet<String>,
        name: &str,
        event: &str,
        exec_time: f64,
    ) {
        emit_one_live_status(
            ids,
            gate,
            &mut LiveEmitState { remaining, seen },
            name,
            event,
            exec_time,
        );
    }

    #[test]
    fn maps_libtest_suffix_and_event() {
        let _serial = begin_live_status_serial();
        let mut ids = BTreeMap::new();
        ids.insert("case".into(), "src/lib.rs::case".into());
        ids.insert("nested::case".into(), "src/lib.rs::nested".into());
        ids.insert("outer::long_name".into(), "src/lib.rs::long".into());
        ids.insert("space".into(), "src/lib.rs::space".into());
        ids.insert("outer::dup".into(), "src/lib.rs::outer_dup".into());
        ids.insert("inner::dup".into(), "src/lib.rs::inner_dup".into());
        assert_eq!(
            kiss_id_for_libtest(&ids, "pkg::bin$case").as_deref(),
            Some("src/lib.rs::case")
        );
        assert_eq!(
            kiss_id_for_libtest(&ids, "nested::case").as_deref(),
            Some("src/lib.rs::nested")
        );
        assert_eq!(
            kiss_id_for_libtest(&ids, "pkg::bin$long_name").as_deref(),
            Some("src/lib.rs::long")
        );
        assert_eq!(kiss_id_for_libtest(&ids, "pkg::bin$missing"), None);
        assert_eq!(kiss_id_for_libtest(&ids, "pkg::bin$ace"), None);
        assert_eq!(kiss_id_for_libtest(&ids, "pkg::bin$dup"), None);

        ids.insert(
            "tests::test_cache".into(),
            "src/check_cache.rs::test_cache".into(),
        );
        assert_eq!(
            kiss_id_for_libtest(&ids, "kiss-ai::kiss$check_cache::tests::test_cache").as_deref(),
            Some("src/check_cache.rs::test_cache")
        );

        ids.insert("run".into(), "src/alpha.rs::run".into());
        ids.insert("extra::run".into(), "src/beta.rs::run".into());
        assert_eq!(
            kiss_id_for_libtest(&ids, "pkg$gamma::extra::run").as_deref(),
            Some("src/beta.rs::run")
        );

        let mut by_path = BTreeMap::new();
        by_path.insert("sub::test_x".into(), "src/foo.rs::test_x".into());
        by_path.insert("sub::test_y".into(), "src/bar.rs::test_y".into());
        assert_eq!(
            kiss_id_for_libtest(&by_path, "pkg$foo::sub::test_x").as_deref(),
            Some("src/foo.rs::test_x")
        );
        assert_eq!(status_from_libtest_event("ok"), Some(TestStatus::Passed));
        assert_eq!(
            status_from_libtest_event("failed"),
            Some(TestStatus::Failed)
        );
        assert_eq!(
            status_from_libtest_event("timed_out"),
            Some(TestStatus::TimedOut)
        );
        assert_eq!(
            status_from_libtest_event("timeout"),
            Some(TestStatus::TimedOut)
        );
        assert_eq!(status_from_libtest_event("started"), None);

        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
        )
        .unwrap();
        std::fs::create_dir(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("src").join("lib.rs"),
            "#[cfg(test)]\nmod tests {\n    #[test]\n    fn case() {}\n}\n",
        )
        .unwrap();
        install_live_rust_status_hook(
            tmp.path(),
            &["tests::case".into()],
            &kiss::GateConfig::default(),
            &BTreeMap::new(),
        )
        .unwrap();
        kiss::rust_llvm_cov_runner::clear_live_rust_test_hook();

        let gate = kiss::GateConfig::default();
        let mut remaining = 1;
        let mut seen = HashSet::new();
        emit(
            &ids,
            &gate,
            &mut remaining,
            &mut seen,
            "pkg::bin$case",
            "ok",
            0.05,
        );
        assert_eq!(remaining, 0);
    }

    #[test]
    fn missing_report_id_does_not_cancel_the_rust_batch() {
        let _serial = begin_live_status_serial();
        let src = include_str!("live_status.rs");
        let code = src.split("mod live_status_test").next().expect("prod src");
        assert!(
            !code.contains("cancel_active_batch_scope"),
            "unmapped live rust names must not cancel the batch (peer Python SIGPIPE)"
        );
    }

    #[test]
    fn emit_live_status_dedups_and_skips_unknown() {
        let _serial = begin_live_status_serial();
        let mut ids = BTreeMap::new();
        ids.insert("case".into(), "src/lib.rs::case".into());
        let gate = kiss::GateConfig::default();
        let mut remaining = 2;
        let mut seen = HashSet::new();
        let _ = kiss::rust_llvm_cov_runner::take_live_rust_error();
        emit(
            &ids,
            &gate,
            &mut remaining,
            &mut seen,
            "pkg::bin$missing",
            "ok",
            0.1,
        );
        assert_eq!(kiss::rust_llvm_cov_runner::take_live_rust_error(), None);
        assert_eq!(remaining, 2);
        emit(
            &ids,
            &gate,
            &mut remaining,
            &mut seen,
            "pkg::bin$case",
            "started",
            0.1,
        );
        assert_eq!(remaining, 2);
        emit(
            &ids,
            &gate,
            &mut remaining,
            &mut seen,
            "pkg::bin$case",
            "ok",
            0.2,
        );
        assert_eq!(remaining, 1);
        emit(
            &ids,
            &gate,
            &mut remaining,
            &mut seen,
            "pkg::bin$case",
            "ok",
            0.3,
        );
        assert_eq!(remaining, 1);
        kiss::rust_llvm_cov_runner::clear_live_rust_test_hook();
    }

    #[test]
    fn live_ok_over_time_limit_prints_timeout() {
        let _serial = begin_live_status_serial();
        let mut ids = BTreeMap::new();
        ids.insert("over_time_case".into(), "src/lib.rs::over_time_case".into());
        let gate = kiss::GateConfig {
            max_unit_test_seconds: vec![("*".into(), 0.0)],
            ..kiss::GateConfig::default()
        };
        let mut remaining = 1;
        let mut seen = HashSet::new();
        let out = crate::test_runner::capture_stdout::capture_stdout(|| {
            emit(
                &ids,
                &gate,
                &mut remaining,
                &mut seen,
                "pkg::bin$over_time_case",
                "ok",
                1.0,
            );
        });
        assert!(
            out.contains("TIMEOUT: src/lib.rs::over_time_case"),
            "over-limit ok must print TIMEOUT: {out}"
        );
        assert!(
            !out.contains("PASS: src/lib.rs::over_time_case"),
            "over-limit ok must not print PASS: {out}"
        );
    }

    #[test]
    fn zero_sla_timeout_prints_before_batch() {
        let _serial = begin_live_status_serial();
        let tmp = tempfile::TempDir::new().unwrap();
        let selectors = vec!["banned".to_string(), "runnable".to_string()];
        let timeouts = BTreeMap::from([
            ("banned".to_string(), 0u64),
            ("runnable".to_string(), 5_000),
        ]);
        let out = crate::test_runner::capture_stdout::capture_stdout(|| {
            install_live_rust_status_hook(
                tmp.path(),
                &selectors,
                &kiss::GateConfig::default(),
                &timeouts,
            )
            .unwrap();
        });
        assert!(
            out.contains("TIMEOUT: banned"),
            "zero-SLA ban must print TIMEOUT before the batch: {out}"
        );
        assert_eq!(LIVE_REMAINING.load(Ordering::SeqCst), 1);
        assert!(kiss::rust_llvm_cov_runner::live_rust_was_printed("banned"));
        kiss::rust_llvm_cov_runner::clear_live_rust_test_hook();
    }

    #[test]
    fn prepared_cache_hit_time_gate_timeout_prints_before_misses() {
        let _serial = begin_live_status_serial();
        let gate = kiss::GateConfig {
            max_unit_test_seconds: vec![("*".into(), 0.5)],
            ..kiss::GateConfig::default()
        };
        let mut remaining = 2usize;
        let mut seen = HashSet::new();
        LIVE_REMAINING.store(remaining, Ordering::SeqCst);
        let outcomes = [kiss::rust_llvm_cov_runner::RustLlvmCovOutcome {
            selector: "cached_over".to_string(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_secs(2),
            coverage: Default::default(),
            test_binary_ids: Vec::new(),
            cache_status: kiss::rust_llvm_cov_runner::RustCovCacheStatus::Hit,
            stdout: None,
            stderr: None,
        }];
        let out = crate::test_runner::capture_stdout::capture_stdout(|| {
            emit_prepared_cache_hit_statuses(
                &BTreeMap::new(),
                &gate,
                &mut LiveEmitState {
                    remaining: &mut remaining,
                    seen: &mut seen,
                },
                &outcomes,
            );
        });
        assert!(
            out.contains("TIMEOUT") && out.contains("cached_over"),
            "prepare-time over-limit cache hit must print TIMEOUT before misses: {out}"
        );
        assert_eq!(remaining, 1);
        assert!(kiss::rust_llvm_cov_runner::live_rust_was_printed(
            "cached_over"
        ));
    }
}
