use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn touch_source(path: &Path, body: &str) -> PathBuf {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
    path.to_path_buf()
}

pub(crate) fn write_python_population_for_cache_tests(repo: &Path, input_fingerprint: &str) {
    let host = crate::test_runner::python_coverage_index::python_coverage_cache_root(repo).unwrap();
    fs::create_dir_all(&host).unwrap();
    let payload = format!(
        r#"{{
            "schema_version":"rslip-python-population-v1",
            "input_fingerprint":"{input_fingerprint}",
            "entries_fingerprint":"def",
            "selectors":["t::one"]
        }}"#
    );
    fs::write(host.join("population.json"), payload).unwrap();
}
