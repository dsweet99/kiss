use crate::rust_llvm_cov_runner::execute_or_reuse::batch_lock::lock_batch;
use crate::rust_llvm_cov_runner::publish_derived::batch_entry_state::read_entry_state;
use crate::rust_llvm_cov_runner::publish_derived::batch_reverse_publish::snapshot_path;
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
use std::process::{Child, Command, Output};
use std::thread;
use std::time::{Duration, Instant};

const CHILD_ENV: &str = "RUST_REVERSE_PROCESS_RACE_CHILD";
const ROOT_ENV: &str = "RUST_REVERSE_PROCESS_RACE_ROOT";
const MODE_ENV: &str = "RUST_REVERSE_PROCESS_RACE_MODE";

#[test]
fn two_os_process_publishers_single_manifest_activation() {
    if env::var_os(CHILD_ENV).is_some() {
        run_publisher_child();
        return;
    }
    let (repo, req, work) = primed_shared_work_dir();
    store_batch_executor_selector(repo.path(), &req, "alpha");
    store_batch_executor_selector(repo.path(), &req, "beta");
    fs::write(
        work.join("repo_path.txt"),
        repo.path().to_string_lossy().as_bytes(),
    )
    .unwrap();

    let exe = env::current_exe().unwrap();
    let children = [
        spawn_child(&exe, work.as_path(), "first", "publish"),
        spawn_child(&exe, work.as_path(), "second", "publish"),
    ];
    wait_ready(&work, &["first", "second"]);
    fs::write(work.join("go"), b"go").unwrap();
    for (label, child) in ["first", "second"].into_iter().zip(children) {
        assert_child_ok(label, &child.wait_with_output().unwrap());
    }
    let tools = witness_batch_tools();
    let identity =
        crate::rust_llvm_cov_runner::plan::batch_fingerprint::batch_identity(&req, &tools).unwrap();
    assert_single_activation(&req.cache_root, &work, &identity.generation_fingerprint);
}

#[test]
fn os_process_readers_during_pre_manifest_pause_never_see_partial() {
    if env::var_os(CHILD_ENV).is_some() {
        dispatch_barrier_child_mode();
        return;
    }
    let (repo, req, work) = primed_shared_work_dir();
    store_batch_executor_selector(repo.path(), &req, "alpha");
    store_batch_executor_selector(repo.path(), &req, "beta");
    let prior = seed_alpha_beta_reverse(&req);
    let barrier = work.join("barrier");
    fs::create_dir_all(&barrier).unwrap();
    fs::write(
        work.join("repo_path.txt"),
        repo.path().to_string_lossy().as_bytes(),
    )
    .unwrap();
    fs::write(work.join("prior_snapshot.txt"), prior.as_bytes()).unwrap();

    let exe = env::current_exe().unwrap();
    let publisher = spawn_child_with_barrier(
        &exe,
        work.as_path(),
        "publisher",
        "publish_barrier",
        &barrier,
        "rust_population:after_sync_before_rename",
    );
    wait_for_barrier_ready(&barrier, "rust_population", "after_sync_before_rename");
    run_concurrent_readers_then_release(&exe, &work, &barrier, publisher);
    assert_active_snapshot_readable(&req.cache_root);
}

fn primed_shared_work_dir() -> (
    tempfile::TempDir,
    crate::rust_llvm_cov_runner::RustCoverageBatchRequest,
    PathBuf,
) {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    let work = repo.path().join("process-race-work");
    fs::create_dir_all(work.join("ready")).unwrap();
    (repo, req, work)
}

fn assert_single_activation(cache_root: &Path, work: &Path, generation: &str) {
    let snap_a = fs::read_to_string(work.join("snapshot-first.txt")).unwrap();
    let snap_b = fs::read_to_string(work.join("snapshot-second.txt")).unwrap();
    let population: serde_json::Value =
        serde_json::from_slice(&fs::read(cache_root.join("population.json")).unwrap()).unwrap();
    let active = population["reverse_line_index"]["snapshot_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        active == snap_a.trim() || active == snap_b.trim(),
        "manifest must point at one publisher snapshot: active={active}"
    );
    assert!(snapshot_path(cache_root, &active).is_dir());
    let state = read_entry_state(cache_root).expect("entry_state");
    assert_eq!(
        state.revision,
        population["reverse_line_index"]["entry_state_revision"]
            .as_u64()
            .unwrap()
    );
    let hit = query_reverse_line_index(
        cache_root,
        generation,
        &BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([1_u32]))]),
    );
    if let Some(map) = hit {
        let covered: BTreeSet<&str> = map.values().flatten().map(String::as_str).collect();
        assert!(
            covered.is_empty() || covered.contains("alpha") || covered.contains("beta"),
            "reverse index must be coherent after concurrent publish: {map:?}"
        );
    }
}

fn assert_active_snapshot_readable(cache_root: &Path) {
    let population: serde_json::Value =
        serde_json::from_slice(&fs::read(cache_root.join("population.json")).unwrap()).unwrap();
    let active = population["reverse_line_index"]["snapshot_id"]
        .as_str()
        .unwrap();
    assert!(snapshot_path(cache_root, active).is_dir());
}

fn run_concurrent_readers_then_release(exe: &Path, work: &Path, barrier: &Path, publisher: Child) {
    let readers: Vec<Child> = (0..4)
        .map(|i| spawn_child(exe, work, &format!("reader{i}"), "reader"))
        .collect();
    wait_ready(work, &["reader0", "reader1", "reader2", "reader3"]);
    fs::write(work.join("go_readers"), b"go").unwrap();
    for reader in readers {
        assert_child_ok("reader", &reader.wait_with_output().unwrap());
    }
    release_barrier(barrier);
    assert_child_ok("publisher", &publisher.wait_with_output().unwrap());
}

fn dispatch_barrier_child_mode() {
    match env::var(MODE_ENV).unwrap().as_str() {
        "publish_barrier" => run_barrier_publisher_child(),
        "reader" => run_reader_child(),
        other => panic!("unknown mode {other}"),
    }
}

fn wait_ready(work: &Path, ids: &[&str]) {
    for id in ids {
        wait_for_path(&work.join("ready").join(id), Duration::from_secs(5));
    }
}

fn spawn_child(exe: &Path, work: &Path, child_id: &str, mode: &str) -> Child {
    Command::new(exe)
        .arg("--exact")
        .arg(test_name_for_mode(mode))
        .arg("--nocapture")
        .env(CHILD_ENV, child_id)
        .env(ROOT_ENV, work)
        .env(MODE_ENV, mode)
        .env_remove("LLVM_PROFILE_FILE")
        .spawn()
        .unwrap()
}

fn spawn_child_with_barrier(
    exe: &Path,
    work: &Path,
    child_id: &str,
    mode: &str,
    barrier: &Path,
    target: &str,
) -> Child {
    Command::new(exe)
        .arg("--exact")
        .arg(test_name_for_mode(mode))
        .arg("--nocapture")
        .env(CHILD_ENV, child_id)
        .env(ROOT_ENV, work)
        .env(MODE_ENV, mode)
        .env("KISS_QA_PUBLICATION_BARRIER_DIR", barrier)
        .env("KISS_QA_PUBLICATION_BARRIER_TARGET", target)
        .env_remove("LLVM_PROFILE_FILE")
        .spawn()
        .unwrap()
}

fn test_name_for_mode(mode: &str) -> &'static str {
    match mode {
        "publish" => {
            "rust_llvm_cov_runner::publish_derived::batch_reverse_line_index::process_race_tests::two_os_process_publishers_single_manifest_activation"
        }
        "publish_barrier" | "reader" => {
            "rust_llvm_cov_runner::publish_derived::batch_reverse_line_index::process_race_tests::os_process_readers_during_pre_manifest_pause_never_see_partial"
        }
        _ => panic!("unknown mode"),
    }
}

fn run_publisher_child() {
    let (child_id, work, req) = child_context();
    fs::write(work.join("ready").join(&child_id), b"ready").unwrap();
    wait_for_path(&work.join("go"), Duration::from_secs(10));
    publish_under_lock_and_record_snapshot(&req, &work, &child_id);
}

fn run_barrier_publisher_child() {
    let (child_id, work, req) = child_context();
    fs::write(work.join("ready").join(&child_id), b"ready").unwrap();
    let tools = witness_batch_tools();
    let identity =
        crate::rust_llvm_cov_runner::plan::batch_fingerprint::batch_identity(&req, &tools).unwrap();
    let _guard = lock_batch(&req.cache_root).unwrap();
    publish_derived_state(
        &req,
        &tools,
        &identity,
        &["alpha".to_string(), "beta".to_string()],
        true,
    )
    .unwrap();
}

fn run_reader_child() {
    let (child_id, work, req) = child_context();
    fs::write(work.join("ready").join(&child_id), b"ready").unwrap();
    wait_for_path(&work.join("go_readers"), Duration::from_secs(10));
    let prior = fs::read_to_string(work.join("prior_snapshot.txt")).unwrap();
    let tools = witness_batch_tools();
    let identity =
        crate::rust_llvm_cov_runner::plan::batch_fingerprint::batch_identity(&req, &tools).unwrap();
    for _ in 0..20 {
        assert_reader_observation_safe(&req, &identity.generation_fingerprint, prior.trim());
        thread::sleep(Duration::from_millis(10));
    }
}

fn child_context() -> (
    String,
    PathBuf,
    crate::rust_llvm_cov_runner::RustCoverageBatchRequest,
) {
    let child_id = env::var(CHILD_ENV).unwrap();
    let work = PathBuf::from(env::var_os(ROOT_ENV).unwrap());
    let repo = PathBuf::from(fs::read_to_string(work.join("repo_path.txt")).unwrap());
    (child_id, work, batch_executor_request(&repo))
}

fn publish_under_lock_and_record_snapshot(
    req: &crate::rust_llvm_cov_runner::RustCoverageBatchRequest,
    work: &Path,
    child_id: &str,
) {
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
    let population: serde_json::Value =
        serde_json::from_slice(&fs::read(req.cache_root.join("population.json")).unwrap()).unwrap();
    let snap = population["reverse_line_index"]["snapshot_id"]
        .as_str()
        .unwrap();
    fs::write(
        work.join(format!("snapshot-{child_id}.txt")),
        snap.as_bytes(),
    )
    .unwrap();
}

fn assert_reader_observation_safe(
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
            let Ok(bytes) = fs::read(req.cache_root.join("population.json")) else {
                return;
            };
            let Ok(population) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                return;
            };
            let Some(id) = population
                .get("reverse_line_index")
                .and_then(|r| r.get("snapshot_id"))
                .and_then(|v| v.as_str())
            else {
                return;
            };
            assert!(
                snapshot_path(&req.cache_root, id)
                    .join("meta.json")
                    .is_file(),
                "reader must not observe manifest→missing snapshot"
            );
            assert!(
                id == prior
                    || snapshot_path(&req.cache_root, id)
                        .join("selectors.json")
                        .is_file(),
                "partial snapshot hit"
            );
        }
    }
}

fn wait_for_path(path: &Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "timeout waiting for {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_barrier_ready(barrier: &Path, artifact: &str, phase: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if try_capture_barrier_ready(barrier, artifact, phase) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timeout waiting for barrier ready"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

fn try_capture_barrier_ready(barrier: &Path, artifact: &str, phase: &str) -> bool {
    for entry in fs::read_dir(barrier).into_iter().flatten().flatten() {
        let path = entry.path();
        if !path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".ready.json"))
        {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        if value.get("artifact").and_then(|v| v.as_str()) == Some(artifact)
            && value.get("phase").and_then(|v| v.as_str()) == Some(phase)
        {
            fs::write(barrier.join("ready_copy.json"), text.as_bytes()).unwrap();
            return true;
        }
    }
    false
}

fn release_barrier(barrier: &Path) {
    let ready = fs::read_to_string(barrier.join("ready_copy.json")).unwrap();
    let value: serde_json::Value = serde_json::from_str(&ready).unwrap();
    let op = value["operation_id"].as_str().unwrap();
    let release = barrier.join(format!("{op}.release.json"));
    let payload = format!(
        "{{\n  \"schema_version\": 1,\n  \"operation_id\": \"{op}\",\n  \"artifact\": \"{}\",\n  \"phase\": \"{}\"\n}}\n",
        value["artifact"].as_str().unwrap(),
        value["phase"].as_str().unwrap()
    );
    let tmp = barrier.join(format!(".{op}.release.tmp"));
    fs::write(&tmp, payload.as_bytes()).unwrap();
    fs::rename(tmp, release).unwrap();
}

fn assert_child_ok(label: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{label} failed status={} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
