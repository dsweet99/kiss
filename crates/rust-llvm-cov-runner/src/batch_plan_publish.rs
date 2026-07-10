use std::fs::{self, OpenOptions};
use std::io::{self, Write};

use crate::RustCoverageBatchPlan;
use crate::rust_cov_cache::rust_cov_unique_suffix;

pub fn publish_generated_nextest_config(plan: &RustCoverageBatchPlan) -> io::Result<()> {
    let parent = plan
        .generated_config
        .parent()
        .ok_or_else(|| io::Error::other("generated nextest config path has no parent"))?;
    fs::create_dir_all(parent)?;
    let file_name = plan
        .generated_config
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::other("generated nextest config path has no file name"))?;
    let tmp_path = parent.join(format!(".{file_name}.{}.tmp", rust_cov_unique_suffix()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp_path)?;
    file.write_all(plan.generated_config_toml.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(tmp_path, &plan.generated_config)
}
