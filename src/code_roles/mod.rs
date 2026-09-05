mod build;
mod cfg_attr;
mod cfg_parse;
mod cfg_pred;
mod cfg_sat;
mod error;
mod facts;
mod fingerprint;
mod index;
mod python;
mod python_path;
mod rust;
mod rust_cargo;
mod rust_include_parse;
mod rust_modules;
mod rust_walk;
mod rust_walk_attrs;
mod span;
pub(crate) mod sweep;
mod types;

pub use build::build_source_role_index;
pub use error::RoleBuildError;
pub use fingerprint::{
    ROLE_SCHEMA_VERSION, role_input_fingerprint, workspace_preflight_fingerprint,
};
pub use index::{
    SourceRoleIndex, contains_file, contexts_at, contexts_for_span, is_test_only_file,
    production_line_count, skip_syn,
};
pub use python::classify_python;
pub use python_path::{is_default_pytest_collect_candidate, is_python_test_module_path};
pub use rust::{classify_rust, reachable_workspace_rust_sources};
pub(crate) use rust_cargo::cargo_entry_src_paths;
pub(crate) use rust_modules::declared_mod_path;
pub use span::{SourcePosition, SourceSpan};
pub use types::{CodeContextSet, CodeRole, FileComposition};

pub use facts::{FileRoleFacts, RoleRange};

#[cfg(test)]
mod empty_comment_test {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn empty_test_file_keeps_test_base() {
        let facts = FileRoleFacts::new(CodeContextSet::test_only(), Vec::new());
        let mut files = std::collections::BTreeMap::new();
        files.insert(PathBuf::from("tests/empty.py"), facts);
        let index = SourceRoleIndex::new(files);
        assert_eq!(
            index.file_composition(std::path::Path::new("tests/empty.py")),
            FileComposition::TestOnly
        );
        assert_eq!(
            index.role_at(std::path::Path::new("tests/empty.py"), 1),
            CodeRole::TestOnly
        );
    }
}
