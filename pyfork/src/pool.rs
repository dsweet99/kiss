use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::flags::validate_pytest_extra;
use crate::scripts::{pid_once_script, pool_script, slow_pool_script};

#[derive(Serialize)]
struct PoolConfig<'a> {
    mode: &'a str,
    nodeids: &'a [String],
    j: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    extra: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    trace_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sleep_s: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    peak_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pid_path: Option<String>,
}

fn write_pool_config(path: &Path, config: &PoolConfig<'_>) -> Result<(), String> {
    let bytes =
        serde_json::to_vec(config).map_err(|e| format!("failed to encode pool config: {e}"))?;
    fs::write(path, bytes).map_err(|e| format!("failed to write {}: {e}", path.display()))
}

fn pool_config_path(repo_root: &Path) -> PathBuf {
    std::env::temp_dir().join(format!(
        "kiss-pyfork-{}-{}.json",
        std::process::id(),
        repo_root.file_name().unwrap_or_default().to_string_lossy()
    ))
}

pub fn default_parallelism() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

pub fn build_fork_argv(repo_root: &Path, nodeid: &str, extra: &[String]) -> Vec<String> {
    let mut argv = vec![
        "python".to_string(),
        "-c".to_string(),
        "<pyfork-pool>".to_string(),
        repo_root.display().to_string(),
        nodeid.to_string(),
    ];
    argv.extend(extra.iter().cloned());
    argv
}

pub fn shell_quote_line(argv: &[String]) -> String {
    argv.iter()
        .map(|a| {
            if a.chars().all(|c| {
                c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '=' | ',')
            }) && !a.starts_with('-')
            {
                a.clone()
            } else {
                format!("'{}'", a.replace('\'', "'\"'\"'"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn run_pool(
    repo_root: &Path,
    nodeids: &[String],
    j: usize,
    extra: &[String],
) -> Result<i32, String> {
    if nodeids.is_empty() {
        return Ok(0);
    }
    validate_pytest_extra(extra)?;
    let config_path = pool_config_path(repo_root);
    write_pool_config(
        &config_path,
        &PoolConfig {
            mode: "run",
            nodeids,
            j,
            extra: Some(extra),
            trace_dir: None,
            sleep_s: None,
            peak_path: None,
            pid_path: None,
        },
    )?;
    let status = Command::new("python")
        .arg("-c")
        .arg(pool_script())
        .arg(repo_root)
        .arg(&config_path)
        .current_dir(repo_root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .map_err(|e| format!("failed to run pyfork pool: {e}"))?;
    let _ = fs::remove_file(&config_path);
    Ok(status.code().unwrap_or_else(|| i32::from(!status.success())))
}

pub fn trace_pool(
    repo_root: &Path,
    nodeids: &[String],
    j: usize,
) -> Result<(i32, PathBuf), String> {
    if nodeids.is_empty() {
        let trace_dir = std::env::temp_dir().join(format!(
            "kiss-pyfork-trace-{}-empty",
            std::process::id()
        ));
        let _ = fs::create_dir_all(&trace_dir);
        return Ok((0, trace_dir));
    }
    let trace_dir = std::env::temp_dir().join(format!(
        "kiss-pyfork-trace-{}-{}",
        std::process::id(),
        repo_root.file_name().unwrap_or_default().to_string_lossy()
    ));
    fs::create_dir_all(&trace_dir)
        .map_err(|e| format!("failed to create trace dir {}: {e}", trace_dir.display()))?;
    let config_path = pool_config_path(repo_root);
    write_pool_config(
        &config_path,
        &PoolConfig {
            mode: "trace",
            nodeids,
            j,
            extra: None,
            trace_dir: Some(trace_dir.display().to_string()),
            sleep_s: None,
            peak_path: None,
            pid_path: None,
        },
    )?;
    let status = Command::new("python")
        .arg("-c")
        .arg(pool_script())
        .arg(repo_root)
        .arg(&config_path)
        .current_dir(repo_root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .status()
        .map_err(|e| format!("failed to run pyfork trace pool: {e}"))?;
    let _ = fs::remove_file(&config_path);
    Ok((
        status.code().unwrap_or_else(|| i32::from(!status.success())),
        trace_dir,
    ))
}

#[cfg(unix)]
#[allow(dead_code)]
pub(crate) fn fork_runs_exactly_one_nodeid(repo_root: &Path, nodeid: &str) -> Result<(), String> {
    let pid_path = std::env::temp_dir().join(format!("kiss-pyfork-pid-{}", std::process::id()));
    let config_path = pool_config_path(repo_root);
    write_pool_config(
        &config_path,
        &PoolConfig {
            mode: "run",
            nodeids: &[nodeid.to_string()],
            j: 1,
            extra: Some(&[]),
            trace_dir: None,
            sleep_s: None,
            peak_path: None,
            pid_path: Some(pid_path.display().to_string()),
        },
    )?;
    let status = Command::new("python")
        .arg("-c")
        .arg(pid_once_script())
        .arg(repo_root)
        .arg(&config_path)
        .current_dir(repo_root)
        .status()
        .map_err(|e| format!("failed to run single-nodeid fork: {e}"))?;
    let _ = fs::remove_file(&config_path);
    assert!(status.success(), "single nodeid run should succeed");
    let pid_text = fs::read_to_string(&pid_path).map_err(|e| format!("read pid file: {e}"))?;
    let _ = fs::remove_file(&pid_path);
    assert!(!pid_text.is_empty(), "child should record its pid");
    Ok(())
}

#[cfg(unix)]
#[allow(dead_code)]
pub(crate) fn scheduler_peak_concurrency(nodeids: &[String], j: usize, sleep_s: f64) -> Result<usize, String> {
    let peak_path = std::env::temp_dir().join(format!("kiss-pyfork-peak-{}", std::process::id()));
    let config_path = std::env::temp_dir().join(format!(
        "kiss-pyfork-slow-{}-{}.json",
        std::process::id(),
        nodeids.len()
    ));
    let config = PoolConfig {
        mode: "run",
        nodeids,
        j,
        extra: None,
        trace_dir: None,
        sleep_s: Some(sleep_s),
        peak_path: Some(peak_path.display().to_string()),
        pid_path: None,
    };
    write_pool_config(&config_path, &config)?;
    let status = Command::new("python")
        .arg("-c")
        .arg(slow_pool_script())
        .arg(".")
        .arg(&config_path)
        .status()
        .map_err(|e| format!("failed to run slow pool: {e}"))?;
    let _ = fs::remove_file(&config_path);
    if !status.success() {
        return Err("slow pool scheduler failed".into());
    }
    let peak = fs::read_to_string(&peak_path)
        .map_err(|e| format!("read peak file: {e}"))?
        .trim()
        .parse::<usize>()
        .map_err(|e| format!("parse peak: {e}"))?;
    let _ = fs::remove_file(&peak_path);
    Ok(peak)
}
