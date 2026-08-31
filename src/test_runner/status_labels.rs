use kiss::GateConfig;
use kiss::rpytest_runner::TestStatus;
use std::time::Duration;

pub(crate) fn apply_unit_test_time_limit(
    status: TestStatus,
    selector: &str,
    duration: Duration,
    gate: &GateConfig,
) -> TestStatus {
    if gate.unit_test_time_gate_disabled() {
        return status;
    }
    if status == TestStatus::TimedOut {
        return TestStatus::TimedOut;
    }
    if kiss::exceeds_limit(
        &gate.max_unit_test_seconds,
        selector,
        duration.as_secs_f64(),
    ) {
        return TestStatus::TimedOut;
    }
    status
}

pub(crate) fn print_classified_status_line(
    status: TestStatus,
    selector: &str,
    duration: std::time::Duration,
    cache_tag: Option<&str>,
    show_duration: bool,
) {
    if cache_tag == Some("cached")
        && !crate::test_runner::check_runtime_refresh::test_runner_stdout_enabled()
    {
        return;
    }
    let duration_s = crate::test_runner::duration::format_test_duration(duration);
    let line = format_status_line(
        status,
        selector,
        if show_duration {
            duration_s.as_str()
        } else {
            ""
        },
        cache_tag,
    );
    crate::test_runner::emit_test_status(&line);
}

pub(crate) fn format_status_line(
    status: TestStatus,
    selector: &str,
    duration: &str,
    cache_tag: Option<&str>,
) -> String {
    let label = match status {
        TestStatus::Passed => "PASS",
        TestStatus::Failed => "FAIL",
        TestStatus::TimedOut => "TIMEOUT",
    };
    let head = match cache_tag {
        Some(tag) => format!("{label} ({tag}): {selector}"),
        None => format!("{label}: {selector}"),
    };
    if duration.is_empty() {
        head
    } else {
        format!("{head} ({duration})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_limit_marks_timeout() {
        let gate = GateConfig {
            max_unit_test_seconds: vec![("tests/fast".into(), 2.0), ("*".into(), 0.0)],
            ..GateConfig::default()
        };
        assert_eq!(
            apply_unit_test_time_limit(
                TestStatus::Passed,
                "tests/other/a.py::t",
                Duration::from_millis(1),
                &gate
            ),
            TestStatus::TimedOut
        );
        assert_eq!(
            apply_unit_test_time_limit(
                TestStatus::Passed,
                "tests/fast/a.py::t",
                Duration::from_millis(500),
                &gate
            ),
            TestStatus::Passed
        );
    }
}
