use std::fmt;
use std::path::Path;
use std::sync::Mutex;

use crate::test_runner::check_line_coverage::RequiredCoverageLanguages;
use crate::test_runner::ensure_runtime::{ensure_languages_runtime, ensure_request_for_all};

pub(crate) const COVERAGE_RUNTIME_REFRESH_ACTIVE_ENV: &str = "KISS_COVERAGE_RUNTIME_REFRESH_ACTIVE";

pub(crate) fn test_runner_stdout_enabled() -> bool {
    std::env::var_os(COVERAGE_RUNTIME_REFRESH_ACTIVE_ENV).is_none()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CoverageRefreshError {
    Discovery {
        language: &'static str,
        reason: String,
    },
    TestExecution {
        language: &'static str,
        total: usize,
        failed: usize,
        exit_code: i32,
    },
    Publication {
        language: &'static str,
        reason: String,
    },
}

impl CoverageRefreshError {
    pub(crate) fn discovery(language: &'static str, err: impl ToString) -> Self {
        Self::Discovery {
            language,
            reason: err.to_string(),
        }
    }

    pub(crate) fn publication(language: &'static str, err: impl ToString) -> Self {
        Self::Publication {
            language,
            reason: err.to_string(),
        }
    }
}

impl fmt::Display for CoverageRefreshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoverageRefreshError::Discovery { language, reason } => write!(
                f,
                "error: kiss test: failed to refresh {language} runtime line coverage during test discovery: {reason}"
            ),
            CoverageRefreshError::TestExecution {
                language,
                total,
                failed,
                exit_code,
            } => write!(
                f,
                "error: kiss test: failed to refresh {language} runtime line coverage because the population test run failed ({failed}/{total} tests failed, exit code {exit_code})"
            ),
            CoverageRefreshError::Publication { language, reason } => write!(
                f,
                "error: kiss test: failed to refresh {language} runtime line coverage during publication: {reason}"
            ),
        }
    }
}

pub(crate) fn ensure_check_runtime_coverage(
    repo_root: &Path,
    required: RequiredCoverageLanguages,
    ignore: &[String],
    jobs: usize,
    pytest_args: &[String],
    gate: &kiss::GateConfig,
) -> Result<(), CoverageRefreshError> {
    let languages: Vec<kiss::Language> = [
        (kiss::Language::Python, required.python),
        (kiss::Language::Rust, required.rust),
    ]
    .into_iter()
    .filter_map(|(language, wanted)| wanted.then_some(language))
    .collect();
    if languages.is_empty() {
        return Ok(());
    }
    let _refresh_env = ScopedRefreshEnvGuard::set();
    std::thread::scope(|scope| {
        let runs: Vec<_> = languages
            .iter()
            .map(|&language| {
                let run = scope.spawn(move || {
                    refresh_language(repo_root, ignore, jobs, pytest_args, gate, language)
                });
                (language, run)
            })
            .collect();
        runs.into_iter().try_for_each(|(language, run)| {
            run.join().unwrap_or_else(|_| {
                Err(CoverageRefreshError::publication(
                    display_name(language),
                    "refresh thread panicked",
                ))
            })
        })
    })
}

fn refresh_language(
    repo_root: &Path,
    ignore: &[String],
    jobs: usize,
    pytest_args: &[String],
    gate: &kiss::GateConfig,
    language: kiss::Language,
) -> Result<(), CoverageRefreshError> {
    let name = display_name(language);
    let request = ensure_request_for_all(
        repo_root,
        ignore,
        jobs,
        Some(language),
        false,
        gate.clone(),
        pytest_args.to_vec(),
    )
    .map_err(|err| CoverageRefreshError::discovery(name, err))?;
    eprintln!(
        "kiss test: refreshing {name} runtime coverage ({} tests)",
        request.planned.get(language).len()
    );
    let result = ensure_languages_runtime(&request)
        .map_err(|err| CoverageRefreshError::publication(name, err))?;
    let summary = result
        .by_language
        .get(language)
        .as_ref()
        .map(|run| run.summary.clone())
        .unwrap_or_default();
    if summary.exit_code != 0 {
        return Err(CoverageRefreshError::TestExecution {
            language: name,
            total: summary.total,
            failed: summary.failed,
            exit_code: summary.exit_code,
        });
    }
    Ok(())
}

fn display_name(language: kiss::Language) -> &'static str {
    match language {
        kiss::Language::Python => "Python",
        kiss::Language::Rust => "Rust",
    }
}

pub(crate) struct ScopedRefreshEnvGuard {
    _private: (),
}

static REFRESH_ENV_STATE: Mutex<(usize, Option<Option<std::ffi::OsString>>)> =
    Mutex::new((0, None));

impl ScopedRefreshEnvGuard {
    pub(crate) fn set() -> Self {
        let mut state = REFRESH_ENV_STATE.lock().expect("refresh env state lock");
        if state.0 == 0 {
            state.1 = Some(std::env::var_os(COVERAGE_RUNTIME_REFRESH_ACTIVE_ENV));

            unsafe { std::env::set_var(COVERAGE_RUNTIME_REFRESH_ACTIVE_ENV, "1") };
        }
        state.0 += 1;
        Self { _private: () }
    }
}

impl Drop for ScopedRefreshEnvGuard {
    fn drop(&mut self) {
        let mut state = REFRESH_ENV_STATE.lock().expect("refresh env state lock");
        state.0 = state.0.saturating_sub(1);
        if state.0 == 0 {
            let old = state.1.take().flatten();
            restore_refresh_active_env(old);
        }
    }
}

pub(crate) fn restore_refresh_active_env(old: Option<std::ffi::OsString>) {
    let key = COVERAGE_RUNTIME_REFRESH_ACTIVE_ENV;
    match old {
        Some(value) => unsafe { std::env::set_var(key, value) },

        None => unsafe { std::env::remove_var(key) },
    }
}

#[cfg(test)]
#[path = "check_runtime_refresh_test.rs"]
mod tests;
