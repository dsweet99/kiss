use crate::test_runner::execution_witness::{
    rust_identity_digest_from_batch, try_load_rust_execution_witness,
};
use crate::test_runner::lang_iface::{accept_witness, AcceptDecision, AcceptMode};
use kiss::GateConfig;

#[cfg(test)]
pub(super) fn rust_population_current_for_all_selectors(
    repo_root: &std::path::Path,
    selectors: &[String],
    gate: &GateConfig,
) -> bool {
    let cache_root = crate::test_runner::rust_coverage_index::rust_coverage_cache_root(repo_root);
    if !cache_root.join("population.json").is_file()
        && !cache_root.join("execution_witness.json").is_file()
    {
        return false;
    }
    let identity_started = std::time::Instant::now();
    let Ok(identity) =
        crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity(
            repo_root,
            &[],
        )
    else {
        crate::test_runner::emit_stage_time("rust_identity", identity_started.elapsed());
        return false;
    };
    crate::test_runner::emit_stage_time("rust_identity", identity_started.elapsed());
    rust_current_selectors(repo_root, selectors, &identity, gate).is_some()
}

pub(super) fn rust_plan_selectors(
    repo_root: &std::path::Path,
    selectors: Vec<String>,
    gate: &GateConfig,
) -> (Vec<String>, bool) {
    if selectors.is_empty() {
        return (selectors, false);
    }
    let cache_root = crate::test_runner::rust_coverage_index::rust_coverage_cache_root(repo_root);
    if !cache_root.join("population.json").is_file()
        && !cache_root.join("execution_witness.json").is_file()
    {
        return (selectors, true);
    }
    let identity_started = std::time::Instant::now();
    let identity =
        crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity(
            repo_root,
            &[],
        )
        .ok();
    crate::test_runner::emit_stage_time("rust_identity", identity_started.elapsed());
    if let Some(identity) = identity.as_ref()
        && let Some(current) = rust_current_selectors(repo_root, &selectors, identity, gate)
    {
        return (current, false);
    }
    if let Some(pop) = population_cache_selectors(repo_root)
        && selectors_are_subset(&pop, &selectors)
    {
        return (pop, false);
    }
    (selectors, true)
}

fn population_cache_selectors(repo_root: &std::path::Path) -> Option<Vec<String>> {
    let path = crate::test_runner::rust_coverage_index::rust_coverage_cache_root(repo_root)
        .join("population.json");
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    let selectors: Vec<String> = value
        .get("selectors")?
        .as_array()?
        .iter()
        .filter_map(|item| item.as_str().map(str::to_string))
        .collect();
    (!selectors.is_empty()).then_some(selectors)
}

fn rust_current_selectors(
    repo_root: &std::path::Path,
    selectors: &[String],
    identity: &kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity,
    gate: &GateConfig,
) -> Option<Vec<String>> {
    let mut expected = selectors.to_vec();
    expected.sort();
    expected.dedup();
    let cache_root = crate::test_runner::rust_coverage_index::rust_coverage_cache_root(repo_root);
    if kiss::rust_llvm_cov_runner::load_current_population_state(
        &cache_root,
        repo_root,
        identity,
        Some(&expected),
    )
    .is_some()
    {
        return Some(expected);
    }
    if let Some(state) = kiss::rust_llvm_cov_runner::load_current_population_state(
        &cache_root,
        repo_root,
        identity,
        None,
    ) && selectors_are_subset(&state.selectors, &expected)
    {
        return Some(state.selectors);
    }
    if rust_witness_accepts_full_universe(repo_root, &expected, identity, gate) {
        return Some(expected);
    }
    complete_witness_subset_plan(repo_root, &expected, identity, gate)
}

fn selectors_are_subset(inner: &[String], outer: &[String]) -> bool {
    if inner.is_empty() {
        return false;
    }
    let outer: std::collections::BTreeSet<&str> = outer.iter().map(String::as_str).collect();
    inner.iter().all(|selector| outer.contains(selector.as_str()))
}

fn complete_witness_subset_plan(
    repo_root: &std::path::Path,
    planned: &[String],
    identity: &kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity,
    _gate: &GateConfig,
) -> Option<Vec<String>> {
    let witness = try_load_rust_execution_witness(repo_root).ok()?;
    let current = rust_identity_digest_from_batch(identity);
    if !witness.complete {
        return None;
    }
    if !selectors_are_subset(&witness.selectors, planned) {
        return None;
    }
    (accept_witness(AcceptMode::All, &witness.selectors, &current, &witness)
        == AcceptDecision::Accept)
        .then_some(witness.selectors)
}

fn rust_witness_accepts_full_universe(
    repo_root: &std::path::Path,
    selectors: &[String],
    identity: &kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity,
    _gate: &GateConfig,
) -> bool {
    let Ok(witness) = try_load_rust_execution_witness(repo_root) else {
        return false;
    };
    let current = rust_identity_digest_from_batch(identity);
    if !witness.complete {
        return false;
    }
    accept_witness(AcceptMode::All, selectors, &current, &witness) == AcceptDecision::Accept
}

#[cfg(test)]
mod tests {
    use super::{
        rust_plan_selectors, rust_population_current_for_all_selectors,
        rust_witness_accepts_full_universe,
    };
    use crate::test_runner::execution_witness::{
        publish_rust_execution_witness, rust_identity_digest_from_batch, PublishRustWitness,
        WitnessScope, WitnessStatus,
    };
    use kiss::GateConfig;
    use std::collections::{BTreeMap, BTreeSet};

    fn identity() -> kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity {
        kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity {
            input_digest: "i".into(),
            generation_fingerprint: "g".into(),
            selection_context_fingerprint: "s".into(),
            ordinary_source_digests: Default::default(),
        }
    }

    #[test]
    fn rust_witness_accept_helpers_fail_closed_without_witness() {
        let tmp = tempfile::tempdir().unwrap();
        let id = identity();
        let selectors = vec!["a".into()];
        let gate = GateConfig::default();
        assert!(!rust_witness_accepts_full_universe(
            tmp.path(),
            &selectors,
            &id,
            &gate
        ));
        assert!(!rust_population_current_for_all_selectors(
            tmp.path(),
            &selectors,
            &gate
        ));
    }

    #[test]
    fn rust_population_current_unreadable_population_without_witness_is_not_current() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path().join(".kiss").join("rust_llvm_cov_cache");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("population.json"), b"{}").unwrap();
        assert!(!rust_population_current_for_all_selectors(
            tmp.path(),
            &["a".into()],
            &GateConfig::default()
        ));
    }

    #[test]
    fn rust_population_current_when_published_population_matches() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
        )
        .unwrap();
        std::fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
        crate::test_runner::rust_coverage_index::write_rust_population_manifest_for_args(
            tmp.path(),
            &["a".into()],
            &[],
        )
        .unwrap();
        assert!(rust_population_current_for_all_selectors(
            tmp.path(),
            &["a".into()],
            &GateConfig::default()
        ));
    }

    #[test]
    fn rust_population_current_uses_matching_witness_when_population_unreadable() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
        )
        .unwrap();
        std::fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
        let id = crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity(
            tmp.path(),
            &[],
        )
        .unwrap();
        let cache = tmp.path().join(".kiss").join("rust_llvm_cov_cache");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("population.json"), b"{not-json").unwrap();
        let selectors = vec!["a".into(), "b".into()];
        let covered = BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([1u32]))]);
        publish_rust_execution_witness(PublishRustWitness {
            repo_root: tmp.path(),
            identity: &id,
            scope: WitnessScope::Full,
            selectors: &selectors,
            statuses: &[WitnessStatus::Passed, WitnessStatus::Passed],
            durations_ns: &[Some(1), Some(1)],
            covered_lines: &covered,
            complete: true,
        })
        .unwrap();
        assert!(rust_population_current_for_all_selectors(
            tmp.path(),
            &selectors,
            &GateConfig::default()
        ));
        let other = kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity {
            input_digest: "other".into(),
            generation_fingerprint: "g".into(),
            selection_context_fingerprint: "s".into(),
            ordinary_source_digests: Default::default(),
        };
        assert!(!rust_witness_accepts_full_universe(
            tmp.path(),
            &selectors,
            &other,
            &GateConfig::default()
        ));
    }

    #[test]
    fn rust_witness_accepts_complete_full_universe() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
        )
        .unwrap();
        std::fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
        let id = identity();
        let selectors = vec!["a".into(), "b".into()];
        let covered = BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([1u32]))]);
        publish_rust_execution_witness(PublishRustWitness {
            repo_root: tmp.path(),
            identity: &id,
            scope: WitnessScope::Full,
            selectors: &selectors,
            statuses: &[WitnessStatus::Passed, WitnessStatus::Passed],
            durations_ns: &[Some(1), Some(1)],
            covered_lines: &covered,
            complete: true,
        })
        .unwrap();
        assert!(!rust_identity_digest_from_batch(&id).is_empty());
        let gate = GateConfig::default();
        assert!(rust_witness_accepts_full_universe(
            tmp.path(),
            &selectors,
            &id,
            &gate
        ));

        publish_rust_execution_witness(PublishRustWitness {
            repo_root: tmp.path(),
            identity: &id,
            scope: WitnessScope::Full,
            selectors: &selectors,
            statuses: &[WitnessStatus::Passed, WitnessStatus::Unresolved],
            durations_ns: &[Some(1), Some(1)],
            covered_lines: &covered,
            complete: false,
        })
        .unwrap();
        assert!(!rust_witness_accepts_full_universe(
            tmp.path(),
            &selectors,
            &id,
            &gate
        ));
    }

    fn demo_lib(tmp: &tempfile::TempDir) {
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
        )
        .unwrap();
        std::fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    }

    #[test]
    fn rust_plan_selectors_uses_current_population_subset() {
        let tmp = tempfile::tempdir().unwrap();
        demo_lib(&tmp);
        crate::test_runner::rust_coverage_index::write_rust_population_manifest_for_args(
            tmp.path(),
            &["a".into()],
            &[],
        )
        .unwrap();
        let (planned, population_required) = rust_plan_selectors(
            tmp.path(),
            vec!["a".into(), "b".into()],
            &GateConfig::default(),
        );
        assert!(!population_required);
        assert_eq!(planned, vec!["a".to_string()]);
    }

    #[test]
    fn rust_plan_selectors_uses_complete_witness_subset() {
        let tmp = tempfile::tempdir().unwrap();
        demo_lib(&tmp);
        let id = crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity(
            tmp.path(),
            &[],
        )
        .unwrap();
        let cache = tmp.path().join(".kiss").join("rust_llvm_cov_cache");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("population.json"), b"{not-json").unwrap();
        let covered = BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([1u32]))]);
        publish_rust_execution_witness(PublishRustWitness {
            repo_root: tmp.path(),
            identity: &id,
            scope: WitnessScope::Full,
            selectors: &["a".into(), "b".into()],
            statuses: &[WitnessStatus::Passed, WitnessStatus::Passed],
            durations_ns: &[Some(1), Some(1)],
            covered_lines: &covered,
            complete: true,
        })
        .unwrap();
        let (planned, population_required) = rust_plan_selectors(
            tmp.path(),
            vec!["a".into(), "b".into(), "c".into()],
            &GateConfig::default(),
        );
        assert!(!population_required);
        assert_eq!(planned, vec!["a".to_string(), "b".to_string()]);
    }
}
