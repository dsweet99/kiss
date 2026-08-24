use std::time::Duration;

pub(crate) fn format_test_duration(duration: Duration) -> String {
    let secs = duration.as_secs_f64();
    format!("{secs:.2}s")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_test_duration_rounds_to_two_decimals() {
        assert_eq!(format_test_duration(Duration::from_secs(0)), "0.00s");
        assert_eq!(format_test_duration(Duration::from_millis(120)), "0.12s");
        assert_eq!(format_test_duration(Duration::from_millis(12345)), "12.35s");
        assert_eq!(
            format_test_duration(Duration::from_secs_f64(12.344)),
            "12.34s"
        );
    }
}
