use crate::common::seed_python_runtime_coverage;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn write_corpus(dir: &std::path::Path) {
    fs::write(dir.join("lib.py"), "def add(a, b):\n    return a + b\n").unwrap();
    fs::write(
        dir.join("test_lib.py"),
        "from lib import add\n\ndef test_add():\n    assert add(1, 2) == 3\n",
    )
    .unwrap();
    seed_python_runtime_coverage(
        dir,
        &[("test_lib.py::test_add", vec![("lib.py", vec![1, 2])])],
    );

    fs::write(
        dir.join(".kissconfig"),
        "[global]\n\
         duplication_enabled = false\n\
         \n\
[test]\n\
         test_coverage_threshold = 0\n\
         [python]\n\
         [rust]\n\
         \n\
         [thresholds]\n\
         statements_per_function = 100\n\
         lines_per_file = 1000\n\
         statements_per_file = 1000\n\
         functions_per_file = 100\n\
         imported_names_per_file = 100\n\
         arguments_positional = 100\n\
         arguments_keyword_only = 100\n\
         max_indentation_depth = 100\n\
         interface_types_per_file = 100\n\
         concrete_types_per_file = 100\n\
         nested_function_depth = 100\n\
         returns_per_function = 100\n\
         return_values_per_function = 100\n\
         branches_per_function = 100\n\
         local_variables_per_function = 100\n\
         statements_per_try_block = 100\n\
         boolean_parameters = 100\n\
         annotations_per_function = 100\n\
         calls_per_function = 100\n\
         methods_per_class = 100\n\
         cycle_size = 100\n\
         indirect_dependencies = 100\n\
         dependency_depth = 100\n",
    )
    .unwrap();
}

fn is_check_full_file(entry: &fs::DirEntry) -> bool {
    let path = entry.path();
    let stem_starts = path
        .file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|stem| stem.starts_with("check_full_"));
    let ext_is_bin = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("bin"));
    stem_starts && ext_is_bin
}

fn count_check_full_files(cache_dir: &std::path::Path) -> usize {
    let Ok(it) = fs::read_dir(cache_dir) else {
        return 0;
    };
    it.filter_map(Result::ok).filter(is_check_full_file).count()
}

#[test]
fn kiss_check_default_writes_full_check_cache() {
    let corpus = TempDir::new().unwrap();
    write_corpus(corpus.path());

    let home = TempDir::new().unwrap();
    let cache_dir = corpus.path().join(".kiss");

    assert_eq!(
        count_check_full_files(&cache_dir),
        0,
        "precondition: cache dir should be empty before the run"
    );

    let out = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .arg("check")
        .arg(corpus.path())
        .env("HOME", home.path())
        .output()
        .expect("kiss check should run");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "kiss check failed (exit {:?})\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status.code(),
    );

    let written = count_check_full_files(&cache_dir);
    assert!(
        written >= 1,
        "kiss check (no --all) did not write any check_full_*.bin to {} \
         — the full-check cache is silently bypassed for the default \
         inner-loop invocation. Subsequent `kiss check` calls will pay \
         the full analysis cost every time.\n\
         Cache dir contents: {:?}\n\
         kiss stdout:\n{stdout}",
        cache_dir.display(),
        fs::read_dir(&cache_dir)
            .map(|it| it
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect::<Vec<_>>())
            .unwrap_or_default(),
    );

    let home_cache = home.path().join(".cache").join("kiss");
    assert_eq!(
        count_check_full_files(&home_cache),
        0,
        "check_full_*.bin must not land under HOME/.cache/kiss"
    );
}
