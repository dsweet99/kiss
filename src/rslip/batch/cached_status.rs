use std::collections::BTreeMap;

use crate::rslip::{RslipError, RslipOutcome};

use super::RslipBatchProgress;

pub(super) fn format_cached_status_dump(outcomes: &[RslipOutcome]) -> String {
    if outcomes.len() > 32 {
        return format_cached_status_totals(outcomes);
    }
    format_cached_status_each(outcomes)
}

fn format_cached_status_totals(outcomes: &[RslipOutcome]) -> String {
    let (passed, failed, timed_out) = count_cached_statuses(outcomes);
    let mut body = String::new();
    append_cached_total(&mut body, "PASS", passed);
    append_cached_total(&mut body, "FAIL", failed);
    append_cached_total(&mut body, "TIMEOUT", timed_out);
    body
}

fn format_cached_status_each(outcomes: &[RslipOutcome]) -> String {
    let mut body = String::with_capacity(outcomes.len().saturating_mul(48));
    for outcome in outcomes {
        body.push_str(cached_status_label(outcome.status));
        body.push_str(&outcome.nodeid);
        body.push('\n');
    }
    body
}

fn count_cached_statuses(outcomes: &[RslipOutcome]) -> (usize, usize, usize) {
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut timed_out = 0usize;
    for outcome in outcomes {
        match outcome.status {
            crate::rpytest_runner::TestStatus::Passed => passed += 1,
            crate::rpytest_runner::TestStatus::Failed => failed += 1,
            crate::rpytest_runner::TestStatus::TimedOut => timed_out += 1,
        }
    }
    (passed, failed, timed_out)
}

fn append_cached_total(body: &mut String, label: &str, count: usize) {
    if count > 0 {
        body.push_str(&format!("{label} (cached): {count} selectors\n"));
    }
}

fn cached_status_label(status: crate::rpytest_runner::TestStatus) -> &'static str {
    match status {
        crate::rpytest_runner::TestStatus::Passed => "PASS (cached): ",
        crate::rpytest_runner::TestStatus::Failed => "FAIL (cached): ",
        crate::rpytest_runner::TestStatus::TimedOut => "TIMEOUT (cached): ",
    }
}

pub(super) fn emit_prepare_resolved_progress(
    out: &[Option<Result<RslipOutcome, RslipError>>],
    on_progress: &mut impl FnMut(RslipBatchProgress),
) {
    let hits: Vec<RslipOutcome> = out
        .iter()
        .filter_map(|slot| match slot {
            Some(Ok(outcome)) => Some(RslipOutcome {
                nodeid: outcome.nodeid.clone(),
                status: outcome.status,
                exit_code: outcome.exit_code,
                duration: outcome.duration,
                coverage: crate::rslip::LineCoverage {
                    files: BTreeMap::new(),
                },
                cache_status: outcome.cache_status,
                stdout: None,
                stderr: None,
            }),
            _ => None,
        })
        .collect();
    if hits.is_empty() {
        return;
    }

    on_progress(RslipBatchProgress::CachedStatusDump {
        body: format_cached_status_dump(&hits),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpytest_runner::TestStatus;
    use crate::rslip::{CacheStatus, LineCoverage};
    use std::time::Duration;

    fn outcome(nodeid: &str, status: TestStatus) -> RslipOutcome {
        RslipOutcome {
            nodeid: nodeid.to_string(),
            status,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            coverage: LineCoverage {
                files: BTreeMap::new(),
            },
            cache_status: CacheStatus::Hit,
            stdout: None,
            stderr: None,
        }
    }

    #[test]
    fn format_cached_status_dump_lists_each_small_batch() {
        let body = format_cached_status_dump(&[
            outcome("a::t", TestStatus::Passed),
            outcome("b::t", TestStatus::Failed),
            outcome("c::t", TestStatus::TimedOut),
        ]);
        assert!(body.contains("PASS (cached): a::t"));
        assert!(body.contains("FAIL (cached): b::t"));
        assert!(body.contains("TIMEOUT (cached): c::t"));
    }

    #[test]
    fn format_cached_status_dump_collapses_large_batches() {
        let mut outcomes = Vec::new();
        for i in 0..20 {
            outcomes.push(outcome(&format!("p::{i}"), TestStatus::Passed));
        }
        for i in 0..10 {
            outcomes.push(outcome(&format!("f::{i}"), TestStatus::Failed));
        }
        for i in 0..5 {
            outcomes.push(outcome(&format!("t::{i}"), TestStatus::TimedOut));
        }
        assert!(outcomes.len() > 32);
        let body = format_cached_status_dump(&outcomes);
        assert_eq!(
            body,
            "PASS (cached): 20 selectors\nFAIL (cached): 10 selectors\nTIMEOUT (cached): 5 selectors\n"
        );
    }

    #[test]
    fn emit_prepare_resolved_progress_skips_empty_and_dumps_hits() {
        let mut events = Vec::new();
        emit_prepare_resolved_progress(&[], &mut |ev| events.push(ev));
        assert!(events.is_empty());

        let slots = vec![
            Some(Ok(outcome("hit::1", TestStatus::Passed))),
            None,
            Some(Err(RslipError::MissingArtifact("x".into()))),
        ];
        emit_prepare_resolved_progress(&slots, &mut |ev| events.push(ev));
        assert_eq!(events.len(), 1);
        match &events[0] {
            RslipBatchProgress::CachedStatusDump { body } => {
                assert!(body.contains("PASS (cached): hit::1"));
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
