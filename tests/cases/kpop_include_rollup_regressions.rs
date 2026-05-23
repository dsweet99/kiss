//! Regression tests for include! rollup and graph edge resolution.

use kiss::config::Config;
use kiss::rust_counts::{analyze_rust_file, analyze_rust_file_include_rollup};
use kiss::rust_graph::build_rust_dependency_graph;
use kiss::rust_graph::{build_include_graph, expand_rust_files};
use kiss::rust_include::canonical_path;
use kiss::rust_parsing::{parse_rust_file, parse_rust_files};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn repo_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn read_fake_rust_fixture(name: &str) -> String {
    fs::read_to_string(repo_path("tests/fake_rust").join(name)).unwrap()
}

struct ChdirGuard(PathBuf);

impl Drop for ChdirGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.0);
    }
}

fn duplicate_child_include_fixture() -> (TempDir, PathBuf, PathBuf, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir_all(src.join("a")).unwrap();
    fs::create_dir_all(src.join("b")).unwrap();
    fs::write(src.join("lib.rs"), "include!(\"a/child.rs\");\n").unwrap();
    fs::write(src.join("a").join("child.rs"), "pub fn from_a() {}\n").unwrap();
    fs::write(src.join("b").join("child.rs"), "pub fn from_b() {}\n").unwrap();
    (
        tmp,
        PathBuf::from("src/lib.rs"),
        PathBuf::from("src/a/child.rs"),
        PathBuf::from("src/b/child.rs"),
    )
}

fn assert_canonical_include_edge(
    graph: &kiss::graph::DependencyGraph,
    path_lib: &Path,
    path_child_a: &Path,
    path_child_b: &Path,
) {
    let lib_module = graph
        .path_to_module
        .get(&canonical_path(path_lib))
        .expect("lib registered");
    let a_module = graph
        .path_to_module
        .get(&canonical_path(path_child_a))
        .expect("a registered");
    let b_module = graph
        .path_to_module
        .get(&canonical_path(path_child_b))
        .expect("b registered");
    assert!(
        graph.imports(lib_module, a_module),
        "include!(\"a/child.rs\") must resolve via canonical path, not basename fallback to b/child.rs"
    );
    assert!(
        !graph.imports(lib_module, b_module),
        "lib must not depend on b/child.rs when including a/child.rs"
    );
}

#[test]
fn include_edge_uses_canonical_path_when_parsed_paths_are_relative() {
    let (tmp, path_lib, path_child_a, path_child_b) = duplicate_child_include_fixture();
    let _cwd = ChdirGuard(std::env::current_dir().unwrap());
    std::env::set_current_dir(tmp.path()).unwrap();

    let lib = parse_rust_file(&path_lib).unwrap();
    let parsed_a = parse_rust_file(&path_child_a).unwrap();
    let parsed_b = parse_rust_file(&path_child_b).unwrap();
    let graph = build_rust_dependency_graph(&[&lib, &parsed_a, &parsed_b]);
    assert_canonical_include_edge(&graph, &path_lib, &path_child_a, &path_child_b);
}

#[test]
fn parse_and_analyze_rs_rollup_flags_includer_lines_per_file() {
    let lib_fixture = read_fake_rust_fixture("include_inc_lib.rs");
    let inc_fixture = read_fake_rust_fixture("include_inc_fragment.inc");

    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("lib.rs"), lib_fixture).unwrap();
    std::fs::write(src.join("include_inc_fragment.inc"), inc_fixture).unwrap();

    let files = expand_rust_files(vec![src.join("lib.rs")]);
    let mut cfg = Config::rust_defaults();
    cfg.statements_per_file = 2;

    let parsed: Vec<_> = parse_rust_files(&files)
        .into_iter()
        .filter_map(Result::ok)
        .collect();
    let mut viols = Vec::new();
    for p in &parsed {
        viols.extend(analyze_rust_file(p, &cfg));
    }
    let refs: Vec<_> = parsed.iter().collect();
    let include_graph = build_include_graph(&refs);
    let by_path: std::collections::HashMap<_, _> = parsed
        .iter()
        .map(|p| (canonical_path(&p.path), p))
        .collect();
    for parent in &parsed {
        let included_paths = include_graph.transitive_from(&parent.path);
        let included: Vec<_> = included_paths
            .iter()
            .filter_map(|path| by_path.get(path).copied())
            .collect();
        viols.extend(analyze_rust_file_include_rollup(parent, &included, &cfg));
    }

    assert!(
        viols.iter().any(|v| {
            v.metric == "statements_per_file"
                && v.file.file_name().is_some_and(|n| n == "lib.rs")
                && v.message.contains("include_inc_fragment.inc")
        }),
        "rolled-up includer should exceed statements_per_file; got:\n{viols:#?}"
    );
}

#[test]
fn graph_mod_split_includes_keep_rollup_under_lines_per_file() {
    let files: Vec<PathBuf> = [
        "src/graph/mod.rs",
        "src/graph/graph_analyze.rs",
        "src/graph/graph_build.rs",
        "src/graph/graph_python.rs",
        "src/graph/dependency_graph_body.rs",
        "src/graph/python_imports_body.rs",
        "src/graph/build_body.rs",
        "src/graph/analyze_body.rs",
    ]
    .into_iter()
    .map(repo_path)
    .collect();

    let expanded = expand_rust_files(files);
    let mut cfg = Config::rust_defaults();
    cfg.lines_per_file = 400;

    let parsed: Vec<_> = parse_rust_files(&expanded)
        .into_iter()
        .filter_map(Result::ok)
        .collect();
    let refs: Vec<_> = parsed.iter().collect();
    let include_graph = build_include_graph(&refs);
    let by_path: std::collections::HashMap<_, _> = parsed
        .iter()
        .map(|p| (canonical_path(&p.path), p))
        .collect();

    let mut rollup_viols = Vec::new();
    for parent in &parsed {
        let included: Vec<_> = include_graph
            .transitive_from(&parent.path)
            .iter()
            .filter_map(|path| by_path.get(path).copied())
            .collect();
        rollup_viols.extend(analyze_rust_file_include_rollup(parent, &included, &cfg));
    }

    let graph_shells: Vec<PathBuf> = [
        "src/graph/mod.rs",
        "src/graph/graph_analyze.rs",
        "src/graph/graph_build.rs",
        "src/graph/graph_python.rs",
    ]
    .into_iter()
    .map(repo_path)
    .collect();

    let shell_viols: Vec<_> = rollup_viols
        .iter()
        .filter(|v| {
            v.metric == "lines_per_file"
                && graph_shells
                    .iter()
                    .any(|shell| canonical_path(shell) == canonical_path(&v.file))
        })
        .collect();

    assert!(
        shell_viols.is_empty(),
        "graph include shells should stay under lines_per_file after split; got:\n{shell_viols:#?}"
    );
}

#[test]
fn include_in_mod_block_resolves_and_rollup_counts_body() {
    use kiss::graph::analyze_graph;

    let lib_fixture = read_fake_rust_fixture("include_mod_block_lib.rs");
    let body_fixture = read_fake_rust_fixture("include_mod_block_body.inc");

    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("lib.rs"), lib_fixture).unwrap();
    std::fs::write(src.join("include_mod_block_body.inc"), body_fixture).unwrap();

    let lib = parse_rust_file(&src.join("lib.rs")).unwrap();
    let body = parse_rust_file(&src.join("include_mod_block_body.inc")).unwrap();
    let parsed = vec![&lib, &body];
    let g = build_rust_dependency_graph(&parsed);
    let viols = analyze_graph(&g, &Config::rust_defaults(), true);
    assert!(
        !viols.iter().any(|v| {
            v.metric == "orphan_module"
                && v.file.file_name().is_some_and(|n| n == "include_mod_block_body.inc")
        }),
        "body included from mod block should not be orphan; got:\n{viols:#?}"
    );

    let mut cfg = Config::rust_defaults();
    cfg.statements_per_file = 1;
    let viols = analyze_rust_file_include_rollup(&lib, &[&body], &cfg);
    assert!(
        viols.iter().any(|v| {
            v.metric == "statements_per_file"
                && v.message.contains("include_mod_block_body.inc")
        }),
        "rollup should cite mod-block include body; got:\n{viols:#?}"
    );
}

#[test]
fn nested_include_chain_not_orphan() {
    use kiss::graph::analyze_graph;

    let lib_fixture = read_fake_rust_fixture("include_nested_lib.rs");
    let outer_fixture = read_fake_rust_fixture("include_nested_outer.inc");
    let inner_fixture = read_fake_rust_fixture("include_nested_inner.inc");

    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    fs::write(src.join("lib.rs"), lib_fixture).unwrap();
    fs::write(src.join("include_nested_outer.inc"), outer_fixture).unwrap();
    fs::write(src.join("include_nested_inner.inc"), inner_fixture).unwrap();

    let lib = parse_rust_file(&src.join("lib.rs")).unwrap();
    let outer = parse_rust_file(&src.join("include_nested_outer.inc")).unwrap();
    let inner = parse_rust_file(&src.join("include_nested_inner.inc")).unwrap();
    let parsed = vec![&lib, &outer, &inner];
    let g = build_rust_dependency_graph(&parsed);
    let viols = analyze_graph(&g, &Config::rust_defaults(), true);

    assert!(
        !viols.iter().any(|v| {
            v.metric == "orphan_module"
                && (v.file.file_name().is_some_and(|n| n == "include_nested_outer.inc")
                    || v.file.file_name().is_some_and(|n| n == "include_nested_inner.inc"))
        }),
        "nested include chain should not orphan fragments; got:\n{viols:#?}"
    );
}

#[test]
fn expand_rust_files_discovers_nested_include_chain() {
    let lib = repo_path("tests/fake_rust/include_nested_lib.rs");
    let expanded = expand_rust_files(vec![lib]);
    let names: Vec<_> = expanded
        .iter()
        .filter_map(|p| p.file_name().and_then(|n| n.to_str()))
        .collect();
    assert!(
        names.contains(&"include_nested_inner.inc"),
        "nested include! chain should pull in inner .inc; got files: {names:?}"
    );
}
