pub(crate) fn emit_tests_remaining(remaining: usize) {
    crate::test_runner::emit_test_progress(&format!("kiss test: tests_remaining={remaining}"));
}

#[cfg(test)]
mod tests {
    use super::emit_tests_remaining;
    use std::sync::Mutex;

    static EMIT_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[cfg(unix)]
    #[test]
    fn emit_prints_every_remaining_count() {
        let _guard = EMIT_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
}
