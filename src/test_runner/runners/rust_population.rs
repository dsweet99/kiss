use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use rust_llvm_cov_runner::parse_batch_event_stream;

use super::{SelectorCacheRecord, SelectorExecutionSummary};

pub(crate) fn run_uninstrumented_rust_population_selectors(
    repo_root: &Path,
    selectors: &[String],
    jobs: usize,
) -> Result<SelectorExecutionSummary, String> {
    run_uninstrumented_rust_population_selectors_with_runner(selectors, jobs, |args| {
        Command::new("cargo")
            .args(args)
            .env("NEXTEST_EXPERIMENTAL_LIBTEST_JSON", "1")
            .current_dir(repo_root)
            .stdin(Stdio::null())
            .output()
            .map_err(|err| format!("error: kiss test: failed to spawn cargo nextest: {err}"))
    })
}

fn run_uninstrumented_rust_population_selectors_with_runner<F>(
    selectors: &[String],
    jobs: usize,
    run_nextest: F,
) -> Result<SelectorExecutionSummary, String>
where
    F: FnOnce(&[String]) -> Result<std::process::Output, String>,
{
    assert!(jobs > 0, "jobs must be greater than zero");
    let args = build_uninstrumented_nextest_population_args(jobs);
    let output = run_nextest(&args)?;
    if !output.status.success() {
        return Err(format!(
            "error: kiss test: uninstrumented Rust population failed: {}",
            command_output_text(&output)
        ));
    }
    let stream = parse_batch_event_stream(&output.stdout).map_err(|err| {
        format!("error: kiss test: failed to parse uninstrumented nextest events: {err:?}")
    })?;
    let mut summary = SelectorExecutionSummary::default();
    if stream.terminal_tests.is_empty() {
        // Structured events absent (for example, empty fixture runners): keep
        // selector accounting without inventing a fresh duration.
        for selector in selectors {
            println!("PASSED: {selector} (0.00s)");
            summary.record(
                rpytest_runner::TestStatus::Passed,
                SelectorCacheRecord::MissUnstored,
                Some(0),
            );
        }
        return Ok(summary);
    }
    for terminal in &stream.terminal_tests {
        let duration = Duration::from_secs_f64(terminal.exec_time_secs.max(0.0));
        let formatted = crate::test_runner::duration::format_test_duration(duration);
        let status = if terminal.passed {
            println!("PASSED: {} ({formatted})", terminal.test_name);
            rpytest_runner::TestStatus::Passed
        } else {
            println!("FAILED: {} ({formatted})", terminal.test_name);
            rpytest_runner::TestStatus::Failed
        };
        summary.record(status, SelectorCacheRecord::MissUnstored, Some(0));
    }
    Ok(summary)
}

fn build_uninstrumented_nextest_population_args(jobs: usize) -> Vec<String> {
    vec![
        "nextest".to_string(),
        "run".to_string(),
        "--workspace".to_string(),
        "--no-fail-fast".to_string(),
        "--retries".to_string(),
        "0".to_string(),
        "-j".to_string(),
        jobs.to_string(),
        "--message-format".to_string(),
        "libtest-json-plus".to_string(),
        "--message-format-version".to_string(),
        "0.1".to_string(),
        "--status-level".to_string(),
        "none".to_string(),
        "--final-status-level".to_string(),
        "none".to_string(),
        "--failure-output".to_string(),
        "never".to_string(),
        "--success-output".to_string(),
        "never".to_string(),
    ]
}

fn command_output_text(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    fn output(status_code: i32, stdout: &str, stderr: &str) -> std::process::Output {
        std::process::Output {
            status: std::process::ExitStatus::from_raw(status_code << 8),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    #[test]
    fn uninstrumented_nextest_population_args_are_quiet_and_parallel() {
        let args = build_uninstrumented_nextest_population_args(16);

        assert_eq!(args[0..3], ["nextest", "run", "--workspace"]);
        assert!(args.windows(2).any(|pair| pair == ["-j", "16"]));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--message-format", "libtest-json-plus"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--status-level", "none"])
        );
    }

    #[test]
    fn uninstrumented_population_prints_structured_durations() {
        let selectors = vec!["crate::tests::alpha".to_string()];
        let stdout = concat!(
            r#"{"type":"suite","event":"started","test_count":1}"#,
            "\n",
            r#"{"type":"test","event":"ok","name":"pkg::bin$alpha","exec_time":0.12}"#,
            "\n",
        );
        let summary =
            run_uninstrumented_rust_population_selectors_with_runner(&selectors, 3, |_args| {
                Ok(output(0, stdout, ""))
            })
            .unwrap();

        assert_eq!(summary.total, 1);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.exit_code, 0);
    }

    #[test]
    fn uninstrumented_population_failure_reports_stderr_before_stdout() {
        let err = run_uninstrumented_rust_population_selectors_with_runner(
            &["crate::tests::alpha".to_string()],
            1,
            |_args| Ok(output(1, "stdout details", "stderr details\n")),
        )
        .unwrap_err();

        assert!(err.contains("uninstrumented Rust population failed"));
        assert!(err.contains("stderr details"));
        assert!(!err.contains("stdout details"));
    }
}
