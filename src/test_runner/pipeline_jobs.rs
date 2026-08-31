#[cfg(test)]
use std::sync::Arc;
use std::sync::Mutex;
#[cfg(test)]
use std::sync::OnceLock;
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use kiss::Language;

use super::SharedPrefix;
use super::cover_language;
use super::split_jobs;
use crate::test_runner::RunTestCmdArgs;
use crate::test_runner::planned_selectors::{
    PlannedSelectors, SelectorRunOptions, apply_cold_initialization_population,
    apply_force_all_population,
};
use crate::test_runner::run_logic::{execute_one_language, language_has_work};

#[cfg(test)]
pub(crate) struct CoveringHooks {
    pub python: Option<Arc<dyn Fn() + Send + Sync>>,
    pub rust: Option<Arc<dyn Fn() + Send + Sync>>,
}

#[cfg(test)]
pub(crate) static COVERING_HOOKS: Mutex<CoveringHooks> = Mutex::new(CoveringHooks {
    python: None,
    rust: None,
});

#[cfg(test)]
static BLOCKED_PLANNER: OnceLock<Mutex<Option<Language>>> = OnceLock::new();
#[cfg(test)]
static PARKED_COVERING: OnceLock<Mutex<Option<std::thread::Thread>>> = OnceLock::new();

#[cfg(test)]
pub(crate) static STUB_LANGUAGE_EXECUTE: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
pub(crate) struct ExecuteHooks {
    pub python: Option<Arc<dyn Fn() + Send + Sync>>,
    pub rust: Option<Arc<dyn Fn() + Send + Sync>>,
}

#[cfg(test)]
pub(crate) static EXECUTE_HOOKS: Mutex<ExecuteHooks> = Mutex::new(ExecuteHooks {
    python: None,
    rust: None,
});

#[cfg(test)]
static FAIL_COVERING: Mutex<Option<Language>> = Mutex::new(None);

#[cfg(test)]
pub(crate) fn set_fail_covering(language: Option<Language>) {
    *FAIL_COVERING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = language;
}

#[cfg(test)]
pub(crate) fn set_blocked_covering_language(language: Option<Language>) {
    *BLOCKED_PLANNER
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = language;
    if language.is_none() {
        unpark_blocked_covering();
    }
}

#[cfg(test)]
pub(crate) fn unpark_blocked_covering() {
    if let Some(thread) = PARKED_COVERING
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
    {
        thread.unpark();
    }
}

#[derive(Default)]
pub(super) struct LanguageSlots {
    python_planned: Mutex<Option<PlannedSelectors>>,
    rust_planned: Mutex<Option<PlannedSelectors>>,
    python_outcome:
        Mutex<Option<Result<crate::test_runner::run_logic::LanguagePhaseOutcome, String>>>,
    rust_outcome:
        Mutex<Option<Result<crate::test_runner::run_logic::LanguagePhaseOutcome, String>>>,
}

pub(super) fn spawn_language_jobs(
    a: &RunTestCmdArgs<'_>,
    prefix: &SharedPrefix,
    slots: &LanguageSlots,
) -> Result<(), String> {
    let spawn_python = prefix.python_may_work && a.lang_filter != Some(Language::Rust);
    let spawn_rust = prefix.rust_may_work && a.lang_filter != Some(Language::Python);
    let (python_jobs, rust_jobs) = split_jobs(a.jobs, spawn_python && spawn_rust);
    std::thread::scope(|scope| {
        let python = spawn_one(
            scope,
            spawn_python,
            a,
            prefix,
            Language::Python,
            python_jobs,
            &slots.python_planned,
            &slots.python_outcome,
        );
        let rust = spawn_one(
            scope,
            spawn_rust,
            a,
            prefix,
            Language::Rust,
            rust_jobs,
            &slots.rust_planned,
            &slots.rust_outcome,
        );
        join_named(python, "python")?;
        join_named(rust, "rust")
    })
}

#[allow(clippy::too_many_arguments)]
fn spawn_one<'scope, 'env>(
    scope: &'scope std::thread::Scope<'scope, 'env>,
    spawn: bool,
    a: &'env RunTestCmdArgs<'env>,
    prefix: &'env SharedPrefix,
    language: Language,
    jobs: usize,
    planned_out: &'env Mutex<Option<PlannedSelectors>>,
    outcome_out: &'env Mutex<
        Option<Result<crate::test_runner::run_logic::LanguagePhaseOutcome, String>>,
    >,
) -> Option<std::thread::ScopedJoinHandle<'scope, Result<(), String>>> {
    spawn.then(|| {
        scope.spawn(move || language_job(a, prefix, language, jobs, planned_out, outcome_out))
    })
}

fn join_named(
    handle: Option<std::thread::ScopedJoinHandle<'_, Result<(), String>>>,
    name: &str,
) -> Result<(), String> {
    match handle {
        Some(handle) => handle
            .join()
            .unwrap_or_else(|_| Err(format!("{name} language job panicked"))),
        None => Ok(()),
    }
}

pub(super) fn take_job_results(
    slots: &LanguageSlots,
) -> Result<
    (
        Option<crate::test_runner::run_logic::LanguagePhaseOutcome>,
        Option<crate::test_runner::run_logic::LanguagePhaseOutcome>,
    ),
    String,
> {
    Ok((
        take_outcome(&slots.python_outcome)?,
        take_outcome(&slots.rust_outcome)?,
    ))
}

fn take_outcome(
    slot: &Mutex<Option<Result<crate::test_runner::run_logic::LanguagePhaseOutcome, String>>>,
) -> Result<Option<crate::test_runner::run_logic::LanguagePhaseOutcome>, String> {
    match take_mutex(slot) {
        Some(Err(err)) => Err(err),
        Some(Ok(outcome)) => Ok(Some(outcome)),
        None => Ok(None),
    }
}

pub(super) fn take_planned(
    slots: &LanguageSlots,
) -> (Option<PlannedSelectors>, Option<PlannedSelectors>) {
    (
        take_mutex(&slots.python_planned),
        take_mutex(&slots.rust_planned),
    )
}

fn take_mutex<T>(slot: &Mutex<Option<T>>) -> Option<T> {
    slot.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
}

fn language_job(
    a: &RunTestCmdArgs<'_>,
    prefix: &SharedPrefix,
    language: Language,
    jobs: usize,
    planned_out: &Mutex<Option<PlannedSelectors>>,
    outcome_out: &Mutex<
        Option<Result<crate::test_runner::run_logic::LanguagePhaseOutcome, String>>,
    >,
) -> Result<(), String> {
    let covering_name = match language {
        Language::Python => "covering_python",
        Language::Rust => "covering_rust",
    };
    crate::test_runner::emit_test_progress(&format!("kiss test: Running {covering_name}"));
    invoke_covering_hook(language);
    if language == Language::Rust {
        kiss::rust_llvm_cov_runner::begin_identity_memo();
    }
    let _list_build = (language == Language::Rust).then(|| {
        crate::test_runner::rust_list_build::install_job(
            prefix.repo_root.clone(),
            a.extra.to_vec(),
            jobs,
            a.dry_run,
        )
    });
    let covering_started = Instant::now();
    let mut planned = cover_language(a, prefix, language)?;
    crate::test_runner::emit_test_progress(&format!(
        "kiss test: Ran {covering_name} {}ms",
        covering_started.elapsed().as_millis()
    ));
    if prefix.cold_init {
        apply_cold_initialization_population(a, &mut planned);
    }
    apply_force_all_population(a, &mut planned);
    crate::test_runner::apply_force_bad(a, &mut planned)?;
    *planned_out
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(planned.clone());
    if a.dry_run {
        return Ok(());
    }
    invoke_execute_hook(language);
    if stub_language_execute() {
        return Ok(());
    }
    if !language_has_work(&planned, language) {
        return Ok(());
    }
    let options = SelectorRunOptions {
        dry_run: false,
        force_rerun: a.force_rerun,
        metrics: a.metrics,
        jobs,
        extras: crate::test_runner::language_keyed::LanguageKeyed {
            python: a.python_extra,
            rust: a.extra,
        },
        plan_duration: std::time::Duration::ZERO,
        gate: a.gate_config.clone(),
    };
    let result = execute_one_language(&planned, &options, language);
    *outcome_out
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(result);
    Ok(())
}

fn invoke_covering_hook(language: Language) {
    #[cfg(test)]
    {
        if let Some(hook) = match language {
            Language::Python => COVERING_HOOKS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .python
                .clone(),
            Language::Rust => COVERING_HOOKS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .rust
                .clone(),
        } {
            hook();
        }
        if BLOCKED_PLANNER
            .get()
            .and_then(|lock| {
                lock.lock()
                    .ok()
                    .and_then(|guard| (*guard == Some(language)).then_some(()))
            })
            .is_some()
        {
            *PARKED_COVERING
                .get_or_init(|| Mutex::new(None))
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(std::thread::current());
            std::thread::park();
        }
    }
    let _ = language;
}

fn invoke_execute_hook(language: Language) {
    #[cfg(test)]
    {
        if let Some(hook) = match language {
            Language::Python => EXECUTE_HOOKS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .python
                .clone(),
            Language::Rust => EXECUTE_HOOKS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .rust
                .clone(),
        } {
            hook();
        }
    }
    let _ = language;
}

fn stub_language_execute() -> bool {
    #[cfg(test)]
    {
        STUB_LANGUAGE_EXECUTE.load(Ordering::SeqCst)
    }
    #[cfg(not(test))]
    {
        false
    }
}

pub(super) fn covering_should_fail(language: Language) -> bool {
    #[cfg(test)]
    {
        FAIL_COVERING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            == Some(&language)
    }
    #[cfg(not(test))]
    {
        let _ = language;
        false
    }
}
