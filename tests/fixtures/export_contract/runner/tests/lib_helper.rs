use export_contract_runner::run_helper;

#[test]
fn lib_helper_value_is_checked() {
    assert_eq!(run_helper(), 42);
}
