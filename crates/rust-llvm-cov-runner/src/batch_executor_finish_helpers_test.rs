use super::FreshBatchFinishContext;
use crate::RustTestBinaryIdentity;

pub(super) fn test_binary() -> RustTestBinaryIdentity {
    RustTestBinaryIdentity {
        id: "/tmp/bin".to_string(),
        executable: "/tmp/bin".to_string(),
        digest: "0000000000000000".to_string(),
    }
}

pub(super) fn finish_context() -> FreshBatchFinishContext {
    FreshBatchFinishContext {
        test_binaries: vec![test_binary()],
        ..FreshBatchFinishContext::witness()
    }
}
