use std::path::Path;

pub(crate) fn python_all_plan(
    repo_root: &Path,
    python_extra: &[String],
    py_sel: Vec<String>,
) -> (Vec<String>, bool) {
    if py_sel.is_empty() {
        return (py_sel, false);
    }
    let index_present =
        crate::test_runner::python_coverage_index::python_coverage_index_file_present(repo_root);
    if !index_present {
        return (py_sel, true);
    }
    let fingerprint_started = std::time::Instant::now();
    let current = crate::test_runner::python_coverage_index::python_population_manifest_is_current_for_args_with_env_keys(
        repo_root,
        &py_sel,
        python_extra,
        crate::test_runner::python_coverage_index::PYTHON_COVERAGE_ENV_KEYS,
    );
    crate::test_runner::emit_stage_time("python_source_fingerprint", fingerprint_started.elapsed());
    (py_sel, !current)
}
