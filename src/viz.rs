use kiss::parsing::parse_files;
use kiss::rust_parsing::parse_rust_files;
use kiss::{
    DependencyGraph, Language, build_dependency_graph, rust_graph::build_rust_dependency_graph,
};
use petgraph::visit::EdgeRef;
use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

fn dot_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn node_label(name: &str, path: Option<&PathBuf>) -> String {
    path.map_or_else(
        || name.to_string(),
        |p| format!("{}\\n{}", name, p.display()),
    )
}

fn write_graph_dot(
    out: &mut dyn Write,
    graph: &DependencyGraph,
    prefix: &str,
) -> std::io::Result<()> {
    // Keep output stable for diffs: sort nodes/edges.
    let mut nodes: BTreeSet<String> = BTreeSet::new();
    for name in graph.nodes.keys() {
        nodes.insert(name.clone());
    }

    for n in &nodes {
        let label = node_label(n, graph.paths.get(n));
        writeln!(
            out,
            "  \"{}:{}\" [label=\"{}\"];",
            prefix,
            dot_escape(n),
            dot_escape(&label)
        )?;
    }

    let mut edges: BTreeSet<(String, String)> = BTreeSet::new();
    for e in graph.graph.edge_references() {
        let from = graph.graph[e.source()].clone();
        let to = graph.graph[e.target()].clone();
        edges.insert((from, to));
    }
    for (from, to) in edges {
        writeln!(
            out,
            "  \"{}:{}\" -> \"{}:{}\";",
            prefix,
            dot_escape(&from),
            prefix,
            dot_escape(&to)
        )?;
    }
    Ok(())
}

pub fn run_viz(
    out_path: &Path,
    paths: &[String],
    lang_filter: Option<Language>,
    ignore: &[String],
) -> std::io::Result<()> {
    let (py_files, rs_files) = kiss::discovery::gather_files_by_lang(paths, lang_filter, ignore);
    if py_files.is_empty() && rs_files.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "No source files found.",
        ));
    }

    let mut buf: Vec<u8> = Vec::new();
    writeln!(buf, "digraph kiss {{")?;
    writeln!(buf, "  rankdir=LR;")?;
    writeln!(buf, "  node [shape=box];")?;

    if !py_files.is_empty() {
        let results = parse_files(&py_files).map_err(|e| std::io::Error::other(e.to_string()))?;
        let parsed: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
        let graph = build_dependency_graph(&parsed);
        write_graph_dot(&mut buf, &graph, "py")?;
    }

    if !rs_files.is_empty() {
        let results = parse_rust_files(&rs_files);
        let parsed: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
        let graph = build_rust_dependency_graph(&parsed);
        write_graph_dot(&mut buf, &graph, "rs")?;
    }

    writeln!(buf, "}}")?;
    std::fs::write(out_path, buf)
}

#[cfg(test)]
mod tests {
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
    fn test_gather_files_by_lang_empty_input() {
        let (py, rs) = kiss::discovery::gather_files_by_lang(&[], None, &[]);
        assert!(py.is_empty());
        assert!(rs.is_empty());
    }

    #[test]
    fn test_run_viz_errors_on_no_source_files() {
        let out_path = Path::new("does-not-matter.dot");
        let err = run_viz(out_path, &[], None, &[]).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("No source files"));
    }
}
