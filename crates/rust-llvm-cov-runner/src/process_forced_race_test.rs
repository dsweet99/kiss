use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use rpytest_runner::TestStatus;

use super::{
    CargoLlvmCovRunOutcome, CargoLlvmCovRunRequest, CargoLlvmCovRunner, RustCovCacheStatus,
    RustLlvmCov, rust_cov_sample_request,
};
use crate::test_support::{
    llvm_cov_json_for_file, wait_child, wait_for_path, write_demo_crate_source,
};

#[test]
fn rust_cov_forced_child_helper() {
    if std::env::var("KISS_RUST_COV_HELPER").as_deref() == Ok("forced_selector") {
        run_forced_selector_child();
    }
}

#[test]
fn overlapping_forced_same_selector_runs_are_serialized_with_distinct_artifacts() {
    let tmp = tempfile::tempdir().unwrap();
    write_demo_crate_source(tmp.path());
    let control = tmp.path().join("control");
    let invocations = control.join("invocations");
    let releases = control.join("releases");
    fs::create_dir_all(&invocations).unwrap();
    fs::create_dir_all(&releases).unwrap();
    let go = control.join("go");
    let overlap = control.join("overlap");
    let children = [
        ChildSpec {
            ready: control.join("ready-forced-0"),
            outcome: control.join("outcome-forced-0"),
        },
        ChildSpec {
            ready: control.join("ready-forced-1"),
            outcome: control.join("outcome-forced-1"),
        },
    ];
    let mut processes: Vec<_> = children
        .iter()
        .map(|child| {
            spawn_forced_selector_child(
                tmp.path(),
                "smoke::passes",
                0,
                &invocations,
                &releases,
                &go,
                &overlap,
                child,
            )
        })
        .collect();

    for child in &children {
        wait_for_path(&child.ready);
    }
    fs::write(&go, b"go").unwrap();
    release_ready_invocations(&invocations, &releases, 1);
    release_ready_invocations(&invocations, &releases, 2);
    for child in &mut processes {
        wait_child(child);
    }

    let statuses: Vec<_> = children
        .iter()
        .map(|child| fs::read_to_string(&child.outcome).unwrap())
        .collect();
    assert!(statuses.iter().all(|status| status.trim() == "MissStored"));
    assert!(!overlap.exists(), "forced runners overlapped");
    let artifact_paths = ready_artifact_paths(&invocations);
    assert_eq!(artifact_paths.len(), 2);
    assert_ne!(artifact_paths[0], artifact_paths[1]);
    assert!(artifact_paths.iter().all(|path| !path.exists()));
    let cache_root = tmp.path().join(".rust_llvm_cov_cache");
    assert!(!has_entries(&cache_root.join("artifacts")));
}

#[test]
fn different_selectors_on_same_worker_slot_do_not_overlap_runner_sections() {
    let tmp = tempfile::tempdir().unwrap();
    write_demo_crate_source(tmp.path());
    let control = tmp.path().join("control");
    let invocations = control.join("invocations");
    let releases = control.join("releases");
    fs::create_dir_all(&invocations).unwrap();
    fs::create_dir_all(&releases).unwrap();
    let go = control.join("go");
    let overlap = control.join("overlap");
    let children = [
        ChildSpec {
            ready: control.join("ready-same-slot-0"),
            outcome: control.join("outcome-same-slot-0"),
        },
        ChildSpec {
            ready: control.join("ready-same-slot-1"),
            outcome: control.join("outcome-same-slot-1"),
        },
    ];
    let mut processes = vec![
        spawn_forced_selector_child(
            tmp.path(),
            "smoke::slot_a",
            0,
            &invocations,
            &releases,
            &go,
            &overlap,
            &children[0],
        ),
        spawn_forced_selector_child(
            tmp.path(),
            "smoke::slot_b",
            0,
            &invocations,
            &releases,
            &go,
            &overlap,
            &children[1],
        ),
    ];

    for child in &children {
        wait_for_path(&child.ready);
    }
    fs::write(&go, b"go").unwrap();
    release_ready_invocations(&invocations, &releases, 1);
    release_ready_invocations(&invocations, &releases, 2);
    for child in &mut processes {
        wait_child(child);
    }

    assert!(!overlap.exists(), "same worker-slot runners overlapped");
    assert_eq!(ready_artifact_paths(&invocations).len(), 2);
}

#[test]
fn different_worker_slots_can_overlap_runner_sections() {
    let tmp = tempfile::tempdir().unwrap();
    write_demo_crate_source(tmp.path());
    let control = tmp.path().join("control");
    let invocations = control.join("invocations");
    let releases = control.join("releases");
    fs::create_dir_all(&invocations).unwrap();
    fs::create_dir_all(&releases).unwrap();
    let go = control.join("go");
    let overlap = control.join("overlap");
    let children = [
        ChildSpec {
            ready: control.join("ready-different-slot-0"),
            outcome: control.join("outcome-different-slot-0"),
        },
        ChildSpec {
            ready: control.join("ready-different-slot-1"),
            outcome: control.join("outcome-different-slot-1"),
        },
    ];
    let mut processes = vec![
        spawn_forced_selector_child(
            tmp.path(),
            "smoke::slot_a",
            0,
            &invocations,
            &releases,
            &go,
            &overlap,
            &children[0],
        ),
        spawn_forced_selector_child(
            tmp.path(),
            "smoke::slot_b",
            1,
            &invocations,
            &releases,
            &go,
            &overlap,
            &children[1],
        ),
    ];

    for child in &children {
        wait_for_path(&child.ready);
    }
    fs::write(&go, b"go").unwrap();
    wait_for_ready_invocation_count(&invocations, 2);
    assert!(
        overlap.exists(),
        "different worker-slot runners did not overlap"
    );
    release_ready_invocations(&invocations, &releases, 2);
    for child in &mut processes {
        wait_child(child);
    }
}

struct ChildSpec {
    ready: PathBuf,
    outcome: PathBuf,
}

fn spawn_forced_selector_child(
    root: &Path,
    selector: &str,
    worker_slot: usize,
    invocations: &Path,
    releases: &Path,
    go: &Path,
    overlap: &Path,
    child: &ChildSpec,
) -> Child {
    Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("process_forced_race_test::rust_cov_forced_child_helper")
        .arg("--nocapture")
        .env("KISS_RUST_COV_HELPER", "forced_selector")
        .env("KISS_RUST_COV_ROOT", root)
        .env("KISS_RUST_COV_SELECTOR", selector)
        .env("KISS_RUST_COV_WORKER_SLOT", worker_slot.to_string())
        .env("KISS_RUST_COV_INVOCATIONS", invocations)
        .env("KISS_RUST_COV_RELEASES", releases)
        .env("KISS_RUST_COV_OVERLAP", overlap)
        .env("KISS_RUST_COV_OUTCOME", &child.outcome)
        .env("KISS_RUST_COV_UNLOCKED_MISS_READY", &child.ready)
        .env("KISS_RUST_COV_UNLOCKED_MISS_GO", go)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

fn run_forced_selector_child() {
    let root = PathBuf::from(std::env::var("KISS_RUST_COV_ROOT").unwrap());
    let invocations = PathBuf::from(std::env::var("KISS_RUST_COV_INVOCATIONS").unwrap());
    let releases = PathBuf::from(std::env::var("KISS_RUST_COV_RELEASES").unwrap());
    let overlap = PathBuf::from(std::env::var("KISS_RUST_COV_OVERLAP").unwrap());
    let outcome_path = PathBuf::from(std::env::var("KISS_RUST_COV_OUTCOME").unwrap());
    let lib = root.join("src").join("lib.rs");
    let active = invocations.join("active");
    let runner_lib = lib.clone();
    let runner = CargoLlvmCovRunner::from_fn(move |req| {
        Ok(run_forced_fake_runner(
            req,
            &active,
            &overlap,
            &invocations,
            &releases,
            &runner_lib,
        ))
    });
    let mut req = rust_cov_sample_request(&root);
    req.selector = std::env::var("KISS_RUST_COV_SELECTOR").unwrap();
    req.worker_slot = std::env::var("KISS_RUST_COV_WORKER_SLOT")
        .unwrap()
        .parse()
        .unwrap();
    req.force_rerun = true;
    let outcome = RustLlvmCov::new(runner).run_or_reuse(req).unwrap();
    assert_eq!(outcome.status, TestStatus::Passed);
    let text = match outcome.cache_status {
        RustCovCacheStatus::Hit => "Hit",
        RustCovCacheStatus::MissStored => "MissStored",
        RustCovCacheStatus::FreshUnstored => "FreshUnstored",
    };
    fs::write(outcome_path, text).unwrap();
}

fn run_forced_fake_runner(
    req: CargoLlvmCovRunRequest,
    active: &Path,
    overlap: &Path,
    invocations: &Path,
    releases: &Path,
    runner_lib: &Path,
) -> CargoLlvmCovRunOutcome {
    let pid = std::process::id();
    mark_runner_active(active, overlap);
    fs::write(
        invocations.join(format!("{pid}.ready")),
        req.artifact_path.to_string_lossy().as_bytes(),
    )
    .unwrap();
    wait_for_path(&releases.join(pid.to_string()));
    let _ = fs::remove_file(active);
    fs::create_dir_all(req.artifact_path.parent().unwrap()).unwrap();
    fs::write(&req.artifact_path, llvm_cov_json_for_file(runner_lib)).unwrap();
    CargoLlvmCovRunOutcome {
        selector: req.selector,
        status: TestStatus::Passed,
        exit_code: Some(0),
        duration: Duration::from_millis(1),
        stdout: Vec::new(),
        stderr: Vec::new(),
        artifact_path: req.artifact_path,
    }
}

fn mark_runner_active(active: &Path, overlap: &Path) {
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(active)
    {
        Ok(_) => {}
        Err(_) => fs::write(overlap, b"overlap").unwrap(),
    }
}

fn release_ready_invocations(invocations: &Path, releases: &Path, expected_count: usize) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let ready = ready_invocation_names(invocations);
        for name in &ready {
            fs::write(releases.join(name), b"release").unwrap();
        }
        if ready.len() >= expected_count {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {expected_count} forced invocations"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_ready_invocation_count(invocations: &Path, expected_count: usize) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while ready_invocation_names(invocations).len() < expected_count {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {expected_count} ready invocations"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn ready_invocation_names(invocations: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(invocations)
        .unwrap()
        .flatten()
        .filter_map(|entry| {
            entry
                .file_name()
                .to_str()
                .and_then(|name| name.strip_suffix(".ready").map(ToString::to_string))
        })
        .collect();
    names.sort();
    names
}

fn ready_artifact_paths(invocations: &Path) -> Vec<PathBuf> {
    ready_invocation_names(invocations)
        .into_iter()
        .map(|name| {
            PathBuf::from(
                fs::read_to_string(invocations.join(format!("{name}.ready")))
                    .unwrap()
                    .trim(),
            )
        })
        .collect()
}

fn has_entries(path: &Path) -> bool {
    path.exists() && fs::read_dir(path).unwrap().next().is_some()
}
