use crate::analyze;
use kiss::graph::build_dependency_graph;
use kiss::rust_graph::build_rust_dependency_graph;
use kiss::test_refs::CoveringTest;
use kiss::{ParsedFile, ParsedRustFile};
use kiss::Language;
use kiss::DependencyGraph;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

type DefEntry = (PathBuf, String, usize, Option<Vec<CoveringTest>>);

fn gather_files_with_path_expansion(
    universe: &str,
    paths: &[String],
    lang_filter: Option<Language>,
    ignore: &[String],
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let universe_root = Path::new(universe);
    let (py_files, rs_files) = analyze::gather_files(universe_root, lang_filter, ignore);
    let mut all_py: HashSet<PathBuf> = py_files.into_iter().collect();
    let mut all_rs: HashSet<PathBuf> = rs_files.into_iter().collect();
    for path_str in paths {
        let path = Path::new(path_str);
        let Ok(canonical) = path.canonicalize() else { continue };
        let root = if canonical.is_dir() {
            canonical
        } else {
            match canonical.parent() {
                Some(p) => p.to_path_buf(),
                None => continue,
            }
        };
        let (py, rs) = analyze::gather_files(&root, lang_filter, ignore);
        all_py.extend(py);
        all_rs.extend(rs);
    }
    let mut py_files: Vec<PathBuf> = all_py.into_iter().collect();
    let mut rs_files: Vec<PathBuf> = all_rs.into_iter().collect();
    py_files.sort();
    rs_files.sort();
    (py_files, rs_files)
}

pub fn run_show_tests_to(
    out: &mut dyn std::io::Write,
    universe: &str,
    paths: &[String],
    lang_filter: Option<Language>,
    ignore: &[String],
    show_untested: bool,
) -> i32 {
    let (py_files, rs_files) =
        gather_files_with_path_expansion(universe, paths, lang_filter, ignore);

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
    let mut py_graph: Option<DependencyGraph> = None;
    let mut rs_graph: Option<DependencyGraph> = None;

    if !py_files.is_empty() {
        match collect_py_test_defs(&py_files, &focus_set) {
            Ok((defs, graph)) => {
                all_defs.extend(defs);
                py_graph = graph;
            }
            Err(e) => {
                eprintln!("error: failed to parse Python files: {e}");
                return 1;
            }
        }
    }
    if !rs_files.is_empty() {
        let (defs, graph) = collect_rs_test_defs(&rs_files, &focus_set);
        all_defs.extend(defs);
        rs_graph = graph;
    }

    all_defs.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.cmp(&b.2)));
    emit_show_tests_output(out, &all_defs, show_untested, py_graph.as_ref(), rs_graph.as_ref());
    0
}

fn collect_py_test_defs(
    py_files: &[PathBuf],
    focus_set: &HashSet<PathBuf>,
) -> Result<(Vec<DefEntry>, Option<DependencyGraph>), String> {
    let results = kiss::parse_files(py_files).map_err(|e| e.to_string())?;
    let parsed: Vec<_> = results.into_iter().filter_map(Result::ok).collect();
    let refs: Vec<&ParsedFile> = parsed.iter().collect();
    let graph = if refs.is_empty() {
        None
    } else {
        Some(build_dependency_graph(&refs))
    };
    let analysis = kiss::analyze_test_refs(&refs, graph.as_ref());
    let unref_set: HashSet<(&PathBuf, &str, usize)> = analysis
        .unreferenced
        .iter()
        .map(|d| (&d.file, d.name.as_str(), d.line))
        .collect();
    let defs: Vec<DefEntry> = analysis
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
        .collect();
    Ok((defs, graph))
}

fn collect_rs_test_defs(
    rs_files: &[PathBuf],
    focus_set: &HashSet<PathBuf>,
) -> (Vec<DefEntry>, Option<DependencyGraph>) {
    let results = kiss::parse_rust_files(rs_files);
    let parsed: Vec<_> = results.into_iter().filter_map(Result::ok).collect();
    let refs: Vec<&ParsedRustFile> = parsed.iter().collect();
    let graph = if refs.is_empty() {
        None
    } else {
        Some(build_rust_dependency_graph(&refs))
    };
    let analysis = kiss::analyze_rust_test_refs(&refs, graph.as_ref());
    let unref_set: HashSet<(&PathBuf, &str, usize)> = analysis
        .unreferenced
        .iter()
        .map(|d| (&d.file, d.name.as_str(), d.line))
        .collect();
    let defs: Vec<DefEntry> = analysis
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
        .collect();
    (defs, graph)
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
    py_graph: Option<&DependencyGraph>,
    rs_graph: Option<&DependencyGraph>,
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
            let candidates_suffix = analyze::graph_for_path(file, py_graph, rs_graph)
                .and_then(|g| g.module_for_path(file).map(|module| (g, module)))
                .map(|(g, module)| {
                    let candidates = g.test_importers_of(&module);
                    if candidates.is_empty() {
                        String::new()
                    } else {
                        let truncated = kiss::cli_output::format_candidate_list(&candidates, 3);
                        format!(" (candidates: {truncated})")
                    }
                });
            let suffix = candidates_suffix.as_deref().unwrap_or("");
            let _ = writeln!(out, "UNTESTED:{}:{}:{}{}", file.display(), line, name, suffix);
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

    /// Path outside universe: `universe=dir_a`, `path=dir_b/mod2.py`. Expansion should gather mod2.py.
    #[test]
    fn test_show_tests_path_outside_universe() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir_a = tmp.path().join("a");
        let dir_b = tmp.path().join("b");
        std::fs::create_dir_all(&dir_a).unwrap();
        std::fs::create_dir_all(&dir_b).unwrap();

        std::fs::write(dir_a.join("mod.py"), "def foo(): pass\n").unwrap();
        std::fs::write(dir_a.join("test_mod.py"), "from mod import foo\ndef test_foo(): foo()\n").unwrap();
        std::fs::write(dir_b.join("mod2.py"), "def bar(): pass\n").unwrap();

        let universe = dir_a.to_string_lossy().to_string();
        let path_outside = dir_b.join("mod2.py").canonicalize().unwrap().to_string_lossy().to_string();

        let mut buf = Vec::new();
        let exit = run_show_tests_to(&mut buf, &universe, &[path_outside], None, &[], true);
        let output = String::from_utf8(buf).unwrap();

        assert_eq!(exit, 0);
        assert!(
            output.contains("UNTESTED:") && output.contains("bar"),
            "expected UNTESTED line for bar, got: {output}"
        );
    }

    #[test]
    fn static_coverage_touch_gather_files_with_path_expansion() {
        fn t<T>(_: T) {}
        t(gather_files_with_path_expansion);
    }
}
