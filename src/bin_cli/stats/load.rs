use kiss::code_roles::{RoleBuildError, SourceRoleIndex, is_test_only_file};
use kiss::parsing::ParsedFile;
use kiss::rust_parsing::ParsedRustFile;
use std::path::PathBuf;

pub(super) fn load_production_python(
    py_files: &[PathBuf],
) -> Result<(Vec<ParsedFile>, SourceRoleIndex), RoleBuildError> {
    if py_files.is_empty() {
        return Ok((Vec::new(), SourceRoleIndex::empty()));
    }
    let (py, _rs, roles) = crate::analyze_parse::parse_classified(py_files, &[])?;
    let parsed = py
        .into_iter()
        .filter(|p| !is_test_only_file(&roles, &p.path))
        .collect();
    Ok((parsed, roles))
}

pub(super) fn load_production_rust(
    rs_files: &[PathBuf],
) -> Result<(Vec<ParsedRustFile>, SourceRoleIndex), RoleBuildError> {
    if rs_files.is_empty() {
        return Ok((Vec::new(), SourceRoleIndex::empty()));
    }
    let (_py, rs, roles) = crate::analyze_parse::parse_classified(&[], rs_files)?;
    let parsed = rs
        .into_iter()
        .filter(|p| !is_test_only_file(&roles, &p.path))
        .collect();
    Ok((parsed, roles))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn load_production_python_drops_test_only_files() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("app.py");
        let test = tmp.path().join("test_app.py");
        fs::write(&app, "def f():\n    return 1\n").unwrap();
        fs::write(&test, "def test_f():\n    assert True\n").unwrap();
        let (parsed, _) = load_production_python(&[app.clone(), test]).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].path, app);
    }

    #[test]
    fn load_production_python_fails_on_syntax_error() {
        let tmp = tempfile::tempdir().unwrap();
        let bad = tmp.path().join("bad.py");
        fs::write(&bad, "def (\n").unwrap();
        let Err(err) = load_production_python(&[bad]) else {
            panic!("expected parse error");
        };
        assert!(err.to_string().contains("parse"));
    }

    #[test]
    fn load_production_rust_keeps_non_empty_lib() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = tmp.path().join("lib.rs");
        fs::write(&lib, "pub fn f() {}\n").unwrap();
        let (parsed, _) = load_production_rust(std::slice::from_ref(&lib)).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].path, lib);
    }
}
