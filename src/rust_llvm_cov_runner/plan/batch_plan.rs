use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::rust_llvm_cov_runner::RustTestBinaryIdentity;
use crate::rust_llvm_cov_runner::plan::batch_plan_env::ensure_coverage_link_build_id;
use crate::rust_llvm_cov_runner::plan::batch_plan_nextest_config::build_nextest_config_toml;
use crate::rust_llvm_cov_runner::plan::batch_plan_test_args::validate_supported_rust_test_args;

pub use crate::rust_llvm_cov_runner::plan::batch_plan_coverage_mode::{
    CheckAggregateRepairPublication, CoverageOutputMode,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RustCoverageBatchRequest {
    pub cwd: PathBuf,
    pub source_root: PathBuf,
    pub cargo: PathBuf,
    pub cache_root: PathBuf,
    pub logical_selectors: Vec<String>,
    pub cargo_args: Vec<String>,
    pub test_args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub force_rerun: bool,
    pub jobs: usize,
    pub generated_config: PathBuf,
    pub population_publication_selectors: Option<Vec<String>>,
    pub delegated_runners: crate::rust_llvm_cov_runner::plan::batch_runner_resolve::DelegatedRunnerMap,
    pub runner_map_fingerprint: String,
    pub host_platform: String,
    pub coverage_output_mode: CoverageOutputMode,
    pub selector_timeout_millis: BTreeMap<String, u64>,
}

#[cfg(test)]
impl RustCoverageBatchRequest {
    pub(crate) fn witness() -> Self {
        Self {
            cwd: PathBuf::from("/repo"),
            source_root: PathBuf::from("/repo"),
            cargo: PathBuf::from("cargo"),
            cache_root: PathBuf::from("/repo/.kiss/rust_llvm_cov_cache"),
            logical_selectors: vec!["alpha".to_string(), "beta".to_string()],
            cargo_args: vec!["--workspace".to_string()],
            test_args: vec!["--exact".to_string()],
            env: BTreeMap::from([("KEEP_ME".to_string(), "1".to_string())]),
            force_rerun: true,
            jobs: 4,
            generated_config: PathBuf::from(
                "/repo/.kiss/rust_llvm_cov_cache/runs/run-witness/nextest.toml",
            ),
            population_publication_selectors: None,
            delegated_runners: BTreeMap::from([(
                "x86_64-unknown-linux-gnu".to_string(),
                Vec::new(),
            )]),
            runner_map_fingerprint: "0000000000000000".to_string(),
            host_platform: "x86_64-unknown-linux-gnu".to_string(),
            coverage_output_mode: CoverageOutputMode::SelectorEntries,
            selector_timeout_millis: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RustCoverageBatchPlan {
    pub build_target: PathBuf,
    pub target_runner_output_dir: PathBuf,
    pub runner_map_path: PathBuf,
    pub env: BTreeMap<String, String>,
    pub argv: Vec<String>,
    pub generated_config: PathBuf,
    pub generated_config_toml: String,
    pub target_runner_cargo_config: PathBuf,
    pub target_runner_cargo_config_toml: String,
    pub output_channel_relay_live: bool,
}

#[cfg(test)]
impl RustCoverageBatchPlan {
    pub(crate) fn witness() -> Self {
        build_rust_coverage_batch_plan(&RustCoverageBatchRequest::witness()).unwrap()
    }
}

pub fn build_rust_coverage_batch_plan(
    req: &RustCoverageBatchRequest,
) -> Result<RustCoverageBatchPlan, String> {
    validate_batch_request(req)?;
    let build_target = req.source_root.join("target");
    let target_runner_output_dir = target_runner_output_dir(req);
    let runner_map_path = super::batch_plan_nextest_config::runner_map_path_for_request(req);
    let mut env = req.env.clone();
    ensure_coverage_link_build_id(&mut env);

    env.remove("KISS_RUST_COVERAGE_PROFILE_POOL");

    crate::rust_llvm_cov_runner::kiss_profraw::ensure_kiss_profraw_env(&mut env, &req.source_root);
    let build_target_value = build_target.to_string_lossy().to_string();
    env.insert(
        "NEXTEST_EXPERIMENTAL_LIBTEST_JSON".to_string(),
        "1".to_string(),
    );
    env.insert("CARGO_TARGET_DIR".to_string(), build_target_value.clone());
    env.insert(
        "CARGO_LLVM_COV_TARGET_DIR".to_string(),
        build_target_value.clone(),
    );
    env.insert("CARGO_LLVM_COV_BUILD_DIR".to_string(), build_target_value);
    super::batch_plan_nextest_config::apply_target_runner_env(&mut env, req, &runner_map_path);

    let target_runner_cargo_config =
        super::batch_plan_nextest_config::target_runner_cargo_config_path(req);
    let target_runner_cargo_config_toml =
        super::batch_plan_nextest_config::build_target_runner_cargo_config_toml(
            req,
            &runner_map_path,
        );

    let jobs = req.jobs.to_string();
    let test_threads = super::batch_plan_nextest_config::nextest_test_threads(req);
    let mut argv = vec![
        req.cargo.to_string_lossy().to_string(),
        "llvm-cov".to_string(),
        "nextest".to_string(),
        "--no-report".to_string(),
        "--build-jobs".to_string(),
        jobs.clone(),
        "--test-threads".to_string(),
        test_threads,
        "--no-fail-fast".to_string(),
        "--retries".to_string(),
        "0".to_string(),
        "--no-tests".to_string(),
        "pass".to_string(),
        "--cargo-message-format".to_string(),
        "json".to_string(),
        "--message-format".to_string(),
        "libtest-json-plus".to_string(),
        "--message-format-version".to_string(),
        "0.1".to_string(),
        "--show-progress".to_string(),
        "none".to_string(),
        "--status-level".to_string(),
        "none".to_string(),
        "--final-status-level".to_string(),
        "none".to_string(),
        "--failure-output".to_string(),
        "never".to_string(),
        "--success-output".to_string(),
        "never".to_string(),
        "--user-config-file".to_string(),
        "none".to_string(),
        "--config".to_string(),
        target_runner_cargo_config.to_string_lossy().to_string(),
        "--config-file".to_string(),
        req.generated_config.to_string_lossy().to_string(),
        "--profile".to_string(),
        "kiss".to_string(),
    ];
    argv.extend(req.cargo_args.iter().cloned());
    if !req.test_args.is_empty() {
        argv.push("--".to_string());
        argv.extend(req.test_args.iter().cloned());
    }

    let generated_config_toml = build_nextest_config_toml(req, &runner_map_path);
    Ok(RustCoverageBatchPlan {
        build_target,
        target_runner_output_dir,
        runner_map_path,
        env,
        argv,
        generated_config: req.generated_config.clone(),
        generated_config_toml,
        target_runner_cargo_config,
        target_runner_cargo_config_toml,
        output_channel_relay_live: super::batch_plan_nextest_config::test_args_request_nocapture(
            &req.test_args,
        ),
    })
}

pub(crate) fn target_runner_output_dir(req: &RustCoverageBatchRequest) -> PathBuf {
    req.generated_config
        .parent()
        .map(|path| path.join("instances"))
        .unwrap_or_else(|| req.cache_root.join("runs").join("instances"))
}

fn validate_batch_request(req: &RustCoverageBatchRequest) -> Result<(), String> {
    if req.jobs == 0 {
        return Err("jobs must be greater than zero".to_string());
    }
    if req.logical_selectors.is_empty()
        || req
            .logical_selectors
            .iter()
            .any(|selector| selector.is_empty())
    {
        return Err("logical selectors must not be empty".to_string());
    }
    let mut seen_selectors = BTreeSet::new();
    for selector in &req.logical_selectors {
        if !seen_selectors.insert(selector) {
            return Err(format!("duplicate logical selector: {selector}"));
        }
    }
    validate_supported_rust_cargo_args(&req.cargo_args)?;
    validate_supported_rust_test_args(&req.test_args)?;
    Ok(())
}

pub fn validate_supported_rust_cargo_args(cargo_args: &[String]) -> Result<(), String> {
    let mut index = 0;
    while index < cargo_args.len() {
        index = next_supported_cargo_arg_index(cargo_args, index)?;
    }
    Ok(())
}

fn next_supported_cargo_arg_index(cargo_args: &[String], index: usize) -> Result<usize, String> {
    let arg = &cargo_args[index];
    match arg.as_str() {
        "--target-dir" | "--jobs" | "-j" => Err(unsupported_cargo_arg_error(arg)),
        "--config" => validate_split_cargo_config_arg(cargo_args, index),
        _ if arg.starts_with("--target-dir=")
            || arg.starts_with("--jobs=")
            || (arg.starts_with("-j") && arg.len() > "-j".len()) =>
        {
            Err(unsupported_cargo_arg_error(arg))
        }
        _ if arg.starts_with("--config=") => validate_inline_cargo_config_arg(arg, index),
        _ if overrides_nextest_batch_controls(arg) => Err(unsupported_cargo_arg_error(arg)),
        _ => Ok(index + 1),
    }
}

fn validate_split_cargo_config_arg(cargo_args: &[String], index: usize) -> Result<usize, String> {
    match cargo_args.get(index + 1) {
        Some(value) => {
            validate_cargo_config_value("--config", value)?;
            Ok(index + 2)
        }
        None => Err(cargo_config_value_error()),
    }
}

fn validate_inline_cargo_config_arg(arg: &str, index: usize) -> Result<usize, String> {
    let value = arg.trim_start_matches("--config=");
    validate_cargo_config_value(arg, value)?;
    Ok(index + 1)
}

fn validate_cargo_config_value(arg: &str, value: &str) -> Result<(), String> {
    if value.is_empty() {
        Err(cargo_config_value_error())
    } else if cargo_config_value_is_unsupported(value) {
        Err(unsupported_cargo_arg_error(arg))
    } else {
        Ok(())
    }
}

fn cargo_config_value_is_unsupported(value: &str) -> bool {
    cargo_config_overrides_compile_once_controls(value)
}

fn overrides_nextest_batch_controls(arg: &str) -> bool {
    let flag = arg.split_once('=').map_or(arg, |(flag, _)| flag);
    matches!(
        flag,
        "--build-jobs"
            | "--test-threads"
            | "--no-fail-fast"
            | "--retries"
            | "--no-tests"
            | "--cargo-message-format"
            | "--message-format"
            | "--message-format-version"
            | "--show-progress"
            | "--status-level"
            | "--final-status-level"
            | "--failure-output"
            | "--success-output"
            | "--user-config-file"
            | "--config-file"
            | "--profile"
            | "--no-report"
    )
}

fn cargo_config_overrides_compile_once_controls(value: &str) -> bool {
    let normalized = normalize_toml_unicode_escapes(value);
    let compact: String = normalized
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace() && *ch != '"' && *ch != '\'')
        .collect();
    has_compile_once_key(&compact)
        || build_table_overrides_compile_once_controls(&normalized)
        || inline_build_table_overrides_compile_once_controls(&compact)
}

fn normalize_toml_unicode_escapes(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            normalized.push_str(&normalize_toml_escape(&mut chars));
        } else {
            normalized.push(ch);
        }
    }
    normalized
}

fn normalize_toml_escape(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let Some(prefix) = chars.peek().copied() else {
        return "\\".to_string();
    };
    let hex_len = match prefix {
        'u' => 4,
        'U' => 8,
        _ => return "\\".to_string(),
    };
    chars.next();
    decode_toml_unicode_escape(chars, prefix, hex_len)
}

fn decode_toml_unicode_escape(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    prefix: char,
    hex_len: usize,
) -> String {
    let mut hex = String::with_capacity(hex_len);
    for _ in 0..hex_len {
        let Some(hex_ch) = chars.peek().copied() else {
            return format!("\\{prefix}{hex}");
        };
        if !hex_ch.is_ascii_hexdigit() {
            return format!("\\{prefix}{hex}");
        }
        hex.push(hex_ch);
        chars.next();
    }
    u32::from_str_radix(&hex, 16)
        .ok()
        .and_then(char::from_u32)
        .map_or_else(|| format!("\\{prefix}{hex}"), |ch| ch.to_string())
}

fn has_compile_once_key(value: &str) -> bool {
    value.contains("build.target-dir=") || value.contains("build.jobs=")
}

fn build_table_overrides_compile_once_controls(value: &str) -> bool {
    let mut in_build_table = false;
    for line in value.lines() {
        let compact = compact_cargo_config_line(line);
        if compact.starts_with('[') {
            in_build_table = compact == "[build]";
        } else if in_build_table && contains_compile_once_field(&compact) {
            return true;
        }
    }
    false
}

fn inline_build_table_overrides_compile_once_controls(value: &str) -> bool {
    let Some(start) = value.find("build={") else {
        return false;
    };
    let rest = &value[start + "build={".len()..];
    let end = matching_inline_table_end(rest).unwrap_or(rest.len());
    contains_compile_once_field(&rest[..end])
}

fn matching_inline_table_end(value: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, ch) in value.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' if depth == 0 => return Some(index),
            '}' => depth -= 1,
            _ => {}
        }
    }
    None
}

fn contains_compile_once_field(value: &str) -> bool {
    value.starts_with("target-dir=")
        || value.starts_with("jobs=")
        || value.contains(",target-dir=")
        || value.contains(",jobs=")
}

fn compact_cargo_config_line(line: &str) -> String {
    line.chars()
        .filter(|ch| !ch.is_ascii_whitespace() && *ch != '"' && *ch != '\'')
        .collect()
}

fn unsupported_cargo_arg_error(arg: &str) -> String {
    format!(
        "unsupported Rust cargo argument `{arg}`; KISS controls the target directory and job budget for compile-once coverage"
    )
}

fn cargo_config_value_error() -> String {
    "--config requires a non-empty value".to_string()
}

#[cfg(test)]
#[path = "batch_plan_public_test.rs"]
mod tests;
