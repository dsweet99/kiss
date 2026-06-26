use std::cell::Cell;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use rpytest_runner::TestStatus;

use super::{
    CargoLlvmCovRunError, CargoLlvmCovRunOutcome, CargoLlvmCovRunner, RustCovCacheStatus,
    RustLlvmCov, RustLlvmCovError, cleanup_legacy_worker_dirs, prepare_worker_slot,
    rust_cov_sample_request,
};

#[test]
fn rust_llvm_cov_cleans_legacy_workers_and_slot_transients() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().join(".rust_llvm_cov_cache");
    let legacy = cache_root.join("workers").join("abcdef123456");
    let slot = cache_root.join("workers").join("slot-0");
    fs::create_dir_all(legacy.join("target")).unwrap();
    fs::create_dir_all(slot.join("target")).unwrap();
    fs::create_dir_all(slot.join("profile")).unwrap();
    fs::create_dir_all(slot.join("tmp")).unwrap();
    fs::write(slot.join("target").join("kept"), "compiled").unwrap();
    fs::write(slot.join("profile").join("stale.profraw"), "old").unwrap();
    fs::write(slot.join("tmp").join("stale.tmp"), "old").unwrap();

    cleanup_legacy_worker_dirs(&cache_root).unwrap();
    prepare_worker_slot(&cache_root, 0).unwrap();

    assert!(!legacy.exists());
    assert!(slot.join("target").join("kept").exists());
    assert!(!slot.join("profile").exists());
    assert!(!slot.join("tmp").exists());
}

#[test]
fn rust_llvm_cov_run_or_reuse_removes_artifact_after_storing_cache_entry() {
    let tmp = tempfile::tempdir().unwrap();
    write_demo_crate_source(tmp.path());
    let lib = tmp.path().join("src").join("lib.rs");
    let artifact_seen = Rc::new(std::cell::RefCell::new(None));
    let artifact_seen_runner = Rc::clone(&artifact_seen);
    let runner = CargoLlvmCovRunner::from_fn(move |req| {
        *artifact_seen_runner.borrow_mut() = Some(req.artifact_path.clone());
        fs::create_dir_all(req.artifact_path.parent().unwrap()).unwrap();
        let json = llvm_cov_json_for_file(&lib, 1);
        fs::write(&req.artifact_path, json).unwrap();
        Ok(passed_run(req))
    });
    let cov = RustLlvmCov::new(runner);

    let outcome = cov
        .run_or_reuse(rust_cov_sample_request(tmp.path()))
        .unwrap();
    let artifact = artifact_seen.borrow().clone().unwrap();

    assert_eq!(outcome.cache_status, RustCovCacheStatus::MissStored);
    assert!(!artifact.exists());
    assert!(artifact.parent().unwrap().exists());
}

#[test]
fn rust_llvm_cov_cache_hit_does_not_clean_workers_or_artifacts() {
    let tmp = tempfile::tempdir().unwrap();
    write_demo_crate_source(tmp.path());
    let lib = tmp.path().join("src").join("lib.rs");
    let calls = Rc::new(Cell::new(0));
    let runner = fake_runner(Rc::clone(&calls), lib);
    let cov = RustLlvmCov::new(runner);
    let req = rust_cov_sample_request(tmp.path());

    cov.run_or_reuse(req.clone()).unwrap();
    let cache_root = tmp.path().join(".rust_llvm_cov_cache");
    let legacy = cache_root.join("workers").join("legacy-worker");
    let profile = cache_root
        .join("workers")
        .join("slot-0")
        .join("profile")
        .join("cached.profraw");
    fs::create_dir_all(&legacy).unwrap();
    fs::create_dir_all(profile.parent().unwrap()).unwrap();
    fs::write(&profile, "cached").unwrap();

    let cached = cov.run_or_reuse(req).unwrap();

    assert_eq!(cached.cache_status, RustCovCacheStatus::Hit);
    assert_eq!(calls.get(), 1);
    assert!(legacy.exists());
    assert!(profile.exists());
}

#[test]
fn rust_llvm_cov_keeps_artifact_when_parse_fails_before_cache_store() {
    let tmp = tempfile::tempdir().unwrap();
    write_demo_crate_manifest(tmp.path());
    let artifact_seen = Rc::new(std::cell::RefCell::new(None));
    let artifact_seen_runner = Rc::clone(&artifact_seen);
    let runner = CargoLlvmCovRunner::from_fn(move |req| {
        *artifact_seen_runner.borrow_mut() = Some(req.artifact_path.clone());
        fs::create_dir_all(req.artifact_path.parent().unwrap()).unwrap();
        fs::write(&req.artifact_path, "{bad json").unwrap();
        Ok(passed_run(req))
    });
    let cov = RustLlvmCov::new(runner);

    let err = cov
        .run_or_reuse(rust_cov_sample_request(tmp.path()))
        .unwrap_err();
    let artifact = artifact_seen.borrow().clone().unwrap();

    assert!(matches!(err, RustLlvmCovError::Json(_)));
    assert!(artifact.exists());
}

#[test]
fn rust_llvm_cov_runner_error_removes_slot_transients_but_preserves_target() {
    let tmp = tempfile::tempdir().unwrap();
    write_demo_crate_manifest(tmp.path());
    let runner = CargoLlvmCovRunner::from_fn(move |req| {
        let target = PathBuf::from(&req.env["CARGO_TARGET_DIR"]);
        let profile_file =
            PathBuf::from(&req.env["LLVM_PROFILE_FILE"]).with_file_name("failed.profraw");
        let tmp_file = PathBuf::from(&req.env["TMPDIR"]).join("failed.tmp");
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(profile_file.parent().unwrap()).unwrap();
        fs::create_dir_all(tmp_file.parent().unwrap()).unwrap();
        fs::write(target.join("kept"), "compiled").unwrap();
        fs::write(profile_file, "profile").unwrap();
        fs::write(tmp_file, "tmp").unwrap();
        Err(CargoLlvmCovRunError::Spawn {
            program: PathBuf::from("cargo"),
            message: "boom".to_string(),
        })
    });
    let cov = RustLlvmCov::new(runner);

    let err = cov
        .run_or_reuse(rust_cov_sample_request(tmp.path()))
        .unwrap_err();
    let slot = tmp
        .path()
        .join(".rust_llvm_cov_cache")
        .join("workers")
        .join("slot-0");

    assert!(matches!(err, RustLlvmCovError::Runner(_)));
    assert!(slot.join("target").join("kept").exists());
    assert!(!slot.join("profile").exists());
    assert!(!slot.join("tmp").exists());
}

fn fake_runner(calls: Rc<Cell<usize>>, covered_file: PathBuf) -> CargoLlvmCovRunner {
    CargoLlvmCovRunner::from_fn(move |req| {
        calls.set(calls.get() + 1);
        fs::create_dir_all(req.artifact_path.parent().unwrap()).unwrap();
        fs::write(
            &req.artifact_path,
            llvm_cov_json_for_file(&covered_file, calls.get()),
        )
        .unwrap();
        Ok(passed_run(req))
    })
}

fn write_demo_crate_source(root: &std::path::Path) {
    write_demo_crate_manifest(root);
    fs::create_dir(root.join("src")).unwrap();
    fs::write(
        root.join("src").join("lib.rs"),
        "pub fn value() -> u32 { 1 }\n",
    )
    .unwrap();
}

fn write_demo_crate_manifest(root: &std::path::Path) {
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
}

fn llvm_cov_json_for_file(file: &std::path::Path, count: usize) -> String {
    format!(
        r#"{{"data":[{{"files":[{{"filename":"{}","segments":[[1,1,{count},true,true,false]]}}]}}]}}"#,
        file.display()
    )
}

fn passed_run(req: super::CargoLlvmCovRunRequest) -> CargoLlvmCovRunOutcome {
    CargoLlvmCovRunOutcome {
        selector: req.selector,
        status: TestStatus::Passed,
        exit_code: Some(0),
        duration: Duration::from_millis(2),
        stdout: Vec::new(),
        stderr: Vec::new(),
        artifact_path: req.artifact_path,
    }
}
