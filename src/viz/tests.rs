use super::*;

#[test]
fn test_dot_escape_escapes_backslashes_and_quotes() {
    assert_eq!(dot_escape(r#"a\b"c"#), r#"a\\b\"c"#);
}

#[test]
fn test_node_label_includes_path_when_present() {
    assert_eq!(node_label("name", None), "name");
    let p = PathBuf::from("src/main.rs");
    let label = node_label("name", Some(&p));
    assert!(label.contains("name"));
    assert!(label.contains("src/main.rs"));
}

#[test]
fn test_viz_format_for_path() {
    assert_eq!(
        viz_format_for_path(Path::new("a.dot")).unwrap(),
        VizFormat::Dot
    );
    assert_eq!(
        viz_format_for_path(Path::new("a.mmd")).unwrap(),
        VizFormat::Mermaid
    );
    assert_eq!(
        viz_format_for_path(Path::new("a.mermaid")).unwrap(),
        VizFormat::Mermaid
    );
    assert_eq!(
        viz_format_for_path(Path::new("a.md")).unwrap(),
        VizFormat::MarkdownMermaid
    );

    let err = viz_format_for_path(Path::new("a.txt")).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert!(
        err.to_string()
            .contains("Unsupported output file extension")
    );

    let err2 = viz_format_for_path(Path::new("a")).unwrap_err();
    assert_eq!(err2.kind(), std::io::ErrorKind::InvalidInput);
    assert!(err2.to_string().contains("must have an extension"));
}

#[test]
fn test_write_graph_dot_writes_stable_nodes_and_edges() {
    let mut g = DependencyGraph::new();
    g.paths.insert("a".to_string(), PathBuf::from("a.py"));
    g.paths.insert("b".to_string(), PathBuf::from("b.py"));
    g.add_dependency("a", "b");

    let mut out: Vec<u8> = Vec::new();
    write_graph_dot(&mut out, &g, "py").unwrap();
    let s = String::from_utf8(out).unwrap();
    assert!(s.contains("\"py:a\""));
    assert!(s.contains("\"py:b\""));
    assert!(s.contains("\"py:a\" -> \"py:b\";"));
}

#[test]
fn test_write_graph_mermaid_writes_nodes_and_edges() {
    let mut g = DependencyGraph::new();
    g.paths.insert("a".to_string(), PathBuf::from("a.py"));
    g.paths.insert("b".to_string(), PathBuf::from("b.py"));
    g.add_dependency("a", "b");

    let mut out: Vec<u8> = Vec::new();
    write_graph_mermaid(&mut out, &g, "py").unwrap();
    let s = String::from_utf8(out).unwrap();
    assert!(s.contains("py__a["));
    assert!(s.contains("py__b["));
    assert!(s.contains("py__a --> py__b"));
}

#[test]
fn test_clamp_zoom() {
    assert!(clamp_zoom(0.0).is_ok());
    assert!(clamp_zoom(1.0).is_ok());
    assert!(clamp_zoom(0.5).is_ok());
    assert!(clamp_zoom(-0.1).is_err());
    assert!(clamp_zoom(1.1).is_err());
}

#[test]
fn test_combined_node_key() {
    assert_eq!(combined_node_key("py", "foo"), "py:foo");
    assert_eq!(combined_node_key("rs", "bar"), "rs:bar");
}

#[test]
fn test_collect_graph_nodes_and_edges() {
    let mut graph = DependencyGraph::new();
    graph.add_dependency("a", "b");
    let (nodes, edges, _) = collect_graph_nodes_and_edges(&graph, "test");
    assert!(nodes.contains("test:a"));
    assert!(nodes.contains("test:b"));
    assert!(edges.contains(&("test:a".to_string(), "test:b".to_string())));
}

#[test]
fn test_mermaid_escape_label() {
    assert_eq!(mermaid_escape_label("a<b>c"), "a&lt;b&gt;c");
    assert_eq!(mermaid_escape_label("a&b"), "a&amp;b");
    assert_eq!(mermaid_escape_label("a\"b"), "a&quot;b");
}

#[test]
fn test_mermaid_node_id() {
    assert_eq!(mermaid_node_id("rs", "Foo"), "rs__foo");
    assert_eq!(mermaid_node_id("py", "bar-baz"), "py__bar_baz");
}

#[test]
fn test_run_viz_errors_on_no_source_files() {
    let out_path = Path::new("does-not-matter.dot");
    let err = run_viz(out_path, &[], None, &[], 1.0).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("No source files"));
}

#[test]
fn test_write_format_header_footer() {
    let mut buf = Vec::new();
    write_format_header(&mut buf, VizFormat::Mermaid).unwrap();
    assert!(String::from_utf8(buf.clone()).unwrap().contains("graph LR"));
    write_format_footer(&mut buf, VizFormat::Mermaid).unwrap();
}

#[test]
fn test_write_graph_for_format() {
    let mut graph = DependencyGraph::new();
    graph.add_dependency("a", "b");
    let mut buf = Vec::new();
    write_graph_for_format(&mut buf, &graph, "test", VizFormat::Mermaid).unwrap();
    assert!(!buf.is_empty());
}

#[test]
fn test_write_coarsened_for_format() {
    let cg = CoarsenedGraph {
        labels: vec!["x".into()],
        edges: BTreeSet::new(),
    };
    let mut buf = Vec::new();
    write_coarsened_for_format(&mut buf, &cg, VizFormat::Mermaid).unwrap();
    assert!(!buf.is_empty());
}

#[test]
fn test_build_py_graph_empty() {
    let graph = crate::analyze::build_py_graph_from_files(&[]).unwrap();
    assert!(graph.nodes.is_empty());
}

#[test]
fn test_build_rs_graph_empty() {
    let graph = crate::analyze::build_rs_graph_from_files(&[]);
    assert!(graph.nodes.is_empty());
}

#[test]
fn test_touch_coarsened_helpers_for_static_coverage() {
    let cg = CoarsenedGraph {
        labels: vec!["x".into()],
        edges: BTreeSet::new(),
    };
    let mut out = Vec::new();
    write_coarsened_dot(&mut out, &cg).unwrap();
    out.clear();
    write_coarsened_mermaid(&mut out, &cg).unwrap();

    let coarse = build_coarsened_graph(&[], &[], 0.2).unwrap();
    assert!(!coarse.labels.is_empty());
}
