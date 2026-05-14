use super::*;
use kiss::test_refs::CoveringTest;

fn discover(universe: &str, paths: &[String]) -> Vec<DefEntry> {
    discover_covering_tests(args::DiscoverArgs {
        universe,
        paths,
        lang_filter: None,
        ignore: &[],
    })
    .unwrap()
}

fn covering_strings(defs: &[DefEntry], def_name: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (_f, n, _, cov) in defs {
        if n == def_name
            && let Some(tests) = cov
        {
            for (p, id) in tests {
                out.push(format!("{}::{}", p.display(), id));
            }
        }
    }
    out
}

#[test]
fn test_discovery_python() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("mymod.py"), "def helper():\n    pass\n").unwrap();
    std::fs::write(
        tmp.path().join("test_mymod.py"),
        "from mymod import helper\ndef test_it():\n    helper()\n",
    )
    .unwrap();

    let universe = tmp.path().to_string_lossy().to_string();
    let p = tmp.path().join("mymod.py").to_string_lossy().to_string();
    let defs = discover(&universe, &[p]);
    let cov = covering_strings(&defs, "helper");
    assert!(
        cov.iter().any(|s| s.contains("test_it")),
        "expected test_it in covering list, got {cov:?}"
    );
}

#[test]
fn test_discovery_python_orphan_unreferenced() {
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
    let defs = discover(&universe, &[p]);
    let orphan = defs.iter().find(|(_, n, _, _)| n == "orphan").unwrap();
    assert!(orphan.3.is_none(), "orphan should be unreferenced");
}

#[test]
fn test_discovery_rust() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("mymod.rs"), "pub fn helper() {}\n").unwrap();
    std::fs::write(
        tmp.path().join("test_mymod.rs"),
        "#[test]\nfn t() { helper(); }\n",
    )
    .unwrap();

    let universe = tmp.path().to_string_lossy().to_string();
    let p = tmp.path().join("mymod.rs").to_string_lossy().to_string();
    let defs = discover(&universe, &[p]);
    let cov = covering_strings(&defs, "helper");
    assert!(!cov.is_empty(), "expected covering tests for helper, got {defs:?}");
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
    let defs = discover(&universe, &[p]);
    let cov = covering_strings(&defs, "parse");
    assert!(cov.iter().any(|s| s.contains("test_parse_empty")));
    assert!(cov.iter().any(|s| s.contains("test_parse_valid")));
}

#[test]
fn test_python_test_class_format() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("widget.py"), "def render(): return 'ok'\n").unwrap();
    std::fs::write(
        tmp.path().join("test_widget.py"),
        "from widget import render\n\nclass TestWidget:\n    def test_render(self):\n        assert render() == 'ok'\n",
    )
    .unwrap();

    let universe = tmp.path().to_string_lossy().to_string();
    let p = tmp.path().join("widget.py").to_string_lossy().to_string();
    let defs = discover(&universe, &[p]);
    let cov = covering_strings(&defs, "render");
    assert!(cov.iter().any(|s| s.contains("TestWidget::test_render")));
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
    let defs = discover(&universe, &[p]);
    let foo_cov = covering_strings(&defs, "foo");
    let bar_cov = covering_strings(&defs, "bar");
    assert!(foo_cov.iter().any(|s| s.contains("test_both")));
    assert!(bar_cov.iter().any(|s| s.contains("test_both")));
}

#[test]
fn test_covered_by_fixture_empty_covering_list() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("mymod.py"),
        "def helper():\n    return 42\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("test_mymod.py"),
        "import pytest\nfrom mymod import helper\n\n@pytest.fixture\ndef my_fixture():\n    return helper()\n\ndef test_it(my_fixture):\n    pass\n",
    )
    .unwrap();

    let universe = tmp.path().to_string_lossy().to_string();
    let p = tmp.path().join("mymod.py").to_string_lossy().to_string();
    let defs = discover(&universe, &[p]);
    let helper = defs.iter().find(|(_, n, _, _)| n == "helper").unwrap();
    assert_eq!(helper.3, Some(Vec::new()));
}

#[test]
fn test_discovery_covering_list_shape() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("mymod.py"), "def helper():\n    pass\n").unwrap();
    std::fs::write(
        tmp.path().join("test_mymod.py"),
        "from mymod import helper\ndef test_it():\n    helper()\n",
    )
    .unwrap();

    let universe = tmp.path().to_string_lossy().to_string();
    let p = tmp.path().join("mymod.py").to_string_lossy().to_string();
    let defs = discover(&universe, &[p]);
    let cov = defs
        .iter()
        .find(|(_, n, _, _)| n == "helper")
        .and_then(|d| d.3.as_ref())
        .expect("covering");
    assert!(
        cov.iter().any(|(_, id)| id == "test_it"),
        "expected test_it id, got {cov:?}"
    );
}

#[test]
fn test_helper_coverage() {
    fn touch<T>(_: T) {}
    touch(collect_py_test_defs);
    touch(collect_rs_test_defs);
    touch(discover_covering_tests);
}

#[test]
fn test_discovery_path_outside_universe() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir_a = tmp.path().join("a");
    let dir_b = tmp.path().join("b");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();

    std::fs::write(dir_a.join("mod.py"), "def foo(): pass\n").unwrap();
    std::fs::write(
        dir_a.join("test_mod.py"),
        "from mod import foo\ndef test_foo(): foo()\n",
    )
    .unwrap();
    std::fs::write(dir_b.join("mod2.py"), "def bar(): pass\n").unwrap();

    let universe = dir_a.to_string_lossy().to_string();
    let path_outside = dir_b
        .join("mod2.py")
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .to_string();

    let defs = discover(&universe, &[path_outside]);
    let bar = defs.iter().find(|(_, n, _, _)| n == "bar").unwrap();
    assert!(bar.3.is_none(), "bar should be untested, got {defs:?}");
}

#[test]
fn discover_two_universe_roots_from_paths() {
    let tmp = tempfile::TempDir::new().unwrap();
    let u1 = tmp.path().join("u1");
    let u2 = tmp.path().join("u2");
    std::fs::create_dir_all(&u1).unwrap();
    std::fs::create_dir_all(&u2).unwrap();
    std::fs::write(u1.join("a.py"), "def fa():\n    pass\n").unwrap();
    std::fs::write(
        u1.join("test_a.py"),
        "from a import fa\ndef test_fa():\n    fa()\n",
    )
    .unwrap();
    std::fs::write(u2.join("b.py"), "def fb():\n    pass\n").unwrap();
    let universe = u1.to_string_lossy().to_string();
    let p2 = u2.join("b.py").canonicalize().unwrap();
    let paths = vec![u1.join("a.py").to_string_lossy().to_string(), p2.to_string_lossy().to_string()];
    let defs = discover(&universe, &paths);
    assert!(defs.iter().any(|(_, n, _, _)| n == "fa"));
    assert!(defs.iter().any(|(_, n, _, _)| n == "fb"));
}

#[test]
fn kiss_test_functions_in_adapter_on_parsed_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("test_x.py"),
        "def test_a():\n    assert 1\n",
    )
    .unwrap();
    let mut parser = kiss::create_parser().unwrap();
    let parsed = kiss::parse_file(&mut parser, &tmp.path().join("test_x.py")).unwrap();
    let ids = kiss::test_functions_in(&parsed);
    assert!(ids.iter().any(|s| s == "test_a"));
}

#[test]
fn test_defs_from_analysis_rows_direct() {
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
    let foo_entry = result.iter().find(|(_, name, _, _)| name == "foo").unwrap();
    assert!(foo_entry.3.is_some());
    let bar_entry = result.iter().find(|(_, name, _, _)| name == "bar").unwrap();
    assert!(bar_entry.3.is_none());
}
