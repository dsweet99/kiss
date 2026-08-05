use rust_llvm_cov_runner::RustLlvmCovError;

use crate::test_runner::rust_batch_interrupt::note_rust_batch_interrupted;

pub(crate) fn map_rust_llvm_cov_error(err: RustLlvmCovError) -> String {
    if matches!(err, RustLlvmCovError::Interrupted) {
        note_rust_batch_interrupted();
    }
    format_rust_llvm_cov_error(err)
}

pub(crate) fn format_rust_llvm_cov_error(err: RustLlvmCovError) -> String {
    format!("error: kiss test: rust llvm-cov failed: {err:?}")
}
