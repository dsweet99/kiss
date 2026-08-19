
#[test]
fn lang_iface_has_no_python_or_rust_impl_files() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/test_runner/lang_iface");
    let entries: Vec<_> = std::fs::read_dir(&root)
        .expect("lang_iface dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        !entries.iter().any(|n| n.contains("python") || n.contains("rust")),
        "lang_iface must stay language-neutral; found {entries:?}"
    );
}

#[test]
fn language_packages_own_generation_and_llvm_cov_homes() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/test_runner");
    assert!(
        root.join("lang_python/generation/mod.rs").is_file(),
        "Python generation must live under lang_python/"
    );
    assert!(
        !root.join("python_coverage_index/generation/mod.rs").is_file(),
        "generation must not remain under python_coverage_index/"
    );
    assert!(
        root.join("lang_rust/llvm_cov/mod.rs").is_file(),
        "Rust llvm-cov adapter home must exist under lang_rust/llvm_cov/"
    );
    assert!(
        !root.join("rust_llvm_cov.rs").is_file(),
        "rust_llvm_cov.rs must move under lang_rust/llvm_cov/"
    );
    assert!(
        root.join("lang_python/collect.rs").is_file(),
        "Python collect adapter must live under lang_python/"
    );
    assert!(
        root.join("lang_rust/workspace.rs").is_file(),
        "Rust workspace adapter must live under lang_rust/"
    );
    assert!(
        root.join("ensure_runtime/planning.rs").is_file(),
        "shared EnsureRequest planning API must exist"
    );
}
