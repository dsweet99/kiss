use super::ensure_batch_platform_supported;

#[test]
fn linux_platform_gate_allows_batch_execution() {
    let result = ensure_batch_platform_supported();
    assert!(result.is_ok(), "{result:?}");
}
