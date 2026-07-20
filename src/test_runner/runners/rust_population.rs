use std::path::Path;
use std::process::{Command, Stdio};

use super::{SelectorCacheRecord, SelectorExecutionSummary};

pub(crate) fn run_uninstrumented_rust_population_selectors(
    repo_root: &Path,
    selectors: &[String],
    jobs: usize,
) -> Result<SelectorExecutionSummary, String> {
    run_uninstrumented_rust_population_selectors_with_runner(selectors, jobs, |args| {
        Command::new("cargo")
            .args(args)
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
    let mut summary = SelectorExecutionSummary::default();
    for selector in selectors {
        println!("PASSED: {selector}");
        summary.record(
            rpytest_runner::TestStatus::Passed,
            SelectorCacheRecord::MissUnstored,
            Some(0),
        );
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
                .any(|pair| pair == ["--status-level", "none"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--final-status-level", "none"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--success-output", "never"])
        );
    }

    #[test]
    fn uninstrumented_population_success_records_requested_selectors_unstored() {
        let selectors = vec![
            "crate::tests::alpha".to_string(),
            "selector with spaces".to_string(),
        ];
        let mut observed_args = Vec::new();

        let summary =
            run_uninstrumented_rust_population_selectors_with_runner(&selectors, 3, |args| {
                observed_args = args.to_vec();
                Ok(output(0, "", ""))
            })
            .unwrap();

        assert_eq!(summary.total, selectors.len());
        assert_eq!(summary.cache_misses, selectors.len());
        assert_eq!(summary.cache_unstored, selectors.len());
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.exit_code, 0);
        assert!(observed_args.windows(2).any(|pair| pair == ["-j", "3"]));
        assert!(!observed_args.iter().any(|arg| selectors.contains(arg)));
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
