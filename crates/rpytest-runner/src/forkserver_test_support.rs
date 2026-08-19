
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::PytestRunRequest;

pub(crate) fn test_python() -> PathBuf {
    PathBuf::from(std::env::var("PYTHON").unwrap_or_else(|_| "python".to_string()))
}

pub(crate) fn base_req(root: &Path, nodeid: &str) -> PytestRunRequest {
    PytestRunRequest::from_parts(
        nodeid.to_string(),
        root.to_path_buf(),
        test_python(),
        vec!["-q".to_string()],
        BTreeMap::new(),
        Vec::new(),
        Vec::new(),
        None,
    )
}
