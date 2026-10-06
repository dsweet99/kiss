//! Runs Rust tests with `cargo nextest` and records each test's status and duration
//! from nextest's status lines.

use std::path::Path;

mod config;
mod env;
mod holding;
mod list;
mod records;
mod results;
mod run;
mod status_line;
mod toolchain;

pub(crate) use env::RUST_IDENTITY_ENV_KEYS;
pub(crate) use holding::{CurrentDeps, bad_record_ids, holding_records};
pub(crate) use list::list_tests;
pub(crate) use records::record_identity;
pub(crate) use run::cancel_active_run;

use crate::test_runner::runners::{SelectorExecutionRecord, SelectorExecutionSummary};

pub(crate) struct RunRequest<'a> {
    pub(crate) repo_root: &'a Path,
    pub(crate) selectors: &'a [String],
    pub(crate) extras: &'a [String],
    pub(crate) jobs: usize,
    pub(crate) gate: &'a kiss::GateConfig,
}

/// True when some stored Rust record was made under the repo's current identity.
#[cfg(test)]
pub(crate) fn has_records_for_current_identity(repo_root: &Path) -> bool {
    record_identity(repo_root, &[])
        .is_ok_and(|identity| !records::records_under(repo_root, &identity).is_empty())
}

/// Writes `Cargo.lock` so later cargo runs do not change the Rust inputs digest.
#[cfg(test)]
pub(crate) fn generate_lockfile(repo_root: &Path) {
    let status = std::process::Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(repo_root)
        .status()
        .unwrap();
    assert!(status.success());
}

/// Stores a record for each selector under the repo's current identity and inputs.
#[cfg(test)]
pub(crate) fn store_records(
    repo_root: &Path,
    outcomes: &[(&str, kiss::rpytest_runner::TestStatus)],
) {
    let identity = record_identity(repo_root, &[]).unwrap();
    let inputs = records::rust_inputs_digest(repo_root).unwrap();
    for (selector, status) in outcomes {
        let outcome = records::Outcome {
            test_id: selector,
            status: *status,
            duration: std::time::Duration::from_millis(1),
            timeout_ms: None,
            declared_inputs: None,
        };
        records::store(repo_root, &identity, &inputs, &outcome).unwrap();
    }
}

/// Accepts the libtest arguments kiss forwards to Rust tests.
pub(crate) fn validate_rust_extra_args(extras: &[String]) -> Result<(), String> {
    let mut args = extras.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--exact" | "--nocapture" | "--no-capture" | "--ignored" | "--include-ignored" => {}
            "--skip" => {
                if args.next().is_none_or(String::is_empty) {
                    return Err("--skip requires a non-empty pattern".to_string());
                }
            }
            _ if arg.len() > "--skip=".len() && arg.starts_with("--skip=") => {}
            _ => {
                return Err(format!(
                    "unsupported Rust test argument `{arg}`; supported forms are --exact, --nocapture, --no-capture, --ignored, --include-ignored, and repeated --skip <pattern>"
                ));
            }
        }
    }
    Ok(())
}

/// The command `kiss test --dry-run` shows for `selectors`.
pub(crate) fn dry_run_lines(
    selectors: &[String],
    extras: &[String],
    jobs: usize,
) -> Result<Vec<String>, String> {
    validate_rust_extra_args(extras)?;
    if selectors.is_empty() {
        return Ok(Vec::new());
    }
    let mut argv: Vec<String> = ["cargo", "nextest", "run", "--workspace", "--test-threads"]
        .map(String::from)
        .to_vec();
    argv.push(jobs.max(1).to_string());
    argv.push("-E".into());
    argv.push(config::selectors_filter(selectors, &Default::default()));
    if !extras.is_empty() {
        argv.push("--".into());
        argv.extend(extras.iter().cloned());
    }
    let mut lines = vec![
        format!("RUST BATCH selectors={} jobs={jobs}", selectors.len()),
        crate::test_runner::runners::shell_quote_line(&argv),
    ];
    lines.extend(
        selectors
            .iter()
            .map(|selector| format!("RUST SELECTOR {selector}")),
    );
    Ok(lines)
}

/// Runs `selectors`, storing each test's record as soon as nextest reports it.
pub(crate) fn run_nextest_selectors(
    req: &RunRequest<'_>,
    on_result: &mut dyn FnMut(SelectorExecutionRecord),
) -> Result<SelectorExecutionSummary, String> {
    validate_rust_extra_args(req.extras)?;
    if req.selectors.is_empty() {
        return Ok(SelectorExecutionSummary::default());
    }
    run::run(req, on_result)
}

#[cfg(test)]
mod tests {
    use super::validate_rust_extra_args;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn supported_test_arguments_are_accepted() {
        validate_rust_extra_args(&args(&[
            "--exact",
            "--nocapture",
            "--no-capture",
            "--ignored",
            "--include-ignored",
            "--skip",
            "slow",
            "--skip=flaky",
        ]))
        .unwrap();
    }

    #[test]
    fn dry_run_shows_the_nextest_command_and_each_test() {
        let selectors = args(&["t::a", "pkg::it$t::b"]);
        let lines = super::dry_run_lines(&selectors, &args(&["--exact"]), 4).unwrap();
        assert_eq!(lines[0], "RUST BATCH selectors=2 jobs=4");
        assert!(
            lines[1].starts_with("cargo nextest run '--workspace' '--test-threads' 4 '-E'"),
            "{}",
            lines[1]
        );
        assert!(
            lines[1].contains("test(/(^|::)t::a$/) | test(/(^|::)t::b$/)"),
            "{}",
            lines[1]
        );
        assert!(lines[1].ends_with("'--' '--exact'"), "{}", lines[1]);
        assert_eq!(
            &lines[2..],
            ["RUST SELECTOR t::a", "RUST SELECTOR pkg::it$t::b"]
        );
        assert!(super::dry_run_lines(&[], &[], 4).unwrap().is_empty());
        assert!(super::dry_run_lines(&selectors, &args(&["--format"]), 4).is_err());
    }

    #[test]
    fn unsupported_or_incomplete_test_arguments_are_rejected() {
        for bad in [
            &["--format", "json"][..],
            &["--skip"],
            &["--skip", ""],
            &["--skip="],
        ] {
            assert!(validate_rust_extra_args(&args(bad)).is_err(), "{bad:?}");
        }
    }
}
