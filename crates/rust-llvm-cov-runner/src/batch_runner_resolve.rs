use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use cargo_config2::{Config, PathAndArgs, ResolveOptions};

use crate::RustLlvmCovError;
use crate::batch_plan::RustCoverageBatchRequest;
use crate::rust_cov_cache::rust_cov_fnv1a64;

pub const RUNNER_RESOLVER_POLICY_VERSION: &str = "runner-resolve-v1";

/// Platform key (Cargo target triple) to delegated runner argv before KISS shim wrapping.
pub type DelegatedRunnerMap = BTreeMap<String, Vec<String>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedDelegatedRunners {
    pub map: DelegatedRunnerMap,
    pub host_platform: String,
}

pub fn resolve_batch_request_runners(
    req: &mut RustCoverageBatchRequest,
) -> Result<(), RustLlvmCovError> {
    let resolved = resolve_delegated_runners(req)?;
    req.runner_map_fingerprint = runner_map_fingerprint(&resolved.map);
    req.host_platform = resolved.host_platform;
    req.delegated_runners = resolved.map;
    Ok(())
}

pub fn placeholder_delegated_runner_fields() -> (DelegatedRunnerMap, String, String) {
    let map = BTreeMap::from([("x86_64-unknown-linux-gnu".to_string(), Vec::new())]);
    (
        map.clone(),
        runner_map_fingerprint(&map),
        "x86_64-unknown-linux-gnu".to_string(),
    )
}

pub fn resolve_delegated_runners(
    req: &RustCoverageBatchRequest,
) -> Result<ResolvedDelegatedRunners, RustLlvmCovError> {
    let config = load_cargo_config(req)?;
    let explicit_targets = explicit_cargo_targets(&req.cargo_args);
    let platforms = config
        .build_target_for_config(explicit_targets.iter().map(String::as_str))
        .map_err(|err| {
            RustLlvmCovError::InvalidRequest(format!("cargo runner resolution failed: {err}"))
        })?;
    let mut map = BTreeMap::new();
    for platform in &platforms {
        let key = platform.triple().to_string();
        let runner = config.runner(platform.triple()).map_err(|err| {
            RustLlvmCovError::InvalidRequest(format!(
                "cargo runner resolution failed for platform `{key}`: {err}"
            ))
        })?;
        map.insert(key, path_and_args_to_strings(runner.as_ref()));
    }
    if map.is_empty() {
        return Err(RustLlvmCovError::InvalidRequest(
            "cargo runner resolution produced no platforms".into(),
        ));
    }
    let host_platform = platforms
        .first()
        .map(|platform| platform.triple().to_string())
        .ok_or_else(|| RustLlvmCovError::InvalidRequest("no Cargo build platforms".into()))?;
    apply_cargo_cli_config_runner_overrides(&req.cargo_args, &req.cwd, &host_platform, &mut map)?;
    Ok(ResolvedDelegatedRunners { map, host_platform })
}

pub fn runner_map_fingerprint(map: &DelegatedRunnerMap) -> String {
    let mut h = rust_cov_fnv1a64(
        0xcbf2_9ce4_8422_2325,
        RUNNER_RESOLVER_POLICY_VERSION.as_bytes(),
    );
    h = rust_cov_fnv1a64(h, &[0]);
    for (platform, argv) in map {
        h = rust_cov_fnv1a64(h, platform.as_bytes());
        h = rust_cov_fnv1a64(h, &[0]);
        for arg in argv {
            h = rust_cov_fnv1a64(h, arg.as_bytes());
            h = rust_cov_fnv1a64(h, &[0]);
        }
        h = rust_cov_fnv1a64(h, &[0xff]);
    }
    format!("{h:016x}")
}

pub fn write_runner_map(path: &Path, map: &DelegatedRunnerMap) -> Result<(), RustLlvmCovError> {
    let parent = path
        .parent()
        .ok_or_else(|| RustLlvmCovError::InvalidRequest("runner map path has no parent".into()))?;
    std::fs::create_dir_all(parent).map_err(RustLlvmCovError::Io)?;
    let bytes = serde_json::to_vec_pretty(map).map_err(|err| {
        RustLlvmCovError::InvalidRequest(format!("failed to encode runner map: {err}"))
    })?;
    std::fs::write(path, bytes).map_err(RustLlvmCovError::Io)
}

pub fn read_runner_map(path: &Path) -> Result<DelegatedRunnerMap, RustLlvmCovError> {
    let bytes = std::fs::read(path).map_err(RustLlvmCovError::Io)?;
    serde_json::from_slice(&bytes).map_err(|err| {
        RustLlvmCovError::InvalidRequest(format!("failed to decode runner map: {err}"))
    })
}

pub fn delegated_runner_for_platform<'a>(
    map: &'a DelegatedRunnerMap,
    platform: &str,
) -> Option<&'a [String]> {
    map.get(platform).map(Vec::as_slice)
}

fn load_cargo_config(req: &RustCoverageBatchRequest) -> Result<Config, RustLlvmCovError> {
    let mut options = ResolveOptions::default().cargo(req.cargo.as_os_str());
    if !req.env.is_empty() {
        options = options.env(
            req.env
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        if let Some(cargo_home) = req.env.get("CARGO_HOME") {
            options = options.cargo_home(PathBuf::from(cargo_home));
        }
    }
    Config::load_with_options(&req.cwd, options).map_err(|err| {
        RustLlvmCovError::InvalidRequest(format!("failed to load Cargo configuration: {err}"))
    })
}

fn explicit_cargo_targets(cargo_args: &[String]) -> Vec<String> {
    let mut targets = Vec::new();
    let mut index = 0usize;
    while index < cargo_args.len() {
        let arg = &cargo_args[index];
        if arg == "--target" {
            if let Some(value) = cargo_args.get(index + 1) {
                targets.push(value.clone());
                index += 2;
                continue;
            }
        } else if let Some(value) = arg.strip_prefix("--target=") {
            targets.push(value.to_string());
        }
        index += 1;
    }
    targets
}

fn path_and_args_to_strings(runner: Option<&PathAndArgs>) -> Vec<String> {
    let Some(runner) = runner else {
        return Vec::new();
    };
    let mut argv = vec![runner.path.to_string_lossy().to_string()];
    argv.extend(
        runner
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().to_string()),
    );
    argv
}

fn apply_cargo_cli_config_runner_overrides(
    cargo_args: &[String],
    cwd: &Path,
    host_platform: &str,
    map: &mut DelegatedRunnerMap,
) -> Result<(), RustLlvmCovError> {
    for fragment in cargo_config_fragments(cargo_args, cwd)? {
        let runner = parse_runner_override_from_config_fragment(&fragment, host_platform)?;
        if let Some(runner) = runner {
            map.insert(host_platform.to_string(), runner);
        }
    }
    Ok(())
}

fn cargo_config_fragments(
    cargo_args: &[String],
    cwd: &Path,
) -> Result<Vec<String>, RustLlvmCovError> {
    let mut fragments = Vec::new();
    let mut index = 0usize;
    while index < cargo_args.len() {
        let arg = &cargo_args[index];
        if arg == "--config" {
            if let Some(value) = cargo_args.get(index + 1) {
                fragments.push(load_cargo_config_fragment(value, cwd)?);
                index += 2;
                continue;
            }
        } else if let Some(value) = arg.strip_prefix("--config=") {
            fragments.push(load_cargo_config_fragment(value, cwd)?);
        }
        index += 1;
    }
    Ok(fragments)
}

fn load_cargo_config_fragment(value: &str, cwd: &Path) -> Result<String, RustLlvmCovError> {
    if cargo_config_value_is_inline(value) {
        return Ok(value.to_string());
    }
    let path = PathBuf::from(value);
    let path = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    fs::read_to_string(&path).map_err(|err| {
        RustLlvmCovError::InvalidRequest(format!(
            "failed to read Cargo --config file `{}`: {err}",
            path.display()
        ))
    })
}

fn cargo_config_value_is_inline(value: &str) -> bool {
    value.contains('=') || value.trim_start().starts_with('[')
}

fn parse_runner_override_from_config_fragment(
    fragment: &str,
    host_platform: &str,
) -> Result<Option<Vec<String>>, RustLlvmCovError> {
    let value: toml::Value = parse_cargo_config_fragment(fragment, host_platform)?;
    let Some(targets) = value.get("target").and_then(toml::Value::as_table) else {
        return Ok(None);
    };
    let Some(target) = targets.get(host_platform).and_then(toml::Value::as_table) else {
        return Ok(None);
    };
    let Some(runner) = target.get("runner") else {
        return Ok(None);
    };
    runner_value_to_argv(runner)
}

fn parse_cargo_config_fragment(
    fragment: &str,
    host_platform: &str,
) -> Result<toml::Value, RustLlvmCovError> {
    toml::from_str(fragment)
        .or_else(|_| toml::from_str(&normalize_host_platform_key(fragment, host_platform)))
        .map_err(|err| {
            RustLlvmCovError::InvalidRequest(format!("failed to parse Cargo --config value: {err}"))
        })
}

fn normalize_host_platform_key(fragment: &str, host_platform: &str) -> String {
    let quoted = format!("\"{host_platform}\"");
    fragment
        .replace(
            &format!("[target.{host_platform}]"),
            &format!("[target.{quoted}]"),
        )
        .replace(
            &format!("target.{host_platform}.runner"),
            &format!("target.{quoted}.runner"),
        )
}

fn runner_value_to_argv(value: &toml::Value) -> Result<Option<Vec<String>>, RustLlvmCovError> {
    if let Some(runner) = value.as_str() {
        return Ok(Some(vec![runner.to_string()]));
    }
    let Some(items) = value.as_array() else {
        return Err(RustLlvmCovError::InvalidRequest(
            "Cargo --config target runner must be a string or string list".into(),
        ));
    };
    let mut argv = Vec::with_capacity(items.len());
    for item in items {
        let Some(item) = item.as_str() else {
            return Err(RustLlvmCovError::InvalidRequest(
                "Cargo --config target runner list must contain only strings".into(),
            ));
        };
        argv.push(item.to_string());
    }
    Ok(Some(argv))
}

#[cfg(test)]
#[path = "batch_runner_resolve_test.rs"]
mod tests;
