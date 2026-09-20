use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const UNREGISTERED: usize = usize::MAX;

static PYTHON_REMAINING: AtomicUsize = AtomicUsize::new(UNREGISTERED);
static RUST_REMAINING: AtomicUsize = AtomicUsize::new(UNREGISTERED);
static PYTHON_EXPECTED: AtomicBool = AtomicBool::new(false);
static RUST_EXPECTED: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
static REMAINING_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) fn emit_tests_remaining(remaining: usize) {
    crate::test_runner::emit_test_progress(&format!("kiss test: tests_remaining={remaining}"));
}

pub(crate) fn reset_tests_remaining() {
    PYTHON_REMAINING.store(UNREGISTERED, Ordering::SeqCst);
    RUST_REMAINING.store(UNREGISTERED, Ordering::SeqCst);
    PYTHON_EXPECTED.store(false, Ordering::SeqCst);
    RUST_EXPECTED.store(false, Ordering::SeqCst);
}

fn language_slot(language: kiss::Language) -> &'static AtomicUsize {
    match language {
        kiss::Language::Python => &PYTHON_REMAINING,
        kiss::Language::Rust => &RUST_REMAINING,
    }
}

fn expected_slot(language: kiss::Language) -> &'static AtomicBool {
    match language {
        kiss::Language::Python => &PYTHON_EXPECTED,
        kiss::Language::Rust => &RUST_EXPECTED,
    }
}

fn registered_count(raw: usize) -> usize {
    if raw == UNREGISTERED { 0 } else { raw }
}

fn peer_pending() -> bool {
    (PYTHON_EXPECTED.load(Ordering::SeqCst)
        && PYTHON_REMAINING.load(Ordering::SeqCst) == UNREGISTERED)
        || (RUST_EXPECTED.load(Ordering::SeqCst)
            && RUST_REMAINING.load(Ordering::SeqCst) == UNREGISTERED)
}

fn combined_remaining() -> usize {
    registered_count(PYTHON_REMAINING.load(Ordering::SeqCst))
        + registered_count(RUST_REMAINING.load(Ordering::SeqCst))
}

fn emit_combined_if_ready() {
    let remaining = combined_remaining();
    if remaining == 0 && peer_pending() {
        return;
    }
    if !PYTHON_EXPECTED.load(Ordering::SeqCst) && !RUST_EXPECTED.load(Ordering::SeqCst) {
        return;
    }
    emit_tests_remaining(remaining);
}

pub(crate) fn expect_language_remaining(language: kiss::Language) {
    expected_slot(language).store(true, Ordering::SeqCst);
}

pub(crate) fn set_language_remaining(language: kiss::Language, remaining: usize) {
    expected_slot(language).store(true, Ordering::SeqCst);
    let slot = language_slot(language);
    if slot.swap(remaining, Ordering::SeqCst) == remaining {
        return;
    }
    emit_combined_if_ready();
}

#[cfg(test)]
pub(crate) fn remaining_test_guard() -> std::sync::MutexGuard<'static, ()> {
    REMAINING_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::{
        emit_tests_remaining, expect_language_remaining, remaining_test_guard, reset_tests_remaining,
        set_language_remaining,
    };

    #[cfg(unix)]
    #[test]
    fn emit_prints_every_remaining_count() {
        let _guard = remaining_test_guard();
        reset_tests_remaining();
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
        assert_eq!(
            out.matches("kiss test: tests_remaining=2").count(),
            1,
            "{out}"
        );
        assert_eq!(
            out.matches("kiss test: tests_remaining=0").count(),
            1,
            "{out}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn language_remaining_prints_process_wide_sum() {
        let _guard = remaining_test_guard();
        reset_tests_remaining();
        let out = crate::test_runner::capture_stdout::capture_stdout(|| {
            set_language_remaining(kiss::Language::Python, 2);
            set_language_remaining(kiss::Language::Rust, 3);
            set_language_remaining(kiss::Language::Rust, 0);
        });
        assert!(
            out.contains("kiss test: tests_remaining=2"),
            "python-only register: {out}"
        );
        assert!(
            out.contains("kiss test: tests_remaining=5"),
            "combined leftover: {out}"
        );
        assert!(
            out.contains("kiss test: tests_remaining=2\n")
                || out.matches("kiss test: tests_remaining=2").count() >= 2,
            "rust zero must keep python leftover, not print 0: {out}"
        );
        assert!(
            !out.contains("kiss test: tests_remaining=0"),
            "rust finish must not wipe python leftover: {out}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn pending_peer_does_not_print_zero_remaining() {
        let _guard = remaining_test_guard();
        reset_tests_remaining();
        let out = crate::test_runner::capture_stdout::capture_stdout(|| {
            expect_language_remaining(kiss::Language::Python);
            expect_language_remaining(kiss::Language::Rust);
            set_language_remaining(kiss::Language::Python, 2);
            set_language_remaining(kiss::Language::Python, 0);
            set_language_remaining(kiss::Language::Rust, 3);
            set_language_remaining(kiss::Language::Rust, 0);
        });
        let zero_at = out.find("kiss test: tests_remaining=0");
        let rust_three_at = out.find("kiss test: tests_remaining=3");
        assert!(
            rust_three_at.is_some(),
            "rust register must print leftover: {out}"
        );
        assert!(zero_at.is_some(), "both settled must print 0: {out}");
        assert!(
            rust_three_at.unwrap() < zero_at.unwrap(),
            "must not print 0 while rust is still pending: {out}"
        );
    }
}
