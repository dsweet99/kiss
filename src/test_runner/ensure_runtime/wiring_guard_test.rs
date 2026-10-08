#[test]
fn language_modules_route_python_and_rust_through_ensure() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/test_runner");
    let python = std::fs::read_to_string(root.join("lang_python/executor.rs"))
        .expect("read python executor");
    let rust =
        std::fs::read_to_string(root.join("lang_rust/executor.rs")).expect("read rust executor");
    assert!(python.contains("ensure_language_via_kernel"));
    assert!(rust.contains("ensure_language_via_kernel"));
    assert!(python.contains("Language::Python"));
    assert!(rust.contains("Language::Rust"));
    let shared = std::fs::read_to_string(root.join("ensure_runtime/planning.rs"))
        .expect("read shared ensure");
    assert!(shared.contains("ensure_request_from_planned"));
    assert!(
        !python.contains("try_warm_python_cached_summary"),
        "direct try_warm_python bypass must be retired from the python executor"
    );
}

#[test]
fn rust_runs_go_through_nextest_and_forward_force_selectors() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/test_runner/lang_rust/runtime.rs");
    let src = std::fs::read_to_string(&path).expect("read rust runtime");
    assert!(
        src.contains("run_nextest_selectors("),
        "Rust selectors must run through cargo nextest"
    );
    let kernel = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/test_runner/ensure_runtime/kernel.rs"),
    )
    .expect("read kernel");
    assert!(
        kernel.contains("&request.force_selectors"),
        "the kernel must run force_selectors for --retry-bad"
    );
}
