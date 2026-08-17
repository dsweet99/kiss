use super::*;
use crate::config::ConfigError;

#[test]
fn rejects_legacy_gate_section() {
    let err = GateConfig::try_load_from_content("[gate]\ntest_coverage_threshold = 1\n").unwrap_err();
    match err {
        ConfigError::InvalidValue { key, message } => {
            assert_eq!(key, "gate");
            assert!(
                message.starts_with("[gate] was renamed"),
                "message={message}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
}
