use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::stamp::GitDepStamp;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum SourceRegion {
    WorkspaceAll,
    FileAll { path: String },
    FileLines { path: String, lines: BTreeSet<u32> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ReverseRecord {
    pub path: String,
    pub selectors: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OperandClass {
    Directory,
    SourceFile,
    SourceSymbol,
    TestFile,
    TestSymbol,
    PythonNodeid,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedTarget {
    pub regions: Vec<SourceRegion>,
    pub direct_selectors: Vec<String>,
    pub historical_paths: Vec<String>,
    pub git_stamp: Option<GitDepStamp>,
    pub operand_classes: Vec<OperandClass>,
}

impl ResolvedTarget {
    pub(crate) fn workspace() -> Self {
        Self {
            regions: vec![SourceRegion::WorkspaceAll],
            direct_selectors: Vec::new(),
            historical_paths: Vec::new(),
            git_stamp: None,
            operand_classes: Vec::new(),
        }
    }
}
