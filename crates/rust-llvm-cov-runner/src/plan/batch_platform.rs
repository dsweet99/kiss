use crate::RustLlvmCovError;

pub fn ensure_batch_platform_supported() -> Result<(), RustLlvmCovError> {
    #[cfg(not(target_os = "linux"))]
    {
        return Err(RustLlvmCovError::InvalidRequest(
            "compile-once Rust coverage batch execution requires Linux process-tree support".into(),
        ));
    }
    #[cfg(not(unix))]
    {
        return Err(RustLlvmCovError::InvalidRequest(
            "compile-once Rust coverage requires a Unix domain socket output channel".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "batch_platform_test.rs"]
mod tests;
