use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::rust_llvm_cov_runner::rust_cov_cache::repo_relative_coverage_file;
use crate::rust_llvm_cov_runner::{RustLineCoverage, RustLlvmCovError};

pub(super) fn normalize_coverage_map(
    source_root: &Path,
    coverage: &RustLineCoverage,
) -> Result<BTreeMap<String, BTreeSet<u32>>, RustLlvmCovError> {
    let mut files = BTreeMap::new();
    for (file, lines) in &coverage.files {
        let rel = repo_relative_coverage_file(source_root, file).ok_or_else(|| {
            RustLlvmCovError::InvalidRequest(format!(
                "aggregate coverage path is outside repository Rust sources: {file}"
            ))
        })?;
        if lines.iter().any(|line| *line == 0) {
            return Err(RustLlvmCovError::InvalidRequest(format!(
                "aggregate coverage path `{rel}` contains non-positive line"
            )));
        }
        files.entry(rel).or_insert_with(BTreeSet::new).extend(lines);
    }
    Ok(files)
}

pub(super) fn is_sorted_unique_nonempty(values: &[String]) -> bool {
    !values.is_empty() && values.windows(2).all(|window| window[0] < window[1])
}
