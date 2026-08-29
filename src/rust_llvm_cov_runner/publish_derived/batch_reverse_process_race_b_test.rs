use crate::rust_llvm_cov_runner::execute_or_reuse::batch_lock::lock_batch;
use crate::rust_llvm_cov_runner::publish_derived::batch_reverse_process_race_support::{
    SpawnExact, assert_ok, child_work_and_repo, release_barrier, spawn_exact, wait_barrier_ready,
    wait_path, wait_ready,
};
use crate::rust_llvm_cov_runner::publish_derived::batch_reverse_publish::{
    prune_unreferenced_snapshots, snapshot_path,
};
use crate::rust_llvm_cov_runner::publish_derived::batch_reverse_test_support::seed_alpha_beta_reverse;
use crate::rust_llvm_cov_runner::publish_derived_state;
use crate::rust_llvm_cov_runner::query_reverse_line_index;
use crate::rust_llvm_cov_runner::test_support::{
    batch_executor_fixture_repo, batch_executor_request, store_batch_executor_selector,
    witness_batch_tools,
};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::thread;
use std::time::Duration;

const CHILD_ENV: &str = "RUST_REVERSE_PROCESS_RACE_B_CHILD";
const ROOT_ENV: &str = "RUST_REVERSE_PROCESS_RACE_B_ROOT";
const MODE_ENV: &str = "RUST_REVERSE_PROCESS_RACE_B_MODE";

#[test]
fn os_process_post_manifest_publisher_b_serializes_on_batch_lock() {
    if env::var_os(CHILD_ENV).is_some() {
        dispatch_child();
        return;
    }
    let (repo, req, work) = primed();
    store_batch_executor_selector(repo.path(), &req, "alpha");
    store_batch_executor_selector(repo.path(), &req, "beta");
    let prior = seed_alpha_beta_reverse(&req);
    write_work_paths(&work, repo.path(), &prior);
    let barrier = work.join("barrier");
    fs::create_dir_all(&barrier).unwrap();
    let exe = env::current_exe().unwrap();
    let publisher_a = spawn(
        &exe,
        &work,
        "a",
        "publish_hold_after_manifest",
        Some((&barrier, "rust_population:after_rename")),
    );
    wait_barrier_ready(&barrier, "rust_population", "after_rename");
    let publisher_b = spawn(&exe, &work, "b", "publish_after_go", None);
    wait_ready(&work, &["b"]);
    fs::write(work.join("go_b"), b"go").unwrap();
    thread::sleep(Duration::from_millis(200));
    assert!(
        !work.join("snapshot-b.txt").exists(),
        "publisher B must not finish while A holds batch.lock"
    );
    run_readers_then_release(&exe, &work, &barrier, publisher_a);
    assert_ok("b", &publisher_b.wait_with_output().unwrap());
    assert!(work.join("snapshot-b.txt").is_file());
    assert_active_readable(&req.cache_root);
}

#[test]
fn os_process_kill_at_entry_state_barrier_then_repair() {
    if env::var_os(CHILD_ENV).is_some() {
        dispatch_child();
        return;
    }
    kill_at_barrier_then_repair("rust_entry_state", "after_sync_before_rename");
}

#[test]
fn os_process_kill_at_population_barrier_then_repair() {
    if env::var_os(CHILD_ENV).is_some() {
        dispatch_child();
        return;
    }
    kill_at_barrier_then_repair("rust_population", "after_sync_before_rename");
}

#[test]
fn os_process_kill_at_each_reverse_publish_barrier_then_repair() {
    if env::var_os(CHILD_ENV).is_some() {
        dispatch_child();
        return;
    }
    const BARRIERS: &[(&str, &str)] = &[
        ("rust_entry_state", "after_sync_before_rename"),
        ("rust_reverse_selectors", "after_sync_before_rename"),
        ("rust_reverse_file", "after_sync_before_rename"),
        ("rust_reverse_meta", "after_sync_before_rename"),
        ("rust_population", "after_sync_before_rename"),
        ("rust_population", "after_rename"),
    ];
    for &(artifact, phase) in BARRIERS {
        kill_at_barrier_then_repair(artifact, phase);
    }
}

fn kill_at_barrier_then_repair(artifact: &str, phase: &str) {
    let (repo, req, work) = primed();
    store_selector_with_lib_coverage(repo.path(), &req, "alpha", BTreeSet::from([1_u32]));
    store_selector_with_lib_coverage(repo.path(), &req, "beta", BTreeSet::from([1_u32]));
    let prior = seed_alpha_beta_reverse(&req);
    write_work_paths(&work, repo.path(), &prior);
    let barrier = work.join(format!("barrier-{artifact}-{phase}"));
    fs::create_dir_all(&barrier).unwrap();
    let target = format!("{artifact}:{phase}");
    let exe = env::current_exe().unwrap();
    let mut publisher = spawn(
        &exe,
        &work,
        "killme",
        "publish_barrier_pre_manifest",
        Some((&barrier, target.as_str())),
    );
    if let Err(err) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wait_barrier_ready(&barrier, artifact, phase);
    })) {
        let output = publisher.wait_with_output().unwrap();
        panic!(
            "barrier {target} failed: {err:?}\nchild status={}\nstdout={}\nstderr={}\nbarrier_dir={:?}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
            fs::read_dir(&barrier)
                .map(|d| d
                    .filter_map(|e| e.ok().map(|e| e.file_name()))
                    .collect::<Vec<_>>())
                .unwrap_or_default()
        );
    }
    let _ = publisher.kill();
    let _ = publisher.wait();
    repair_under_lock(&req);
    assert_active_readable(&req.cache_root);
}

fn store_selector_with_lib_coverage(
    repo: &Path,
    req: &crate::rust_llvm_cov_runner::RustCoverageBatchRequest,
    selector: &str,
    lines: BTreeSet<u32>,
) {
    use crate::rpytest_runner::TestStatus;
    use crate::rust_llvm_cov_runner::plan::batch_fingerprint::{batch_identity, entry_fingerprint};
    use crate::rust_llvm_cov_runner::rust_cov_cache::{
        RustCovCacheEntry, store_rust_cov_cache_entry,
    };
    use crate::rust_llvm_cov_runner::{RustCovCacheStatus, RustLineCoverage, RustLlvmCovOutcome};
    use std::time::Duration;

    let tools = witness_batch_tools();
    let identity = batch_identity(req, &tools).unwrap();
    let fingerprint = entry_fingerprint(&identity.input_digest, req, &tools, selector);
    let file = repo
        .join("src")
        .join("lib.rs")
        .to_string_lossy()
        .into_owned();
    let entry = RustCovCacheEntry::from_outcome(
        &RustLlvmCovOutcome {
            selector: selector.to_string(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            coverage: RustLineCoverage {
                files: BTreeMap::from([(file, lines)]),
            },
            test_binary_ids: vec!["test-bin".to_string()],
            cache_status: RustCovCacheStatus::MissStored,
            stdout: None,
            stderr: None,
        },
        &identity.generation_fingerprint,
    );
    store_rust_cov_cache_entry(&req.cache_root, &fingerprint, &entry).unwrap();
}

#[test]
fn os_process_prune_never_deletes_manifest_active_snapshot() {
    if env::var_os(CHILD_ENV).is_some() {
        dispatch_child();
        return;
    }
    let (repo, req, work) = primed();
    store_batch_executor_selector(repo.path(), &req, "alpha");
    store_batch_executor_selector(repo.path(), &req, "beta");
    let active = seed_alpha_beta_reverse(&req);
    write_work_paths(&work, repo.path(), &active);
    let exe = env::current_exe().unwrap();
    let pruner = spawn(&exe, &work, "pruner", "prune_hold", None);
    wait_ready(&work, &["pruner"]);
    let readers: Vec<Child> = (0..4)
        .map(|i| spawn(&exe, &work, &format!("r{i}"), "reader", None))
        .collect();
    wait_ready(&work, &["r0", "r1", "r2", "r3"]);
    fs::write(work.join("go_readers"), b"go").unwrap();
    fs::write(work.join("go_prune"), b"go").unwrap();
    for reader in readers {
        assert_ok("reader", &reader.wait_with_output().unwrap());
    }
    assert_ok("pruner", &pruner.wait_with_output().unwrap());
    assert!(snapshot_path(&req.cache_root, &active).is_dir());
    assert_active_readable(&req.cache_root);
}

fn primed() -> (
    tempfile::TempDir,
    crate::rust_llvm_cov_runner::RustCoverageBatchRequest,
    PathBuf,
) {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    let work = repo.path().join("process-race-b-work");
    fs::create_dir_all(work.join("ready")).unwrap();
    (repo, req, work)
}

fn write_work_paths(work: &Path, repo: &Path, prior: &str) {
    fs::write(
        work.join("repo_path.txt"),
        repo.to_string_lossy().as_bytes(),
    )
    .unwrap();
    fs::write(work.join("prior_snapshot.txt"), prior.as_bytes()).unwrap();
}

fn run_readers_then_release(exe: &Path, work: &Path, barrier: &Path, publisher_a: Child) {
    let readers: Vec<Child> = (0..3)
        .map(|i| spawn(exe, work, &format!("r{i}"), "reader", None))
        .collect();
    wait_ready(work, &["r0", "r1", "r2"]);
    fs::write(work.join("go_readers"), b"go").unwrap();
    for reader in readers {
        assert_ok("reader", &reader.wait_with_output().unwrap());
    }
    release_barrier(barrier);
    assert_ok("a", &publisher_a.wait_with_output().unwrap());
}

fn repair_under_lock(req: &crate::rust_llvm_cov_runner::RustCoverageBatchRequest) {
    let tools = witness_batch_tools();
    let identity =
        crate::rust_llvm_cov_runner::plan::batch_fingerprint::batch_identity(req, &tools).unwrap();
    let _guard = lock_batch(&req.cache_root).unwrap();
    publish_derived_state(
        req,
        &tools,
        &identity,
        &["alpha".to_string(), "beta".to_string()],
        true,
    )
    .unwrap();
}

fn assert_active_readable(cache_root: &Path) {
    let population: serde_json::Value =
        serde_json::from_slice(&fs::read(cache_root.join("population.json")).unwrap()).unwrap();
    let active = population["reverse_line_index"]["snapshot_id"]
        .as_str()
        .unwrap();
    assert!(
        snapshot_path(cache_root, active)
            .join("meta.json")
            .is_file()
    );
}

fn dispatch_child() {
    match env::var(MODE_ENV).unwrap().as_str() {
        "publish_hold_after_manifest" | "publish_barrier_pre_manifest" => run_locked_publish(),
        "publish_after_go" => run_publish_after_go(),
        "prune_hold" => run_prune_hold(),
        "reader" => run_reader(),
        other => panic!("unknown mode {other}"),
    }
}

fn run_locked_publish() {
    let (id, work, req) = child_ctx();
    fs::write(work.join("ready").join(&id), b"ready").unwrap();
    let _ = publish_under_lock(&req);
}

fn run_publish_after_go() {
    let (id, work, req) = child_ctx();
    fs::write(work.join("ready").join(&id), b"ready").unwrap();
    wait_path(&work.join("go_b"), Duration::from_secs(15));
    let snap = publish_under_lock(&req);
    fs::write(work.join("snapshot-b.txt"), snap.as_bytes()).unwrap();
}

fn run_prune_hold() {
    let (id, work, req) = child_ctx();
    fs::write(work.join("ready").join(&id), b"ready").unwrap();
    wait_path(&work.join("go_prune"), Duration::from_secs(15));
    let prior = fs::read_to_string(work.join("prior_snapshot.txt")).unwrap();
    let _guard = lock_batch(&req.cache_root).unwrap();
    let _ = prune_unreferenced_snapshots(&req.cache_root, prior.trim(), None).unwrap();
    assert!(snapshot_path(&req.cache_root, prior.trim()).is_dir());
}

fn run_reader() {
    let (id, work, req) = child_ctx();
    fs::write(work.join("ready").join(&id), b"ready").unwrap();
    wait_path(&work.join("go_readers"), Duration::from_secs(15));
    let prior = fs::read_to_string(work.join("prior_snapshot.txt")).unwrap();
    let tools = witness_batch_tools();
    let identity =
        crate::rust_llvm_cov_runner::plan::batch_fingerprint::batch_identity(&req, &tools).unwrap();
    for _ in 0..30 {
        assert_reader_safe(&req, &identity.generation_fingerprint, prior.trim());
        thread::sleep(Duration::from_millis(5));
    }
}

fn assert_reader_safe(
    req: &crate::rust_llvm_cov_runner::RustCoverageBatchRequest,
    generation: &str,
    prior: &str,
) {
    match query_reverse_line_index(
        &req.cache_root,
        generation,
        &BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([1_u32]))]),
    ) {
        None => {}
        Some(_) => {
            if let Ok(bytes) = fs::read(req.cache_root.join("population.json"))
                && let Ok(pop) = serde_json::from_slice::<serde_json::Value>(&bytes)
                && let Some(sid) = pop
                    .get("reverse_line_index")
                    .and_then(|r| r.get("snapshot_id"))
                    .and_then(|v| v.as_str())
            {
                assert!(
                    snapshot_path(&req.cache_root, sid)
                        .join("meta.json")
                        .is_file(),
                    "manifest must not point at deleted snapshot"
                );
                assert!(sid == prior || snapshot_path(&req.cache_root, sid).is_dir());
            }
        }
    }
}

fn publish_under_lock(req: &crate::rust_llvm_cov_runner::RustCoverageBatchRequest) -> String {
    let tools = witness_batch_tools();
    let identity =
        crate::rust_llvm_cov_runner::plan::batch_fingerprint::batch_identity(req, &tools).unwrap();
    let _guard = lock_batch(&req.cache_root).unwrap();
    publish_derived_state(
        req,
        &tools,
        &identity,
        &["alpha".to_string(), "beta".to_string()],
        true,
    )
    .unwrap();
    serde_json::from_slice::<serde_json::Value>(
        &fs::read(req.cache_root.join("population.json")).unwrap(),
    )
    .unwrap()["reverse_line_index"]["snapshot_id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn child_ctx() -> (
    String,
    PathBuf,
    crate::rust_llvm_cov_runner::RustCoverageBatchRequest,
) {
    let (id, work, repo) = child_work_and_repo(CHILD_ENV, ROOT_ENV);
    (id, work, batch_executor_request(&repo))
}

fn test_name(mode: &str) -> &'static str {
    match mode {
        "publish_hold_after_manifest" | "publish_after_go" | "reader" => {
            "rust_llvm_cov_runner::publish_derived::batch_reverse_line_index::process_race_b_tests::os_process_post_manifest_publisher_b_serializes_on_batch_lock"
        }
        "publish_barrier_pre_manifest" => {
            "rust_llvm_cov_runner::publish_derived::batch_reverse_line_index::process_race_b_tests::os_process_kill_at_entry_state_barrier_then_repair"
        }
        "prune_hold" => {
            "rust_llvm_cov_runner::publish_derived::batch_reverse_line_index::process_race_b_tests::os_process_prune_never_deletes_manifest_active_snapshot"
        }
        _ => panic!("unknown mode"),
    }
}

fn spawn(exe: &Path, work: &Path, id: &str, mode: &str, barrier: Option<(&Path, &str)>) -> Child {
    spawn_exact(SpawnExact {
        exe,
        work,
        id,
        mode,
        test_name: test_name(mode),
        child_env: CHILD_ENV,
        root_env: ROOT_ENV,
        mode_env: MODE_ENV,
        barrier,
    })
}
