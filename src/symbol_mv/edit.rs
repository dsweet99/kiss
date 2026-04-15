//! Edit plan types for `kiss mv`.

use std::path::PathBuf;

#[derive(Debug, Clone, Copy)]
pub enum EditKind {
    Definition,
    Reference,
}

#[derive(Debug, Clone)]
pub struct PlannedEdit {
    pub path: PathBuf,
    pub start_byte: usize,
    pub end_byte: usize,
    pub line: usize,
    pub old_snippet: String,
    pub new_snippet: String,
    pub kind: EditKind,
}

#[derive(Debug, Clone)]
pub struct MvPlan {
    pub files: Vec<PathBuf>,
    pub edits: Vec<PlannedEdit>,
}
