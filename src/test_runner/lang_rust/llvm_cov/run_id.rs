use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn unique_rust_coverage_batch_config_path(repo_root: &Path) -> PathBuf {
    static NEXT_RUN_ID: AtomicU64 = AtomicU64::new(0);
    let run_id = NEXT_RUN_ID.fetch_add(1, Ordering::Relaxed);
    let timestamp_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time must be after Unix epoch")
        .as_nanos();
    repo_root
        .join(".kiss")
        .join("rust_llvm_cov_cache")
        .join("runs")
        .join(format!(
            "run-{}-{timestamp_nanos}-{run_id}",
            std::process::id()
        ))
        .join("nextest.toml")
}
