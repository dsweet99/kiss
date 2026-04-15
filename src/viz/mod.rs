use crate::viz_coarsen::{CoarsenedGraph, coarsen_with_zoom};
use kiss::{DependencyGraph, Language};
use petgraph::visit::EdgeRef;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests;

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

pub(crate) fn write_coarsened_dot(out: &mut dyn Write, g: &CoarsenedGraph) -> std::io::Result<()> {
    for (i, label) in g.labels.iter().enumerate() {
        writeln!(out, "  \"c{i}\" [label=\"{}\"];", dot_escape(label))?;
    }
    for (a, b) in &g.edges {
        writeln!(out, "  \"c{a}\" -> \"c{b}\";")?;
    }
    Ok(())
}

pub(crate) fn write_coarsened_mermaid(out: &mut dyn Write, g: &CoarsenedGraph) -> std::io::Result<()> {
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
pub(crate) enum VizFormat {
    Dot,
    Mermaid,
    MarkdownMermaid,
}

pub(crate) fn viz_format_for_path(path: &Path) -> std::io::Result<VizFormat> {
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
            format!("Unsupported output file extension '.{ext}'. Use .dot, .mmd/.mermaid, or .md"),
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

pub(crate) fn collect_graph_nodes_and_edges(graph: &DependencyGraph, prefix: &str) -> CombinedGraphParts {
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

pub(crate) fn write_format_header(buf: &mut Vec<u8>, format: VizFormat) -> std::io::Result<()> {
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

pub(crate) fn write_format_footer(buf: &mut Vec<u8>, format: VizFormat) -> std::io::Result<()> {
    match format {
        VizFormat::Dot => writeln!(buf, "}}")?,
        VizFormat::Mermaid => {}
        VizFormat::MarkdownMermaid => writeln!(buf, "```")?,
    }
    Ok(())
}

pub(crate) fn write_graph_for_format(
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

pub(crate) fn write_coarsened_for_format(
    buf: &mut Vec<u8>,
    coarse: &CoarsenedGraph,
    format: VizFormat,
) -> std::io::Result<()> {
    match format {
        VizFormat::Dot => write_coarsened_dot(buf, coarse),
        VizFormat::Mermaid | VizFormat::MarkdownMermaid => write_coarsened_mermaid(buf, coarse),
    }
}

pub(crate) fn build_coarsened_graph(
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    zoom: f64,
) -> std::io::Result<CoarsenedGraph> {
    let mut all_nodes: BTreeSet<String> = BTreeSet::new();
    let mut all_edges: BTreeSet<(String, String)> = BTreeSet::new();
    let mut all_paths: BTreeMap<String, PathBuf> = BTreeMap::new();

    if !py_files.is_empty() {
        let py_graph = crate::analyze::build_py_graph_from_files(py_files)?;
        let (n, e, p) = collect_graph_nodes_and_edges(&py_graph, "py");
        all_nodes.extend(n);
        all_edges.extend(e);
        all_paths.extend(p);
    }
    if !rs_files.is_empty() {
        let rs_graph = crate::analyze::build_rs_graph_from_files(rs_files);
        let (n, e, p) = collect_graph_nodes_and_edges(&rs_graph, "rs");
        all_nodes.extend(n);
        all_edges.extend(e);
        all_paths.extend(p);
    }

    let nodes_vec: Vec<String> = all_nodes.into_iter().collect();
    Ok(coarsen_with_zoom(&nodes_vec, &all_edges, &all_paths, zoom))
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
            let py_graph = crate::analyze::build_py_graph_from_files(&py_files)?;
            write_graph_for_format(&mut buf, &py_graph, "py", format)?;
        }
        if !rs_files.is_empty() {
            let rs_graph = crate::analyze::build_rs_graph_from_files(&rs_files);
            write_graph_for_format(&mut buf, &rs_graph, "rs", format)?;
        }
    } else {
        write_coarsened_for_format(
            &mut buf,
            &build_coarsened_graph(&py_files, &rs_files, zoom)?,
            format,
        )?;
    }

    write_format_footer(&mut buf, format)?;
    std::fs::write(out_path, buf)
}
