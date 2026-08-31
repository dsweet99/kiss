use std::path::PathBuf;

use crate::rust_llvm_cov_runner::{
    RustCoverageBatchPlan, RustCoverageBatchRequest, build_rust_coverage_batch_plan,
    publish_generated_nextest_config,
};

fn request() -> RustCoverageBatchRequest {
    RustCoverageBatchRequest::witness()
}

#[test]
fn batch_request_public_data_contract_preserves_every_field() {
    let req = request();
    let cloned = req.clone();

    assert_eq!(cloned, req);
    assert!(format!("{req:?}").contains("RustCoverageBatchRequest"));
    assert_eq!(req.cwd, PathBuf::from("/repo"));
    assert_eq!(req.source_root, PathBuf::from("/repo"));
    assert_eq!(req.cargo, PathBuf::from("cargo"));
    assert_eq!(
        req.cache_root,
        PathBuf::from("/repo/.kiss/rust_llvm_cov_cache")
    );
    assert_eq!(req.logical_selectors, ["alpha", "beta"]);
    assert_eq!(req.cargo_args, ["--workspace"]);
    assert_eq!(req.test_args, ["--exact"]);
    assert_eq!(req.env["KEEP_ME"], "1");
    assert!(req.force_rerun);
    assert_eq!(req.jobs, 4);
    assert_eq!(
        req.generated_config,
        PathBuf::from("/repo/.kiss/rust_llvm_cov_cache/runs/run-witness/nextest.toml")
    );
}

#[test]
fn batch_plan_uses_one_shared_build_target_and_bounded_nextest_jobs() {
    let plan = build_rust_coverage_batch_plan(&request()).unwrap();
    let build_target = "/repo/target";

    assert_eq!(plan.build_target, PathBuf::from(build_target));
    assert_eq!(plan.env["CARGO_TARGET_DIR"], build_target);
    assert_eq!(plan.env["CARGO_LLVM_COV_TARGET_DIR"], build_target);
    assert_eq!(plan.env["CARGO_LLVM_COV_BUILD_DIR"], build_target);
    assert_eq!(plan.env["NEXTEST_EXPERIMENTAL_LIBTEST_JSON"], "1");
    assert_eq!(plan.env["KEEP_ME"], "1");
    assert!(
        plan.target_runner_cargo_config_toml
            .contains("__rust-llvm-cov-target-runner")
    );
    assert!(
        plan.argv
            .windows(2)
            .any(|args| args == ["--build-jobs", "4"])
    );
    assert!(
        plan.argv
            .windows(2)
            .any(|args| args == ["--test-threads", "4"])
    );
}

#[test]
fn batch_plan_uses_serial_nextest_threads_when_nocapture_is_requested() {
    for no_capture_arg in ["--nocapture", "--no-capture"] {
        let mut req = request();
        req.test_args = vec![no_capture_arg.to_string()];

        let plan = build_rust_coverage_batch_plan(&req).unwrap();

        assert!(
            plan.argv
                .windows(2)
                .any(|args| args == ["--build-jobs", "4"])
        );
        assert!(
            plan.argv
                .windows(2)
                .any(|args| args == ["--test-threads", "1"])
        );
    }
}

#[test]
fn batch_plan_public_data_contract_preserves_every_field() {
    let plan = RustCoverageBatchPlan::witness();
    let cloned = plan.clone();

    assert_eq!(cloned, plan);
    assert!(format!("{plan:?}").contains("RustCoverageBatchPlan"));
    assert_eq!(plan.build_target, PathBuf::from("/repo/target"));
    assert_eq!(
        plan.target_runner_output_dir,
        PathBuf::from("/repo/.kiss/rust_llvm_cov_cache/runs/run-witness/instances")
    );
    assert_eq!(plan.env["KEEP_ME"], "1");
    assert_eq!(
        plan.env[crate::rust_llvm_cov_runner::kiss_profraw::KISS_PROFRAW_DIR_ENV],
        "/repo/.kiss/profraw"
    );
    assert_eq!(
        plan.env["LLVM_PROFILE_FILE"],
        "/repo/.kiss/profraw/default_%m_%p.profraw"
    );
    assert_eq!(plan.argv[0], "cargo");
    assert!(plan.generated_config_toml.contains("[profile.kiss]"));
    assert!(plan.target_runner_cargo_config_toml.contains("runner = ["));
    assert!(
        plan.target_runner_cargo_config_toml
            .contains("__rust-llvm-cov-target-runner")
    );
    assert!(plan.argv.windows(2).any(|args| {
        args[0] == "--config"
            && args[1] == "/repo/.kiss/rust_llvm_cov_cache/runs/run-witness/cargo-runner.toml"
    }));
}

#[test]
fn batch_plan_constructs_nextest_command_without_legacy_no_clean() {
    let plan = build_rust_coverage_batch_plan(&request()).unwrap();

    assert_eq!(plan.argv[0..3], ["cargo", "llvm-cov", "nextest"]);
    assert!(plan.argv.contains(&"--no-report".to_string()));
    assert!(!plan.argv.contains(&"--no-clean".to_string()));
    assert!(
        plan.argv
            .windows(2)
            .any(|args| args == ["--message-format-version", "0.1"])
    );
    assert!(plan.argv.windows(2).any(|args| args
        == [
            "--config-file",
            "/repo/.kiss/rust_llvm_cov_cache/runs/run-witness/nextest.toml",
        ]));
    assert_eq!(
        &plan.argv[plan.argv.len() - 3..],
        ["--workspace", "--", "--exact"]
    );
}

#[test]
fn publish_generated_nextest_config_writes_run_scoped_config_atomically() {
    let tmp = tempfile::tempdir().unwrap();
    let mut req = request();
    req.cache_root = tmp.path().join(".kiss").join("rust_llvm_cov_cache");
    req.generated_config = req
        .cache_root
        .join("runs")
        .join("run-123")
        .join("nextest.toml");
    let plan = build_rust_coverage_batch_plan(&req).unwrap();

    publish_generated_nextest_config(&plan, &req).unwrap();

    assert_eq!(
        std::fs::read_to_string(&plan.generated_config).unwrap(),
        plan.generated_config_toml
    );
    assert!(
        plan.generated_config
            .starts_with(req.cache_root.join("runs"))
    );
    assert!(!plan.generated_config.to_string_lossy().contains("slot-"));
}

#[test]
fn batch_plan_generates_escaped_nextest_filter_config() {
    let mut req = request();
    req.logical_selectors = vec![
        "alpha::case".to_string(),
        "quote\"slash\\case".to_string(),
        "line\nbreak".to_string(),
        "foo\") | all() | test(\"bar".to_string(),
    ];
    req.test_args = Vec::new();

    let plan = build_rust_coverage_batch_plan(&req).unwrap();

    assert!(plan.generated_config_toml.contains("[profile.kiss]"));
    assert!(plan.generated_config_toml.contains(
        r#"default-filter = "test(/alpha::case/) | test(/quote\"slash\\\\case/) | test(/line\\nbreak/) | test(/foo\"\\) \\| all\\(\\) \\| test\\(\"bar/)""#
    ));

    req.test_args = vec!["--exact".to_string()];
    let exact_plan = build_rust_coverage_batch_plan(&req).unwrap();

    assert!(exact_plan.generated_config_toml.contains(
        r#"default-filter = "test(/(^|\\$)alpha::case$/) | test(/(^|\\$)quote\"slash\\\\case$/) | test(/(^|\\$)line\\nbreak$/) | test(/(^|\\$)foo\"\\) \\| all\\(\\) \\| test\\(\"bar$/)""#
    ));
}

#[test]
fn batch_plan_rejects_zero_jobs_and_empty_selectors_before_mutation() {
    let mut zero_jobs = request();
    zero_jobs.jobs = 0;
    assert!(
        build_rust_coverage_batch_plan(&zero_jobs)
            .unwrap_err()
            .contains("jobs")
    );

    let mut no_selectors = request();
    no_selectors.logical_selectors = Vec::new();
    assert!(
        build_rust_coverage_batch_plan(&no_selectors)
            .unwrap_err()
            .contains("selectors")
    );

    let mut list_build = request();
    list_build.logical_selectors.clear();
    list_build.population_publication_selectors = Some(Vec::new());
    let list_plan = build_rust_coverage_batch_plan(&list_build).expect("list-build");
    assert!(
        list_plan.generated_config_toml.contains("all()"),
        "workspace list-build must compile the full suite: {}",
        list_plan.generated_config_toml
    );

    let mut empty_selector = request();
    empty_selector.logical_selectors.push(String::new());
    assert!(
        build_rust_coverage_batch_plan(&empty_selector)
            .unwrap_err()
            .contains("selectors")
    );

    let mut duplicate_selector = request();
    duplicate_selector
        .logical_selectors
        .push(duplicate_selector.logical_selectors[0].clone());
    assert!(
        build_rust_coverage_batch_plan(&duplicate_selector)
            .unwrap_err()
            .contains("duplicate logical selector")
    );
}

#[test]
fn batch_plan_accepts_required_supported_test_arguments() {
    let mut req = request();
    req.test_args = vec![
        "--exact".to_string(),
        "--nocapture".to_string(),
        "--no-capture".to_string(),
        "--ignored".to_string(),
        "--include-ignored".to_string(),
        "--skip".to_string(),
        "slow_case".to_string(),
        "--skip".to_string(),
        "flaky_case".to_string(),
    ];

    let plan = build_rust_coverage_batch_plan(&req).unwrap();

    assert_eq!(
        &plan.argv[plan.argv.len() - req.test_args.len() - 1..],
        [
            "--",
            "--exact",
            "--nocapture",
            "--no-capture",
            "--ignored",
            "--include-ignored",
            "--skip",
            "slow_case",
            "--skip",
            "flaky_case"
        ]
    );
}

#[test]
fn batch_plan_rejects_unsupported_test_arguments_before_mutation() {
    for unsupported in [
        vec!["--format".to_string(), "json".to_string()],
        vec!["--test-threads".to_string(), "8".to_string()],
        vec!["--".to_string()],
        vec!["--skip".to_string()],
        vec!["--skip".to_string(), String::new()],
    ] {
        let mut req = request();
        req.test_args = unsupported;

        let err = build_rust_coverage_batch_plan(&req).unwrap_err();

        assert!(err.contains("unsupported Rust test argument") || err.contains("--skip"));
    }
}

#[test]
fn batch_plan_rejects_cargo_args_that_override_compile_once_controls() {
    for cargo_args in [
        vec!["--target-dir".to_string(), "/tmp/other-target".to_string()],
        vec!["--target-dir=/tmp/other-target".to_string()],
        vec!["--jobs".to_string(), "99".to_string()],
        vec!["-j".to_string(), "99".to_string()],
        vec!["-j99".to_string()],
        vec![
            "--config".to_string(),
            "build.target-dir='/tmp/other'".to_string(),
        ],
        vec![
            "--config".to_string(),
            "[build]\ntarget-dir = '/tmp/other'".to_string(),
        ],
        vec![
            "--config".to_string(),
            "[build]\nrustflags = []\ntarget-dir = '/tmp/other'".to_string(),
        ],
        vec!["--config=build.jobs=99".to_string()],
        vec!["--config=build = { jobs = 99 }".to_string()],
        vec!["--config=build = { rustflags = [], jobs = 99 }".to_string()],
        vec!["--config=build = { env = { RUSTFLAGS = '' }, jobs = 99 }".to_string()],
        vec![
            "--config".to_string(),
            r#""bui\u006cd"."jo\u0062s" = 99"#.to_string(),
        ],
        vec![
            "--config".to_string(),
            r#""b\U00000075ild"."jobs" = 99"#.to_string(),
        ],
    ] {
        let mut req = request();
        req.cargo_args = cargo_args;

        let err = build_rust_coverage_batch_plan(&req).unwrap_err();

        assert!(err.contains("unsupported Rust cargo argument"));
    }
}

#[test]
fn batch_plan_accepts_safe_cargo_config_values() {
    for cargo_args in [
        vec!["--config".to_string(), "/tmp/other-config.toml".to_string()],
        vec!["--config=/tmp/other-config.toml".to_string()],
        vec![
            "--config".to_string(),
            "net.git-fetch-with-cli=true".to_string(),
        ],
        vec![
            "--config".to_string(),
            r#""bui\u006cd"."rust\u0066lags" = ["--cfg", "kiss"]"#.to_string(),
        ],
    ] {
        let mut req = request();
        req.cargo_args = cargo_args;

        build_rust_coverage_batch_plan(&req).unwrap();
    }
}

#[test]
fn batch_plan_rejects_cargo_args_that_override_nextest_controls() {
    for cargo_args in [
        vec!["--profile".to_string(), "other".to_string()],
        vec!["--profile=other".to_string()],
        vec!["--config-file".to_string(), "/tmp/nextest.toml".to_string()],
        vec!["--message-format".to_string(), "human".to_string()],
        vec!["--message-format=json".to_string()],
        vec!["--message-format-version".to_string(), "0.2".to_string()],
        vec!["--message-format-version=0.2".to_string()],
        vec!["--test-threads".to_string(), "99".to_string()],
        vec!["--test-threads=99".to_string()],
        vec!["--no-fail-fast".to_string()],
        vec!["--retries".to_string(), "3".to_string()],
        vec!["--no-tests".to_string(), "fail".to_string()],
        vec!["--no-tests=fail".to_string()],
    ] {
        let mut req = request();
        req.cargo_args = cargo_args;

        let err = build_rust_coverage_batch_plan(&req).unwrap_err();

        assert!(err.contains("unsupported Rust cargo argument"));
    }
}

#[test]
fn batch_plan_rejects_empty_cargo_config_values_before_mutation() {
    for cargo_args in [
        vec!["--config".to_string()],
        vec!["--config".to_string(), String::new()],
        vec!["--config=".to_string()],
    ] {
        let mut req = request();
        req.cargo_args = cargo_args;

        let err = build_rust_coverage_batch_plan(&req).unwrap_err();

        assert!(err.contains("--config requires a non-empty value"));
    }
}
