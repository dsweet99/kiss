use std::fs;
use std::os::unix::fs::PermissionsExt;

use crate::worker::{
    cleanup_legacy_worker_data_nonblocking, cleanup_legacy_worker_dirs, lock_worker_for_test,
};

#[test]
fn rust_llvm_cov_cleans_legacy_worker_dirs_without_touching_slot_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().join(".rust_llvm_cov_cache");
    let legacy = cache_root.join("workers").join("abcdef123456");
    let slot = cache_root.join("workers").join("slot-0");
    fs::create_dir_all(legacy.join("target")).unwrap();
    fs::create_dir_all(slot.join("target")).unwrap();
    fs::write(slot.join("target").join("kept"), "compiled").unwrap();

    cleanup_legacy_worker_dirs(&cache_root).unwrap();

    assert!(!legacy.exists());
    assert!(slot.join("target").join("kept").exists());
}

#[test]
fn batch_legacy_cleanup_defers_when_worker_slot_is_leased() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().join(".rust_llvm_cov_cache");
    let slot = cache_root.join("workers").join("slot-0");
    fs::create_dir_all(slot.join("target")).unwrap();
    let _slot_guard = lock_worker_for_test(&cache_root, 0).unwrap();

    let report = cleanup_legacy_worker_data_nonblocking(&cache_root).unwrap();

    assert!(report.deferred);
    assert!(slot.join("target").exists());
}

#[test]
fn batch_legacy_cleanup_fails_when_slot_directory_cannot_be_removed() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().join(".rust_llvm_cov_cache");
    let slot = cache_root.join("workers").join("slot-0");
    fs::create_dir_all(slot.join("nested")).unwrap();
    fs::write(slot.join("nested").join("locked"), "compiled").unwrap();
    let mut permissions = fs::metadata(&slot).unwrap().permissions();
    permissions.set_mode(0o555);
    fs::set_permissions(&slot, permissions).unwrap();

    let err = cleanup_legacy_worker_data_nonblocking(&cache_root).unwrap_err();

    let mut permissions = fs::metadata(&slot).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&slot, permissions).unwrap();
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
}

#[test]
fn batch_legacy_cleanup_removes_idle_worker_slots_without_deleting_locks() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().join(".rust_llvm_cov_cache");
    let slot = cache_root.join("workers").join("slot-0");
    fs::create_dir_all(slot.join("target")).unwrap();
    fs::write(slot.join("target").join("old"), "compiled").unwrap();
    drop(lock_worker_for_test(&cache_root, 0).unwrap());

    let report = cleanup_legacy_worker_data_nonblocking(&cache_root).unwrap();

    assert!(!report.deferred);
    assert!(!slot.exists());
    assert!(
        cache_root
            .join("locks")
            .join("workers")
            .join("slot-0.lock")
            .exists()
    );
}

#[test]
fn worker_tmp_parent_and_digest_helpers_are_stable() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().join("cache");
    std::fs::create_dir_all(&cache_root).unwrap();
    let parent = crate::rust_cov_cache_tmp_parent(&cache_root);
    assert!(parent.to_string_lossy().contains("kiss-rust-llvm-cov"));
    assert_eq!(crate::worker::hex_lower(&[0xab, 0xcd]), "abcd");
    assert_eq!(
        crate::worker::os_str_bytes(std::ffi::OsStr::new("ab")),
        b"ab".to_vec()
    );
    let digest = crate::worker::cache_root_digest(&cache_root);
    assert_eq!(digest.len(), 64);
    let attempt = crate::worker::RustLegacyCleanupAttempt { deferred: false };
    assert!(!attempt.deferred);
}
