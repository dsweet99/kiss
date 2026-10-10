use std::time::Duration;

use kiss::rpytest_runner::TestStatus;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FinishedTest {
    pub(crate) status: TestStatus,
    pub(crate) duration: Duration,
    pub(crate) binary_id: String,
    pub(crate) test_name: String,
}

fn final_status(label: &str) -> Option<TestStatus> {
    match label {
        "PASS" | "LEAK" => Some(TestStatus::Passed),
        "TIMEOUT" => Some(TestStatus::TimedOut),
        "FAIL" | "LEAK-FAIL" | "ABORT" => Some(TestStatus::Failed),
        _ if label.starts_with("SIG") && label.len() > 3 => Some(TestStatus::Failed),
        _ => None,
    }
}

pub(crate) fn parse_finished_test(line: &str) -> Option<FinishedTest> {
    let line = line.trim();
    let (label, rest) = line.split_once(char::is_whitespace)?;
    let status = final_status(label)?;
    let rest = rest.trim_start().strip_prefix('[')?;
    let (duration, rest) = rest.split_once(']')?;
    let duration = parse_duration(duration)?;
    let (binary_id, test_name) = parse_test_id(rest)?;
    Some(FinishedTest {
        status,
        duration,
        binary_id,
        test_name,
    })
}

pub(crate) fn parse_skipped_test(line: &str) -> Option<(String, String)> {
    let rest = line
        .trim()
        .strip_prefix("SKIP")?
        .trim_start()
        .strip_prefix('[')?;
    let (blank, rest) = rest.split_once(']')?;
    if !blank.trim().is_empty() {
        return None;
    }
    parse_test_id(rest)
}

fn parse_test_id(rest: &str) -> Option<(String, String)> {
    let rest = skip_counter(rest.trim_start());
    let (binary_id, test_name) = rest.split_once(char::is_whitespace)?;
    let test_name = test_name.trim();
    if test_name.is_empty() || test_name.contains(char::is_whitespace) {
        return None;
    }
    Some((binary_id.to_string(), test_name.to_string()))
}

fn skip_counter(rest: &str) -> &str {
    if let Some(inner) = rest.strip_prefix('(')
        && let Some((counter, after)) = inner.split_once(')')
        && counter.trim().split('/').all(|part| {
            let part = part.trim();
            part.chars().all(|ch| ch.is_ascii_digit() || ch == '─')
        })
    {
        return after.trim_start();
    }
    rest
}

fn parse_duration(text: &str) -> Option<Duration> {
    let text = text.trim().trim_start_matches('>').trim();
    let mut total = 0.0_f64;
    let mut parts = 0;
    for part in text.split_whitespace() {
        let (number, unit) = part.split_at(part.find(|ch: char| ch.is_ascii_alphabetic())?);
        let value: f64 = number.parse().ok()?;
        total += value
            * match unit {
                "s" => 1.0,
                "m" => 60.0,
                "h" => 3600.0,
                _ => return None,
            };
        parts += 1;
    }
    (parts > 0 && total.is_finite() && total >= 0.0).then(|| Duration::from_secs_f64(total))
}

pub(crate) fn is_nextest_framing(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.chars().all(|ch| ch == '─') {
        return true;
    }
    let label = trimmed.split_whitespace().next().unwrap_or_default();
    matches!(
        label,
        "Starting" | "Nextest" | "Summary" | "SLOW" | "TERMINATING" | "SKIP" | "START"
    ) || trimmed == "error: test run failed"
        || final_status(label).is_some()
}

pub(crate) fn starts_test_run(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("Starting ") || trimmed.starts_with("Nextest run ID")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_final_status_lines_with_and_without_counter() {
        assert_eq!(
            parse_finished_test("        PASS [   0.007s] (1/6) ntx-pkg::it ok"),
            Some(FinishedTest {
                status: TestStatus::Passed,
                duration: Duration::from_millis(7),
                binary_id: "ntx-pkg::it".into(),
                test_name: "ok".into(),
            })
        );
        let fail = parse_finished_test("FAIL [   0.250s] ntx-pkg t::bad").unwrap();
        assert_eq!(fail.status, TestStatus::Failed);
        assert_eq!(fail.binary_id, "ntx-pkg");
        assert_eq!(fail.test_name, "t::bad");
        let segv = parse_finished_test("     SIGSEGV [   0.779s] (5/6) ntx-pkg t::segv").unwrap();
        assert_eq!(segv.status, TestStatus::Failed);
        let timeout =
            parse_finished_test("     TIMEOUT [   2.004s] (6/6) ntx-pkg::bin/tool t::slow")
                .unwrap();
        assert_eq!(timeout.status, TestStatus::TimedOut);
        assert_eq!(timeout.binary_id, "ntx-pkg::bin/tool");
        assert_eq!(timeout.duration, Duration::from_millis(2004));
        assert_eq!(
            parse_finished_test("LEAK [   0.100s] p t::leaky").map(|t| t.status),
            Some(TestStatus::Passed)
        );
        assert_eq!(
            parse_finished_test("LEAK-FAIL [   0.100s] p t::leaky").map(|t| t.status),
            Some(TestStatus::Failed)
        );
    }

    #[test]
    fn ignores_lines_that_do_not_finish_a_test() {
        for line in [
            "        SLOW [>  1.000s] (───) ntx-pkg t::slow",
            " TERMINATING [>  2.000s] (───) ntx-pkg t::slow",
            "        SKIP [         ] (───) ntx-pkg t::ign",
            "     Summary [   2.005s] 6 tests run: 3 passed, 2 failed, 1 timed out",
            "    Starting 6 tests across 3 binaries (1 test skipped)",
            "thread 't::bad' panicked at src/lib.rs:1:1:",
            "PASS [   0.007s] (1/6) ntx-pkg",
            "SIG [   0.007s] ntx-pkg t::x",
        ] {
            assert_eq!(parse_finished_test(line), None, "{line}");
        }
    }

    #[test]
    fn parses_skip_lines() {
        assert_eq!(
            parse_skipped_test("        SKIP [         ] (───) ntx-pkg t::ign"),
            Some(("ntx-pkg".into(), "t::ign".into()))
        );
        assert_eq!(parse_skipped_test("SKIP [   0.1s] ntx-pkg t::ign"), None);
        assert_eq!(parse_skipped_test("PASS [   0.1s] ntx-pkg t::ok"), None);
    }

    #[test]
    fn parses_long_durations() {
        assert_eq!(
            parse_duration("1m 2.500s"),
            Some(Duration::from_millis(62_500))
        );
        assert_eq!(parse_duration("1h 2m 3s"), Some(Duration::from_secs(3723)));
        assert_eq!(parse_duration(">  1.000s"), Some(Duration::from_secs(1)));
        assert_eq!(parse_duration("         "), None);
        assert_eq!(parse_duration("1.0x"), None);
    }

    #[test]
    fn framing_lines_are_recognized_and_test_output_is_not() {
        assert!(is_nextest_framing("────────────"));
        assert!(is_nextest_framing(
            " Nextest run ID 35b8 with nextest profile: kiss"
        ));
        assert!(is_nextest_framing("    Starting 6 tests across 3 binaries"));
        assert!(is_nextest_framing(
            "        PASS [   0.007s] (1/6) ntx-pkg::it ok"
        ));
        assert!(is_nextest_framing("error: test run failed"));
        assert!(!is_nextest_framing(
            "thread 't::bad' panicked at src/lib.rs:1:1:"
        ));
        assert!(!is_nextest_framing(
            "──── STDERR:             ntx-pkg t::bad"
        ));
        assert!(starts_test_run("    Starting 6 tests across 3 binaries"));
        assert!(!starts_test_run("   Compiling ntx-pkg v0.1.0"));
    }
}
