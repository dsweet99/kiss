#[test]
fn worker_tmp_parent_and_digest_helpers_are_stable() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().join("cache");
    std::fs::create_dir_all(&cache_root).unwrap();
    let parent = crate::rust_llvm_cov_runner::rust_cov_cache_tmp_parent(&cache_root);
    assert!(parent.to_string_lossy().contains("kiss-rust-llvm-cov"));
    assert_eq!(
        crate::rust_llvm_cov_runner::execute_or_reuse::worker::hex_lower(&[0xab, 0xcd]),
        "abcd"
    );
    assert_eq!(
        crate::rust_llvm_cov_runner::execute_or_reuse::worker::os_str_bytes(std::ffi::OsStr::new(
            "ab"
        )),
        b"ab".to_vec()
    );
    let digest =
        crate::rust_llvm_cov_runner::execute_or_reuse::worker::cache_root_digest(&cache_root);
    assert_eq!(digest.len(), 64);
}
