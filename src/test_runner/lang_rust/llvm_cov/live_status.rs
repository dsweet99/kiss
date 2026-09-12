use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use kiss::rpytest_runner::TestStatus;
use kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity;

use crate::test_runner::execution_witness::{
    PublishRustWitness, WitnessScope, WitnessStatus, publish_rust_execution_witness,
};
use crate::test_runner::last_status::{LastStatusIdentity, record_statuses};

static LIVE_REMAINING: AtomicUsize = AtomicUsize::new(0);
static LIVE_WITNESS: Mutex<Option<LiveWitnessCache>> = Mutex::new(None);

pub(crate) struct LiveWitnessCache {
    repo_root: PathBuf,
    identity: RustCoverageBatchIdentity,
    selectors: Vec<String>,
    statuses: Vec<WitnessStatus>,
    durations_ns: Vec<Option<u64>>,
    covered_lines: BTreeMap<String, BTreeSet<u32>>,
    selector_indices: BTreeMap<String, usize>,
    dirty: bool,
    jobs: usize,
}

impl LiveWitnessCache {
    pub(crate) fn new(
        repo_root: &Path,
        batch_identity: &RustCoverageBatchIdentity,
        population_selectors: Option<&[String]>,
        fallback_selectors: &[String],
        jobs: usize,
    ) -> Self {
        let existing = super::witness::load_matching_full_witness(repo_root, batch_identity);
        let mut universe: Vec<String> = match population_selectors {
            Some(pop) => pop.to_vec(),
            None => fallback_selectors.to_vec(),
        };
        if let Some(existing) = existing.as_ref() {
            for sel in &existing.selectors {
                if !universe.contains(sel) {
                    universe.push(sel.clone());
                }
            }
        }
        for sel in fallback_selectors {
            if !universe.contains(sel) {
                universe.push(sel.clone());
            }
        }
        universe.sort();
        universe.dedup();

        let mut statuses = vec![WitnessStatus::Unresolved; universe.len()];
        let mut durations_ns = vec![None; universe.len()];
        let mut covered_lines = BTreeMap::new();

        if let Some(existing) = existing.as_ref() {
            let existing_idx: BTreeMap<&str, usize> = existing
                .selectors
                .iter()
                .enumerate()
                .map(|(i, s)| (s.as_str(), i))
                .collect();
            for (i, sel) in universe.iter().enumerate() {
                if let Some(&ei) = existing_idx.get(sel.as_str()) {
                    statuses[i] = existing.statuses[ei];
                    durations_ns[i] = existing.durations_ns[ei];
                }
            }
            for (path, lines) in &existing.covered_lines {
                covered_lines.insert(path.clone(), lines.iter().copied().collect());
            }
        }

        let selector_indices: BTreeMap<String, usize> = universe
            .iter()
            .enumerate()
            .map(|(i, s)| (s.clone(), i))
            .collect();

        Self {
            repo_root: repo_root.to_path_buf(),
            identity: batch_identity.clone(),
            selectors: universe,
            statuses,
            durations_ns,
            covered_lines,
            selector_indices,
            dirty: false,
            jobs,
        }
    }

    pub(crate) fn record_pass(&mut self, logical: &str, report: &str, duration: Duration) {
        let idx = self
            .selector_indices
            .get(logical)
            .or_else(|| self.selector_indices.get(report))
            .copied()
            .unwrap_or_else(|| {
                let i = self.selectors.len();
                self.selectors.push(logical.to_string());
                self.statuses.push(WitnessStatus::Unresolved);
                self.durations_ns.push(None);
                self.selector_indices.insert(logical.to_string(), i);
                i
            });
        self.statuses[idx] = WitnessStatus::Passed;
        self.durations_ns[idx] = Some(duration.as_nanos() as u64);
        self.dirty = true;
    }

    pub(crate) fn record_non_pass(&mut self, logical: &str, report: &str, status: WitnessStatus) {
        let idx = self
            .selector_indices
            .get(logical)
            .or_else(|| self.selector_indices.get(report))
            .copied()
            .unwrap_or_else(|| {
                let i = self.selectors.len();
                self.selectors.push(logical.to_string());
                self.statuses.push(WitnessStatus::Unresolved);
                self.durations_ns.push(None);
                self.selector_indices.insert(logical.to_string(), i);
                i
            });
        self.statuses[idx] = status;
        self.dirty = true;
    }

    pub(crate) fn persist(&mut self) {
        if !self.dirty {
            return;
        }
        let _ = publish_rust_execution_witness(PublishRustWitness {
            repo_root: &self.repo_root,
            identity: &self.identity,
            scope: WitnessScope::Full,
            selectors: &self.selectors,
            statuses: &self.statuses,
            durations_ns: &self.durations_ns,
            covered_lines: &self.covered_lines,
            complete: false,
            jobs: self.jobs,
        });
        self.dirty = false;
    }
}

pub(super) fn record_live_rust_pass(logical: &str, report: &str, duration: Duration) {
    if let Ok(mut guard) = LIVE_WITNESS.lock()
        && let Some(cache) = guard.as_mut()
    {
        cache.record_pass(logical, report, duration);
    }
}

pub(super) fn record_live_rust_non_pass(logical: &str, report: &str, status: WitnessStatus) {
    if let Ok(mut guard) = LIVE_WITNESS.lock()
        && let Some(cache) = guard.as_mut()
    {
        cache.record_non_pass(logical, report, status);
    }
}

pub(super) fn flush_live_rust_witness() {
    if let Ok(mut guard) = LIVE_WITNESS.lock()
        && let Some(mut cache) = guard.take()
    {
        cache.persist();
    }
}

pub(super) fn clear_live_rust_witness() {
    if let Ok(mut guard) = LIVE_WITNESS.lock() {
        *guard = None;
    }
}

pub(super) fn install_live_rust_status_hook(
    repo_root: &Path,
    selectors: &[String],
    gate: &kiss::GateConfig,
    identity: &LastStatusIdentity,
    batch_identity: Option<&RustCoverageBatchIdentity>,
    population_selectors: Option<&[String]>,
    jobs: usize,
) -> Result<(), String> {
    if let Some(batch_id) = batch_identity {
        let cache = LiveWitnessCache::new(repo_root, batch_id, population_selectors, selectors, jobs);
        if let Ok(mut guard) = LIVE_WITNESS.lock() {
            *guard = Some(cache);
        }
    }
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

    if let Some((repo_root, identity)) = state.persist
        && matches!(raw, TestStatus::Failed | TestStatus::TimedOut)
    {
        let _ = record_statuses(
            repo_root,
            kiss::Language::Rust,
            identity,
            &[(logical.to_string(), raw)],
        );
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
        LiveEmitState, LiveWitnessCache, emit_one_live_status, flush_live_rust_witness,
        install_live_rust_status_hook, kiss_id_for_libtest, record_live_rust_pass,
        status_from_libtest_event,
    };
    use crate::test_runner::lang_iface::WitnessStatus;
    use crate::test_runner::last_status::LastStatusIdentity;
    use kiss::rpytest_runner::TestStatus;
    use std::collections::{BTreeMap, HashSet};
    use std::path::Path;
    use std::time::Duration;

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

        // Nextest qualified prefix resolution
        ids.insert(
            "tests::test_cache".into(),
            "src/check_cache.rs::test_cache".into(),
        );
        assert_eq!(
            kiss_id_for_libtest(&ids, "kiss-ai::kiss$check_cache::tests::test_cache").as_deref(),
            Some("src/check_cache.rs::test_cache")
        );

        // Disambiguate by key length / specificity
        ids.insert("run".into(), "src/alpha.rs::run".into());
        ids.insert("extra::run".into(), "src/beta.rs::run".into());
        assert_eq!(
            kiss_id_for_libtest(&ids, "pkg$gamma::extra::run").as_deref(),
            Some("src/beta.rs::run")
        );

        // Disambiguate by source file path
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
    fn live_witness_cache_records_pass_and_persists_to_disk() {
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
    fn live_witness_cache_records_non_pass_and_flush() {
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
    fn install_live_hook_and_record_live_rust_pass_flow() {
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
}
