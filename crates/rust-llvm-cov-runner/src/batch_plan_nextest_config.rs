use std::path::{Path, PathBuf};

use crate::batch_plan::RustCoverageBatchRequest;
use crate::batch_shim::TARGET_RUNNER_SHIM_SUBCOMMAND;

pub(crate) fn build_nextest_config_toml(
    req: &RustCoverageBatchRequest,
    _runner_map_path: &Path,
) -> String {
    let default_filter = build_nextest_default_filter(req);
    format!(
        "[profile.kiss]\ndefault-filter = {}\nretries = 0\nfail-fast = false\n",
        toml_basic_string(&default_filter),
    )
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
    _req: &RustCoverageBatchRequest,
    _runner_map_path: &Path,
) {
    let _ = env;
}

fn build_nextest_default_filter(req: &RustCoverageBatchRequest) -> String {
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

fn nextest_filter_string(value: &str, exact: bool) -> String {
    let escaped = escape_nextest_regex(value);
    if exact {
        format!("/(^|\\$){escaped}$/")
    } else {
        format!("/{escaped}/")
    }
}

fn escape_nextest_regex(value: &str) -> String {
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

fn toml_basic_string(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .chars()
            .flat_map(|ch| ch.escape_default())
            .collect::<String>()
    )
}

fn target_runner_argv(req: &RustCoverageBatchRequest, runner_map_path: &Path) -> Vec<String> {
    let kiss_bin = target_runner_shim_program();
    let output_dir = super::batch_plan::target_runner_output_dir(req);
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

fn target_runner_shim_program() -> String {
    if let Some(path) = std::env::var_os("KISS_RUST_LLVM_COV_TARGET_RUNNER_SHIM") {
        return path.to_string_lossy().to_string();
    }
    std::env::current_exe()
        .ok()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| "kiss".to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        build_target_runner_cargo_config_toml, escape_nextest_regex, nextest_filter_string,
        nextest_test_threads, target_runner_shim_program, toml_basic_string,
    };

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

    use std::path::Path;

    #[test]
    fn target_runner_cargo_config_uses_list_runner_for_platform() {
        let req = crate::batch_plan::RustCoverageBatchRequest::witness();
        let toml = build_target_runner_cargo_config_toml(&req, Path::new("/tmp/runner-map.json"));
        assert!(toml.contains("[target.\"x86_64-unknown-linux-gnu\"]"));
        assert!(toml.contains("runner = ["));
    }

    #[test]
    fn target_runner_shim_program_honors_test_override() {
        // SAFETY: this unit test reads the variable immediately and restores it
        // before returning; no other test in this module depends on it.
        unsafe {
            std::env::set_var("KISS_RUST_LLVM_COV_TARGET_RUNNER_SHIM", "/tmp/kiss-test");
        }
        assert_eq!(target_runner_shim_program(), "/tmp/kiss-test");
        // SAFETY: see the set_var note above.
        unsafe {
            std::env::remove_var("KISS_RUST_LLVM_COV_TARGET_RUNNER_SHIM");
        }
    }

    #[test]
    fn refresh_guard_does_not_serialize_nextest_threads() {
        let req = crate::batch_plan::RustCoverageBatchRequest::witness();
        // SAFETY: this test observes the variable immediately and restores it
        // before returning; scheduling must not depend on this guard.
        unsafe {
            std::env::set_var("KISS_CHECK_RUNTIME_REFRESH_ACTIVE", "1");
        }
        assert_eq!(nextest_test_threads(&req), req.jobs.to_string());
        // SAFETY: see the set_var note above.
        unsafe {
            std::env::remove_var("KISS_CHECK_RUNTIME_REFRESH_ACTIVE");
        }
    }

    #[test]
    fn no_capture_serializes_nextest_threads() {
        let mut req = crate::batch_plan::RustCoverageBatchRequest::witness();
        req.test_args = vec!["--nocapture".to_string()];
        assert_eq!(nextest_test_threads(&req), "1");
    }
}
