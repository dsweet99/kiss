use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use kiss::rpytest_runner::TestStatus;
use kiss::test_records::{RECORD_SCHEMA, TestRecord, load_records, records_dir, store_record};

const IDENTITY_SCHEMA: &str = "kiss-rust-nextest-record-v1";
const RUST_INPUTS_DEP: &str = "rust_inputs";
const TIMEOUT_DEP: &str = "timeout_ms";
const DECLARED_INPUTS_DEP: &str = "declared_inputs";

fn digest(bytes: &[u8]) -> String {
    format!(
        "{:016x}",
        crate::analyze_cache::fnv1a64(0xcbf2_9ce4_8422_2325, bytes)
    )
}

/// The test arguments that change which tests run or what they do; `--nocapture` only
/// changes where their output goes.
fn identity_args(extras: &[String]) -> Vec<&str> {
    extras
        .iter()
        .map(String::as_str)
        .filter(|arg| !matches!(*arg, "--nocapture" | "--no-capture"))
        .collect()
}

/// What every Rust record is stored under: the toolchain, the test arguments, and the
/// build environment. A record made under another identity never holds.
pub(crate) fn record_identity(repo_root: &Path, extras: &[String]) -> Result<String, String> {
    let toolchain = super::toolchain::current_rust_toolchain(repo_root)?;
    let payload = serde_json::json!({
        "schema": IDENTITY_SCHEMA,
        "toolchain": toolchain,
        "args": identity_args(extras),
        "env": super::env::identity_env(),
    });
    Ok(format!(
        "rust-nextest:{}",
        digest(payload.to_string().as_bytes())
    ))
}

/// One digest of every Rust input in the repository: sources, `include!`/`#[path]`
/// files, manifests, lockfiles, cargo config, and toolchain files.
pub(crate) fn rust_inputs_digest(repo_root: &Path) -> Result<String, String> {
    let sources =
        crate::test_runner::workspace_selector_cache::rust_full_source_fingerprint(repo_root, &[])
            .map_err(|err| format!("error: kiss: fingerprint Rust sources: {err}"))?;
    let included = crate::test_runner::lang_rust::rust_expanded_include_extras_fingerprint(
        repo_root,
        |parts| digest(&parts.concat()),
    );
    Ok(digest(format!("{sources}\0{included}").as_bytes()))
}

/// The `[test.cache]` policy of the repository's `.kissconfig`.
pub(crate) fn cache_policy(repo_root: &Path) -> kiss::test_cache_policy::TestCachePolicy {
    kiss::TestSectionConfig::try_load_path_only(&kiss::kissconfig_path_for_repo(repo_root))
        .map(|config| config.cache_policy)
        .unwrap_or_default()
}

/// One digest of the files `[test.cache] inputs` declares for `test_id`; `None` when it
/// declares none.
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
    let mut deps = BTreeMap::from([(RUST_INPUTS_DEP.to_string(), inputs.to_string())]);
    if let Some(declared) = declared {
        deps.insert(DECLARED_INPUTS_DEP.to_string(), declared);
    }
    deps
}

/// The dependencies a record keeps. A timed-out test also depends on its time limit,
/// so raising the limit runs it again.
fn record_deps(inputs: &str, outcome: &Outcome<'_>) -> BTreeMap<String, String> {
    let mut deps = input_deps(inputs, outcome.declared_inputs.clone());
    if outcome.status == TestStatus::TimedOut {
        deps.insert(TIMEOUT_DEP.to_string(), timeout_text(outcome.timeout_ms));
    }
    deps
}

/// The current values of the dependencies `row` recorded. A record that ran longer
/// than today's time limit gains a limit it never recorded, so it runs again.
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

/// One finished test, ready to be stored.
pub(crate) struct Outcome<'a> {
    pub(crate) test_id: &'a str,
    pub(crate) status: TestStatus,
    pub(crate) duration: Duration,
    pub(crate) timeout_ms: Option<u64>,
    pub(crate) declared_inputs: Option<String>,
}

pub(crate) fn store(
    repo_root: &Path,
    identity: &str,
    inputs: &str,
    outcome: &Outcome<'_>,
) -> Result<(), String> {
    let dir = records_dir(repo_root, "rust");
    std::fs::create_dir_all(&dir)
        .map_err(|err| format!("error: kiss: create {}: {err}", dir.display()))?;
    let record = TestRecord {
        schema: RECORD_SCHEMA.to_string(),
        language: "rust".to_string(),
        test_id: outcome.test_id.to_string(),
        identity: identity.to_string(),
        deps: record_deps(inputs, outcome),
        status: outcome.status,
        exit_code: Some(i32::from(outcome.status != TestStatus::Passed)),
        duration: outcome.duration,
        covered: BTreeMap::new(),
    };
    store_record(&dir, &record).map_err(|err| format!("error: kiss: store Rust test record: {err}"))
}

/// The stored Rust records made under `identity`.
pub(crate) fn records_under(repo_root: &Path, identity: &str) -> Vec<TestRecord> {
    load_records(&records_dir(repo_root, "rust"))
        .into_iter()
        .filter(|record| record.identity == identity)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nocapture_does_not_change_identity_args() {
        let extras = vec!["--nocapture".to_string(), "--ignored".to_string()];
        assert_eq!(identity_args(&extras), ["--ignored"]);
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
        store(root, "id", "inputs", &outcome).unwrap();
        store(
            root,
            "id",
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
        let mut rows = records_under(root, "id");
        rows.sort_by(|a, b| a.test_id.cmp(&b.test_id));
        assert_eq!(rows.len(), 2);
        assert!(records_under(root, "other").is_empty());
        let (ok, slow) = (&rows[0], &rows[1]);
        let d1 = || Some("d1".to_string());
        assert_eq!(ok.deps, current_deps("inputs", d1(), ok, || Some(5)));
        assert_ne!(ok.deps, current_deps("inputs", d1(), ok, || Some(4)));
        assert_ne!(ok.deps, current_deps("inputs", Some("d2".into()), ok, || Some(5)));
        assert_ne!(ok.deps, current_deps("inputs", None, ok, || Some(5)));
        assert_eq!(slow.deps, current_deps("inputs", None, slow, || Some(3000)));
        assert_ne!(slow.deps, current_deps("inputs", None, slow, || Some(6000)));
        assert_ne!(ok.deps, current_deps("changed", d1(), ok, || None));
        assert_eq!(slow.exit_code, Some(1));
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

    #[test]
    fn inputs_digest_changes_with_rust_sources() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"p\"\n").unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
        let before = rust_inputs_digest(root).unwrap();
        assert_eq!(before, rust_inputs_digest(root).unwrap());
        std::fs::write(root.join("src/lib.rs"), "pub fn b() {}\n").unwrap();
        assert_ne!(before, rust_inputs_digest(root).unwrap());
    }

    #[test]
    fn inputs_digest_sees_a_lockfile_written_mid_session_once_forgotten() {
        use crate::test_runner::workspace_selector_cache::{
            begin_inventory_session, forget_inventory,
        };
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"p\"\n").unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
        let _session = begin_inventory_session(root);
        let before = rust_inputs_digest(root).unwrap();
        std::fs::write(root.join("Cargo.lock"), "version = 4\n").unwrap();
        assert_eq!(before, rust_inputs_digest(root).unwrap());
        forget_inventory(root);
        assert_ne!(before, rust_inputs_digest(root).unwrap());
    }
}
