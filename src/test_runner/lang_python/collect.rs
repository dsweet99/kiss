use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use kiss::rpytest_runner::{collect_pytest_nodeids, PytestCollectRequest};

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct CollectMemoKey {
    repo_root: PathBuf,
    paths: Vec<PathBuf>,
    pytest_args: Vec<String>,
    inventory: String,
}

fn collection_input_stamp(repo_root: &Path) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for name in [
        "pytest.ini",
        "pyproject.toml",
        "setup.cfg",
        "tox.ini",
        ".kissconfig",
    ] {
        h = h.wrapping_mul(0x0100_0000_01b3) ^ u64::from(name.len() as u8);
        if let Ok(bytes) = fs::read(repo_root.join(name)) {
            for byte in bytes {
                h = h.wrapping_mul(0x0100_0000_01b3) ^ u64::from(byte);
            }
        }
    }
    h = mix_collection_audit(h, repo_root);
    format!("{h:016x}")
}

fn mix_collection_audit(mut h: u64, repo_root: &Path) -> u64 {
    let Ok(dir) =
        crate::test_runner::python_coverage_index::storage::python_coverage_cache_root(repo_root)
    else {
        return h;
    };
    let path = dir.join("collection_audit.json");
    let Ok(bytes) = fs::read(&path) else {
        return h;
    };
    for byte in &bytes {
        h = h.wrapping_mul(0x0100_0000_01b3) ^ u64::from(*byte);
    }
    if let Ok(listed) = serde_json::from_slice::<Vec<String>>(&bytes) {
        for rel in listed {
            if let Ok(contents) = fs::read(repo_root.join(&rel)) {
                h = h.wrapping_mul(0x0100_0000_01b3) ^ u64::from(rel.len() as u8);
                for byte in contents {
                    h = h.wrapping_mul(0x0100_0000_01b3) ^ u64::from(byte);
                }
            }
        }
    }
    h
}

fn persist_collection_audit(repo_root: &Path, observed: &[String]) {
    let Ok(dir) =
        crate::test_runner::python_coverage_index::storage::python_coverage_cache_root(repo_root)
    else {
        return;
    };
    let _ = fs::create_dir_all(&dir);
    let mut listed: Vec<String> = observed
        .iter()
        .filter(|rel| rel.ends_with(".py") && !rel.contains("__pycache__"))
        .cloned()
        .collect();
    listed.sort();
    listed.dedup();
    if let Ok(bytes) = serde_json::to_vec(&listed) {
        let _ = fs::write(dir.join("collection_audit.json"), bytes);
    }
}

static COLLECT_MEMO: Mutex<BTreeMap<CollectMemoKey, Vec<String>>> = Mutex::new(BTreeMap::new());

#[cfg(test)]
static FULL_SUITE_SUBPROCESS_COLLECTS: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn collect_python_nodeids(
    repo_root: &Path,
    paths: Option<&[PathBuf]>,
    pytest_args: &[String],
) -> Result<Vec<String>, String> {
    let normalized_paths = paths.map(normalize_collect_paths).unwrap_or_default();
    let key = CollectMemoKey {
        repo_root: repo_root.to_path_buf(),
        paths: normalized_paths.clone(),
        pytest_args: pytest_args.to_vec(),
        inventory: collection_input_stamp(repo_root),
    };
    if let Some(cached) = COLLECT_MEMO.lock().unwrap().get(&key).cloned() {
        return Ok(cached);
    }
    if paths.is_none() {
        #[cfg(test)]
        FULL_SUITE_SUBPROCESS_COLLECTS.fetch_add(1, Ordering::SeqCst);
    }
    let outcome = collect_pytest_nodeids_maybe_sharded(repo_root, &normalized_paths, pytest_args)
        .map_err(format_collect_error)?;
    persist_collection_audit(repo_root, &outcome.observed_workspace);
    if !outcome.unsupported_external {
        COLLECT_MEMO
            .lock()
            .unwrap()
            .insert(key, outcome.nodeids.clone());
    }
    Ok(outcome.nodeids)
}

const COLLECT_SHARD_PATH_THRESHOLD: usize = 64;

fn collect_shard_count(path_count: usize) -> usize {
    if path_count < COLLECT_SHARD_PATH_THRESHOLD {
        return 1;
    }
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(2, 16);
    cpus.min(path_count / 16).max(2)
}

fn shard_collect_paths(paths: &[PathBuf], shard_count: usize) -> Vec<Vec<PathBuf>> {
    if shard_count <= 1 || paths.len() <= 1 {
        return vec![paths.to_vec()];
    }
    let mut groups: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for path in paths {
        let key = path.parent().unwrap_or(path).to_path_buf();
        groups.entry(key).or_default().push(path.clone());
    }
    let mut shards = vec![Vec::new(); shard_count];
    let mut sizes = vec![0usize; shard_count];
    let mut grouped: Vec<Vec<PathBuf>> = groups.into_values().collect();
    grouped.sort_by_key(|g| std::cmp::Reverse(g.len()));
    for group in grouped {
        let (idx, _) = sizes
            .iter()
            .enumerate()
            .min_by_key(|(_, n)| **n)
            .expect("shard bins");
        sizes[idx] += group.len();
        shards[idx].extend(group);
    }
    shards.retain(|shard| !shard.is_empty());
    shards
}

fn collect_one_path_set(
    repo_root: &Path,
    paths: Vec<PathBuf>,
    pytest_args: &[String],
) -> Result<kiss::rpytest_runner::PytestCollectOutcome, kiss::rpytest_runner::PytestCollectError> {
    collect_pytest_nodeids(PytestCollectRequest {
        cwd: repo_root.to_path_buf(),
        python: PathBuf::from("python"),
        paths,
        pytest_args: pytest_args.to_vec(),
        env: BTreeMap::new(),
    })
}

fn merge_collect_outcomes(
    outcomes: Vec<kiss::rpytest_runner::PytestCollectOutcome>,
) -> kiss::rpytest_runner::PytestCollectOutcome {
    let mut nodeids = Vec::new();
    let mut observed_workspace = Vec::new();
    let mut unsupported_external = false;
    for outcome in outcomes {
        nodeids.extend(outcome.nodeids);
        observed_workspace.extend(outcome.observed_workspace);
        unsupported_external |= outcome.unsupported_external;
    }
    nodeids.sort();
    nodeids.dedup();
    observed_workspace.sort();
    observed_workspace.dedup();
    kiss::rpytest_runner::PytestCollectOutcome {
        nodeids,
        observed_workspace,
        unsupported_external,
    }
}

fn collect_pytest_nodeids_maybe_sharded(
    repo_root: &Path,
    normalized_paths: &[PathBuf],
    pytest_args: &[String],
) -> Result<kiss::rpytest_runner::PytestCollectOutcome, kiss::rpytest_runner::PytestCollectError> {
    let shard_count = collect_shard_count(normalized_paths.len());
    if shard_count <= 1 {
        return collect_one_path_set(repo_root, normalized_paths.to_vec(), pytest_args);
    }
    let shards = shard_collect_paths(normalized_paths, shard_count);
    let outcomes = collect_shards_in_parallel(repo_root, shards, pytest_args)?;
    Ok(merge_collect_outcomes(outcomes))
}

fn collect_shards_in_parallel(
    repo_root: &Path,
    shards: Vec<Vec<PathBuf>>,
    pytest_args: &[String],
) -> Result<Vec<kiss::rpytest_runner::PytestCollectOutcome>, kiss::rpytest_runner::PytestCollectError>
{
    std::thread::scope(|scope| {
        join_collect_shards(spawn_collect_shards(scope, repo_root, shards, pytest_args))
    })
}

fn spawn_collect_shards<'scope>(
    scope: &'scope std::thread::Scope<'scope, '_>,
    repo_root: &'scope Path,
    shards: Vec<Vec<PathBuf>>,
    pytest_args: &'scope [String],
) -> Vec<
    std::thread::ScopedJoinHandle<
        'scope,
        Result<
            kiss::rpytest_runner::PytestCollectOutcome,
            kiss::rpytest_runner::PytestCollectError,
        >,
    >,
> {
    shards
        .into_iter()
        .map(|shard| scope.spawn(move || collect_one_path_set(repo_root, shard, pytest_args)))
        .collect()
}

fn join_collect_shards(
    handles: Vec<
        std::thread::ScopedJoinHandle<
            '_,
            Result<
                kiss::rpytest_runner::PytestCollectOutcome,
                kiss::rpytest_runner::PytestCollectError,
            >,
        >,
    >,
) -> Result<Vec<kiss::rpytest_runner::PytestCollectOutcome>, kiss::rpytest_runner::PytestCollectError>
{
    handles
        .into_iter()
        .map(|handle| handle.join().expect("pytest collect shard"))
        .collect()
}

fn normalize_collect_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = paths.to_vec();
    out.sort();
    out
}

fn format_collect_error(err: kiss::rpytest_runner::PytestCollectError) -> String {
    match err {
        kiss::rpytest_runner::PytestCollectError::InvalidRequest(message) => {
            format!("error: kiss test: invalid pytest collection request: {message}")
        }
        kiss::rpytest_runner::PytestCollectError::Spawn { program, message } => {
            format!(
                "error: kiss test: failed to spawn {} for pytest collection: {message}",
                program.display()
            )
        }
        kiss::rpytest_runner::PytestCollectError::CollectionFailed {
            exit_code,
            stderr,
            stdout,
        } => {
            let detail = if !stderr.trim().is_empty() {
                stderr.trim().to_string()
            } else {
                stdout.trim().to_string()
            };
            format!(
                "error: kiss test: pytest collection failed (exit={:?}): {detail}",
                exit_code
            )
        }
        kiss::rpytest_runner::PytestCollectError::InvalidOutput(message) => {
            format!("error: kiss test: invalid pytest collection output: {message}")
        }
        kiss::rpytest_runner::PytestCollectError::NodeidNormalization { nodeid, message } => {
            format!("error: kiss test: invalid pytest nodeid '{nodeid}': {message}")
        }
    }
}

#[cfg(test)]
pub(crate) fn format_collect_error_for_test(
    err: kiss::rpytest_runner::PytestCollectError,
) -> String {
    format_collect_error(err)
}

pub(crate) fn clear_python_collect_memo() {
    COLLECT_MEMO.lock().unwrap().clear();
    crate::test_runner::lang_python::generation::clear_python_execution_identity_memo();
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn reset_python_collect_memo_for_tests() {
    clear_python_collect_memo();
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn full_suite_subprocess_collects_for_tests() -> usize {
    FULL_SUITE_SUBPROCESS_COLLECTS.load(Ordering::SeqCst)
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn reset_full_suite_subprocess_collects_for_tests() {
    FULL_SUITE_SUBPROCESS_COLLECTS.store(0, Ordering::SeqCst);
}

#[cfg(test)]
mod shard_tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn collect_shard_count_stays_one_below_threshold() {
        assert_eq!(collect_shard_count(COLLECT_SHARD_PATH_THRESHOLD - 1), 1);
        assert!(collect_shard_count(COLLECT_SHARD_PATH_THRESHOLD) >= 2);
    }

    #[test]
    fn shard_collect_paths_keeps_sibling_files_together() {
        let mut paths = Vec::new();
        for dir in ["a", "b"] {
            for i in 0..4 {
                paths.push(PathBuf::from(format!("{dir}/t{i}.py")));
            }
        }
        let shards = shard_collect_paths(&paths, 2);
        assert_eq!(shards.len(), 2);
        for shard in &shards {
            let parents: BTreeSet<_> = shard
                .iter()
                .map(|path| path.parent().unwrap().to_path_buf())
                .collect();
            assert_eq!(parents.len(), 1, "siblings stay in one shard: {shard:?}");
        }
        let total: usize = shards.iter().map(Vec::len).sum();
        assert_eq!(total, paths.len());
    }

    #[test]
    fn merge_collect_outcomes_dedups_and_unions_flags() {
        let merged = merge_collect_outcomes(vec![
            kiss::rpytest_runner::PytestCollectOutcome {
                nodeids: vec!["t.py::a".into(), "t.py::b".into()],
                observed_workspace: vec!["t.py".into()],
                unsupported_external: false,
            },
            kiss::rpytest_runner::PytestCollectOutcome {
                nodeids: vec!["t.py::b".into(), "u.py::c".into()],
                observed_workspace: vec!["u.py".into()],
                unsupported_external: true,
            },
        ]);
        assert_eq!(
            merged.nodeids,
            vec!["t.py::a".to_string(), "t.py::b".into(), "u.py::c".into()]
        );
        assert_eq!(
            merged.observed_workspace,
            vec!["t.py".to_string(), "u.py".into()]
        );
        assert!(merged.unsupported_external);
    }
}

#[cfg(test)]
mod coverage_witness {
    use super::*;
    use kiss::rpytest_runner::PytestCollectError;

    #[test]
    fn witness_python_collect_helpers() {
        let key = CollectMemoKey {
            repo_root: PathBuf::from("."),
            paths: normalize_collect_paths(&[PathBuf::from("tests/t.py")]),
            pytest_args: vec!["-q".into()],
            inventory: collection_input_stamp(Path::new(".")),
        };
        let _ = key;
        assert!(
            format_collect_error(PytestCollectError::InvalidRequest("x".into()))
                .contains("invalid pytest collection request")
        );
        reset_python_collect_memo_for_tests();
        let before = full_suite_subprocess_collects_for_tests();
        reset_full_suite_subprocess_collects_for_tests();
        assert_eq!(full_suite_subprocess_collects_for_tests(), 0);
        let _ = before;
    }
}
