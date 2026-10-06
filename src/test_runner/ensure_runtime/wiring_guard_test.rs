#[test]
fn language_modules_route_python_and_rust_through_ensure() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/test_runner");
    let python = std::fs::read_to_string(root.join("lang_python/executor.rs"))
        .expect("read python executor");
    let rust =
        std::fs::read_to_string(root.join("lang_rust/executor.rs")).expect("read rust executor");
    assert!(python.contains("ensure_python_via_kernel"));
    assert!(rust.contains("ensure_rust_via_kernel"));
    assert!(python.contains("ensure_request_from_planned"));
    assert!(rust.contains("ensure_request_from_planned"));
    assert!(
        !python.contains("try_warm_python_cached_summary"),
        "direct try_warm_python bypass must be retired from the python executor"
    );
}

#[test]
fn rust_all_mode_records_per_test_coverage_and_forwards_force_selectors() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/test_runner/lang_rust/runtime.rs");
    let src = std::fs::read_to_string(&path).expect("read rust runtime");
    assert!(
        src.contains("run_rust_llvm_cov_selectors_streaming(")
            && src.contains("CoverageOutputMode::SelectorEntries"),
        "population runs must export per-test coverage"
    );
    assert!(
        !src.contains("run_rust_llvm_cov_check_aggregate_selectors"),
        "pooled check-aggregate coverage leaves the first edit after a cold run unable to select"
    );
    assert!(
        src.contains("&request.force_selectors"),
        "All mode must forward force_selectors for --retry-bad"
    );
    let cov = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/test_runner/lang_rust/llvm_cov/mod.rs");
    let cov_src = std::fs::read_to_string(&cov).expect("read llvm_cov mod");
    assert!(
        !cov_src.contains("force_rerun_selectors: &[],"),
        "check-aggregate publication helper must not hardcode empty force_rerun_selectors"
    );
}
