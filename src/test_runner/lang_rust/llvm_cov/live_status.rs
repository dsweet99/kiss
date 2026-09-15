use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use kiss::rpytest_runner::TestStatus;
use kiss::rust_llvm_cov_runner::{RustCoverageBatchIdentity, RustLlvmCovOutcome};

use crate::test_runner::execution_witness::WitnessStatus;
use crate::test_runner::last_status::{LastStatusIdentity, record_statuses};

pub(super) use super::live_witness::flush_live_rust_witness;
#[cfg(test)]
pub(super) use super::live_witness::clear_live_rust_witness;
use super::live_witness::{
    LiveWitnessCache, record_live_rust_non_pass, record_live_rust_pass, seed_live_witness_cache,
};

static LIVE_REMAINING: AtomicUsize = AtomicUsize::new(0);

struct LiveHookShared {
    report_ids: BTreeMap<String, String>,
    gate: kiss::GateConfig,
    repo_root: PathBuf,
    identity: LastStatusIdentity,
    seen: std::sync::Arc<Mutex<HashSet<String>>>,
    remaining: std::sync::Arc<Mutex<usize>>,
    /// Known retry-bad selectors; PASS only touches disk when clearing one of these.
    pending_failures: std::sync::Arc<Mutex<HashSet<String>>>,
}

fn install_live_test_event_hook(shared: LiveHookShared) {
    let LiveHookShared {
        report_ids,
        gate,
        repo_root,
        identity,
        seen,
        remaining,
        pending_failures,
    } = shared;
    kiss::rust_llvm_cov_runner::install_live_rust_test_hook(move |name, event, exec_time| {
        let Ok(mut remaining_guard) = remaining.lock() else {
            return;
        };
        let Ok(mut seen_guard) = seen.lock() else {
            return;
        };
        let Ok(mut pending_failures_guard) = pending_failures.lock() else {
            return;
        };
        emit_one_live_status(
            &report_ids,
            &gate,
            &mut LiveEmitState {
                remaining: &mut remaining_guard,
                seen: &mut seen_guard,
                pending_failures: &mut pending_failures_guard,
                persist: Some((repo_root.as_path(), &identity)),
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
        repo_root,
        identity,
        seen,
        remaining,
        pending_failures,
    } = shared;
    kiss::rust_llvm_cov_runner::install_prepared_rust_cache_hits_hook(move |outcomes| {
        let Ok(mut remaining_guard) = remaining.lock() else {
            return;
        };
        let Ok(mut seen_guard) = seen.lock() else {
            return;
        };
        let Ok(mut pending_failures_guard) = pending_failures.lock() else {
            return;
        };
        emit_prepared_cache_hit_statuses(
            &report_ids,
            &gate,
            &mut LiveEmitState {
                remaining: &mut remaining_guard,
                seen: &mut seen_guard,
                pending_failures: &mut pending_failures_guard,
                persist: Some((repo_root.as_path(), &identity)),
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
    identity: &LastStatusIdentity,
    batch_identity: Option<&RustCoverageBatchIdentity>,
    population_selectors: Option<&[String]>,
    jobs: usize,
    selector_timeout_millis: &BTreeMap<String, u64>,
) -> Result<(), String> {
    if let Some(batch_id) = batch_identity {
        seed_live_witness_cache(LiveWitnessCache::new(
            repo_root,
            batch_id,
            population_selectors,
            selectors,
            jobs,
        ));
    }
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
    let pending_failures = std::sync::Arc::new(Mutex::new(
        crate::test_runner::last_status::prior_failures(
            repo_root,
            kiss::Language::Rust,
            identity,
        )
        .unwrap_or_default()
        .into_iter()
        .collect::<HashSet<_>>(),
    ));
    let shared = LiveHookShared {
        report_ids: report_ids.clone(),
        gate: gate.clone(),
        repo_root: repo_root.to_path_buf(),
        identity: identity.clone(),
        seen: std::sync::Arc::clone(&seen),
        remaining: std::sync::Arc::clone(&remaining_slot),
        pending_failures: std::sync::Arc::clone(&pending_failures),
    };
    install_live_test_event_hook(LiveHookShared {
        report_ids: shared.report_ids.clone(),
        gate: shared.gate.clone(),
        repo_root: shared.repo_root.clone(),
        identity: shared.identity.clone(),
        seen: std::sync::Arc::clone(&shared.seen),
        remaining: std::sync::Arc::clone(&shared.remaining),
        pending_failures: std::sync::Arc::clone(&shared.pending_failures),
    });
    install_prepared_cache_hits_hook(shared);
    let banned = persist_zero_sla_rust_timeouts_before_batch(
        repo_root,
        identity,
        selectors,
        selector_timeout_millis,
        &report_ids,
    );
    debug_assert_eq!(banned, banned_count);
    if banned > 0 {
        crate::test_runner::tests_remaining::emit_tests_remaining(
            LIVE_REMAINING.load(Ordering::SeqCst),
        );
    }
    Ok(())
}


fn persist_zero_sla_rust_timeouts_before_batch(
    repo_root: &Path,
    identity: &LastStatusIdentity,
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
    let mut statuses = Vec::with_capacity(banned.len());
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
        record_live_rust_non_pass(logical, &report, WitnessStatus::TimedOut);
        statuses.push(((*logical).clone(), status));
    }
    let _ = record_statuses(repo_root, kiss::Language::Rust, identity, &statuses);
    banned.len()
}

pub(super) fn finish_live_rust_remaining() {
    if LIVE_REMAINING.swap(0, Ordering::SeqCst) > 0 {
        crate::test_runner::tests_remaining::emit_tests_remaining(0);
    }
}

struct LiveEmitState<'a> {
    remaining: &'a mut usize,
    seen: &'a mut HashSet<String>,
    pending_failures: &'a mut HashSet<String>,
    persist: Option<(&'a Path, &'a LastStatusIdentity)>,
}

fn maybe_record_last_status(
    state: &mut LiveEmitState<'_>,
    logical: &str,
    status: TestStatus,
) {
    let Some((repo_root, identity)) = state.persist else {
        return;
    };
    match status {
        TestStatus::Failed | TestStatus::TimedOut => {
            state.pending_failures.insert(logical.to_string());
            let _ = record_statuses(
                repo_root,
                kiss::Language::Rust,
                identity,
                &[(logical.to_string(), status)],
            );
        }
        TestStatus::Passed => {
            if state.pending_failures.remove(logical) {
                let _ = record_statuses(
                    repo_root,
                    kiss::Language::Rust,
                    identity,
                    &[(logical.to_string(), status)],
                );
            }
        }
    }
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
        let logical = outcome.selector.as_str();
        if status == TestStatus::Passed {
            record_live_rust_pass(logical, &report, outcome.duration);
        } else if matches!(status, TestStatus::Failed | TestStatus::TimedOut) {
            let st = if status == TestStatus::Failed {
                WitnessStatus::Failed
            } else {
                WitnessStatus::TimedOut
            };
            record_live_rust_non_pass(logical, &report, st);
        }
        maybe_record_last_status(state, logical, status);
        *state.remaining = state.remaining.saturating_sub(1);
        LIVE_REMAINING.store(*state.remaining, Ordering::SeqCst);
    }
    if !outcomes.is_empty() {
        crate::test_runner::tests_remaining::emit_tests_remaining(*state.remaining);
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
    let logical = report_ids
        .iter()
        .find(|(_, id)| *id == &report)
        .map(|(key, _)| key.as_str())
        .unwrap_or(&report);

    if status == TestStatus::Passed {
        record_live_rust_pass(logical, &report, duration);
    } else if matches!(status, TestStatus::Failed | TestStatus::TimedOut) {
        let st = if status == TestStatus::Failed {
            WitnessStatus::Failed
        } else {
            WitnessStatus::TimedOut
        };
        record_live_rust_non_pass(logical, &report, st);
    }

    maybe_record_last_status(state, logical, status);
    *state.remaining = state.remaining.saturating_sub(1);
    LIVE_REMAINING.store(*state.remaining, Ordering::SeqCst);
    crate::test_runner::tests_remaining::emit_tests_remaining(*state.remaining);
}

fn kiss_id_for_libtest(report_ids: &BTreeMap<String, String>, name: &str) -> Option<String> {
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

    let max_len = candidates.iter().map(|(key, _)| key.len()).max().unwrap_or(0);
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
        path.parent().and_then(|p| p.file_name()).and_then(|s| s.to_str())
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
        LIVE_REMAINING, LiveEmitState, LiveWitnessCache, clear_live_rust_witness,
        emit_one_live_status, emit_prepared_cache_hit_statuses, flush_live_rust_witness,
        install_live_rust_status_hook, kiss_id_for_libtest, record_live_rust_pass,
        status_from_libtest_event,
    };
    use crate::test_runner::lang_iface::{WitnessScope, WitnessStatus};
    use crate::test_runner::last_status::LastStatusIdentity;
    use kiss::rpytest_runner::TestStatus;
    use std::collections::{BTreeMap, BTreeSet, HashSet};
    use std::path::Path;
    use std::sync::{Mutex, MutexGuard};
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    fn live_status_serial_guard() -> MutexGuard<'static, ()> {
        static GUARD: Mutex<()> = Mutex::new(());
        GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn begin_live_status_serial() -> MutexGuard<'static, ()> {
        let guard = live_status_serial_guard();
        kiss::rust_llvm_cov_runner::clear_live_rust_test_hook();
        clear_live_rust_witness();
        LIVE_REMAINING.store(0, Ordering::SeqCst);
        guard
    }

    #[allow(clippy::too_many_arguments)]
    fn emit(
        ids: &BTreeMap<String, String>,
        gate: &kiss::GateConfig,
        remaining: &mut usize,
        seen: &mut HashSet<String>,
        name: &str,
        event: &str,
        exec_time: f64,
        persist: Option<(&Path, &LastStatusIdentity)>,
    ) {
        let mut pending_failures = HashSet::new();
        if let Some((repo_root, identity)) = persist {
            pending_failures.extend(
                crate::test_runner::last_status::prior_failures(
                    repo_root,
                    kiss::Language::Rust,
                    identity,
                )
                .unwrap_or_default(),
            );
        }
        emit_one_live_status(
            ids,
            gate,
            &mut LiveEmitState {
                remaining,
                seen,
                pending_failures: &mut pending_failures,
                persist,
            },
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
            &crate::test_runner::last_status::rust_last_status_identity(
                "c",
                "l",
                "r",
                "n",
                &[],
                "map",
            ),
            None,
            None,
            1,
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
            None,
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
            None,
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
            None,
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
            None,
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
            None,
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
                None,
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
    fn live_failure_persists_last_status_immediately() {
        let _serial = begin_live_status_serial();
        let tmp = tempfile::TempDir::new().unwrap();
        let identity = crate::test_runner::last_status::rust_last_status_identity(
            "c",
            "l",
            "r",
            "n",
            &[],
            "map",
        );
        let mut ids = BTreeMap::new();
        ids.insert("case".into(), "src/lib.rs::case".into());
        let gate = kiss::GateConfig::default();
        let mut remaining = 1;
        let mut seen = HashSet::new();
        let repo = tmp.path().to_path_buf();
        emit(
            &ids,
            &gate,
            &mut remaining,
            &mut seen,
            "pkg::bin$case",
            "failed",
            0.2,
            Some((repo.as_path(), &identity)),
        );
        assert_eq!(
            crate::test_runner::last_status::prior_failures(
                tmp.path(),
                kiss::Language::Rust,
                &identity
            )
            .unwrap(),
            vec!["case".to_string()]
        );
    }

    #[test]
    fn live_time_gate_timeout_persists_effective_status() {
        let _serial = begin_live_status_serial();
        let tmp = tempfile::TempDir::new().unwrap();
        let identity = crate::test_runner::last_status::rust_last_status_identity(
            "c",
            "l",
            "r",
            "n",
            &[],
            "map",
        );
        let mut ids = BTreeMap::new();
        ids.insert("over_time_case".into(), "src/lib.rs::over_time_case".into());
        let gate = kiss::GateConfig {
            max_unit_test_seconds: vec![("*".into(), 0.5)],
            ..kiss::GateConfig::default()
        };
        let mut remaining = 1;
        let mut seen = HashSet::new();
        let repo = tmp.path().to_path_buf();
        emit(
            &ids,
            &gate,
            &mut remaining,
            &mut seen,
            "pkg::bin$over_time_case",
            "ok",
            1.0,
            Some((repo.as_path(), &identity)),
        );
        assert_eq!(
            crate::test_runner::last_status::prior_failures(
                tmp.path(),
                kiss::Language::Rust,
                &identity
            )
            .unwrap(),
            vec!["over_time_case".to_string()],
            "effective TimedOut from time gate must be retry-bad eligible"
        );
    }

    #[test]
    fn live_pass_clears_prior_failure_immediately() {
        let _serial = begin_live_status_serial();
        let tmp = tempfile::TempDir::new().unwrap();
        let identity = crate::test_runner::last_status::rust_last_status_identity(
            "c",
            "l",
            "r",
            "n",
            &[],
            "map",
        );
        let mut ids = BTreeMap::new();
        ids.insert("case".into(), "src/lib.rs::case".into());
        crate::test_runner::last_status::record_statuses(
            tmp.path(),
            kiss::Language::Rust,
            &identity,
            &[("case".into(), TestStatus::Failed)],
        )
        .unwrap();
        let gate = kiss::GateConfig::default();
        let mut remaining = 1;
        let mut seen = HashSet::new();
        let repo = tmp.path().to_path_buf();
        emit(
            &ids,
            &gate,
            &mut remaining,
            &mut seen,
            "pkg::bin$case",
            "ok",
            0.2,
            Some((repo.as_path(), &identity)),
        );
        assert!(
            crate::test_runner::last_status::prior_failures(
                tmp.path(),
                kiss::Language::Rust,
                &identity
            )
            .unwrap()
            .is_empty(),
            "live PASS must clear prior FAIL so --retry-bad does not keep a stale mark"
        );
    }

    #[test]
    fn live_witness_cache_records_pass_and_persists_to_disk() {
        let _serial = begin_live_status_serial();
        let tmp = tempfile::TempDir::new().unwrap();
        let identity = kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity {
            input_digest: "inp123".into(),
            generation_fingerprint: "gen123".into(),
            selection_context_fingerprint: "ctx123".into(),
            ordinary_source_digests: BTreeMap::new(),
        };
        let mut cache = LiveWitnessCache::new(
            tmp.path(),
            &identity,
            Some(&["t1".into(), "t2".into()]),
            &["t1".into(), "t2".into()],
            4,
        );
        assert_eq!(cache.selectors, vec!["t1".to_string(), "t2".to_string()]);
        assert_eq!(
            cache.statuses,
            vec![WitnessStatus::Unresolved, WitnessStatus::Unresolved]
        );
        assert!(!cache.dirty);

        cache.record_pass("t1", "src/lib.rs::t1", Duration::from_millis(42));
        assert!(cache.dirty);
        cache.persist();
        assert!(!cache.dirty);

        let loaded =
            crate::test_runner::lang_rust::generation_publish::load_full_generation_witness(
                tmp.path(),
            )
            .expect("load full generation witness");
        assert_eq!(loaded.selectors, vec!["t1".to_string(), "t2".to_string()]);
        assert_eq!(loaded.statuses[0], WitnessStatus::Passed);
        assert_eq!(loaded.durations_ns[0], Some(42_000_000));
        assert_eq!(loaded.statuses[1], WitnessStatus::Unresolved);
        assert!(!loaded.complete);
    }

    #[test]
    fn live_witness_new_keeps_universe_closed_to_unknown_selectors() {
        let _serial = begin_live_status_serial();
        let tmp = tempfile::TempDir::new().unwrap();
        let identity = kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity {
            input_digest: "inp_merge".into(),
            generation_fingerprint: "gen_merge".into(),
            selection_context_fingerprint: "ctx_merge".into(),
            ordinary_source_digests: BTreeMap::new(),
        };
        let _ = crate::test_runner::execution_witness::publish_rust_execution_witness(
            crate::test_runner::execution_witness::PublishRustWitness {
                repo_root: tmp.path(),
                identity: &identity,
                scope: WitnessScope::Full,
                selectors: &["seeded".into(), "extra".into()],
                statuses: &[WitnessStatus::Passed, WitnessStatus::Failed],
                durations_ns: &[Some(1), Some(2)],
                covered_lines: &BTreeMap::from([(
                    "src/lib.rs".into(),
                    BTreeSet::from([1u32]),
                )]),
                complete: false,
                jobs: 1,
            },
        );
        let mut cache = LiveWitnessCache::new(
            tmp.path(),
            &identity,
            Some(&["seeded".into()]),
            &["seeded".into()],
            1,
        );
        assert_eq!(cache.selectors, vec!["seeded".to_string()]);
        assert!(cache.covered_lines.contains_key("src/lib.rs"));
        assert_eq!(cache.statuses[0], WitnessStatus::Passed);
        cache.record_pass("brand_new", "brand_new", Duration::from_millis(3));
        cache.record_non_pass("another_new", "another_new", WitnessStatus::Failed);
        assert_eq!(cache.selectors, vec!["seeded".to_string()]);
        cache.persist();
    }

    #[test]
    fn live_witness_cache_records_non_pass_and_flush() {
        let _serial = begin_live_status_serial();
        let tmp = tempfile::TempDir::new().unwrap();
        let identity = kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity {
            input_digest: "inp456".into(),
            generation_fingerprint: "gen456".into(),
            selection_context_fingerprint: "ctx456".into(),
            ordinary_source_digests: BTreeMap::new(),
        };
        let mut cache = LiveWitnessCache::new(
            tmp.path(),
            &identity,
            None,
            &["alpha".into(), "beta".into()],
            2,
        );
        cache.record_pass("alpha", "alpha", Duration::from_millis(10));
        cache.record_non_pass("beta", "beta", WitnessStatus::Failed);
        assert!(cache.dirty);
        cache.persist();
        assert!(!cache.dirty);

        let loaded =
            crate::test_runner::lang_rust::generation_publish::load_full_generation_witness(
                tmp.path(),
            )
            .expect("load full generation witness");
        assert_eq!(loaded.selectors, vec!["alpha".to_string(), "beta".to_string()]);
        assert_eq!(loaded.statuses[0], WitnessStatus::Passed);
        assert_eq!(loaded.statuses[1], WitnessStatus::Failed);
        assert!(!loaded.complete);
    }

    #[test]
    fn live_witness_persist_marks_complete_when_all_statuses_passed() {
        let _serial = begin_live_status_serial();
        let tmp = tempfile::TempDir::new().unwrap();
        let identity = kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity {
            input_digest: "inp-all-pass".into(),
            generation_fingerprint: "gen-all-pass".into(),
            selection_context_fingerprint: "ctx-all-pass".into(),
            ordinary_source_digests: BTreeMap::new(),
        };
        let mut cache = LiveWitnessCache::new(
            tmp.path(),
            &identity,
            None,
            &["a".into(), "b".into()],
            2,
        );
        cache.record_pass("a", "a", Duration::from_millis(1));
        cache.record_pass("b", "b", Duration::from_millis(2));
        cache.persist();

        let loaded =
            crate::test_runner::lang_rust::generation_publish::load_full_generation_witness(
                tmp.path(),
            )
            .expect("load full generation witness");
        assert!(
            loaded.complete,
            "all-Passed live persist must publish a complete witness"
        );
        assert_eq!(
            loaded.statuses,
            vec![WitnessStatus::Passed, WitnessStatus::Passed]
        );
    }

    #[test]
    fn install_live_hook_and_record_live_rust_pass_flow() {
        let _serial = begin_live_status_serial();
        let tmp = tempfile::TempDir::new().unwrap();
        let identity = kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity {
            input_digest: "inp789".into(),
            generation_fingerprint: "gen789".into(),
            selection_context_fingerprint: "ctx789".into(),
            ordinary_source_digests: BTreeMap::new(),
        };
        let last_identity = crate::test_runner::last_status::rust_last_status_identity(
            "c",
            "l",
            "r",
            "n",
            &[],
            "map",
        );
        let selectors = vec!["fast".to_string(), "slow".to_string()];
        install_live_rust_status_hook(
            tmp.path(),
            &selectors,
            &kiss::GateConfig::default(),
            &last_identity,
            Some(&identity),
            Some(&selectors),
            1,
            &BTreeMap::new(),
        )
        .unwrap();

        record_live_rust_pass("fast", "src/lib.rs::fast", Duration::from_millis(15));
        flush_live_rust_witness();

        let loaded =
            crate::test_runner::lang_rust::generation_publish::load_full_generation_witness(
                tmp.path(),
            )
            .expect("load full generation witness after flush");
        assert_eq!(loaded.selectors, vec!["fast".to_string(), "slow".to_string()]);
        assert_eq!(loaded.statuses[0], WitnessStatus::Passed);
        assert_eq!(loaded.durations_ns[0], Some(15_000_000));
        assert_eq!(loaded.statuses[1], WitnessStatus::Unresolved);
        assert!(!loaded.complete);
    }

    #[test]
    fn live_witness_pass_flushes_to_disk_on_explicit_flush() {
        let _serial = begin_live_status_serial();
        let tmp = tempfile::TempDir::new().unwrap();
        let identity = kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity {
            input_digest: "inp_mid".into(),
            generation_fingerprint: "gen_mid".into(),
            selection_context_fingerprint: "ctx_mid".into(),
            ordinary_source_digests: BTreeMap::new(),
        };
        let last_identity = crate::test_runner::last_status::rust_last_status_identity(
            "c",
            "l",
            "r",
            "n",
            &[],
            "map",
        );
        let selectors = vec!["done".to_string(), "pending".to_string()];
        install_live_rust_status_hook(
            tmp.path(),
            &selectors,
            &kiss::GateConfig::default(),
            &last_identity,
            Some(&identity),
            Some(&selectors),
            1,
            &BTreeMap::new(),
        )
        .unwrap();

        record_live_rust_pass("done", "src/lib.rs::done", Duration::from_millis(11));
        // Mid-run passes stay in memory (throttled persist) until flush/end.
        assert!(
            crate::test_runner::lang_rust::generation_publish::try_load_full_generation_witness(
                tmp.path(),
            )
            .is_none(),
            "single mid-run pass must not fsync the full witness"
        );
        flush_live_rust_witness();

        let loaded =
            crate::test_runner::lang_rust::generation_publish::load_full_generation_witness(
                tmp.path(),
            )
            .expect("flush must publish the live witness");
        assert_eq!(loaded.statuses[0], WitnessStatus::Passed);
        assert_eq!(loaded.durations_ns[0], Some(11_000_000));
        assert_eq!(loaded.statuses[1], WitnessStatus::Unresolved);
        assert!(!loaded.complete);
    }

    #[test]
    fn zero_sla_timeout_enters_last_status_before_batch() {
        let _serial = begin_live_status_serial();
        let tmp = tempfile::TempDir::new().unwrap();
        let last_identity = crate::test_runner::last_status::rust_last_status_identity(
            "c",
            "l",
            "r",
            "n",
            &[],
            "map",
        );
        let selectors = vec!["banned".to_string(), "runnable".to_string()];
        let timeouts = BTreeMap::from([("banned".to_string(), 0u64), ("runnable".to_string(), 5_000)]);
        let out = crate::test_runner::capture_stdout::capture_stdout(|| {
            install_live_rust_status_hook(
                tmp.path(),
                &selectors,
                &kiss::GateConfig::default(),
                &last_identity,
                None,
                None,
                1,
                &timeouts,
            )
            .unwrap();
        });
        assert!(
            out.contains("TIMEOUT: banned"),
            "zero-SLA ban must print TIMEOUT before the batch: {out}"
        );
        assert_eq!(
            crate::test_runner::last_status::prior_failures(
                tmp.path(),
                kiss::Language::Rust,
                &last_identity
            )
            .unwrap(),
            vec!["banned".to_string()],
            "zero-SLA TIMEOUT must be retry-bad eligible before the coverage batch runs"
        );
        assert_eq!(LIVE_REMAINING.load(Ordering::SeqCst), 1);
        assert!(kiss::rust_llvm_cov_runner::live_rust_was_printed("banned"));
        kiss::rust_llvm_cov_runner::clear_live_rust_test_hook();
        clear_live_rust_witness();
    }

    #[test]
    fn prepared_cache_hit_time_gate_timeout_enters_last_status_before_misses() {
        let _serial = begin_live_status_serial();
        let tmp = tempfile::TempDir::new().unwrap();
        let last_identity = crate::test_runner::last_status::rust_last_status_identity(
            "c",
            "l",
            "r",
            "n",
            &[],
            "map",
        );
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
            let mut pending_failures = HashSet::new();
            emit_prepared_cache_hit_statuses(
                &BTreeMap::new(),
                &gate,
                &mut LiveEmitState {
                    remaining: &mut remaining,
                    seen: &mut seen,
                    pending_failures: &mut pending_failures,
                    persist: Some((tmp.path(), &last_identity)),
                },
                &outcomes,
            );
        });
        assert!(
            out.contains("TIMEOUT") && out.contains("cached_over"),
            "prepare-time over-limit cache hit must print TIMEOUT before misses: {out}"
        );
        assert_eq!(
            crate::test_runner::last_status::prior_failures(
                tmp.path(),
                kiss::Language::Rust,
                &last_identity
            )
            .unwrap(),
            vec!["cached_over".to_string()],
            "prepare-time cache hits must apply time gate for retry-bad before miss batch"
        );
        assert_eq!(remaining, 1);
        assert!(kiss::rust_llvm_cov_runner::live_rust_was_printed("cached_over"));
    }
}
