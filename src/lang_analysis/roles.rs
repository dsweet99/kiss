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
    let results =
        crate::parsing::parse_files(files).map_err(|err| RoleBuildError::PythonParse {
            path: files[0].clone(),
            message: err.to_string(),
        })?;
    collect_named_parses(files, results, |path, message| {
        RoleBuildError::PythonParse { path, message }
    })
}

fn parse_rust_batch(files: &[PathBuf]) -> Result<Vec<ParsedRustFile>, RoleBuildError> {
    if files.is_empty() {
        return Ok(Vec::new());
    }
    collect_named_parses(
        files,
        crate::rust_parsing::parse_rust_files(files),
        |path, message| RoleBuildError::RustParse { path, message },
    )
}

fn collect_named_parses<T, E: std::fmt::Display>(
    files: &[PathBuf],
    results: impl IntoIterator<Item = Result<T, E>>,
    make: impl Fn(PathBuf, String) -> RoleBuildError,
) -> Result<Vec<T>, RoleBuildError> {
    let mut parsed = Vec::new();
    for (path, result) in files.iter().zip(results) {
        match result {
            Ok(item) => parsed.push(item),
            Err(err) => return Err(make(path.clone(), err.to_string())),
        }
    }
    Ok(parsed)
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

    #[test]
    fn parse_then_classify_names_the_broken_python_file() {
        let tmp = tempfile::tempdir().unwrap();
        let valid = tmp.path().join("aaa.py");
        let broken = tmp.path().join("zzz.py");
        std::fs::write(&valid, "def ok():\n    return 1\n").unwrap();
        std::fs::write(&broken, "def bad(\n").unwrap();
        let Err(err) = parse_then_classify(&[valid, broken], &[]) else {
            panic!("expected a python parse error");
        };
        let message = err.to_string();
        assert!(message.contains("zzz.py"), "{message}");
        assert!(!message.contains("aaa.py"), "{message}");
    }

    #[test]
    fn parse_then_classify_names_the_broken_rust_file() {
        let tmp = tempfile::tempdir().unwrap();
        let valid = tmp.path().join("aaa.rs");
        let broken = tmp.path().join("zzz.rs");
        std::fs::write(&valid, "pub fn ok() {}\n").unwrap();
        std::fs::write(&broken, "fn bad(\n").unwrap();
        let Err(err) = parse_then_classify(&[], &[valid, broken]) else {
            panic!("expected a rust parse error");
        };
        let message = err.to_string();
        assert!(message.contains("zzz.rs"), "{message}");
        assert!(!message.contains("aaa.rs"), "{message}");
    }
}
