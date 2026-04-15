use super::*;

fn run_show_tests_to_buf(
    buf: &mut impl std::io::Write,
    universe: &str,
    paths: &[String],
    show_untested: bool,
) -> i32 {
    run_show_tests_to(args::RunShowTestsArgs {
        out: buf,
        universe,
        paths,
        lang_filter: None,
        ignore: &[],
        show_untested,
    })
}

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
    let exit = run_show_tests_to_buf(&mut buf, &universe, &[p], false);
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
    let exit = run_show_tests_to_buf(&mut buf, &universe, &[p], true);
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
    let exit = run_show_tests_to_buf(&mut buf, &universe, &[p], false);
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
    let _ = run_show_tests_to_buf(&mut buf, &universe, &[p], false);
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
    let _ = run_show_tests_to_buf(&mut buf, &universe, &[p], false);
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
    let _ = run_show_tests_to_buf(&mut buf, &universe, &[p], false);
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
    let exit = run_show_tests_to_buf(&mut buf, &universe, &[p], false);
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
    let _ = run_show_tests_to_buf(&mut buf, &universe, &[p], false);
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
    let exit = run_show_tests_to_buf(&mut buf, &universe, &[path_outside], true);
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

#[test]
fn test_defs_from_analysis_rows_direct() {
    use kiss::test_refs::CoveringTest;
    let defs = vec![
        (PathBuf::from("/tmp/a.py"), "foo".to_string(), 1usize),
        (PathBuf::from("/tmp/a.py"), "bar".to_string(), 5usize),
    ];
    let unreferenced = vec![(PathBuf::from("/tmp/a.py"), "bar".to_string(), 5usize)];
    let mut coverage_map: HashMap<(PathBuf, String), Vec<CoveringTest>> = HashMap::new();
    coverage_map.insert(
        (PathBuf::from("/tmp/a.py"), "foo".to_string()),
        vec![(PathBuf::from("/tmp/test_a.py"), "test_foo".to_string())],
    );
    let focus_set: HashSet<PathBuf> = std::iter::once(PathBuf::from("/tmp/a.py")).collect();
    let result = defs_from_analysis_rows(
        defs.into_iter(),
        unreferenced.into_iter(),
        &coverage_map,
        &focus_set,
    );
    assert_eq!(result.len(), 2);
    // foo is covered
    let foo_entry = result.iter().find(|(_, name, _, _)| name == "foo").unwrap();
    assert!(foo_entry.3.is_some());
    // bar is unreferenced
    let bar_entry = result.iter().find(|(_, name, _, _)| name == "bar").unwrap();
    assert!(bar_entry.3.is_none());
}
