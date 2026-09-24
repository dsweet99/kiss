use std::collections::BTreeMap;

use crate::rslip::{RslipError, RslipOutcome};

use super::RslipBatchProgress;

pub fn format_cached_status_dump(outcomes: &[RslipOutcome]) -> String {
    format_cached_status_each(outcomes)
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

fn cached_status_label(status: crate::rpytest_runner::TestStatus) -> &'static str {
    match status {
        crate::rpytest_runner::TestStatus::Passed => "PASS ",
        crate::rpytest_runner::TestStatus::Failed => "FAIL ",
        crate::rpytest_runner::TestStatus::TimedOut => "TIMEOUT ",
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

    on_progress(RslipBatchProgress::CachedStatusDump { outcomes: hits });
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
        assert!(body.contains("PASS a::t"));
        assert!(body.contains("FAIL b::t"));
        assert!(body.contains("TIMEOUT c::t"));
    }

    #[test]
    fn format_cached_status_dump_lists_every_large_batch_selector() {
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
        assert!(
            body.starts_with(
                "PASS p::0\nPASS p::1\nPASS p::2\n"
            ),
            "totals must lead; body={body}"
        );
        for i in 0..10 {
            assert!(
                body.contains(&format!("FAIL f::{i}")),
                "must name FAIL f::{i}; body={body}"
            );
        }
        for i in 0..5 {
            assert!(
                body.contains(&format!("TIMEOUT t::{i}")),
                "must name TIMEOUT t::{i}; body={body}"
            );
        }
        assert!(
            body.contains("PASS p::19"),
            "must list every cached selector; body={body}"
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
            RslipBatchProgress::CachedStatusDump { outcomes } => {
                assert_eq!(outcomes.len(), 1);
                assert_eq!(outcomes[0].nodeid, "hit::1");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
