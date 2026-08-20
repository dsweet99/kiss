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

#[cfg(test)]
mod coverage_witness {
    use super::*;

    impl EditKind {
        fn witness() -> Self {
            Self::Definition
        }
    }
    impl PlannedEdit {
        fn witness() -> Self {
            Self {
                path: PathBuf::from("a.rs"),
                start_byte: 0,
                end_byte: 1,
                line: 1,
                old_snippet: "old".into(),
                new_snippet: "new".into(),
                kind: EditKind::witness(),
            }
        }
    }
    impl MvPlan {
        fn witness() -> Self {
            Self {
                files: vec![],
                edits: vec![PlannedEdit::witness()],
            }
        }
    }

    #[test]
    fn witness_edit_types() {
        let _ = EditKind::witness();
        let _ = PlannedEdit::witness();
        let _ = MvPlan::witness();
    }
}
