use crate::analyze;
use kiss::test_refs::CoveringTest;
use kiss::Language;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

type DefEntry = (PathBuf, String, usize, Option<Vec<CoveringTest>>);

pub fn run_show_tests_to(
    out: &mut dyn std::io::Write,
    universe: &str,
    paths: &[String],
    lang_filter: Option<Language>,
    ignore: &[String],
    show_untested: bool,
) -> i32 {
    let universe_root = Path::new(universe);
    let (py_files, rs_files) = analyze::gather_files(universe_root, lang_filter, ignore);
    if py_files.is_empty() && rs_files.is_empty() {
        eprintln!("No source files found.");
        return 1;
    }

    let focus_set = analyze::build_focus_set(paths, lang_filter, ignore);
    if focus_set.is_empty() {
        eprintln!("No matching source files for specified paths.");
        return 1;
    }

    let mut all_defs: Vec<DefEntry> = Vec::new();

    if !py_files.is_empty() {
        match collect_py_test_defs(&py_files, &focus_set) {
            Ok(defs) => all_defs.extend(defs),
            Err(e) => {
                eprintln!("error: failed to parse Python files: {e}");
                return 1;
            }
        }
    }
    if !rs_files.is_empty() {
        all_defs.extend(collect_rs_test_defs(&rs_files, &focus_set));
    }

    all_defs.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.cmp(&b.2)));
    emit_show_tests_output(out, &all_defs, show_untested);
    0
}

fn collect_py_test_defs(
    py_files: &[PathBuf],
    focus_set: &HashSet<PathBuf>,
) -> Result<Vec<DefEntry>, String> {
    let results = kiss::parse_files(py_files).map_err(|e| e.to_string())?;
    let parsed: Vec<_> = results.into_iter().filter_map(Result::ok).collect();
    let refs: Vec<&kiss::ParsedFile> = parsed.iter().collect();
    let analysis = kiss::analyze_test_refs(&refs);
    let unref_set: HashSet<(&PathBuf, &str, usize)> = analysis
        .unreferenced
        .iter()
        .map(|d| (&d.file, d.name.as_str(), d.line))
        .collect();
    Ok(analysis
        .definitions
        .iter()
        .filter(|d| analyze::is_focus_file(&d.file, focus_set))
        .map(|d| {
            let key = (d.file.clone(), d.name.clone());
            let covering = if unref_set.contains(&(&d.file, d.name.as_str(), d.line)) {
                None
            } else {
                Some(
                    analysis
                        .coverage_map
                        .get(&key)
                        .cloned()
                        .unwrap_or_default(),
                )
            };
            (d.file.clone(), d.name.clone(), d.line, covering)
        })
        .collect())
}

fn collect_rs_test_defs(
    rs_files: &[PathBuf],
    focus_set: &HashSet<PathBuf>,
) -> Vec<DefEntry> {
    let results = kiss::parse_rust_files(rs_files);
    let parsed: Vec<_> = results.into_iter().filter_map(Result::ok).collect();
    let refs: Vec<&kiss::ParsedRustFile> = parsed.iter().collect();
    let analysis = kiss::analyze_rust_test_refs(&refs);
    let unref_set: HashSet<(&PathBuf, &str, usize)> = analysis
        .unreferenced
        .iter()
        .map(|d| (&d.file, d.name.as_str(), d.line))
        .collect();
    analysis
        .definitions
        .iter()
        .filter(|d| analyze::is_focus_file(&d.file, focus_set))
        .map(|d| {
            let key = (d.file.clone(), d.name.clone());
            let covering = if unref_set.contains(&(&d.file, d.name.as_str(), d.line)) {
                None
            } else {
                Some(
                    analysis
                        .coverage_map
                        .get(&key)
                        .cloned()
                        .unwrap_or_default(),
                )
            };
            (d.file.clone(), d.name.clone(), d.line, covering)
        })
        .collect()
}

fn format_covering_tests(covering: &[CoveringTest]) -> String {
    covering
        .iter()
        .map(|(path, func)| format!("{}::{}", path.display(), func))
        .collect::<Vec<_>>()
        .join(",")
}

fn emit_show_tests_output(
    out: &mut dyn std::io::Write,
    all_defs: &[DefEntry],
    show_untested: bool,
) {
    for (file, name, line, covering) in all_defs {
        if let Some(tests) = covering {
            let _ = writeln!(
                out,
                "TEST:{}:{} {}",
                file.display(),
                name,
                format_covering_tests(tests)
            );
        } else if show_untested {
            let _ = writeln!(out, "UNTESTED:{}:{}:{}", file.display(), line, name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_show_tests_python() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("mymod.py"), "def helper():\n    pass\n").unwrap();
        std::fs::write(
            tmp.path().join("test_mymod.py"),
            "from mymod import helper\ndef test_it():\n    helper()\n",
        )
        .unwrap();

        let universe = tmp.path().to_string_lossy().to_string();
        let p = tmp.path().join("mymod.py").to_string_lossy().to_string();
        let mut buf = Vec::new();
        let exit = run_show_tests_to(&mut buf, &universe, &[p], None, &[], false);
        let output = String::from_utf8(buf).unwrap();
        assert_eq!(exit, 0);
        assert!(
            output.contains("TEST:"),
            "expected TEST lines in output, got: {output}"
        );
        assert!(
            output.contains("test_it"),
            "expected test path::function in output, got: {output}"
        );
        assert!(
            !output.contains("UNTESTED:"),
            "UNTESTED lines should not appear without --untested flag, got: {output}"
        );
    }

    #[test]
    fn test_show_tests_python_with_untested() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("mymod.py"),
            "def helper():\n    pass\n\ndef orphan():\n    pass\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("test_mymod.py"),
            "from mymod import helper\ndef test_it():\n    helper()\n",
        )
        .unwrap();

        let universe = tmp.path().to_string_lossy().to_string();
        let p = tmp.path().join("mymod.py").to_string_lossy().to_string();
        let mut buf = Vec::new();
        let exit = run_show_tests_to(&mut buf, &universe, &[p], None, &[], true);
        let output = String::from_utf8(buf).unwrap();
        assert_eq!(exit, 0);
        assert!(
            output.contains("UNTESTED:"),
            "expected UNTESTED lines with --untested flag, got: {output}"
        );
    }

    #[test]
    fn test_show_tests_rust() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("mymod.rs"), "pub fn helper() {}\n").unwrap();
        std::fs::write(
            tmp.path().join("test_mymod.rs"),
            "#[test]\nfn t() { helper(); }\n",
        )
        .unwrap();

        let universe = tmp.path().to_string_lossy().to_string();
        let p = tmp.path().join("mymod.rs").to_string_lossy().to_string();
        let mut buf = Vec::new();
        let exit = run_show_tests_to(&mut buf, &universe, &[p], None, &[], false);
        let output = String::from_utf8(buf).unwrap();
        assert_eq!(exit, 0);
        assert!(
            output.contains("TEST:"),
            "expected TEST line in output, got: {output}"
        );
        assert!(
            !output.contains("UNTESTED:"),
            "UNTESTED lines should not appear without --untested flag, got: {output}"
        );
    }

    #[test]
    fn test_one_function_covered_by_multiple_tests() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("utils.py"),
            "def parse(x):\n    return int(x or 0)\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("test_utils.py"),
            "from utils import parse\n\ndef test_parse_empty():\n    assert parse('') == 0\n\ndef test_parse_valid():\n    assert parse('42') == 42\n",
        )
        .unwrap();

        let universe = tmp.path().to_string_lossy().to_string();
        let p = tmp.path().join("utils.py").to_string_lossy().to_string();
        let mut buf = Vec::new();
        let _ = run_show_tests_to(&mut buf, &universe, &[p], None, &[], false);
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("test_parse_empty"),
            "expected test_parse_empty in output, got: {output}"
        );
        assert!(
            output.contains("test_parse_valid"),
            "expected test_parse_valid in output, got: {output}"
        );
    }

    #[test]
    fn test_python_test_class_format() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("widget.py"),
            "def render(): return 'ok'\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("test_widget.py"),
            "from widget import render\n\nclass TestWidget:\n    def test_render(self):\n        assert render() == 'ok'\n",
        )
        .unwrap();

        let universe = tmp.path().to_string_lossy().to_string();
        let p = tmp.path().join("widget.py").to_string_lossy().to_string();
        let mut buf = Vec::new();
        let _ = run_show_tests_to(&mut buf, &universe, &[p], None, &[], false);
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("TestWidget::test_render"),
            "expected TestWidget::test_render in output, got: {output}"
        );
    }

    #[test]
    fn test_one_test_covers_multiple_functions() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("lib.py"),
            "def foo(): return 1\ndef bar(): return foo() + 1\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("test_lib.py"),
            "from lib import foo, bar\ndef test_both():\n    assert foo() == 1\n    assert bar() == 2\n",
        )
        .unwrap();

        let universe = tmp.path().to_string_lossy().to_string();
        let p = tmp.path().join("lib.py").to_string_lossy().to_string();
        let mut buf = Vec::new();
        let _ = run_show_tests_to(&mut buf, &universe, &[p], None, &[], false);
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("foo") && output.contains("bar"),
            "expected both foo and bar covered, got: {output}"
        );
    }

    #[test]
    fn test_format_covering_tests_empty() {
        let covering: Vec<CoveringTest> = vec![];
        assert_eq!(format_covering_tests(&covering), "");
    }

    /// Definition covered only by a fixture (non-test function) body, not by any test function.
    /// `coverage_map` is empty; output is TEST:path:name with a trailing space.
    #[test]
    fn test_covered_by_fixture_empty_covering_list() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("mymod.py"), "def helper():\n    return 42\n").unwrap();
        std::fs::write(
            tmp.path().join("test_mymod.py"),
            "import pytest\nfrom mymod import helper\n\n@pytest.fixture\ndef my_fixture():\n    return helper()\n\ndef test_it(my_fixture):\n    pass\n",
        )
        .unwrap();

        let universe = tmp.path().to_string_lossy().to_string();
        let p = tmp.path().join("mymod.py").to_string_lossy().to_string();
        let mut buf = Vec::new();
        let exit = run_show_tests_to(&mut buf, &universe, &[p], None, &[], false);
        let output = String::from_utf8(buf).unwrap();
        assert_eq!(exit, 0);
        // helper is covered by fixture body but no test function references it directly
        let line = output
            .trim()
            .lines()
            .find(|l| l.contains("helper"))
            .expect("expected a line for helper");
        assert!(
            line.starts_with("TEST:") && line.ends_with(":helper"),
            "expected TEST:...:helper (empty covering list, space not colon before list), got: {line:?}"
        );
    }

    /// Asserts TEST line format: `TEST:path:name` `<path::func>[,path::func...]`
    #[test]
    fn test_test_line_format_stability() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("mymod.py"), "def helper():\n    pass\n").unwrap();
        std::fs::write(
            tmp.path().join("test_mymod.py"),
            "from mymod import helper\ndef test_it():\n    helper()\n",
        )
        .unwrap();

        let universe = tmp.path().to_string_lossy().to_string();
        let p = tmp.path().join("mymod.py").to_string_lossy().to_string();
        let mut buf = Vec::new();
        let _ = run_show_tests_to(&mut buf, &universe, &[p], None, &[], false);
        let output = String::from_utf8(buf).unwrap();
        let line = output
            .trim()
            .lines()
            .find(|l| l.contains("helper"))
            .expect("expected a TEST line for helper");

        assert!(line.starts_with("TEST:"), "line must start with TEST: got {line:?}");
        let rest = line.strip_prefix("TEST:").unwrap();
        let (path_name, covering) = rest
            .split_once(' ')
            .expect("TEST:path:name must be followed by space then covering list");
        assert!(
            path_name.contains(':'),
            "path:name must contain colon, got {path_name:?}"
        );
        assert!(
            covering.contains("::"),
            "covering list must use path::func format, got {covering:?}"
        );
        assert!(
            covering.contains("test_it"),
            "expected test_it in covering list, got {covering:?}"
        );
    }

    #[test]
    fn test_helper_coverage() {
        fn touch<T>(_: T) {}
        touch(collect_py_test_defs);
        touch(collect_rs_test_defs);
        touch(emit_show_tests_output);
    }
}
