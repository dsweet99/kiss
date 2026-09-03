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
#[path = "pipeline_job_share.rs"]
mod job_share;
use crate::test_runner::RunTestCmdArgs;
use crate::test_runner::planned_selectors::{
    PlannedSelectors, SelectorRunOptions, apply_cold_initialization_population,
    apply_force_all_population,
};
use crate::test_runner::run_logic::{execute_one_language, language_has_work};
use job_share::JobShare;

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
    first_error: Mutex<Option<String>>,
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
    let spawn = LanguageSpawn {
        python: prefix.python_may_work && a.lang_filter != Some(Language::Rust),
        rust: prefix.rust_may_work && a.lang_filter != Some(Language::Python),
    };
    let share = JobShare::new(a.jobs, spawn.python && spawn.rust);
    std::thread::scope(|scope| join_language_scope(scope, a, prefix, &share, spawn, slots))
}

fn join_language_scope<'scope, 'env: 'scope>(
    scope: &'scope std::thread::Scope<'scope, 'env>,
    a: &'env RunTestCmdArgs<'env>,
    prefix: &'env SharedPrefix,
    share: &'env JobShare,
    spawn: LanguageSpawn,
    slots: &'env LanguageSlots,
) -> Result<(), String> {
    let python = start_language(
        scope,
        spawn.python,
        LanguageJob {
            a,
            prefix,
            language: Language::Python,
            share,
            planned_out: &slots.python_planned,
            outcome_out: &slots.python_outcome,
            first_error: &slots.first_error,
        },
    );
    let rust = start_language(
        scope,
        spawn.rust,
        LanguageJob {
            a,
            prefix,
            language: Language::Rust,
            share,
            planned_out: &slots.rust_planned,
            outcome_out: &slots.rust_outcome,
            first_error: &slots.first_error,
        },
    );
    if let Err(err) = join_named(python, "python") {
        let needs_cancel = !has_recorded_error(&slots.first_error);
        record_first_error(&slots.first_error, err);
        if needs_cancel {
            cancel_peer(Language::Python);
        }
    }
    if let Err(err) = join_named(rust, "rust") {
        let needs_cancel = !has_recorded_error(&slots.first_error);
        record_first_error(&slots.first_error, err);
        if needs_cancel {
            cancel_peer(Language::Rust);
        }
    }
    take_mutex(&slots.first_error).map_or(Ok(()), Err)
}

fn start_language<'scope, 'env: 'scope>(
    scope: &'scope std::thread::Scope<'scope, 'env>,
    spawn: bool,
    job: LanguageJob<'env>,
) -> Option<std::thread::ScopedJoinHandle<'scope, Result<(), String>>> {
    spawn.then(|| scope.spawn(move || language_job(job)))
}

#[derive(Clone, Copy)]
struct LanguageSpawn {
    python: bool,
    rust: bool,
}

struct LanguageJob<'a> {
    a: &'a RunTestCmdArgs<'a>,
    prefix: &'a SharedPrefix,
    language: Language,
    share: &'a JobShare,
    planned_out: &'a Mutex<Option<PlannedSelectors>>,
    outcome_out:
        &'a Mutex<Option<Result<crate::test_runner::run_logic::LanguagePhaseOutcome, String>>>,
    first_error: &'a Mutex<Option<String>>,
}

fn join_named(
    handle: Option<std::thread::ScopedJoinHandle<'_, Result<(), String>>>,
    name: &str,
) -> Result<(), String> {
    handle.map_or(Ok(()), |handle| {
        handle
            .join()
            .unwrap_or_else(|_| Err(format!("{name} language job panicked")))
    })
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
    take_mutex(slot).map_or(Ok(None), |result| result.map(Some))
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

fn language_job(job: LanguageJob<'_>) -> Result<(), String> {
    let LanguageJob {
        a,
        prefix,
        language,
        share,
        planned_out,
        outcome_out,
        first_error,
    } = job;
    let planned = match run_covering(a, prefix, language, share.covering(language)) {
        Ok(planned) => planned,
        Err(err) => return fail_language_job(language, first_error, err),
    };
    if has_recorded_error(first_error) {
        return Ok(());
    }
    *planned_out
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(planned.clone());
    if a.dry_run || has_recorded_error(first_error) {
        return Ok(());
    }
    invoke_execute_hook(language);
    if stub_language_execute() || !language_has_work(&planned, language) {
        return Ok(());
    }
    let turn = share.acquire_execute(language);
    if let Err(err) = execute_planned(a, turn.jobs, language, &planned, outcome_out) {
        return fail_language_job(language, first_error, err);
    }
    Ok(())
}

fn fail_language_job(
    language: Language,
    first_error: &Mutex<Option<String>>,
    err: String,
) -> Result<(), String> {
    record_first_error(first_error, err.clone());
    cancel_peer(language);
    Err(err)
}

fn cancel_peer(language: Language) {
    match language {
        Language::Python => kiss::rust_llvm_cov_runner::cancel_active_batch_scope(),
        Language::Rust => kiss::rpytest_runner::cancel_active_forkservers(),
    }
}

fn record_first_error(slot: &Mutex<Option<String>>, err: String) {
    let mut first = slot
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if first.is_none() {
        *first = Some(err);
    }
}

fn has_recorded_error(slot: &Mutex<Option<String>>) -> bool {
    slot.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_some()
}

fn run_covering(
    a: &RunTestCmdArgs<'_>,
    prefix: &SharedPrefix,
    language: Language,
    jobs: usize,
) -> Result<PlannedSelectors, String> {
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
    Ok(planned)
}

fn execute_planned(
    a: &RunTestCmdArgs<'_>,
    jobs: usize,
    language: Language,
    planned: &PlannedSelectors,
    outcome_out: &Mutex<
        Option<Result<crate::test_runner::run_logic::LanguagePhaseOutcome, String>>,
    >,
) -> Result<(), String> {
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
    match execute_one_language(planned, &options, language) {
        Ok(outcome) => {
            *outcome_out
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Ok(outcome));
            Ok(())
        }
        Err(err) => {
            *outcome_out
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Err(err.clone()));
            Err(err)
        }
    }
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
    if STUB_LANGUAGE_EXECUTE.load(Ordering::SeqCst) {
        return true;
    }
    false
}

pub(super) fn covering_should_fail(language: Language) -> bool {
    #[cfg(test)]
    if FAIL_COVERING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        == Some(&language)
    {
        return true;
    }
    let _ = language;
    false
}
