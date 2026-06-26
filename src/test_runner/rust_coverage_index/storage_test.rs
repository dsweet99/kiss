use std::path::Path;

#[test]
fn command_stdout_reports_success_and_failure() {
    let text = crate::test_runner::rust_coverage_index::storage::command_stdout(
        Path::new("printf"),
        &["ok"],
        Path::new("."),
    )
    .unwrap();

    assert_eq!(text, "ok");
    assert!(
        crate::test_runner::rust_coverage_index::storage::command_stdout(
            Path::new("/definitely/not/a/command"),
            &[],
            Path::new(".")
        )
        .is_err()
    );
}

#[test]
fn is_cargo_config_input_path_matches_only_cargo_configs() {
    assert!(
        crate::test_runner::rust_coverage_index::storage::is_cargo_config_input_path(Path::new(
            ".cargo/config"
        ))
    );
    assert!(
        crate::test_runner::rust_coverage_index::storage::is_cargo_config_input_path(Path::new(
            ".cargo/config.toml"
        ))
    );
    assert!(
        !crate::test_runner::rust_coverage_index::storage::is_cargo_config_input_path(Path::new(
            "config.toml"
        ))
    );
}

#[test]
fn fnv1a64_is_stable_and_order_sensitive() {
    let h0 = 0xcbf2_9ce4_8422_2325;

    assert_eq!(
        crate::test_runner::rust_coverage_index::storage::fnv1a64(h0, &[]),
        h0
    );
    assert_ne!(
        crate::test_runner::rust_coverage_index::storage::fnv1a64(h0, b"a"),
        h0
    );
    assert_ne!(
        crate::test_runner::rust_coverage_index::storage::fnv1a64(h0, b"ab"),
        crate::test_runner::rust_coverage_index::storage::fnv1a64(h0, b"ba")
    );
}
