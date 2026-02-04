use crate::viz_coarsen::{coarsen_with_zoom, CoarsenedGraph};
use crate::viz_py_scan::extract_py_imports_fast;
use kiss::rust_parsing::parse_rust_files;
use kiss::{DependencyGraph, Language, rust_graph::build_rust_dependency_graph};
use petgraph::visit::EdgeRef;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

fn dot_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn mermaid_escape_label(s: &str) -> String {
    // Mermaid labels render HTML; keep it simple and safe.
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn mermaid_node_id(prefix: &str, name: &str) -> String {
    // Mermaid identifiers should be simple; normalize to [a-z0-9_].
    let mut out = String::with_capacity(prefix.len() + name.len() + 3);
    out.push_str(prefix);
    out.push_str("__");
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert(0, 'n');
    }
    out
}

fn node_label(name: &str, path: Option<&PathBuf>) -> String {
    path.map_or_else(
        || name.to_string(),
        |p| format!("{}\\n{}", name, p.display()),
    )
}

fn write_graph_dot(out: &mut dyn Write, graph: &DependencyGraph, prefix: &str) -> std::io::Result<()> {
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

fn write_graph_mermaid(
    out: &mut dyn Write,
    graph: &DependencyGraph,
    prefix: &str,
) -> std::io::Result<()> {
    let mut nodes: BTreeSet<String> = BTreeSet::new();
    for name in graph.nodes.keys() {
        nodes.insert(name.clone());
    }

    for n in &nodes {
        let label = node_label(n, graph.paths.get(n));
        let id = mermaid_node_id(prefix, n);
        writeln!(out, "  {id}[\"{}\"]", mermaid_escape_label(&label))?;
    }

    let mut edges: BTreeSet<(String, String)> = BTreeSet::new();
    for e in graph.graph.edge_references() {
        let from = graph.graph[e.source()].clone();
        let to = graph.graph[e.target()].clone();
        edges.insert((from, to));
    }
    for (from, to) in edges {
        let from_id = mermaid_node_id(prefix, &from);
        let to_id = mermaid_node_id(prefix, &to);
        writeln!(out, "  {from_id} --> {to_id}")?;
    }
    Ok(())
}

fn write_coarsened_dot(out: &mut dyn Write, g: &CoarsenedGraph) -> std::io::Result<()> {
    for (i, label) in g.labels.iter().enumerate() {
        writeln!(out, "  \"c{i}\" [label=\"{}\"];", dot_escape(label))?;
    }
    for (a, b) in &g.edges {
        writeln!(out, "  \"c{a}\" -> \"c{b}\";")?;
    }
    Ok(())
}

fn write_coarsened_mermaid(out: &mut dyn Write, g: &CoarsenedGraph) -> std::io::Result<()> {
    for (i, label) in g.labels.iter().enumerate() {
        let label = mermaid_escape_label(label);
        writeln!(out, "  c{i}[\"{label}\"]")?;
    }
    for (a, b) in &g.edges {
        writeln!(out, "  c{a} --> c{b}")?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VizFormat {
    Dot,
    Mermaid,
    MarkdownMermaid,
}

fn viz_format_for_path(path: &Path) -> std::io::Result<VizFormat> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_lowercase)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Output file must have an extension: .dot, .mmd/.mermaid, or .md",
            )
        })?;

    match ext.as_str() {
        "dot" => Ok(VizFormat::Dot),
        "mmd" | "mermaid" => Ok(VizFormat::Mermaid),
        "md" | "markdown" => Ok(VizFormat::MarkdownMermaid),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "Unsupported output file extension '.{ext}'. Use .dot, .mmd/.mermaid, or .md"
            ),
        )),
    }
}

fn combined_node_key(prefix: &str, name: &str) -> String {
    format!("{prefix}:{name}")
}

type CombinedGraphParts = (
    BTreeSet<String>,
    BTreeSet<(String, String)>,
    BTreeMap<String, PathBuf>,
);

fn collect_graph_nodes_and_edges(graph: &DependencyGraph, prefix: &str) -> CombinedGraphParts {
    let mut nodes: BTreeSet<String> = BTreeSet::new();
    let mut paths: BTreeMap<String, PathBuf> = BTreeMap::new();
    for (name, path) in &graph.paths {
        let key = combined_node_key(prefix, name);
        nodes.insert(key.clone());
        paths.insert(key, path.clone());
    }
    for name in graph.nodes.keys() {
        nodes.insert(combined_node_key(prefix, name));
    }

    let mut edges: BTreeSet<(String, String)> = BTreeSet::new();
    for e in graph.graph.edge_references() {
        let from = combined_node_key(prefix, &graph.graph[e.source()]);
        let to = combined_node_key(prefix, &graph.graph[e.target()]);
        edges.insert((from, to));
    }
    (nodes, edges, paths)
}

fn clamp_zoom(z: f64) -> std::io::Result<f64> {
    if !(0.0..=1.0).contains(&z) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "zoom must be within [0,1]",
        ));
    }
    Ok(z)
}

fn write_format_header(buf: &mut Vec<u8>, format: VizFormat) -> std::io::Result<()> {
    match format {
        VizFormat::Dot => {
            writeln!(buf, "digraph kiss {{")?;
            writeln!(buf, "  rankdir=LR;")?;
            writeln!(buf, "  node [shape=box];")?;
        }
        VizFormat::Mermaid => {
            writeln!(buf, "graph LR")?;
        }
        VizFormat::MarkdownMermaid => {
            writeln!(buf, "```mermaid")?;
            writeln!(buf, "graph LR")?;
        }
    }
    Ok(())
}

fn write_format_footer(buf: &mut Vec<u8>, format: VizFormat) -> std::io::Result<()> {
    match format {
        VizFormat::Dot => writeln!(buf, "}}")?,
        VizFormat::Mermaid => {}
        VizFormat::MarkdownMermaid => writeln!(buf, "```")?,
    }
    Ok(())
}

fn write_graph_for_format(
    buf: &mut Vec<u8>,
    graph: &DependencyGraph,
    prefix: &str,
    format: VizFormat,
) -> std::io::Result<()> {
    match format {
        VizFormat::Dot => write_graph_dot(buf, graph, prefix),
        VizFormat::Mermaid | VizFormat::MarkdownMermaid => write_graph_mermaid(buf, graph, prefix),
    }
}

fn write_coarsened_for_format(
    buf: &mut Vec<u8>,
    coarse: &CoarsenedGraph,
    format: VizFormat,
) -> std::io::Result<()> {
    match format {
        VizFormat::Dot => write_coarsened_dot(buf, coarse),
        VizFormat::Mermaid | VizFormat::MarkdownMermaid => write_coarsened_mermaid(buf, coarse),
    }
}

fn build_py_graph(py_files: &[PathBuf]) -> DependencyGraph {
    let imports: Vec<(PathBuf, Vec<String>)> = py_files
        .iter()
        .filter_map(|path| {
            let src = std::fs::read_to_string(path).ok()?;
            Some((path.clone(), extract_py_imports_fast(&src)))
        })
        .collect();
    kiss::graph::build_dependency_graph_from_import_lists(&imports)
}

fn build_rs_graph(rs_files: &[PathBuf]) -> DependencyGraph {
    let results = parse_rust_files(rs_files);
    let parsed: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
    build_rust_dependency_graph(&parsed)
}

fn build_coarsened_graph(py_files: &[PathBuf], rs_files: &[PathBuf], zoom: f64) -> CoarsenedGraph {
    let mut all_nodes: BTreeSet<String> = BTreeSet::new();
    let mut all_edges: BTreeSet<(String, String)> = BTreeSet::new();
    let mut all_paths: BTreeMap<String, PathBuf> = BTreeMap::new();

    if !py_files.is_empty() {
        let (n, e, p) = collect_graph_nodes_and_edges(&build_py_graph(py_files), "py");
        all_nodes.extend(n);
        all_edges.extend(e);
        all_paths.extend(p);
    }
    if !rs_files.is_empty() {
        let (n, e, p) = collect_graph_nodes_and_edges(&build_rs_graph(rs_files), "rs");
        all_nodes.extend(n);
        all_edges.extend(e);
        all_paths.extend(p);
    }

    let nodes_vec: Vec<String> = all_nodes.into_iter().collect();
    coarsen_with_zoom(&nodes_vec, &all_edges, &all_paths, zoom)
}

pub fn run_viz(
    out_path: &Path,
    paths: &[String],
    lang_filter: Option<Language>,
    ignore: &[String],
    zoom: f64,
) -> std::io::Result<()> {
    let zoom = clamp_zoom(zoom)?;
    let (py_files, rs_files) = kiss::discovery::gather_files_by_lang(paths, lang_filter, ignore);
    if py_files.is_empty() && rs_files.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "No source files found.",
        ));
    }

    let format = viz_format_for_path(out_path)?;
    let mut buf: Vec<u8> = Vec::new();
    write_format_header(&mut buf, format)?;

    if zoom >= 1.0 {
        if !py_files.is_empty() {
            write_graph_for_format(&mut buf, &build_py_graph(&py_files), "py", format)?;
        }
        if !rs_files.is_empty() {
            write_graph_for_format(&mut buf, &build_rs_graph(&rs_files), "rs", format)?;
        }
    } else {
        write_coarsened_for_format(
            &mut buf,
            &build_coarsened_graph(&py_files, &rs_files, zoom),
            format,
        )?;
    }

    write_format_footer(&mut buf, format)?;
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
    fn test_viz_format_for_path() {
        assert_eq!(viz_format_for_path(Path::new("a.dot")).unwrap(), VizFormat::Dot);
        assert_eq!(viz_format_for_path(Path::new("a.mmd")).unwrap(), VizFormat::Mermaid);
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
        assert!(err.to_string().contains("Unsupported output file extension"));

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
        let cg = CoarsenedGraph { labels: vec!["x".into()], edges: BTreeSet::new() };
        let mut buf = Vec::new();
        write_coarsened_for_format(&mut buf, &cg, VizFormat::Mermaid).unwrap();
        assert!(!buf.is_empty());
    }

    #[test]
    fn test_build_py_graph_empty() {
        let graph = build_py_graph(&[]);
        assert!(graph.nodes.is_empty());
    }

    #[test]
    fn test_build_rs_graph_empty() {
        let graph = build_rs_graph(&[]);
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

        let coarse = build_coarsened_graph(&[], &[], 0.2);
        assert!(!coarse.labels.is_empty());
    }
}

