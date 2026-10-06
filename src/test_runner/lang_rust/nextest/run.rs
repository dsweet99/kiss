use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Mutex;
use std::time::Instant;

use super::RunRequest;
use super::config::{SelectorIndex, test_threads, tool_config_toml};
use super::results::{Plan, Results};
use super::status_line::{
    is_nextest_framing, parse_finished_test, parse_skipped_test, starts_test_run,
};
use crate::test_runner::emit_test_progress;
use crate::test_runner::runners::{SelectorExecutionRecord, SelectorExecutionSummary};

static ACTIVE_NEXTEST: Mutex<Option<u32>> = Mutex::new(None);

/// Asks the running `cargo nextest` to stop; it ends its tests and exits.
pub(crate) fn cancel_active_run() {
    let pid = ACTIVE_NEXTEST.lock().ok().and_then(|slot| *slot);
    if let Some(pid) = pid.and_then(|pid| libc::pid_t::try_from(pid).ok()) {
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
    }
}

struct ActiveNextest;

impl ActiveNextest {
    fn register(pid: u32) -> Self {
        if let Ok(mut slot) = ACTIVE_NEXTEST.lock() {
            *slot = Some(pid);
        }
        Self
    }
}

impl Drop for ActiveNextest {
    fn drop(&mut self) {
        if let Ok(mut slot) = ACTIVE_NEXTEST.lock() {
            *slot = None;
        }
    }
}

fn millis_since(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn prepare(req: &RunRequest<'_>) -> Result<(Plan, String), String> {
    let identity = super::records::record_identity(req.repo_root, req.extras)?;
    let inputs = super::records::rust_inputs_digest(req.repo_root)?;
    let report_ids =
        crate::test_runner::rust_report_id_cache::rust_logical_to_kiss_test_ids_cached(
            req.repo_root,
            &[],
        )?;
    let names = super::config::TestNames::resolve(req.repo_root, req.selectors, &report_ids)?;
    let timeouts: BTreeMap<String, u64> = req
        .selectors
        .iter()
        .filter_map(|selector| {
            let report_id = report_ids.get(selector).unwrap_or(selector);
            Some((
                selector.clone(),
                super::holding::timeout_millis(req.gate, report_id)?,
            ))
        })
        .collect();
    let toml = tool_config_toml(req.selectors, &names, &timeouts);
    let plan = Plan {
        identity,
        inputs,
        report_ids,
        timeout_millis: timeouts,
        index: SelectorIndex::new(req.selectors, &names),
        cache_policy: super::records::cache_policy(req.repo_root),
    };
    Ok((plan, toml))
}

fn tool_config_dir(repo_root: &Path) -> PathBuf {
    crate::test_runner::test_state_dir(repo_root).join("nextest")
}

/// Writes the tool config for this run, first removing any left by an interrupted run.
fn write_tool_config(repo_root: &Path, toml: &str) -> Result<PathBuf, String> {
    let dir = tool_config_dir(repo_root);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)
        .map_err(|err| format!("error: kiss test: create {}: {err}", dir.display()))?;
    let dir = dir.canonicalize().unwrap_or(dir);
    let path = dir.join(format!("kiss-{}.toml", std::process::id()));
    std::fs::write(&path, toml)
        .map_err(|err| format!("error: kiss test: write {}: {err}", path.display()))?;
    Ok(path)
}

fn nextest_args(req: &RunRequest<'_>, tool_config: &Path) -> Vec<String> {
    let mut args: Vec<String> = [
        "nextest",
        "run",
        "--workspace",
        "--profile",
        "kiss",
        "--no-fail-fast",
        "--retries",
        "0",
        "--no-tests",
        "pass",
        "--status-level",
        "skip",
        "--final-status-level",
        "none",
        "--failure-output",
        "immediate",
        "--success-output",
        "never",
        "--color",
        "never",
        "--cargo-quiet",
        "--user-config-file",
        "none",
    ]
    .map(String::from)
    .to_vec();
    args.push("--tool-config-file".into());
    args.push(format!("kiss:{}", tool_config.display()));
    args.push("--test-threads".into());
    args.push(test_threads(req.repo_root, req.extras, req.jobs).to_string());
    if !req.extras.is_empty() {
        args.push("--".into());
        args.extend(req.extras.iter().cloned());
    }
    args
}

fn spawn(req: &RunRequest<'_>, tool_config: &Path) -> Result<Child, String> {
    Command::new("cargo")
        .args(nextest_args(req, tool_config))
        .current_dir(req.repo_root)
        .env_clear()
        .envs(super::env::child_env())
        .stdin(Stdio::null())
        .stdout(Stdio::from(std::io::stderr()))
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("error: kiss test: failed to run cargo nextest: {err}"))
}

/// Follows nextest's stderr: the build phase is held back and shown only if the build
/// fails; during the run each finished test is stored and printed, and test output is
/// passed through.
struct Stream {
    started: Instant,
    running: bool,
    build_output: Vec<String>,
}

impl Stream {
    fn line(&mut self, line: &str, results: &mut Results<'_>) -> Result<(), String> {
        if !self.running {
            if starts_test_run(line) {
                self.running = true;
                emit_test_progress(&format!(
                    "kiss test: Ran cargo {:.1}ms",
                    millis_since(self.started)
                ));
                emit_test_progress("kiss test: Running nextest");
                self.started = Instant::now();
                return results.refresh_inputs();
            } else {
                self.build_output.push(line.to_string());
            }
            return Ok(());
        }
        if let Some(test) = parse_finished_test(line) {
            return results.finished(&test);
        }
        if let Some((binary_id, test_name)) = parse_skipped_test(line) {
            return results.skipped(&binary_id, &test_name);
        }
        if !is_nextest_framing(line) {
            eprintln!("{line}");
        }
        Ok(())
    }
}

fn follow(child: &mut Child, results: &mut Results<'_>) -> Result<(Stream, ExitStatus), String> {
    let mut stream = Stream {
        started: Instant::now(),
        running: false,
        build_output: Vec::new(),
    };
    emit_test_progress("kiss test: Running cargo");
    let mut reader = BufReader::new(child.stderr.take().expect("nextest stderr is piped"));
    let mut buf = Vec::new();
    let mut failure = None;
    while failure.is_none() {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) => break,
            Ok(_) => {
                let line = String::from_utf8_lossy(&buf);
                failure = stream
                    .line(line.trim_end_matches(['\n', '\r']), results)
                    .err();
            }
            Err(err) => failure = Some(format!("error: kiss test: read nextest output: {err}")),
        }
    }
    if failure.is_some() {
        let _ = child.kill();
    }
    let status = child
        .wait()
        .map_err(|err| format!("error: kiss test: wait for cargo nextest: {err}"))?;
    match failure {
        Some(err) => Err(err),
        None => Ok((stream, status)),
    }
}

fn interrupted(status: ExitStatus) -> bool {
    use std::os::unix::process::ExitStatusExt;
    matches!(
        status.signal(),
        Some(libc::SIGINT | libc::SIGTERM | libc::SIGKILL)
    )
}

fn check_exit(stream: &Stream, status: ExitStatus) -> Result<(), String> {
    if interrupted(status) {
        crate::test_runner::rust_batch_interrupt::note_rust_batch_interrupted();
        return Err("error: kiss test: cargo nextest was interrupted".into());
    }
    if !stream.running && !status.success() {
        for line in &stream.build_output {
            eprintln!("{line}");
        }
        return Err(format!(
            "error: kiss test: cargo nextest failed before running tests ({status})"
        ));
    }
    if !matches!(status.code(), Some(0 | 100)) {
        return Err(format!("error: kiss test: cargo nextest failed ({status})"));
    }
    Ok(())
}

fn warn_missing(missing: &[&str]) {
    if missing.is_empty() {
        return;
    }
    let shown: Vec<&str> = missing.iter().take(5).copied().collect();
    let more = missing.len() - shown.len();
    let tail = if more > 0 {
        format!(" and {more} more")
    } else {
        String::new()
    };
    eprintln!(
        "kiss test: nextest reported no result for {} requested Rust test(s): {}{tail}",
        missing.len(),
        shown.join(", ")
    );
}

pub(super) fn run(
    req: &RunRequest<'_>,
    on_result: &mut dyn FnMut(SelectorExecutionRecord),
) -> Result<SelectorExecutionSummary, String> {
    let stage_started = Instant::now();
    let (plan, toml) = prepare(req)?;
    let tool_config = write_tool_config(req.repo_root, &toml)?;
    let mut results = Results::new(req.repo_root, req.gate, plan, req.selectors.len());
    let followed = spawn(req, &tool_config).and_then(|mut child| {
        kiss::subprocess_observer::record_nextest_invocation();
        let _active = ActiveNextest::register(child.id());
        follow(&mut child, &mut results)
    });
    let _ = std::fs::remove_file(&tool_config);
    let (stream, status) = followed?;
    check_exit(&stream, status)?;
    emit_test_progress(&format!(
        "kiss test: Ran nextest {:.1}ms",
        millis_since(stream.started)
    ));
    crate::test_runner::emit_stage_time("rust_nextest", stage_started.elapsed());
    warn_missing(&results.missing(req.selectors));
    crate::test_runner::tests_remaining::set_language_remaining(kiss::Language::Rust, 0);
    Ok(results.finish(on_result))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv_names_the_kiss_profile_and_passes_test_arguments_last() {
        let tmp = tempfile::tempdir().unwrap();
        let gate = kiss::GateConfig::default();
        let extras = vec!["--skip".to_string(), "slow".to_string()];
        let req = RunRequest {
            repo_root: tmp.path(),
            selectors: &[],
            extras: &extras,
            jobs: 3,
            gate: &gate,
        };
        let args = nextest_args(&req, Path::new("/abs/kiss.toml"));
        assert_eq!(&args[..2], ["nextest", "run"]);
        let at = |flag: &str| &args[args.iter().position(|a| a == flag).unwrap() + 1];
        assert_eq!(at("--profile"), "kiss");
        assert_eq!(at("--tool-config-file"), "kiss:/abs/kiss.toml");
        assert_eq!(at("--test-threads"), "3");
        assert_eq!(&args[args.len() - 3..], ["--", "--skip", "slow"]);
    }

    #[test]
    fn exit_codes_other_than_pass_and_test_failure_are_errors() {
        use std::os::unix::process::ExitStatusExt;
        let ran = Stream {
            started: Instant::now(),
            running: true,
            build_output: Vec::new(),
        };
        assert!(check_exit(&ran, ExitStatus::from_raw(0)).is_ok());
        assert!(check_exit(&ran, ExitStatus::from_raw(100 << 8)).is_ok());
        assert!(check_exit(&ran, ExitStatus::from_raw(4 << 8)).is_err());
        let built = Stream {
            started: Instant::now(),
            running: false,
            build_output: vec!["error[E0425]: cannot find value".into()],
        };
        let err = check_exit(&built, ExitStatus::from_raw(101 << 8)).unwrap_err();
        assert!(err.contains("before running tests"), "{err}");
        assert!(check_exit(&ran, ExitStatus::from_raw(libc::SIGINT)).is_err());
        assert!(crate::test_runner::rust_batch_interrupt::consume_rust_batch_interrupted());
    }
}
