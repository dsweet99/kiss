use std::fs::{self, OpenOptions};
use std::io::{self, Write};

use crate::RustCoverageBatchPlan;
use crate::rust_cov_cache::rust_cov_unique_suffix;

pub fn publish_generated_nextest_config(
    plan: &RustCoverageBatchPlan,
    _req: &crate::batch_plan::RustCoverageBatchRequest,
) -> io::Result<()> {
    publish_generated_config_file(
        &plan.generated_config,
        plan.generated_config_toml.as_bytes(),
    )?;
    publish_generated_config_file(
        &plan.target_runner_cargo_config,
        plan.target_runner_cargo_config_toml.as_bytes(),
    )?;
    Ok(())
}

fn publish_generated_config_file(path: &std::path::Path, contents: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("generated config path has no parent"))?;
    fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::other("generated config path has no file name"))?;
    let tmp_path = parent.join(format!(".{file_name}.{}.tmp", rust_cov_unique_suffix()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp_path)?;
    file.write_all(contents)?;
    file.sync_all()?;
    drop(file);
    fs::rename(tmp_path, path)
}
