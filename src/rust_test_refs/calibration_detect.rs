use super::is_rust_test_file;
use crate::rust_parsing::ParsedRustFile;
use std::path::Path;

/// Malvin-like repos: many `src/**` test modules + subprocess integration tests.
pub(crate) fn has_colocated_src_integration_tests(parsed_files: &[&ParsedRustFile]) -> bool {
    if !has_rust_integration_test_runner(parsed_files) {
        return false;
    }
    let n = parsed_files
        .iter()
        .filter(|p| {
            is_rust_test_file(&p.path)
                && p.path
                    .components()
                    .any(|c| matches!(c, std::path::Component::Normal(s) if s == "src"))
        })
        .count();
    n >= 8
}

pub(crate) fn has_rust_integration_test_runner(parsed_files: &[&ParsedRustFile]) -> bool {
    parsed_files
        .iter()
        .any(|parsed| is_subprocess_integration_test_file(parsed))
}

/// `tests/**` files that spawn the built binary: static refs over-credit vs llvm-cov.
pub(crate) fn is_subprocess_integration_test_file(parsed: &ParsedRustFile) -> bool {
    path_is_under_tests(&parsed.path)
        && (parsed.source.contains("current_exe") || parsed.source.contains("Command::new"))
}

pub(crate) fn has_non_subprocess_integration_tests(parsed_files: &[&ParsedRustFile]) -> bool {
    parsed_files.iter().any(|parsed| {
        is_rust_test_file(&parsed.path)
            && path_is_under_tests(&parsed.path)
            && !is_subprocess_integration_test_file(parsed)
    })
}

pub(crate) fn path_is_under_tests(path: &Path) -> bool {
    path.components()
        .any(|c| matches!(c, std::path::Component::Normal(s) if s == "tests"))
}
