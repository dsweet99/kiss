use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use rpytest_runner::TestStatus;

use super::{
    CargoLlvmCovRunOutcome, CargoLlvmCovRunner, RustCovCacheStatus, RustLlvmCov,
    cleanup_surplus_rust_cov_worker_slots, rust_cov_cache_tmp_parent, rust_cov_sample_request,
    worker,
};
use crate::test_support::{
    llvm_cov_json_for_file, wait_child, wait_for_path, write_demo_crate_source,
};

#[test]
fn rust_cov_child_helper() {
    let Ok(kind) = std::env::var("KISS_RUST_COV_HELPER") else {
        return;
    };
    if kind == "same_selector" {
        run_same_selector_child();
    } else if kind == "hold_worker_lock" {
        run_hold_worker_lock_child();
    }
}

#[test]
fn overlapping_same_selector_misses_collapse_to_one_runner_invocation() {
    let tmp = tempfile::tempdir().unwrap();
    write_demo_crate_source(tmp.path());
    let control = tmp.path().join("control");
    let invocations = control.join("invocations");
    fs::create_dir_all(&invocations).unwrap();
    let go = control.join("go");
    let children = [
        ChildSpec {
            ready: control.join("ready-0"),
            outcome: control.join("outcome-0"),
        },
        ChildSpec {
            ready: control.join("ready-1"),
            outcome: control.join("outcome-1"),
        },
    ];
    let mut processes: Vec<_> = children
        .iter()
        .map(|child| spawn_same_selector_child(tmp.path(), &invocations, &go, child))
        .collect();

    for child in &children {
        wait_for_path(&child.ready);
    }
    fs::write(&go, b"go").unwrap();
    for child in &mut processes {
        wait_child(child);
    }

    let statuses: Vec<_> = children
        .iter()
        .map(|child| fs::read_to_string(&child.outcome).unwrap())
        .collect();
    assert_eq!(
        statuses
            .iter()
            .filter(|status| status.trim() == "MissStored")
            .count(),
        1
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|status| status.trim() == "Hit")
            .count(),
        1
    );
    assert_eq!(fs::read_dir(&invocations).unwrap().count(), 1);
    let cache_root = tmp.path().join(".rust_llvm_cov_cache");
    assert!(cache_root.join("locks/selectors").exists());
    assert!(cache_root.join("locks/workers/slot-0.lock").exists());
    assert!(!has_entries(&cache_root.join("artifacts")));
}

#[test]
fn surplus_cleanup_skips_leased_worker_slot_then_removes_it_after_release() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().join(".rust_llvm_cov_cache");
    let worker_root = cache_root.join("workers").join("slot-2");
    let worker_tmp = worker::rust_cov_worker_tmp_root(&cache_root, 2);
    fs::create_dir_all(worker_root.join("target")).unwrap();
    fs::create_dir_all(&worker_tmp).unwrap();
    fs::write(worker_tmp.join("live.tmp"), b"tmp").unwrap();
    let control = tmp.path().join("control");
    fs::create_dir_all(&control).unwrap();
    let ready = control.join("ready");
    let release = control.join("release");
    let mut child = spawn_hold_worker_lock_child(&cache_root, 2, &ready, &release);

    wait_for_path(&ready);
    let report = cleanup_surplus_rust_cov_worker_slots(&cache_root, 2).unwrap();
    assert_eq!(report.skipped_slots, vec![2]);
    assert!(worker_root.exists());
    assert!(worker_tmp.exists());

    fs::write(&release, b"release").unwrap();
    wait_child(&mut child);
    let report = cleanup_surplus_rust_cov_worker_slots(&cache_root, 2).unwrap();
    assert_eq!(report.removed_slots, vec![2]);
    assert!(!worker_root.exists());
    assert!(!worker_tmp.exists());
}

#[test]
fn external_tmp_parent_uses_distinct_bounded_digest_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let first = tmp.path().join("a-b").join(".rust_llvm_cov_cache");
    let second = tmp.path().join("a_b").join(".rust_llvm_cov_cache");
    let long = tmp
        .path()
        .join("very-long-cache-root-name-that-would-be-risky-if-used-as-one-component")
        .join("nested")
        .join(".rust_llvm_cov_cache");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();
    fs::create_dir_all(&long).unwrap();

    let first_tmp = rust_cov_cache_tmp_parent(&first);
    let second_tmp = rust_cov_cache_tmp_parent(&second);
    let long_tmp = rust_cov_cache_tmp_parent(&long);

    assert_ne!(first_tmp, second_tmp);
    for path in [first_tmp, second_tmp, long_tmp] {
        let name = path.file_name().and_then(|name| name.to_str()).unwrap();
        assert!(name.starts_with("cache-"));
        assert_eq!(name.len(), "cache-".len() + 64);
    }
}

struct ChildSpec {
    ready: PathBuf,
    outcome: PathBuf,
}

fn spawn_same_selector_child(
    root: &Path,
    invocations: &Path,
    go: &Path,
    child: &ChildSpec,
) -> Child {
    Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("process_race_test::rust_cov_child_helper")
        .arg("--nocapture")
        .env("KISS_RUST_COV_HELPER", "same_selector")
        .env("KISS_RUST_COV_ROOT", root)
        .env("KISS_RUST_COV_INVOCATIONS", invocations)
        .env("KISS_RUST_COV_OUTCOME", &child.outcome)
        .env("KISS_RUST_COV_UNLOCKED_MISS_READY", &child.ready)
        .env("KISS_RUST_COV_UNLOCKED_MISS_GO", go)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

fn spawn_hold_worker_lock_child(
    cache_root: &Path,
    worker_slot: usize,
    ready: &Path,
    release: &Path,
) -> Child {
    Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("process_race_test::rust_cov_child_helper")
        .arg("--nocapture")
        .env("KISS_RUST_COV_HELPER", "hold_worker_lock")
        .env("KISS_RUST_COV_CACHE_ROOT", cache_root)
        .env("KISS_RUST_COV_WORKER_SLOT", worker_slot.to_string())
        .env("KISS_RUST_COV_LOCK_READY", ready)
        .env("KISS_RUST_COV_LOCK_RELEASE", release)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

fn run_same_selector_child() {
    let root = PathBuf::from(std::env::var("KISS_RUST_COV_ROOT").unwrap());
    let invocations = PathBuf::from(std::env::var("KISS_RUST_COV_INVOCATIONS").unwrap());
    let outcome_path = PathBuf::from(std::env::var("KISS_RUST_COV_OUTCOME").unwrap());
    let lib = root.join("src").join("lib.rs");
    let runner_lib = lib.clone();
    let runner = CargoLlvmCovRunner::from_fn(move |req| {
        fs::write(
            invocations.join(format!("{}.run", std::process::id())),
            b"run",
        )
        .unwrap();
        fs::create_dir_all(req.artifact_path.parent().unwrap()).unwrap();
        fs::write(&req.artifact_path, llvm_cov_json_for_file(&runner_lib)).unwrap();
        Ok(CargoLlvmCovRunOutcome {
            selector: req.selector,
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            stdout: Vec::new(),
            stderr: Vec::new(),
            artifact_path: req.artifact_path,
        })
    });
    let outcome = RustLlvmCov::new(runner)
        .run_or_reuse(rust_cov_sample_request(&root))
        .unwrap();
    assert_eq!(outcome.status, TestStatus::Passed);
    assert!(
        outcome
            .coverage
            .files
            .contains_key(&lib.canonicalize().unwrap().to_string_lossy().to_string())
    );
    let text = match outcome.cache_status {
        RustCovCacheStatus::Hit => "Hit",
        RustCovCacheStatus::MissStored => "MissStored",
    };
    fs::write(outcome_path, text).unwrap();
}

fn run_hold_worker_lock_child() {
    let cache_root = PathBuf::from(std::env::var("KISS_RUST_COV_CACHE_ROOT").unwrap());
    let worker_slot = std::env::var("KISS_RUST_COV_WORKER_SLOT")
        .unwrap()
        .parse()
        .unwrap();
    let ready = PathBuf::from(std::env::var("KISS_RUST_COV_LOCK_READY").unwrap());
    let release = PathBuf::from(std::env::var("KISS_RUST_COV_LOCK_RELEASE").unwrap());
    let _guard = worker::lock_worker(&cache_root, worker_slot).unwrap();
    fs::write(ready, b"ready").unwrap();
    wait_for_path(&release);
}

fn has_entries(path: &Path) -> bool {
    path.exists() && fs::read_dir(path).unwrap().next().is_some()
}
