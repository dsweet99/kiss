use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::batch_plan_nextest_config::build_nextest_config_toml;

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
            generated_config: PathBuf::from("/repo/.kiss/runs/nextest.toml"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RustCoverageBatchPlan {
    pub build_target: PathBuf,
    pub env: BTreeMap<String, String>,
    pub argv: Vec<String>,
    pub generated_config_toml: String,
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
    let build_target = req.cache_root.join("build").join("target");
    let mut env = req.env.clone();
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

    let jobs = req.jobs.to_string();
    let test_threads = nextest_test_threads(req);
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

    let generated_config_toml = build_nextest_config_toml(req);
    Ok(RustCoverageBatchPlan {
        build_target,
        env,
        argv,
        generated_config_toml,
    })
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

fn nextest_test_threads(req: &RustCoverageBatchRequest) -> String {
    if req
        .test_args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--nocapture" | "--no-capture"))
    {
        "1".to_string()
    } else {
        req.jobs.to_string()
    }
}

pub fn validate_supported_rust_test_args(test_args: &[String]) -> Result<(), String> {
    let mut index = 0;
    while index < test_args.len() {
        let arg = &test_args[index];
        match arg.as_str() {
            "--exact" | "--nocapture" | "--no-capture" | "--ignored" | "--include-ignored" => {
                index += 1;
            }
            "--skip" => {
                let Some(pattern) = test_args.get(index + 1) else {
                    return Err("--skip requires a non-empty pattern".to_string());
                };
                if pattern.is_empty() {
                    return Err("--skip requires a non-empty pattern".to_string());
                }
                index += 2;
            }
            _ if arg.starts_with("--skip=") && arg.len() > "--skip=".len() => {
                index += 1;
            }
            _ => {
                return Err(format!(
                    "unsupported Rust test argument `{arg}`; supported forms are --exact, --nocapture, --no-capture, --ignored, --include-ignored, and repeated --skip <pattern>"
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{RustCoverageBatchPlan, RustCoverageBatchRequest, build_rust_coverage_batch_plan};

    #[test]
    fn batch_request_witness_exercises_public_contract_in_module() {
        let req = RustCoverageBatchRequest::witness();
        let cloned = req.clone();

        assert_eq!(cloned, req);
        assert!(format!("{req:?}").contains("RustCoverageBatchRequest"));
        assert_eq!(req.logical_selectors.len(), 2);
        assert_eq!(req.jobs, 4);
        assert!(build_rust_coverage_batch_plan(&req).is_ok());
    }

    #[test]
    fn batch_plan_witness_exercises_public_contract_in_module() {
        let plan = RustCoverageBatchPlan::witness();
        let cloned = plan.clone();

        assert_eq!(cloned, plan);
        assert!(format!("{plan:?}").contains("RustCoverageBatchPlan"));
        assert_eq!(plan.argv[0], "cargo");
        assert_eq!(plan.env["NEXTEST_EXPERIMENTAL_LIBTEST_JSON"], "1");
    }
}
