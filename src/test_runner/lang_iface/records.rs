use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use kiss::rpytest_runner::TestStatus;
use kiss::test_records::{RECORD_SCHEMA, TestRecord, load_records, records_dir, store_record};

const INPUTS_DEP: &str = "inputs";
const TIMEOUT_DEP: &str = "timeout_ms";
const DECLARED_INPUTS_DEP: &str = "declared_inputs";

pub(crate) fn digest(bytes: &[u8]) -> String {
    format!(
        "{:016x}",
        crate::analyze_cache::fnv1a64(0xcbf2_9ce4_8422_2325, bytes)
    )
}

pub(crate) fn cache_policy(repo_root: &Path) -> kiss::test_cache_policy::TestCachePolicy {
    kiss::TestSectionConfig::try_load_path_only(&kiss::kissconfig_path_for_repo(repo_root))
        .map(|config| config.cache_policy)
        .unwrap_or_default()
}

pub(crate) fn declared_inputs_digest(
    repo_root: &Path,
    policy: &kiss::test_cache_policy::TestCachePolicy,
    test_id: &str,
) -> Option<String> {
    let paths = policy.declared_paths(test_id);
    if paths.is_empty() {
        return None;
    }
    let mut bytes = Vec::new();
    for path in paths {
        bytes.extend_from_slice(path.as_bytes());
        bytes.push(0);
        bytes.extend(std::fs::read(repo_root.join(&path)).unwrap_or_default());
        bytes.push(0);
    }
    Some(digest(&bytes))
}

fn timeout_text(timeout_ms: Option<u64>) -> String {
    timeout_ms.map_or_else(|| "none".to_string(), |ms| ms.to_string())
}

fn input_deps(inputs: &str, declared: Option<String>) -> BTreeMap<String, String> {
    let mut deps = BTreeMap::from([(INPUTS_DEP.to_string(), inputs.to_string())]);
    if let Some(declared) = declared {
        deps.insert(DECLARED_INPUTS_DEP.to_string(), declared);
    }
    deps
}

fn record_deps(inputs: &str, outcome: &Outcome<'_>) -> BTreeMap<String, String> {
    let mut deps = input_deps(inputs, outcome.declared_inputs.clone());
    if outcome.status == TestStatus::TimedOut {
        deps.insert(TIMEOUT_DEP.to_string(), timeout_text(outcome.timeout_ms));
    }
    deps
}

pub(crate) fn current_deps(
    inputs: &str,
    declared: Option<String>,
    row: &TestRecord,
    timeout_ms: impl FnOnce() -> Option<u64>,
) -> BTreeMap<String, String> {
    let mut deps = input_deps(inputs, declared);
    if row.deps.contains_key(TIMEOUT_DEP) {
        deps.insert(TIMEOUT_DEP.to_string(), timeout_text(timeout_ms()));
    } else if let Some(ms) = timeout_ms()
        && row.duration.as_millis() > u128::from(ms)
    {
        deps.insert(TIMEOUT_DEP.to_string(), timeout_text(Some(ms)));
    }
    deps
}

pub(crate) struct Outcome<'a> {
    pub(crate) test_id: &'a str,
    pub(crate) status: TestStatus,
    pub(crate) duration: Duration,
    pub(crate) timeout_ms: Option<u64>,
    pub(crate) declared_inputs: Option<String>,
}

#[derive(Clone, Copy)]
pub(crate) struct RecordScope<'a> {
    pub(crate) repo_root: &'a Path,
    pub(crate) language: &'a str,
    pub(crate) identity: &'a str,
}

pub(crate) fn store(
    scope: RecordScope<'_>,
    inputs: &str,
    outcome: &Outcome<'_>,
) -> Result<(), String> {
    let dir = records_dir(scope.repo_root, scope.language);
    std::fs::create_dir_all(&dir)
        .map_err(|err| format!("error: kiss: create {}: {err}", dir.display()))?;
    let record = TestRecord {
        schema: RECORD_SCHEMA.to_string(),
        language: scope.language.to_string(),
        test_id: outcome.test_id.to_string(),
        identity: scope.identity.to_string(),
        deps: record_deps(inputs, outcome),
        status: outcome.status,
        exit_code: Some(i32::from(outcome.status != TestStatus::Passed)),
        duration: outcome.duration,
    };
    store_record(&dir, &record)
        .map_err(|err| format!("error: kiss: store {} test record: {err}", scope.language))
}

pub(crate) fn records_under(scope: RecordScope<'_>) -> Vec<TestRecord> {
    load_records(&records_dir(scope.repo_root, scope.language))
        .into_iter()
        .filter(|record| record.identity == scope.identity)
        .collect()
}

pub(crate) fn records_all_mode_plan(
    repo_root: &Path,
    language: &str,
    mut selectors: Vec<String>,
) -> super::AllModePlan {
    selectors.sort();
    selectors.dedup();
    let population_required = !selectors.is_empty() && !records_dir(repo_root, language).is_dir();
    super::AllModePlan {
        planned: selectors,
        population_required,
    }
}

pub(crate) fn record_misses(
    planned: &[String],
    witness: Option<&super::ExecutionWitness>,
) -> Vec<String> {
    let held: std::collections::BTreeSet<&str> = witness
        .map(|witness| witness.selectors.iter().map(String::as_str).collect())
        .unwrap_or_default();
    planned
        .iter()
        .filter(|selector| !held.contains(selector.as_str()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope<'a>(root: &'a Path, identity: &'a str) -> RecordScope<'a> {
        RecordScope {
            repo_root: root,
            language: "rust",
            identity,
        }
    }

    #[test]
    fn stored_records_round_trip_and_timeouts_track_their_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let outcome = Outcome {
            test_id: "a::slow",
            status: TestStatus::TimedOut,
            duration: Duration::from_millis(3001),
            timeout_ms: Some(3000),
            declared_inputs: None,
        };
        store(scope(root, "id"), "inputs", &outcome).unwrap();
        store(
            scope(root, "id"),
            "inputs",
            &Outcome {
                test_id: "a::ok",
                status: TestStatus::Passed,
                duration: Duration::from_millis(5),
                timeout_ms: Some(3000),
                declared_inputs: Some("d1".into()),
            },
        )
        .unwrap();
        let mut rows = records_under(scope(root, "id"));
        rows.sort_by(|a, b| a.test_id.cmp(&b.test_id));
        assert_eq!(rows.len(), 2);
        assert!(records_under(scope(root, "other")).is_empty());
        let (ok, slow) = (&rows[0], &rows[1]);
        let d1 = || Some("d1".to_string());
        assert_eq!(ok.deps, current_deps("inputs", d1(), ok, || Some(5)));
        assert_ne!(ok.deps, current_deps("inputs", d1(), ok, || Some(4)));
        assert_ne!(
            ok.deps,
            current_deps("inputs", Some("d2".into()), ok, || Some(5))
        );
        assert_ne!(ok.deps, current_deps("inputs", None, ok, || Some(5)));
        assert_eq!(slow.deps, current_deps("inputs", None, slow, || Some(3000)));
        assert_ne!(slow.deps, current_deps("inputs", None, slow, || Some(6000)));
        assert_ne!(ok.deps, current_deps("changed", d1(), ok, || None));
        assert_eq!(slow.exit_code, Some(1));
    }

    #[test]
    fn population_is_required_only_before_any_record_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let plan =
            records_all_mode_plan(tmp.path(), "rust", vec!["b".into(), "a".into(), "a".into()]);
        assert_eq!(plan.planned, ["a", "b"]);
        assert!(plan.population_required);
        assert!(!records_all_mode_plan(tmp.path(), "rust", Vec::new()).population_required);
        std::fs::create_dir_all(records_dir(tmp.path(), "rust")).unwrap();
        assert!(!records_all_mode_plan(tmp.path(), "rust", vec!["a".into()]).population_required);
        assert!(records_all_mode_plan(tmp.path(), "python", vec!["a".into()]).population_required);
    }

    #[test]
    fn declared_inputs_digest_tracks_the_declared_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("data.txt"), "a").unwrap();
        let mut table = toml::Table::new();
        table.insert(
            "inputs".into(),
            toml::Value::try_from(vec![toml::toml! {
                selectors = ["t::uses_data"]
                paths = ["data.txt"]
            }])
            .unwrap(),
        );
        let policy =
            kiss::test_cache_policy::TestCachePolicy::parse_table(&table, Some(root)).unwrap();
        assert_eq!(declared_inputs_digest(root, &policy, "t::other"), None);
        let before = declared_inputs_digest(root, &policy, "t::uses_data").unwrap();
        std::fs::write(root.join("data.txt"), "b").unwrap();
        assert_ne!(
            Some(before),
            declared_inputs_digest(root, &policy, "t::uses_data")
        );
    }
}
