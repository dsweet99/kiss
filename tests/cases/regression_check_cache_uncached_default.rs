//! Regression test for the `kiss check` cache-bypass bug.
//!
//! `kiss check` reads and writes its full-check cache
//! (`~/.cache/kiss/check_full_*.bin`) only when `--all` (i.e.
//! `opts.bypass_gate`) is set. Without `--all`, both the read site
//! (`src/analyze/entry.rs::try_cache_hit`) and the write site
//! (`src/analyze/cache.rs::maybe_store_full_cache`) early-return, so the
//! default inner-loop invocation of `kiss check` pays the full per-run
//! analysis cost forever and never primes the cache for a later `--all`
//! invocation either.
//!
//! Symptom: in a real ~3,900-file repo, `kiss check` takes ~1.7 s wall on
//! every run; `kiss check --all` warms in ~5 s and subsequent `--all` runs
//! drop to ~0.1 s. The cache works — it just isn't engaged for the
//! command users actually type.
//!
//! This test pins the contract: after a successful `kiss check` run from
//! a clean cache, at least one `check_full_*.bin` artifact must exist in
//! the cache directory. It fails today and will pass once the cache write
//! path is made independent of `--all` (or, equivalently, both sites are
//! widened to operate in the gated default flow too).

use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn write_corpus(dir: &std::path::Path) {
    // One trivial source file plus a co-located test reference, so the
    // default `test_coverage` gate can pass and `kiss check` exits 0
    // without `--all`. The gate firing is unrelated to the cache bug —
    // we just don't want it masking what we're trying to measure.
    fs::write(
        dir.join("lib.py"),
        "def add(a, b):\n    return a + b\n",
    )
    .unwrap();
    fs::write(
        dir.join("test_lib.py"),
        "from lib import add\n\ndef test_add():\n    assert add(1, 2) == 3\n",
    )
    .unwrap();
    // Permissive config so no structural violations and the gate is
    // satisfied by the static test reference above.
    fs::write(
        dir.join(".kissconfig"),
        "[gate]\n\
         test_coverage_threshold = 0\n\
         duplication_enabled = false\n\
         orphan_module_enabled = false\n\
         \n\
         [thresholds]\n\
         statements_per_function = 100\n\
         lines_per_file = 1000\n\
         statements_per_file = 1000\n\
         functions_per_file = 100\n\
         imported_names_per_file = 100\n\
         arguments_per_function = 100\n\
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

/// `kiss check` (no `--all`) must populate the full-check cache so that
/// repeated invocations on an unchanged tree can be served cheaply. Today
/// the cache is gated on `--all` and never written by the default flow,
/// so this assertion fails until the gate is removed or widened.
#[test]
fn kiss_check_default_writes_full_check_cache() {
    let corpus = TempDir::new().unwrap();
    write_corpus(corpus.path());

    let home = TempDir::new().unwrap();
    let cache_dir = home.path().join(".cache").join("kiss");

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
}
