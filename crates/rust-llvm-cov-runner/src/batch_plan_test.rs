use std::path::PathBuf;

use super::{RustCoverageBatchPlan, RustCoverageBatchRequest, build_rust_coverage_batch_plan};

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
    assert_eq!(req.jobs, 4);
    assert_eq!(
        req.generated_config,
        PathBuf::from("/repo/.kiss/runs/nextest.toml")
    );
}

#[test]
fn batch_plan_uses_one_shared_build_target_and_bounded_nextest_jobs() {
    let plan = build_rust_coverage_batch_plan(&request()).unwrap();
    let build_target = "/repo/.kiss/rust_llvm_cov_cache/build/target";

    assert_eq!(plan.build_target, PathBuf::from(build_target));
    assert_eq!(plan.env["CARGO_TARGET_DIR"], build_target);
    assert_eq!(plan.env["CARGO_LLVM_COV_TARGET_DIR"], build_target);
    assert_eq!(plan.env["CARGO_LLVM_COV_BUILD_DIR"], build_target);
    assert_eq!(plan.env["NEXTEST_EXPERIMENTAL_LIBTEST_JSON"], "1");
    assert_eq!(plan.env["KEEP_ME"], "1");
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
fn batch_plan_public_data_contract_preserves_every_field() {
    let plan = RustCoverageBatchPlan::witness();
    let cloned = plan.clone();

    assert_eq!(cloned, plan);
    assert!(format!("{plan:?}").contains("RustCoverageBatchPlan"));
    assert_eq!(
        plan.build_target,
        PathBuf::from("/repo/.kiss/rust_llvm_cov_cache/build/target")
    );
    assert_eq!(plan.env["KEEP_ME"], "1");
    assert_eq!(plan.argv[0], "cargo");
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
    assert!(
        plan.argv
            .windows(2)
            .any(|args| args == ["--config-file", "/repo/.kiss/runs/nextest.toml"])
    );
    assert_eq!(
        &plan.argv[plan.argv.len() - 3..],
        ["--workspace", "--", "--exact"]
    );
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

    let mut empty_selector = request();
    empty_selector.logical_selectors.push(String::new());
    assert!(
        build_rust_coverage_batch_plan(&empty_selector)
            .unwrap_err()
            .contains("selectors")
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
