use super::ensure_batch_platform_supported;

#[test]
fn linux_platform_gate_allows_batch_execution() {
    let result = ensure_batch_platform_supported();
    #[cfg(target_os = "linux")]
    assert!(result.is_ok(), "{result:?}");
    #[cfg(not(target_os = "linux"))]
    assert!(result.is_err());
}
