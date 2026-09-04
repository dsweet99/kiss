use std::sync::Mutex;
use std::time::{Duration, Instant};

const MIN_INTERVAL: Duration = Duration::from_secs(3);
static LAST_EMIT: Mutex<Option<Instant>> = Mutex::new(None);

pub(crate) fn should_emit_tests_remaining(
    remaining: usize,
    now: Instant,
    last: Option<Instant>,
) -> bool {
    remaining == 0 || last.is_none_or(|prev| now.saturating_duration_since(prev) >= MIN_INTERVAL)
}

pub(crate) fn emit_tests_remaining(remaining: usize) {
    let now = Instant::now();
    let mut last = LAST_EMIT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !should_emit_tests_remaining(remaining, now, *last) {
        return;
    }
    crate::test_runner::emit_test_progress(&format!("kiss test: tests_remaining={remaining}"));
    *last = if remaining == 0 { None } else { Some(now) };
}

#[cfg(test)]
mod tests {
    use super::{LAST_EMIT, MIN_INTERVAL, emit_tests_remaining, should_emit_tests_remaining};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    static EMIT_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn reset_last() {
        *LAST_EMIT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    #[test]
    fn first_or_zero_emits_and_sub_interval_does_not() {
        let t0 = Instant::now();
        let under = t0 + MIN_INTERVAL - Duration::from_millis(1);
        let at = t0 + MIN_INTERVAL;
        assert!(should_emit_tests_remaining(5, t0, None));
        assert!(!should_emit_tests_remaining(4, under, Some(t0)));
        assert!(should_emit_tests_remaining(4, at, Some(t0)));
        assert!(should_emit_tests_remaining(0, under, Some(t0)));
    }

    #[cfg(unix)]
    #[test]
    fn emit_throttles_nonzero_and_always_prints_zero() {
        let _guard = EMIT_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset_last();
        let out = crate::test_runner::capture_stdout::capture_stdout(|| {
            emit_tests_remaining(3);
            emit_tests_remaining(2);
            emit_tests_remaining(0);
        });
        assert_eq!(
            out.matches("kiss test: tests_remaining=3").count(),
            1,
            "{out}"
        );
        assert!(
            !out.contains("kiss test: tests_remaining=2"),
            "sub-interval remaining must stay quiet: {out}"
        );
        assert_eq!(
            out.matches("kiss test: tests_remaining=0").count(),
            1,
            "{out}"
        );
    }
}
