use super::FreshBatchFinishContext;
use crate::rust_llvm_cov_runner::RustTestBinaryIdentity;

pub(super) fn test_binary() -> RustTestBinaryIdentity {
    RustTestBinaryIdentity {
        id: "/tmp/bin".to_string(),
        executable: "/tmp/bin".to_string(),
        digest: "0000000000000000".to_string(),
    }
}

pub(super) fn finish_context() -> FreshBatchFinishContext {
    FreshBatchFinishContext {
        export_started: std::time::Instant::now(),
        build_target_baseline_bytes: 42,
        process_residual_count: 0,
        test_binaries: vec![test_binary()],
        repair_publication: None,
    }
}
