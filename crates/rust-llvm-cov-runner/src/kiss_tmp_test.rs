use super::*;
use std::fs;

#[test]
fn kiss_tmp_dir_is_under_dot_kiss() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(
        kiss_tmp_dir(tmp.path()),
        tmp.path().join(".kiss").join("tmp")
    );
}

#[test]
fn kiss_tmp_from_cache_root_is_sibling_of_rust_llvm_cov_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join(".kiss").join("rust_llvm_cov_cache");
    assert_eq!(kiss_tmp_from_cache_root(&cache), tmp.path().join(".kiss").join("tmp"));
}

#[test]
fn resolve_kiss_tmp_prefers_env_then_output_dir_layout() {
    let tmp = tempfile::tempdir().unwrap();
    let forced = tmp.path().join("forced-tmp");
    let old = std::env::var_os(KISS_TMP_ENV);
    unsafe {
        std::env::set_var(KISS_TMP_ENV, &forced);
    }
    let resolved = resolve_kiss_tmp(Path::new("/unused/output"));
    match old {
        Some(value) => unsafe { std::env::set_var(KISS_TMP_ENV, value) },
        None => unsafe { std::env::remove_var(KISS_TMP_ENV) },
    }
    assert_eq!(resolved, forced);

    let output = tmp
        .path()
        .join(".kiss")
        .join("rust_llvm_cov_cache")
        .join("runs")
        .join("run-a")
        .join("instances");
    fs::create_dir_all(&output).unwrap();
    let old = std::env::var_os(KISS_TMP_ENV);
    unsafe {
        std::env::remove_var(KISS_TMP_ENV);
    }
    let derived = resolve_kiss_tmp(&output);
    match old {
        Some(value) => unsafe { std::env::set_var(KISS_TMP_ENV, value) },
        None => unsafe { std::env::remove_var(KISS_TMP_ENV) },
    }
    assert_eq!(derived, tmp.path().join(".kiss").join("tmp"));
}

#[test]
fn redirect_sets_discard_pattern_under_kiss_tmp() {
    let tmp = tempfile::tempdir().unwrap();
    let kiss_tmp = tmp.path().join("tmp");
    let old = std::env::var_os("LLVM_PROFILE_FILE");
    let path = redirect_llvm_profile_file_to_kiss_tmp(&kiss_tmp).unwrap();
    assert_eq!(path, kiss_tmp.join(DISCARD_PROFILE_PATTERN));
    assert_eq!(
        std::env::var_os("LLVM_PROFILE_FILE").as_deref(),
        Some(path.as_os_str())
    );
    assert!(kiss_tmp.is_dir());
    match old {
        Some(value) => unsafe { std::env::set_var("LLVM_PROFILE_FILE", value) },
        None => unsafe { std::env::remove_var("LLVM_PROFILE_FILE") },
    }
}

#[test]
fn redirect_inherited_uses_kiss_tmp_env() {
    let tmp = tempfile::tempdir().unwrap();
    let kiss_tmp = tmp.path().join("tmp");
    let old_profile = std::env::var_os("LLVM_PROFILE_FILE");
    let old_kiss_tmp = std::env::var_os(KISS_TMP_ENV);
    unsafe {
        std::env::set_var("LLVM_PROFILE_FILE", "/tmp/should-be-redirected.profraw");
        std::env::set_var(KISS_TMP_ENV, &kiss_tmp);
    }
    redirect_inherited_llvm_profile_file(tmp.path()).unwrap();
    let redirected = std::env::var_os("LLVM_PROFILE_FILE").unwrap();
    assert!(redirected.to_string_lossy().contains(DISCARD_PROFILE_PATTERN));
    assert!(kiss_tmp.is_dir());
    match old_profile {
        Some(value) => unsafe { std::env::set_var("LLVM_PROFILE_FILE", value) },
        None => unsafe { std::env::remove_var("LLVM_PROFILE_FILE") },
    }
    match old_kiss_tmp {
        Some(value) => unsafe { std::env::set_var(KISS_TMP_ENV, value) },
        None => unsafe { std::env::remove_var(KISS_TMP_ENV) },
    }
}

#[test]
fn cleanup_removes_profraw_and_empty_tmp() {
    let tmp = tempfile::tempdir().unwrap();
    let kiss_tmp = tmp.path().join("tmp");
    fs::create_dir_all(&kiss_tmp).unwrap();
    fs::write(kiss_tmp.join("default_1_0_2.profraw"), b"raw").unwrap();
    fs::write(kiss_tmp.join("keep.txt"), b"keep").unwrap();

    cleanup_kiss_tmp_profraw(&kiss_tmp).unwrap();

    assert!(!kiss_tmp.join("default_1_0_2.profraw").exists());
    assert!(kiss_tmp.join("keep.txt").exists());
    assert!(kiss_tmp.is_dir());

    fs::remove_file(kiss_tmp.join("keep.txt")).unwrap();
    cleanup_kiss_tmp_profraw(&kiss_tmp).unwrap();
    assert!(!kiss_tmp.exists());
}
