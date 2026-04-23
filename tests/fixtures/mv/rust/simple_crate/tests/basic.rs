use mv_simple_crate::caller;

#[test]
fn caller_returns_incremented_value() {
    assert_eq!(caller(), 5);
}
