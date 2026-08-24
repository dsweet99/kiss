use super::error::RoleBuildError;
use super::index::SourceRoleIndex;
use super::python::classify_python;
use super::rust::classify_rust;
use crate::parsing::ParsedFile;
use crate::rust_parsing::ParsedRustFile;
use std::path::PathBuf;

pub fn build_source_role_index(
    py_parsed: &[ParsedFile],
    rs_parsed: &[ParsedRustFile],
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
) -> Result<SourceRoleIndex, RoleBuildError> {
    let py_refs: Vec<&ParsedFile> = py_parsed.iter().collect();
    let rs_refs: Vec<&ParsedRustFile> = rs_parsed.iter().collect();
    std::thread::scope(|scope| {
        let py_handle = scope.spawn(|| classify_python(&py_refs, py_files));
        let rust_index = classify_rust(&rs_refs, rs_files)?;
        let mut index = py_handle.join().expect("python roles thread")?;
        index.merge_from(rust_index);
        Ok(index)
    })
}

#[cfg(test)]
mod build_test {
    use super::*;

    #[test]
    fn empty_inputs_build_empty_index() {
        let index = build_source_role_index(&[], &[], &[], &[]).unwrap();
        assert_eq!(index.file_count(), 0);
    }
}
