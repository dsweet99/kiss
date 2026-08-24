use std::path::Path;

#[must_use]
pub fn is_pytest_nodeid_source_file(path: &Path) -> bool {
    let is_conftest = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("conftest.py"));
    !is_conftest
        && path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|name| {
                let is_py = path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("py"));
                is_py
                    && (name.starts_with("test_")
                        || (name.len() > 8 && name[..name.len() - 3].ends_with("_test")))
            })
}

#[must_use]
pub fn is_in_test_directory(path: &Path) -> bool {
    use std::ffi::OsStr;
    path.components()
        .any(|c| c.as_os_str() == OsStr::new("tests") || c.as_os_str() == OsStr::new("test"))
}
