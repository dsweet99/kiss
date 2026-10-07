use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use kiss::Language;

use super::SharedPrefix;
use super::select_language;
#[path = "pipeline_job_share.rs"]
mod job_share;
use crate::test_runner::RunTestCmdArgs;
use crate::test_runner::language_keyed::LanguageKeyed;
use crate::test_runner::planned_selectors::{
    PlannedSelectors, SelectorRunOptions, apply_cold_initialization_population,
    apply_force_all_population,
};
use crate::test_runner::run_logic::{execute_one_language, language_has_work};
use job_share::JobShare;

pub(crate) type PipelineHook = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct PipelineDoubles {
    pub selecting: LanguageKeyed<Option<PipelineHook>>,
    pub execute: LanguageKeyed<Option<PipelineHook>>,
    pub stub_execute: bool,
    pub fail_selecting: Option<Language>,
    pub block_selecting: Option<Language>,
    pub release: SelectingRelease,
}

#[derive(Default)]
pub(crate) struct SelectingRelease {
    released: Mutex<bool>,
    signal: Condvar,
}

#[cfg_attr(not(test), allow(dead_code))]
impl PipelineDoubles {
    pub(crate) fn release_blocked_selecting(&self) {
        *self
            .release
            .released
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.release.signal.notify_all();
    }

    fn wait_if_blocked(&self, language: Language) {
        if self.block_selecting != Some(language) {
            return;
        }
        let mut released = self
            .release
            .released
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while !*released {
            released = self
                .release
                .signal
                .wait(released)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }
}

type OutcomeSlot =
    Mutex<Option<Result<crate::test_runner::run_logic::LanguagePhaseOutcome, String>>>;

#[derive(Default)]
pub(super) struct LanguageSlots {
    planned: LanguageKeyed<Mutex<Option<PlannedSelectors>>>,
    first_error: Mutex<Option<String>>,
    outcome: LanguageKeyed<OutcomeSlot>,
}

pub(super) fn spawn_language_jobs(
    a: &RunTestCmdArgs<'_>,
    prefix: &SharedPrefix,
    slots: &LanguageSlots,
) -> Result<(), String> {
    let spawn = prefix.may_work;
    let share = JobShare::new(a.jobs);
    for language in crate::test_runner::lang_registry::languages() {
        if *spawn.get(language) {
            crate::test_runner::tests_remaining::expect_language_remaining(language);
        }
    }
    std::thread::scope(|scope| join_language_scope(scope, a, prefix, &share, spawn, slots))
}

fn join_language_scope<'scope, 'env: 'scope>(
    scope: &'scope std::thread::Scope<'scope, 'env>,
    a: &'env RunTestCmdArgs<'env>,
    prefix: &'env SharedPrefix,
    share: &'env JobShare,
    spawn: LanguageKeyed<bool>,
    slots: &'env LanguageSlots,
) -> Result<(), String> {
    let handles = crate::test_runner::lang_registry::languages().map(|language| {
        let job = LanguageJob {
            a,
            prefix,
            language,
            share,
            planned_out: slots.planned.get(language),
            outcome_out: slots.outcome.get(language),
            first_error: &slots.first_error,
        };
        (language, start_language(scope, *spawn.get(language), job))
    });
    for (language, handle) in handles {
        if let Err(err) = join_named(handle, language.label()) {
            let needs_cancel = !has_recorded_error(&slots.first_error);
            record_first_error(&slots.first_error, err);
            if needs_cancel {
                cancel_peer(language);
            }
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

struct LanguageJob<'a> {
    a: &'a RunTestCmdArgs<'a>,
    prefix: &'a SharedPrefix,
    language: Language,
    share: &'a JobShare,
    planned_out: &'a Mutex<Option<PlannedSelectors>>,
    outcome_out: &'a OutcomeSlot,
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
) -> Result<LanguageKeyed<Option<crate::test_runner::run_logic::LanguagePhaseOutcome>>, String> {
    let mut results = LanguageKeyed::default();
    for language in crate::test_runner::lang_registry::languages() {
        *results.get_mut(language) = take_outcome(slots.outcome.get(language))?;
    }
    Ok(results)
}

fn take_outcome(
    slot: &OutcomeSlot,
) -> Result<Option<crate::test_runner::run_logic::LanguagePhaseOutcome>, String> {
    take_mutex(slot).map_or(Ok(None), |result| result.map(Some))
}

pub(super) fn take_planned(slots: &LanguageSlots) -> LanguageKeyed<Option<PlannedSelectors>> {
    LanguageKeyed::from_fn(|language| take_mutex(slots.planned.get(language)))
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
    let _progress_lang = kiss::watch_report::ProgressLanguageGuard::enter(language);
    let planned = match run_selecting(a, prefix, language) {
        Ok(planned) => planned,
        Err(err) => return fail_language_job(language, first_error, err),
    };
    if has_recorded_error(first_error) {
        crate::test_runner::tests_remaining::set_language_remaining(language, 0);
        return Ok(());
    }
    *planned_out
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(planned.clone());
    if a.dry_run || has_recorded_error(first_error) {
        crate::test_runner::tests_remaining::set_language_remaining(language, 0);
        return Ok(());
    }
    invoke_execute_hook(a, language);
    if stub_language_execute(a) || !language_has_work(&planned, language) {
        crate::test_runner::tests_remaining::set_language_remaining(language, 0);
        return Ok(());
    }
    let turn = share.acquire_execute(language);
    if let Err(err) = execute_planned(a, turn.jobs, language, &planned, outcome_out) {
        return fail_language_job(language, first_error, err);
    }
    crate::test_runner::tests_remaining::set_language_remaining(language, 0);
    Ok(())
}

fn fail_language_job(
    language: Language,
    first_error: &Mutex<Option<String>>,
    err: String,
) -> Result<(), String> {
    crate::test_runner::tests_remaining::set_language_remaining(language, 0);
    record_first_error(first_error, err.clone());
    cancel_peer(language);
    Err(err)
}

fn cancel_peer(language: Language) {
    for peer in crate::test_runner::lang_registry::languages()
        .into_iter()
        .filter(|peer| *peer != language)
    {
        crate::test_runner::lang_registry::rules_for(peer).cancel_active_work();
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

fn run_selecting(
    a: &RunTestCmdArgs<'_>,
    prefix: &SharedPrefix,
    language: Language,
) -> Result<PlannedSelectors, String> {
    let selecting_name = format!("select_{}", language.label());
    crate::test_runner::emit_test_progress(&format!("kiss test: Running {selecting_name}"));
    invoke_selecting_hook(a, language);
    let selecting_started = Instant::now();
    let mut planned = select_language(a, prefix, language)?;
    crate::test_runner::emit_test_progress(&format!(
        "kiss test: Ran {selecting_name} {}ms",
        selecting_started.elapsed().as_millis()
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
        extras: a.extras,
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

fn doubles<'a>(a: &'a RunTestCmdArgs<'_>) -> Option<&'a PipelineDoubles> {
    a.doubles.as_deref()
}

fn invoke_selecting_hook(a: &RunTestCmdArgs<'_>, language: Language) {
    if let Some(doubles) = doubles(a) {
        if let Some(hook) = doubles.selecting.get(language) {
            hook();
        }
        doubles.wait_if_blocked(language);
    }
}

fn invoke_execute_hook(a: &RunTestCmdArgs<'_>, language: Language) {
    if let Some(hook) = doubles(a).and_then(|doubles| doubles.execute.get(language).as_ref()) {
        hook();
    }
}

fn stub_language_execute(a: &RunTestCmdArgs<'_>) -> bool {
    doubles(a).is_some_and(|doubles| doubles.stub_execute)
}

pub(super) fn selecting_should_fail(a: &RunTestCmdArgs<'_>, language: Language) -> bool {
    doubles(a).is_some_and(|doubles| doubles.fail_selecting == Some(language))
}
