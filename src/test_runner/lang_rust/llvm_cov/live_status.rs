use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use kiss::rpytest_runner::TestStatus;

use crate::test_runner::last_status::{LastStatusIdentity, record_statuses};

static LIVE_REMAINING: AtomicUsize = AtomicUsize::new(0);

pub(super) fn install_live_rust_status_hook(
    repo_root: &Path,
    selectors: &[String],
    gate: &kiss::GateConfig,
    identity: &LastStatusIdentity,
) -> Result<(), String> {
    let report_ids =
        crate::test_runner::rust_report_id_cache::rust_logical_to_kiss_test_ids_cached(
            repo_root,
            &[],
        )?;
    let gate = gate.clone();
    let repo_root = repo_root.to_path_buf();
    let identity = identity.clone();
    let mut remaining = selectors.len();
    LIVE_REMAINING.store(remaining, Ordering::SeqCst);
    let mut seen = HashSet::new();
    kiss::rust_llvm_cov_runner::install_live_rust_test_hook(move |name, event, exec_time| {
        emit_one_live_status(
            &report_ids,
            &gate,
            &mut LiveEmitState {
                remaining: &mut remaining,
                seen: &mut seen,
                persist: Some((repo_root.as_path(), &identity)),
            },
            name,
            event,
            exec_time,
        );
    });
    Ok(())
}

pub(super) fn finish_live_rust_remaining() {
    if LIVE_REMAINING.swap(0, Ordering::SeqCst) > 0 {
        crate::test_runner::tests_remaining::emit_tests_remaining(0);
    }
}

struct LiveEmitState<'a> {
    remaining: &'a mut usize,
    seen: &'a mut HashSet<String>,
    persist: Option<(&'a Path, &'a LastStatusIdentity)>,
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
    if let Some((repo_root, identity)) = state.persist
        && matches!(raw, TestStatus::Failed | TestStatus::TimedOut)
    {
        let logical = report_ids
            .iter()
            .find(|(_, id)| *id == &report)
            .map(|(key, _)| key.clone())
            .unwrap_or_else(|| report.clone());
        let _ = record_statuses(repo_root, kiss::Language::Rust, identity, &[(logical, raw)]);
    }
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
    let mut matches = report_ids
        .iter()
        .filter(|(key, _)| key.ends_with(&suffix))
        .map(|(_, id)| id.clone());
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
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
        LiveEmitState, emit_one_live_status, install_live_rust_status_hook, kiss_id_for_libtest,
        status_from_libtest_event,
    };
    use crate::test_runner::last_status::LastStatusIdentity;
    use kiss::rpytest_runner::TestStatus;
    use std::collections::{BTreeMap, HashSet};
    use std::path::Path;

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
        emit_one_live_status(
            ids,
            gate,
            &mut LiveEmitState {
                remaining,
                seen,
                persist,
            },
            name,
            event,
            exec_time,
        );
    }

    #[test]
    fn maps_libtest_suffix_and_event() {
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
        let src = include_str!("live_status.rs");
        let code = src.split("mod live_status_test").next().expect("prod src");
        assert!(
            !code.contains("cancel_active_batch_scope"),
            "unmapped live rust names must not cancel the batch (peer Python SIGPIPE)"
        );
    }

    #[test]
    fn emit_live_status_dedups_and_skips_unknown() {
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
        let mut ids = BTreeMap::new();
        ids.insert("case".into(), "src/lib.rs::case".into());
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
                "pkg::bin$case",
                "ok",
                1.0,
                None,
            );
        });
        assert!(
            out.contains("TIMEOUT: src/lib.rs::case"),
            "over-limit ok must print TIMEOUT: {out}"
        );
        assert!(
            !out.contains("PASS: src/lib.rs::case"),
            "over-limit ok must not print PASS: {out}"
        );
    }

    #[test]
    fn live_failure_persists_last_status_immediately() {
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
}
