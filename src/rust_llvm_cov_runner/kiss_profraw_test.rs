use super::*;
use std::fs;
use std::path::Path;

#[test]
fn for_current_process_is_named() {
    let _ = KissProfrawProcessGuard::for_current_process;
}

#[test]
fn kiss_profraw_dir_is_under_dot_kiss() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(
        kiss_profraw_dir(tmp.path()),
        tmp.path().join(".kiss").join("profraw")
    );
}

#[test]
fn kiss_profraw_from_cache_root_is_sibling_of_rust_llvm_cov_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join(".kiss").join("rust_llvm_cov_cache");
    assert_eq!(
        kiss_profraw_from_cache_root(&cache),
        tmp.path().join(".kiss").join("profraw")
    );
}

#[test]
fn resolve_kiss_profraw_prefers_env_then_output_dir_layout() {
    let _env_guard = crate::rust_llvm_cov_runner::test_support::shim_test_env_lock();
    let tmp = tempfile::tempdir().unwrap();
    let forced = tmp.path().join("forced-profraw");
    let old = std::env::var_os(KISS_PROFRAW_DIR_ENV);

    unsafe {
        std::env::set_var(KISS_PROFRAW_DIR_ENV, &forced);
    }
    let resolved = resolve_kiss_profraw(Path::new("/unused/output"));
    match old {
        Some(value) => unsafe { std::env::set_var(KISS_PROFRAW_DIR_ENV, value) },
        None => unsafe { std::env::remove_var(KISS_PROFRAW_DIR_ENV) },
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
    let old = std::env::var_os(KISS_PROFRAW_DIR_ENV);

    unsafe {
        std::env::remove_var(KISS_PROFRAW_DIR_ENV);
    }
    let derived = resolve_kiss_profraw(&output);
    match old {
        Some(value) => unsafe { std::env::set_var(KISS_PROFRAW_DIR_ENV, value) },
        None => unsafe { std::env::remove_var(KISS_PROFRAW_DIR_ENV) },
    }
    assert_eq!(derived, tmp.path().join(".kiss").join("profraw"));
}

#[test]
fn redirect_sets_discard_pattern_under_kiss_profraw() {
    let _env_guard = crate::rust_llvm_cov_runner::test_support::shim_test_env_lock();
    let tmp = tempfile::tempdir().unwrap();
    let kiss_profraw = tmp.path().join("profraw");
    let old = std::env::var_os("LLVM_PROFILE_FILE");
    let path = redirect_llvm_profile_file_to_kiss_profraw(&kiss_profraw).unwrap();
    assert_eq!(path, kiss_profraw.join(DISCARD_PROFILE_PATTERN));
    assert_eq!(
        std::env::var_os("LLVM_PROFILE_FILE").as_deref(),
        Some(path.as_os_str())
    );
    assert!(kiss_profraw.is_dir());

    match old {
        Some(value) => unsafe { std::env::set_var("LLVM_PROFILE_FILE", value) },
        None => unsafe { std::env::remove_var("LLVM_PROFILE_FILE") },
    }
}

#[test]
fn redirect_this_process_sets_absolute_discard_path_and_is_idempotent() {
    let _env_guard = crate::rust_llvm_cov_runner::test_support::shim_test_env_lock();
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().canonicalize().unwrap();
    let old = std::env::var_os("LLVM_PROFILE_FILE");
    let first = redirect_this_process(&repo).unwrap();
    let expected = repo
        .join(".kiss")
        .join("profraw")
        .join(DISCARD_PROFILE_PATTERN);
    assert_eq!(first, expected);
    assert!(first.is_absolute());
    assert_eq!(
        std::env::var_os("LLVM_PROFILE_FILE").as_deref(),
        Some(first.as_os_str())
    );
    assert!(repo.join(".kiss").join("profraw").is_dir());
    let second = redirect_this_process(&repo).unwrap();
    assert_eq!(second, first);
    assert_eq!(
        std::env::var_os("LLVM_PROFILE_FILE").as_deref(),
        Some(first.as_os_str())
    );

    match old {
        Some(value) => unsafe { std::env::set_var("LLVM_PROFILE_FILE", value) },
        None => unsafe { std::env::remove_var("LLVM_PROFILE_FILE") },
    }
}

#[test]
fn redirect_this_process_overwrites_prior_env_deliberate_delegation_resets() {
    let _env_guard = crate::rust_llvm_cov_runner::test_support::shim_test_env_lock();
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().canonicalize().unwrap();
    let intentional = repo
        .join(".kiss")
        .join("rust_llvm_cov_cache")
        .join("runs")
        .join("run-a")
        .join("instances")
        .join("binary.profraw");
    let old = std::env::var_os("LLVM_PROFILE_FILE");

    unsafe {
        std::env::set_var("LLVM_PROFILE_FILE", &intentional);
    }
    let redirected = redirect_this_process(&repo).unwrap();
    assert_ne!(redirected, intentional);
    assert_eq!(
        std::env::var_os("LLVM_PROFILE_FILE").as_deref(),
        Some(redirected.as_os_str())
    );

    unsafe {
        std::env::set_var("LLVM_PROFILE_FILE", &intentional);
    }
    assert_eq!(
        std::env::var_os("LLVM_PROFILE_FILE").as_deref(),
        Some(intentional.as_os_str())
    );
    match old {
        Some(value) => unsafe { std::env::set_var("LLVM_PROFILE_FILE", value) },
        None => unsafe { std::env::remove_var("LLVM_PROFILE_FILE") },
    }
}

#[test]
fn sweep_kiss_profraw_dir_clears_discard_dumps() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    let kiss_profraw = kiss_profraw_dir(repo);
    fs::create_dir_all(&kiss_profraw).unwrap();
    fs::write(kiss_profraw.join("default_1_0_2.profraw"), b"raw").unwrap();
    sweep_kiss_profraw_dir(repo).unwrap();
    assert!(!kiss_profraw.join("default_1_0_2.profraw").exists());
    assert!(
        kiss_profraw.is_dir(),
        "startup sweep must keep the discard dir for this process exit dump"
    );
}

#[test]
fn discover_repo_root_walks_up_to_git() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let nested = repo.join("crates").join("pkg");
    fs::create_dir_all(&nested).unwrap();
    fs::create_dir_all(repo.join(".git")).unwrap();
    assert_eq!(
        discover_repo_root(&nested).canonicalize().unwrap(),
        repo.canonicalize().unwrap()
    );
}

#[test]
fn redirect_inherited_uses_kiss_profraw_env() {
    let _env_guard = crate::rust_llvm_cov_runner::test_support::shim_test_env_lock();
    let tmp = tempfile::tempdir().unwrap();
    let kiss_profraw = tmp.path().join("profraw");
    let old_profile = std::env::var_os("LLVM_PROFILE_FILE");
    let old_kiss_profraw = std::env::var_os(KISS_PROFRAW_DIR_ENV);

    unsafe {
        std::env::set_var("LLVM_PROFILE_FILE", "/tmp/should-be-redirected.profraw");
        std::env::set_var(KISS_PROFRAW_DIR_ENV, &kiss_profraw);
    }
    redirect_inherited_llvm_profile_file(tmp.path()).unwrap();
    let redirected = std::env::var_os("LLVM_PROFILE_FILE").unwrap();
    assert!(
        redirected
            .to_string_lossy()
            .contains(DISCARD_PROFILE_PATTERN)
    );
    assert!(kiss_profraw.is_dir());

    match old_profile {
        Some(value) => unsafe { std::env::set_var("LLVM_PROFILE_FILE", value) },
        None => unsafe { std::env::remove_var("LLVM_PROFILE_FILE") },
    }
    match old_kiss_profraw {
        Some(value) => unsafe { std::env::set_var(KISS_PROFRAW_DIR_ENV, value) },
        None => unsafe { std::env::remove_var(KISS_PROFRAW_DIR_ENV) },
    }
}
