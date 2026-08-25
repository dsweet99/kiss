use crate::rust_llvm_cov_runner::RustLlvmCovError;

pub fn ensure_batch_platform_supported() -> Result<(), RustLlvmCovError> {
    Ok(())
}

#[cfg(test)]
#[path = "batch_platform_test.rs"]
mod tests;
