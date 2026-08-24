use super::{PROFILE_POOL_ENV, PROFILE_POOL_FILE_PATTERN, profile_path_for_instance};
use std::ffi::OsString;
use std::path::Path;

#[test]
fn profile_path_uses_per_binary_pool_pattern_when_env_set() {
    let _lock = crate::test_support::shim_test_env_lock();
    let old = std::env::var_os(PROFILE_POOL_ENV);

    unsafe { std::env::set_var(PROFILE_POOL_ENV, "1") };
    let command = [OsString::from("/repo/target/debug/deps/covers_lib-abc")];
    let path = profile_path_for_instance(Path::new("/tmp/out"), "instance", &command);
    assert_eq!(
        path,
        Path::new("/tmp/out")
            .join("_repo_target_debug_deps_covers_lib-abc")
            .join(PROFILE_POOL_FILE_PATTERN)
    );

    unsafe {
        match old {
            Some(value) => std::env::set_var(PROFILE_POOL_ENV, value),
            None => std::env::remove_var(PROFILE_POOL_ENV),
        }
    }
}

#[test]
fn profile_pool_dirs_differ_across_test_binaries() {
    let _lock = crate::test_support::shim_test_env_lock();
    let old = std::env::var_os(PROFILE_POOL_ENV);

    unsafe { std::env::set_var(PROFILE_POOL_ENV, "1") };
    let left = profile_path_for_instance(
        Path::new("/tmp/out"),
        "a",
        &[OsString::from("/bin/covers_lib-1")],
    );
    let right = profile_path_for_instance(
        Path::new("/tmp/out"),
        "b",
        &[OsString::from("/bin/covers_stable-2")],
    );
    assert_ne!(left.parent(), right.parent());
    assert_eq!(
        left.file_name().and_then(|name| name.to_str()),
        Some(PROFILE_POOL_FILE_PATTERN)
    );

    unsafe {
        match old {
            Some(value) => std::env::set_var(PROFILE_POOL_ENV, value),
            None => std::env::remove_var(PROFILE_POOL_ENV),
        }
    }
}

#[test]
fn profile_path_uses_instance_file_when_pool_disabled() {
    let _lock = crate::test_support::shim_test_env_lock();
    let old = std::env::var_os(PROFILE_POOL_ENV);

    unsafe { std::env::remove_var(PROFILE_POOL_ENV) };
    let path = profile_path_for_instance(
        Path::new("/tmp/out"),
        "instance",
        &[OsString::from("/bin/unused")],
    );
    assert_eq!(path, Path::new("/tmp/out/instance.profraw"));

    unsafe {
        match old {
            Some(value) => std::env::set_var(PROFILE_POOL_ENV, value),
            None => std::env::remove_var(PROFILE_POOL_ENV),
        }
    }
}
