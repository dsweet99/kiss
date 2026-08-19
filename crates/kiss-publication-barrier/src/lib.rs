use std::io;
use std::path::Path;
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};

mod publish;
mod wait;
pub use publish::{publish_atomically, publish_atomically_without_parent_sync};

#[cfg(test)]
pub(crate) use wait::{BARRIER_DIR_ENV, BARRIER_TARGET_ENV, unique_nanos};
#[cfg(all(test, debug_assertions))]
pub(crate) use wait::{
    ReleaseRecord, WaitPolicy, ensure_child_path, json_escape, json_number_field,
    json_string_field, operation_id, read_release_record, validate_release_record,
    wait_if_targeted,
};

pub fn unique_process_suffix() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}.{}", process::id(), n)
}

pub fn after_sync_before_rename(
    artifact: &str,
    temporary_path: &Path,
    final_path: &Path,
) -> io::Result<()> {
    wait::wait_if_targeted(
        artifact,
        "after_sync_before_rename",
        temporary_path,
        final_path,
        wait::WaitPolicy::default(),
    )
}

pub fn after_rename(artifact: &str, temporary_path: &Path, final_path: &Path) -> io::Result<()> {
    wait::wait_if_targeted(
        artifact,
        "after_rename",
        temporary_path,
        final_path,
        wait::WaitPolicy::default(),
    )
}

#[cfg(test)]
pub(crate) use publish::{open_publish_tmp, sync_publish_parent};

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod publish_test;
