use super::*;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

#[test]
fn cleanup_removes_profraw_and_empty_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let kiss_profraw = tmp.path().join("profraw");
    fs::create_dir_all(&kiss_profraw).unwrap();
    fs::write(kiss_profraw.join("default_1_0_2.profraw"), b"raw").unwrap();
    fs::write(kiss_profraw.join("keep.txt"), b"keep").unwrap();

    cleanup_kiss_profraw(&kiss_profraw).unwrap();

    assert!(!kiss_profraw.join("default_1_0_2.profraw").exists());
    assert!(kiss_profraw.join("keep.txt").exists());
    assert!(kiss_profraw.is_dir());

    fs::remove_file(kiss_profraw.join("keep.txt")).unwrap();
    cleanup_kiss_profraw(&kiss_profraw).unwrap();
    assert!(!kiss_profraw.exists());
}

#[test]
fn pid_scoped_cleanup_deletes_only_matching_pid_suffix() {
    let tmp = tempfile::tempdir().unwrap();
    let kiss_profraw = tmp.path().join("profraw");
    fs::create_dir_all(&kiss_profraw).unwrap();
    let pid_a = 1111u32;
    let pid_b = 2222u32;
    fs::write(
        kiss_profraw.join(format!("default_abc_0_{pid_a}.profraw")),
        b"a",
    )
    .unwrap();
    fs::write(kiss_profraw.join(format!("default_1_{pid_a}.profraw")), b"a2").unwrap();
    fs::write(kiss_profraw.join(format!("default_1_{pid_b}.profraw")), b"b").unwrap();
    fs::write(
        kiss_profraw.join(format!("default_xyz_0_{pid_b}.profraw")),
        b"b2",
    )
    .unwrap();

    cleanup_kiss_profraw_for_pid(&kiss_profraw, pid_a).unwrap();

    assert!(!kiss_profraw.join(format!("default_abc_0_{pid_a}.profraw")).exists());
    assert!(!kiss_profraw.join(format!("default_1_{pid_a}.profraw")).exists());
    assert!(kiss_profraw.join(format!("default_1_{pid_b}.profraw")).exists());
    assert!(kiss_profraw
        .join(format!("default_xyz_0_{pid_b}.profraw"))
        .exists());
}

#[test]
fn list_child_pid_capture_cleans_spawned_child_pid() {
    let tmp = tempfile::tempdir().unwrap();
    let kiss_profraw = tmp.path().join("profraw");
    fs::create_dir_all(&kiss_profraw).unwrap();
    let mut child = Command::new("sleep")
        .arg("30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    let other = pid.wrapping_add(1);
    fs::write(kiss_profraw.join(format!("default_1_{pid}.profraw")), b"mine").unwrap();
    fs::write(kiss_profraw.join(format!("default_1_{other}.profraw")), b"other").unwrap();
    let _ = child.kill();
    let _ = child.wait();
    cleanup_kiss_profraw_for_pid(&kiss_profraw, pid).unwrap();
    assert!(!kiss_profraw.join(format!("default_1_{pid}.profraw")).exists());
    assert!(kiss_profraw.join(format!("default_1_{other}.profraw")).exists());
}

#[test]
fn list_child_source_records_spawn_pid_not_output_alone() {
    let src = include_str!("execute_or_reuse/batch_shim_list.rs");
    assert!(
        src.contains("child.id()"),
        "list path must record spawned child pid"
    );
    assert!(
        !src.contains(".output()?"),
        "list path must not use Command::output() alone (no child pid)"
    );
    assert!(src.contains("cleanup_kiss_profraw_for_pid"));
}

#[test]
fn orphan_sweep_deletes_root_and_crate_orphans_not_instances() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    let crate_dir = repo.join("crates").join("pkg");
    let instances = repo
        .join(".kiss")
        .join("rust_llvm_cov_cache")
        .join("runs")
        .join("run-a")
        .join("instances");
    let legacy_tmp = repo.join(".kiss").join("tmp");
    let target_dir = repo.join("target");
    fs::create_dir_all(&crate_dir).unwrap();
    fs::create_dir_all(&instances).unwrap();
    fs::create_dir_all(&legacy_tmp).unwrap();
    fs::create_dir_all(&target_dir).unwrap();

    fs::write(repo.join("default_root_0_1.profraw"), b"root").unwrap();
    fs::write(crate_dir.join("default_crate_0_2.profraw"), b"crate").unwrap();
    fs::write(legacy_tmp.join("default_legacy_0_3.profraw"), b"legacy").unwrap();
    fs::write(instances.join("intentional.profraw"), b"keep").unwrap();
    fs::write(instances.join("default_should_keep_0_9.profraw"), b"keep").unwrap();
    fs::write(target_dir.join("default_target_0_4.profraw"), b"target").unwrap();

    sweep_orphan_default_profraw(repo).unwrap();

    assert!(!repo.join("default_root_0_1.profraw").exists());
    assert!(!crate_dir.join("default_crate_0_2.profraw").exists());
    assert!(!legacy_tmp.exists() || !legacy_tmp.join("default_legacy_0_3.profraw").exists());
    assert!(!legacy_tmp.exists());
    assert!(instances.join("intentional.profraw").exists());
    assert!(instances.join("default_should_keep_0_9.profraw").exists());
    assert!(
        target_dir.join("default_target_0_4.profraw").exists(),
        "must not walk target/"
    );
}

#[test]
fn ensure_kiss_profraw_env_sets_dir_and_discard_llvm_profile_file() {
    let mut env = std::collections::BTreeMap::new();
    ensure_kiss_profraw_env(&mut env, Path::new("/repo"));
    assert_eq!(env[KISS_PROFRAW_DIR_ENV], "/repo/.kiss/profraw");
    assert_eq!(
        env["LLVM_PROFILE_FILE"],
        "/repo/.kiss/profraw/default_%m_%p.profraw"
    );
}

#[test]
fn no_production_kiss_tmp_discard_references() {
    let src_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    assert!(
        !src_root.join("kiss_tmp.rs").exists(),
        "kiss_tmp module must be removed"
    );
    let lib = fs::read_to_string(src_root.join("lib.rs")).unwrap();
    assert!(!lib.contains("mod kiss_tmp"));
    for name in [
        "plan/batch_plan.rs",
        "execute_or_reuse/batch_run_cleanup.rs",
        "execute_or_reuse/batch_shim_child.rs",
        "execute_or_reuse/batch_shim_list.rs",
    ] {
        let text = fs::read_to_string(src_root.join(name)).unwrap();
        assert!(
            !text.contains("KISS_TMP"),
            "{name} must not reference KISS_TMP"
        );
        assert!(
            !text.contains("kiss_tmp"),
            "{name} must not reference kiss_tmp"
        );
    }
    let kiss_profraw = fs::read_to_string(src_root.join("kiss_profraw.rs")).unwrap();
    assert!(!kiss_profraw.contains("KISS_TMP"));
    assert!(
        kiss_profraw.contains("join(\"tmp\")"),
        "orphan sweep must still clear leftover .kiss/tmp"
    );
    assert!(
        !kiss_profraw.contains("KISS_TMP_ENV"),
        "discard env must be KISS_PROFRAW_DIR, not KISS_TMP"
    );
}
