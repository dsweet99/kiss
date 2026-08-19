
#[test]
fn cov_python_refresh_uses_factory_and_planning_api() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/test_runner/check_runtime_refresh_python.rs");
    let src = std::fs::read_to_string(&path).expect("read refresh python");
    assert!(
        src.contains("ensure_languages_runtime"),
        "Python cov refresh must go through ensure factory"
    );
    assert!(
        src.contains("ensure_request_for_all") || src.contains("ensure_request_for_selectors"),
        "Python cov must use shared planning API"
    );
    assert!(
        !src.contains("refresh_full_python_runtime_coverage"),
        "retired private full-refresh path must be gone"
    );
    assert!(
        !src.contains("publish_python_derived_state_with_filter"),
        "command-local python publish must go through kernel"
    );
}

#[test]
fn cov_rust_refresh_uses_ensure_factory() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/test_runner/check_runtime_refresh.rs");
    let src = std::fs::read_to_string(&path).expect("read refresh");
    assert!(
        src.contains("ensure_languages_runtime"),
        "Rust cov refresh must go through ensure factory"
    );
    assert!(
        !src.contains("refresh_full_rust_check_aggregate_labeled"),
        "retired rust aggregate full-refresh must be gone"
    );
}

#[test]
fn language_modules_route_python_and_rust_through_ensure() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/test_runner/run_logic/language_modules.rs");
    let src = std::fs::read_to_string(&path).expect("read language_modules");
    assert!(src.contains("ensure_python_via_kernel"));
    assert!(src.contains("ensure_rust_via_kernel"));
    assert!(src.contains("ensure_request_from_planned"));
    assert!(
        !src.contains("try_warm_python_cached_summary"),
        "direct try_warm_python bypass must be retired from language_modules"
    );
}
