use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::RustLlvmCovError;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RustLineCoverage {
    pub files: BTreeMap<String, BTreeSet<u32>>,
}

#[cfg(test)]
impl RustLineCoverage {
    pub(crate) fn witness() -> Self {
        Self {
            files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1, 2]))]),
        }
    }
}

pub(crate) fn parse_llvm_cov_json_file(
    path: &Path,
    source_root: &Path,
) -> Result<RustLineCoverage, RustLlvmCovError> {
    if !path.exists() {
        return Err(RustLlvmCovError::MissingArtifact(path.to_path_buf()));
    }
    let bytes = fs::read(path)?;
    parse_llvm_cov_json(&bytes, source_root)
}

pub(crate) fn parse_llvm_cov_json(
    bytes: &[u8],
    source_root: &Path,
) -> Result<RustLineCoverage, RustLlvmCovError> {
    let report: LlvmCovReport = serde_json::from_slice(bytes)?;
    let root = source_root
        .canonicalize()
        .unwrap_or_else(|_| source_root.to_path_buf());
    let mut files = BTreeMap::new();
    for data in report.data {
        for file in data.files {
            let path = PathBuf::from(&file.filename);
            let canonical = path.canonicalize().unwrap_or(path);
            if !canonical.starts_with(&root) || should_ignore_report_path(&canonical) {
                continue;
            }
            let lines: BTreeSet<u32> = file
                .segments
                .into_iter()
                .filter_map(covered_line_from_segment)
                .collect();
            if !lines.is_empty() {
                files.insert(canonical.to_string_lossy().to_string(), lines);
            }
        }
    }
    Ok(RustLineCoverage { files })
}

fn should_ignore_report_path(path: &Path) -> bool {
    path.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        matches!(name.as_ref(), "target" | ".git" | ".kiss")
    })
}

#[derive(Deserialize)]
pub(crate) struct LlvmCovReport {
    pub(crate) data: Vec<LlvmCovData>,
}

#[cfg(test)]
impl LlvmCovReport {
    pub(crate) fn witness(filename: String) -> Self {
        Self {
            data: vec![LlvmCovData::witness(filename)],
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct LlvmCovData {
    pub(crate) files: Vec<LlvmCovFile>,
}

#[cfg(test)]
impl LlvmCovData {
    pub(crate) fn witness(filename: String) -> Self {
        Self {
            files: vec![LlvmCovFile::witness(filename)],
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct LlvmCovFile {
    pub(crate) filename: String,
    #[serde(default)]
    pub(crate) segments: Vec<Vec<serde_json::Value>>,
}

#[cfg(test)]
impl LlvmCovFile {
    pub(crate) fn witness(filename: String) -> Self {
        Self {
            filename,
            segments: vec![vec![
                serde_json::Value::from(1),
                serde_json::Value::from(1),
                serde_json::Value::from(1),
            ]],
        }
    }
}

pub(crate) fn covered_line_from_segment(segment: Vec<serde_json::Value>) -> Option<u32> {
    let line = segment.first()?.as_u64()?;
    let count = segment.get(2)?.as_u64()?;
    if line == 0 || count == 0 {
        return None;
    }
    u32::try_from(line).ok()
}
