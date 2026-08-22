use std::path::PathBuf;

use super::analysis::{LanguageAnalysis, PythonAnalysis, RustAnalysis};
use crate::code_roles::{RoleBuildError, SourceRoleIndex, build_source_role_index};
use crate::parsing::ParsedFile;
use crate::rust_parsing::ParsedRustFile;

pub trait LanguageCodeRoles: LanguageAnalysis {
    type Parsed;
    fn classify_roles(
        &self,
        parsed: &[&Self::Parsed],
        discovered: &[PathBuf],
    ) -> Result<SourceRoleIndex, RoleBuildError>;
}

impl LanguageCodeRoles for PythonAnalysis {
    type Parsed = ParsedFile;

    fn classify_roles(
        &self,
        parsed: &[&Self::Parsed],
        discovered: &[PathBuf],
    ) -> Result<SourceRoleIndex, RoleBuildError> {
        crate::code_roles::classify_python(parsed, discovered)
    }
}

impl LanguageCodeRoles for RustAnalysis {
    type Parsed = ParsedRustFile;

    fn classify_roles(
        &self,
        parsed: &[&Self::Parsed],
        discovered: &[PathBuf],
    ) -> Result<SourceRoleIndex, RoleBuildError> {
        crate::code_roles::classify_rust(parsed, discovered)
    }
}

pub fn classify_parsed_sources(
    py_parsed: &[ParsedFile],
    rs_parsed: &[ParsedRustFile],
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
) -> Result<SourceRoleIndex, RoleBuildError> {
    build_source_role_index(py_parsed, rs_parsed, py_files, rs_files)
}

pub fn parse_then_classify(
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
) -> Result<(Vec<ParsedFile>, Vec<ParsedRustFile>, SourceRoleIndex), RoleBuildError> {
    let py_parsed = parse_python_batch(py_files)?;
    let rs_parsed = parse_rust_batch(rs_files)?;
    let roles = build_source_role_index(&py_parsed, &rs_parsed, py_files, rs_files)?;
    Ok((py_parsed, rs_parsed, roles))
}

fn parse_python_batch(files: &[PathBuf]) -> Result<Vec<ParsedFile>, RoleBuildError> {
    if files.is_empty() {
        return Ok(Vec::new());
    }
    crate::parsing::parse_files(files)
        .map_err(|err| RoleBuildError::PythonParse {
            path: files[0].clone(),
            message: err.to_string(),
        })?
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| RoleBuildError::PythonParse {
            path: files[0].clone(),
            message: err.to_string(),
        })
}

fn parse_rust_batch(files: &[PathBuf]) -> Result<Vec<ParsedRustFile>, RoleBuildError> {
    if files.is_empty() {
        return Ok(Vec::new());
    }
    crate::rust_parsing::parse_rust_files(files)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| RoleBuildError::RustParse {
            path: files[0].clone(),
            message: err.to_string(),
        })
}

#[cfg(test)]
mod roles_trait_test {
    use super::*;

    #[test]
    fn trait_impls_match_languages() {
        assert_eq!(
            PythonAnalysis.language(),
            crate::discovery::Language::Python
        );
        assert_eq!(RustAnalysis.language(), crate::discovery::Language::Rust);
        let index = classify_parsed_sources(&[], &[], &[], &[]).unwrap();
        assert_eq!(index.file_count(), 0);
        let tmp = tempfile::tempdir().unwrap();
        let py = tmp.path().join("a.py");
        std::fs::write(&py, "x = 1\n").unwrap();
        let mut parser = crate::parsing::create_parser().unwrap();
        let parsed = crate::parsing::parse_file(&mut parser, &py).unwrap();
        let py_index = PythonAnalysis
            .classify_roles(std::slice::from_ref(&&parsed), std::slice::from_ref(&py))
            .unwrap();
        assert_eq!(py_index.file_count(), 1);
        let rs = tmp.path().join("lib.rs");
        std::fs::write(&rs, "pub fn f() {}\n").unwrap();
        let parsed_rs = crate::rust_parsing::parse_rust_file(&rs).unwrap();
        let rs_index = RustAnalysis
            .classify_roles(std::slice::from_ref(&&parsed_rs), std::slice::from_ref(&rs))
            .unwrap();
        assert!(rs_index.file_count() >= 1);
    }
}
