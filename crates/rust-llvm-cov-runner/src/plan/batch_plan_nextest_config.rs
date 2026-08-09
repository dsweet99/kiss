use std::path::{Path, PathBuf};

use crate::plan::batch_plan::{CoverageOutputMode, RustCoverageBatchRequest};
use crate::plan::batch_plan_shim_const::TARGET_RUNNER_SHIM_SUBCOMMAND;

pub(crate) fn build_nextest_config_toml(
    req: &RustCoverageBatchRequest,
    _runner_map_path: &Path,
) -> String {
    let default_filter = build_nextest_default_filter(req);
    let mut toml = format!(
        "[profile.kiss]\ndefault-filter = {}\nretries = 0\nfail-fast = false\n",
        toml_basic_string(&default_filter),
    );
    super::batch_plan_nextest_timeouts::append_slow_timeout_toml(&mut toml, req);
    toml
}

pub(crate) fn runner_map_path_for_request(req: &RustCoverageBatchRequest) -> PathBuf {
    req.generated_config
        .parent()
        .map(|path| path.join("runner-map.json"))
        .unwrap_or_else(|| req.cache_root.join("runs").join("runner-map.json"))
}

pub(crate) fn test_args_request_nocapture(test_args: &[String]) -> bool {
    test_args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--nocapture" | "--no-capture"))
}

pub(crate) fn nextest_test_threads(req: &RustCoverageBatchRequest) -> String {
    if test_args_request_nocapture(&req.test_args) {
        "1".to_string()
    } else {
        req.jobs.to_string()
    }
}

pub(crate) fn target_runner_cargo_config_path(req: &RustCoverageBatchRequest) -> PathBuf {
    req.generated_config
        .parent()
        .map(|path| path.join("cargo-runner.toml"))
        .unwrap_or_else(|| req.cache_root.join("runs").join("cargo-runner.toml"))
}

pub(crate) fn build_target_runner_cargo_config_toml(
    req: &RustCoverageBatchRequest,
    runner_map_path: &Path,
) -> String {
    if matches!(
        req.coverage_output_mode,
        CoverageOutputMode::CheckAggregate { .. }
    ) {
        // No target.runner: tests run directly under nextest. Profile data goes to
        // the shared LLVM_PROFILE_FILE pool configured on the batch env.
        return String::new();
    }
    let platform = toml_basic_string(&req.host_platform);
    let runner = target_runner_argv(req, runner_map_path)
        .into_iter()
        .map(|arg| toml_basic_string(&arg))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[target.{platform}]\nrunner = [{runner}]\n")
}

pub(crate) fn apply_target_runner_env(
    env: &mut std::collections::BTreeMap<String, String>,
    req: &RustCoverageBatchRequest,
    _runner_map_path: &Path,
) {
    if !matches!(
        req.coverage_output_mode,
        CoverageOutputMode::CheckAggregate { .. }
    ) {
        return;
    }
    // cargo-llvm-cov overwrites LLVM_PROFILE_FILE; LLVM_PROFILE_FILE_NAME is
    // honored and placed under CARGO_LLVM_COV_TARGET_DIR (same as build_target).
    // Use a per-run token so stale pool-*.profraw from earlier cov runs are not
    // merged into this export (that produces empty seed-filtered object sets).
    let run_token = req
        .generated_config
        .parent()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("run");
    env.insert(
        "LLVM_PROFILE_FILE_NAME".to_string(),
        format!("{run_token}-pool-%32m.profraw"),
    );
}

fn build_nextest_default_filter(req: &RustCoverageBatchRequest) -> String {
    // A full CheckAggregate population enumerates thousands of selectors. Building
    // an OR of every test name makes nextest matching pathologically slow and
    // dominates cold `kiss cov` wall time versus native llvm-cov nextest.
    if matches!(
        req.coverage_output_mode,
        CoverageOutputMode::CheckAggregate { .. }
    ) && req.logical_selectors.len() > 64
    {
        return "all()".to_string();
    }
    let exact = rust_test_args_request_exact_match(&req.test_args);
    req.logical_selectors
        .iter()
        .map(|selector| format!("test({})", nextest_filter_string(selector, exact)))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn rust_test_args_request_exact_match(test_args: &[String]) -> bool {
    test_args.iter().any(|arg| arg == "--exact")
}

pub(crate) fn nextest_filter_string(value: &str, exact: bool) -> String {
    let escaped = escape_nextest_regex(value);
    if exact {
        format!("/(^|\\$){escaped}$/")
    } else {
        format!("/{escaped}/")
    }
}

pub(crate) fn escape_nextest_regex(value: &str) -> String {
    let mut escaped = String::new();
    for ch in value.chars() {
        match ch {
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '\\' | '.' | '^' | '$' | '|' | '?' | '*' | '+' | '(' | ')' | '[' | ']' | '{' | '}'
            | '-' | '/' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            _ => escaped.push(ch),
        }
    }
    escaped
}

pub(crate) fn toml_basic_string(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .chars()
            .flat_map(|ch| ch.escape_default())
            .collect::<String>()
    )
}

fn target_runner_argv(req: &RustCoverageBatchRequest, runner_map_path: &Path) -> Vec<String> {
    let output_dir = super::batch_plan::target_runner_output_dir(req);
    let kiss_bin = crate::plan::batch_plan_target_runner_program::target_runner_shim_program();
    vec![
        kiss_bin,
        TARGET_RUNNER_SHIM_SUBCOMMAND.to_string(),
        "--output-dir".to_string(),
        output_dir.to_string_lossy().to_string(),
        "--runner-map".to_string(),
        runner_map_path.to_string_lossy().to_string(),
        "--platform".to_string(),
        req.host_platform.clone(),
        "--".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::{
        build_target_runner_cargo_config_toml, escape_nextest_regex, nextest_filter_string,
        nextest_test_threads, toml_basic_string,
    };
    use std::path::Path;

    #[test]
    fn escaping_preserves_nextest_and_toml_string_boundaries() {
        let value = "quote\"slash\\line\n";

        assert_eq!(escape_nextest_regex(value), r#"quote"slash\\line\n"#);
        assert_eq!(
            nextest_filter_string(value, false),
            r#"/quote"slash\\line\n/"#
        );
        assert_eq!(
            nextest_filter_string(value, true),
            r#"/(^|\$)quote"slash\\line\n$/"#
        );
        assert_eq!(toml_basic_string(value), r#""quote\"slash\\line\n""#);
    }

    #[test]
    fn target_runner_cargo_config_uses_list_runner_for_platform() {
        let req = crate::plan::batch_plan::RustCoverageBatchRequest::witness();
        let toml = build_target_runner_cargo_config_toml(&req, Path::new("/tmp/runner-map.json"));
        assert!(toml.contains("[target.\"x86_64-unknown-linux-gnu\"]"));
        assert!(toml.contains("runner = ["));
    }

    #[test]
    fn check_aggregate_large_selector_set_uses_all_filter() {
        let mut req = crate::plan::batch_plan::RustCoverageBatchRequest::witness();
        req.coverage_output_mode = crate::plan::batch_plan::CoverageOutputMode::CheckAggregate {
            publication_binary_ids: None,
            repair_publication: None,
        };
        req.test_args.clear();
        req.logical_selectors = (0..100).map(|i| format!("test_{i}")).collect();
        let plan = crate::plan::batch_plan::build_rust_coverage_batch_plan(&req).unwrap();
        assert!(
            plan.generated_config_toml.contains("default-filter = \"all()\""),
            "toml={}",
            plan.generated_config_toml
        );
        assert!(!plan.generated_config_toml.contains("test(/test_0/)"));
    }

    #[test]
    fn check_aggregate_plan_omits_target_runner_and_sets_profile_pool() {
        let mut req = crate::plan::batch_plan::RustCoverageBatchRequest::witness();
        req.coverage_output_mode = crate::plan::batch_plan::CoverageOutputMode::CheckAggregate {
            publication_binary_ids: None,
            repair_publication: None,
        };
        let plan = crate::plan::batch_plan::build_rust_coverage_batch_plan(&req).unwrap();
        assert!(
            plan.target_runner_cargo_config_toml.is_empty(),
            "toml={}",
            plan.target_runner_cargo_config_toml
        );
        assert!(
            plan.env
                .get("LLVM_PROFILE_FILE_NAME")
                .is_some_and(|name| name.ends_with("-pool-%32m.profraw")),
            "env={:?}",
            plan.env.get("LLVM_PROFILE_FILE_NAME")
        );
        assert_eq!(
            plan.env.get("LLVM_PROFILE_FILE").map(String::as_str),
            Some("/repo/.kiss/profraw/default_%m_%p.profraw")
        );
        assert_eq!(
            plan.env
                .get(crate::kiss_profraw::KISS_PROFRAW_DIR_ENV)
                .map(String::as_str),
            Some("/repo/.kiss/profraw")
        );
        assert!(!plan.argv.iter().any(|arg| arg.contains("__rust-llvm-cov-target-runner")));
    }

    #[test]
    fn refresh_guard_does_not_serialize_nextest_threads() {
        let req = crate::plan::batch_plan::RustCoverageBatchRequest::witness();
        // SAFETY: this test observes the variable immediately and restores it
        // before returning; scheduling must not depend on this guard.
        unsafe {
            std::env::set_var("KISS_COVERAGE_RUNTIME_REFRESH_ACTIVE", "1");
        }
        assert_eq!(nextest_test_threads(&req), req.jobs.to_string());
        // SAFETY: see the set_var note above.
        unsafe {
            std::env::remove_var("KISS_COVERAGE_RUNTIME_REFRESH_ACTIVE");
        }
    }

    #[test]
    fn no_capture_serializes_nextest_threads() {
        let mut req = crate::plan::batch_plan::RustCoverageBatchRequest::witness();
        req.test_args = vec!["--nocapture".to_string()];
        assert_eq!(nextest_test_threads(&req), "1");
    }
}
