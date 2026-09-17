use kiss::rslip::{
    CacheStatus as PyCacheStatus, RslipBatchProgress, RslipError, RslipOutcome,
};

use super::rslip_request::timeout_for_selector_with_gate;

pub(super) fn handle_rslip_batch_progress(
    event: RslipBatchProgress,
    selectors: &[String],
    gate: &kiss::GateConfig,
) {
    match event {
        RslipBatchProgress::Prepared {
            cache_hits,
            cache_misses,
            elapsed: _,
        } => {
            crate::test_runner::emit_test_progress(&format!(
                "kiss test: rslip prepared hits={cache_hits} misses={cache_misses}"
            ));
        }
        RslipBatchProgress::SelectorFinalized { outcomes } => {
            emit_finalized_outcomes(outcomes, selectors, gate);
        }
        RslipBatchProgress::CachedStatusDump { outcomes } => {
            emit_cached_hit_outcomes(&outcomes, gate);
        }
        RslipBatchProgress::TestsRemaining { remaining } => {
            crate::test_runner::tests_remaining::emit_tests_remaining(remaining);
        }
    }
}

pub(super) fn emit_finalized_outcomes(
    outcomes: Vec<(usize, Result<RslipOutcome, RslipError>)>,
    selectors: &[String],
    gate: &kiss::GateConfig,
) {
    for (index, result) in outcomes {
        match result {
            Ok(outcome) => print_rslip_outcome(&outcome, gate),
            Err(err) => {
                let selector = selectors
                    .get(index)
                    .map(String::as_str)
                    .unwrap_or("<unknown>");
                if rslip_protocol_is_quiet_timeout(&err) {
                    let timeout = timeout_for_selector_with_gate(gate, selector);
                    crate::test_runner::emit_test_status(&format!(
                        "TIMEOUT: {selector} ({:.2}s)",
                        timeout.as_secs_f64()
                    ));
                } else {
                    crate::test_runner::emit_test_status(&format!(
                        "FAIL: {selector} (rslip error)"
                    ));
                    eprintln!("{}", format_rslip_error(err));
                }
            }
        }
    }
}

fn emit_cached_hit_outcomes(outcomes: &[RslipOutcome], gate: &kiss::GateConfig) {
    let gated: Vec<RslipOutcome> = outcomes
        .iter()
        .map(|outcome| {
            let mut gated = outcome.clone();
            gated.status = crate::test_runner::status_labels::apply_unit_test_time_limit(
                outcome.status,
                &outcome.nodeid,
                outcome.duration,
                gate,
            );
            gated
        })
        .collect();
    emit_progress_lines(&kiss::rslip::format_cached_status_dump(&gated));
}

pub(super) fn emit_progress_lines(body: &str) {
    for line in body.lines() {
        if !line.is_empty() {
            crate::test_runner::emit_test_progress(line);
        }
    }
}

pub(super) fn print_rslip_outcome(outcome: &RslipOutcome, gate: &kiss::GateConfig) {
    let status = crate::test_runner::status_labels::apply_unit_test_time_limit(
        outcome.status,
        &outcome.nodeid,
        outcome.duration,
        gate,
    );
    let cache_tag = match outcome.cache_status {
        PyCacheStatus::Hit => Some("cached"),
        PyCacheStatus::MissStored => None,
    };
    crate::test_runner::status_labels::print_classified_status_line(
        status,
        &outcome.nodeid,
        outcome.duration,
        cache_tag,
        cache_tag.is_none(),
    );
    if matches!(
        status,
        kiss::rpytest_runner::TestStatus::Failed | kiss::rpytest_runner::TestStatus::TimedOut
    ) && outcome.cache_status != PyCacheStatus::Hit
        && let Some(stderr) = &outcome.stderr
        && !stderr.is_empty()
    {
        eprint!("{}", String::from_utf8_lossy(stderr));
    }
}

pub(super) fn format_rslip_error(err: RslipError) -> String {
    format!("error: kiss test: rslip failed: {err:?}")
}

pub(super) fn rslip_protocol_is_quiet_timeout(err: &RslipError) -> bool {
    matches!(
        err,
        RslipError::Runner(kiss::rpytest_runner::PytestRunError::Protocol(message))
            if message.contains("module batch result missing")
                || message.contains("module batch timed out")
    )
}

pub(super) fn status_for_rslip_error(err: &RslipError) -> (kiss::rpytest_runner::TestStatus, i32) {
    if rslip_protocol_is_quiet_timeout(err) {
        (kiss::rpytest_runner::TestStatus::TimedOut, 124)
    } else {
        (kiss::rpytest_runner::TestStatus::Failed, 1)
    }
}
