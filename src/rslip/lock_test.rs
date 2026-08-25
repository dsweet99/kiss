use super::LocalRslipLockGuard;

#[test]
#[allow(non_snake_case)]
fn LocalRslipLockGuard_type_is_test_referenced() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let guard = LocalRslipLockGuard {
        _file: tmp.reopen().unwrap(),
    };

    drop(guard);
}
