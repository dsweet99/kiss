use std::path::{Path, PathBuf};

use crate::test_runner::coverage_decision::TestSelector;
use crate::test_runner::last_status::{
    has_language_records, prior_failures, python_last_status_identity, rust_last_status_identity,
};

pub(crate) fn prior_failures_for_language(
    repo_root: &Path,
    language: kiss::Language,
    test_args: &[String],
) -> Result<Vec<TestSelector>, String> {
    if !has_language_records(repo_root, language)? {
        return Ok(Vec::new());
    }
    let identity = match language {
        kiss::Language::Python => {
            let python = PathBuf::from("python");
            let python_version = crate::test_runner::runners::command_stdout(
                &python,
                &[
                    "-c",
                    "import sys; print('.'.join(map(str, sys.version_info[:3])))",
                ],
                repo_root,
            )?;
            let pytest_version = crate::test_runner::runners::command_stdout(
                &python,
                &["-c", "import pytest; print(pytest.__version__)"],
                repo_root,
            )?;
            python_last_status_identity(&python_version, &pytest_version, test_args)
        }
        kiss::Language::Rust => {
            let cargo = PathBuf::from("cargo");
            let rustc = PathBuf::from("rustc");
            let cargo_version =
                crate::test_runner::runners::command_stdout(&cargo, &["--version"], repo_root)?;
            let llvm_cov_version = crate::test_runner::runners::command_stdout(
                &cargo,
                &["llvm-cov", "--version"],
                repo_root,
            )?;
            let cargo_nextest_version = crate::test_runner::runners::command_stdout(
                &cargo,
                &["nextest", "--version"],
                repo_root,
            )?;
            let rustc_version =
                crate::test_runner::runners::command_stdout(&rustc, &["-Vv"], repo_root)?;
            let runner_map_fingerprint =
                crate::test_runner::rust_coverage_index::current_rust_runner_map_fingerprint(
                    repo_root, test_args,
                )?;
            rust_last_status_identity(
                &cargo_version,
                &llvm_cov_version,
                &rustc_version,
                &cargo_nextest_version,
                test_args,
                &runner_map_fingerprint,
            )
        }
    };
    Ok(prior_failures(repo_root, language, &identity)?
        .into_iter()
        .map(|id| TestSelector::new(language, id))
        .collect())
}
