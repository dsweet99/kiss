use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::time::Duration;

use kiss::rpytest_runner::TestStatus;

pub(super) fn install_live_rust_status_hook(
    repo_root: &Path,
    selectors: &[String],
    gate: &kiss::GateConfig,
) -> Result<(), String> {
    let report_ids = crate::test_runner::runners::rust_logical_to_kiss_test_ids(repo_root, &[])?;
    let gate = gate.clone();
    let mut remaining = selectors.len();
    let mut seen = HashSet::new();
    kiss::rust_llvm_cov_runner::install_live_rust_test_hook(move |name, event, exec_time| {
        emit_one_live_status(
            &report_ids,
            &gate,
            &mut remaining,
            &mut seen,
            name,
            event,
            exec_time,
        );
    });
    Ok(())
}

fn emit_one_live_status(
    report_ids: &BTreeMap<String, String>,
    gate: &kiss::GateConfig,
    remaining: &mut usize,
    seen: &mut HashSet<String>,
    name: &str,
    event: &str,
    exec_time: f64,
) {
    let Some(report) = kiss_id_for_libtest(report_ids, name) else {
        kiss::rust_llvm_cov_runner::set_live_rust_error(format!(
            "error: kiss: missing PATH::symbol report id for rust selector `{name}`"
        ));
        return;
    };
    let Some(raw) = status_from_libtest_event(event) else {
        return;
    };
    if !seen.insert(report.clone()) {
        return;
    }
    kiss::rust_llvm_cov_runner::mark_live_rust_printed(&report);
    let duration = Duration::from_secs_f64(exec_time.max(0.0));
    let status =
        crate::test_runner::status_labels::apply_unit_test_time_limit(raw, &report, duration, gate);
    crate::test_runner::status_labels::print_classified_status_line(
        status, &report, duration, None, true,
    );
    *remaining = remaining.saturating_sub(1);
    crate::test_runner::emit_test_progress(&format!("kiss test: tests_remaining={remaining}"));
}

fn kiss_id_for_libtest(report_ids: &BTreeMap<String, String>, name: &str) -> Option<String> {
    let logical = name.rsplit_once('$').map_or(name, |(_, test)| test);
    if let Some(id) = report_ids.get(logical) {
        return Some(id.clone());
    }
    report_ids
        .iter()
        .find(|(key, _)| key.as_str() == logical || key.ends_with(logical))
        .map(|(_, id)| id.clone())
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
        emit_one_live_status, install_live_rust_status_hook, kiss_id_for_libtest,
        status_from_libtest_event,
    };
    use kiss::rpytest_runner::TestStatus;
    use std::collections::{BTreeMap, HashSet};

    #[test]
    fn maps_libtest_suffix_and_event() {
        let mut ids = BTreeMap::new();
        ids.insert("case".into(), "src/lib.rs::case".into());
        ids.insert("nested::case".into(), "src/lib.rs::nested".into());
        ids.insert("outer::long_name".into(), "src/lib.rs::long".into());
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
        )
        .unwrap();
        kiss::rust_llvm_cov_runner::clear_live_rust_test_hook();

        let gate = kiss::GateConfig::default();
        let mut remaining = 1;
        let mut seen = HashSet::new();
        emit_one_live_status(
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
    fn emit_live_status_dedups_and_fails_unknown() {
        let mut ids = BTreeMap::new();
        ids.insert("case".into(), "src/lib.rs::case".into());
        let gate = kiss::GateConfig::default();
        let mut remaining = 2;
        let mut seen = HashSet::new();
        let _ = kiss::rust_llvm_cov_runner::take_live_rust_error();
        emit_one_live_status(
            &ids,
            &gate,
            &mut remaining,
            &mut seen,
            "pkg::bin$missing",
            "ok",
            0.1,
        );
        assert!(
            kiss::rust_llvm_cov_runner::take_live_rust_error()
                .is_some_and(|err| err.contains("missing PATH::symbol")),
            "unmapped libtest names must fail fast"
        );
        assert_eq!(remaining, 2);
        emit_one_live_status(
            &ids,
            &gate,
            &mut remaining,
            &mut seen,
            "pkg::bin$case",
            "started",
            0.1,
        );
        assert_eq!(remaining, 2);
        emit_one_live_status(
            &ids,
            &gate,
            &mut remaining,
            &mut seen,
            "pkg::bin$case",
            "ok",
            0.2,
        );
        assert_eq!(remaining, 1);
        emit_one_live_status(
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
        let mut ids = BTreeMap::new();
        ids.insert("case".into(), "src/lib.rs::case".into());
        let gate = kiss::GateConfig {
            max_unit_test_seconds: vec![("*".into(), 0.0)],
            ..kiss::GateConfig::default()
        };
        let mut remaining = 1;
        let mut seen = HashSet::new();
        let out = crate::test_runner::capture_stdout::capture_stdout(|| {
            emit_one_live_status(
                &ids,
                &gate,
                &mut remaining,
                &mut seen,
                "pkg::bin$case",
                "ok",
                1.0,
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
}
