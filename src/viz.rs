use fa_leiden_cd::{Graph as LeidenGraph, TrivialModularityOptimizer};
use kiss::parsing::parse_files;
use kiss::rust_parsing::parse_rust_files;
use kiss::{
    DependencyGraph, Language, build_dependency_graph, rust_graph::build_rust_dependency_graph,
};
use petgraph::visit::EdgeRef;
use std::collections::{BTreeMap, BTreeSet, HashMap};
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
            format!("Unsupported output file extension '.{ext}'. Use .dot, .mmd/.mermaid, or .md"),
        )),
    }
}

#[derive(Debug, Clone)]
struct CoarsenedGraph {
    labels: Vec<String>,
    edges: BTreeSet<(usize, usize)>,
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

fn leiden_partition(nodes: &[String], edges: &BTreeSet<(String, String)>) -> Vec<Vec<usize>> {
    // Leiden wants an integer node id space.
    let mut g: LeidenGraph<String, ()> = LeidenGraph::new();
    let mut ids: HashMap<String, usize> = HashMap::new();
    for n in nodes {
        let id = g.add_node(n.clone());
        ids.insert(n.clone(), id);
    }

    for (from, to) in edges {
        let Some(&a) = ids.get(from) else { continue };
        let Some(&b) = ids.get(to) else { continue };
        // Treat as undirected for community detection.
        g.add_edge(a, b, (), 1.0);
        g.add_edge(b, a, (), 1.0);
    }

    let mut optimizer = TrivialModularityOptimizer {
        parallel_scale: 128,
        tol: 1e-11,
    };
    let hierarchy = g.leiden(Some(100), &mut optimizer);

    let mut communities: Vec<Vec<usize>> = Vec::new();
    for comm in hierarchy.node_data_slice() {
        let members = std::cell::RefCell::new(Vec::<usize>::new());
        comm.collect_nodes(&|idx| members.borrow_mut().push(idx));
        let mut members = members.into_inner();
        members.sort_unstable();
        members.dedup();
        communities.push(members);
    }
    // Stable ordering: largest first, then lexicographic.
    communities.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    communities
}

fn target_node_count(node_count: usize, zoom: f64) -> usize {
    if node_count <= 1 {
        return 1;
    }
    let z = zoom.clamp(0.0, 1.0);
    // We accept float zoom, but keep casting local and explicit.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    {
        let span = (node_count - 1) as f64;
        1 + ((z * span).round() as usize).min(node_count - 1)
    }
}

fn build_node_index(nodes: &[String]) -> HashMap<&str, usize> {
    let mut idx = HashMap::new();
    for (i, n) in nodes.iter().enumerate() {
        idx.insert(n.as_str(), i);
    }
    idx
}

fn rebuild_intercommunity_weights(
    edges: &BTreeSet<(String, String)>,
    node_index: &HashMap<&str, usize>,
    node_to_comm: &[usize],
) -> BTreeMap<(usize, usize), usize> {
    let mut weights: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for (from, to) in edges {
        let Some(&a) = node_index.get(from.as_str()) else {
            continue;
        };
        let Some(&b) = node_index.get(to.as_str()) else {
            continue;
        };
        let ca = node_to_comm[a];
        let cb = node_to_comm[b];
        if ca == cb {
            continue;
        }
        let (x, y) = if ca < cb { (ca, cb) } else { (cb, ca) };
        *weights.entry((x, y)).or_insert(0) += 1;
    }
    weights
}

fn assign_nodes_to_communities(communities: &[Vec<usize>], node_count: usize) -> Vec<usize> {
    let mut node_to_comm = vec![0; node_count];
    for (ci, members) in communities.iter().enumerate() {
        for &m in members {
            node_to_comm[m] = ci;
        }
    }
    node_to_comm
}

fn find_best_merge_target(weights: &BTreeMap<(usize, usize), usize>, small_i: usize) -> Option<usize> {
    let mut best: Option<(usize, usize)> = None;
    for ((a, b), w) in weights {
        let other = if *a == small_i { *b } else if *b == small_i { *a } else { continue };
        let cand = (*w, other);
        if best.is_none_or(|cur| cand.0 > cur.0 || (cand.0 == cur.0 && cand.1 < cur.1)) {
            best = Some(cand);
        }
    }
    best.map(|(_, o)| o)
}

fn merge_communities_to_target(
    nodes: &[String],
    edges: &BTreeSet<(String, String)>,
    mut communities: Vec<Vec<usize>>,
    target: usize,
) -> Vec<Vec<usize>> {
    if target <= 1 { return vec![(0..nodes.len()).collect()]; }
    if communities.len() <= target { return communities; }

    let node_index = build_node_index(nodes);
    let mut node_to_comm = assign_nodes_to_communities(&communities, nodes.len());
    let mut weights = rebuild_intercommunity_weights(edges, &node_index, &node_to_comm);

    while communities.len() > target {
        let (small_i, _) = communities.iter().enumerate().min_by_key(|(_, m)| m.len()).unwrap();
        let fallback = usize::from(small_i == 0);
        let merge_into = find_best_merge_target(&weights, small_i).unwrap_or(fallback);
        let (dst, src) = if merge_into < small_i { (merge_into, small_i) } else { (small_i, merge_into) };

        let moved: Vec<usize> = communities[src].drain(..).collect();
        communities[dst].extend(moved);
        communities[dst].sort_unstable();
        communities.remove(src);

        node_to_comm = assign_nodes_to_communities(&communities, nodes.len());
        weights = rebuild_intercommunity_weights(edges, &node_index, &node_to_comm);
    }
    communities
}

fn node_size_and_display(name: &str, paths: &BTreeMap<String, PathBuf>) -> (u64, String) {
    paths.get(name).map_or_else(
        || (0, name.to_string()),
        |p| {
            let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
            (size, p.display().to_string())
        },
    )
}

fn build_cluster_labels(
    nodes: &[String],
    paths: &BTreeMap<String, PathBuf>,
    communities: &[Vec<usize>],
) -> Vec<String> {
    use std::fmt::Write as _;
    let mut labels = Vec::with_capacity(communities.len());
    for members in communities {
        let mut sized: Vec<(u64, String)> = members
            .iter()
            .map(|&idx| node_size_and_display(&nodes[idx], paths))
            .collect();
        sized.sort_by(|a, b| b.0.cmp(&a.0));

        let total = sized.len();
        let mut label = String::new();
        if total <= 4 {
            for (i, (_, s)) in sized.into_iter().enumerate() {
                if i > 0 { label.push('\n'); }
                label.push_str(&s);
            }
        } else {
            for (i, (_, s)) in sized.into_iter().take(3).enumerate() {
                if i > 0 { label.push('\n'); }
                label.push_str(&s);
            }
            let _ = write!(label, "\n[{} more]", total - 3);
        }
        labels.push(label);
    }
    labels
}

fn build_cluster_edges(
    nodes: &[String],
    edges: &BTreeSet<(String, String)>,
    communities: &[Vec<usize>],
) -> BTreeSet<(usize, usize)> {
    let node_index = build_node_index(nodes);

    let mut node_to_comm: Vec<usize> = vec![0; nodes.len()];
    for (ci, members) in communities.iter().enumerate() {
        for &m in members {
            node_to_comm[m] = ci;
        }
    }

    let mut out: BTreeSet<(usize, usize)> = BTreeSet::new();
    for (from, to) in edges {
        let Some(&a) = node_index.get(from.as_str()) else {
            continue;
        };
        let Some(&b) = node_index.get(to.as_str()) else {
            continue;
        };
        let ca = node_to_comm[a];
        let cb = node_to_comm[b];
        if ca != cb {
            out.insert((ca, cb));
        }
    }
    out
}

fn coarsen_with_zoom(
    nodes: &[String],
    edges: &BTreeSet<(String, String)>,
    paths: &BTreeMap<String, PathBuf>,
    zoom: f64,
) -> CoarsenedGraph {
    if zoom <= 0.0 {
        return CoarsenedGraph {
            labels: vec![format!("codebase\n{} nodes", nodes.len())],
            edges: BTreeSet::new(),
        };
    }
    if zoom >= 1.0 {
        // Shouldn't be used; caller handles zoom==1 fast-path.
    }

    let target = target_node_count(nodes.len(), zoom);

    // Leiden sometimes returns very few (even 1) communities. For zoom values near 1, that would
    // incorrectly collapse the graph. If Leiden under-shoots the target, fall back to "start with
    // every node as its own community, then merge down".
    let initial = {
        let communities = leiden_partition(nodes, edges);
        if communities.len() < target {
            (0..nodes.len()).map(|i| vec![i]).collect()
        } else {
            communities
        }
    };
    let communities = merge_communities_to_target(nodes, edges, initial, target);
    let labels = build_cluster_labels(nodes, paths, &communities);
    let co_edges = build_cluster_edges(nodes, edges, &communities);

    CoarsenedGraph {
        labels,
        edges: co_edges,
    }
}

fn write_format_header(buf: &mut Vec<u8>, format: VizFormat) -> std::io::Result<()> {
    match format {
        VizFormat::Dot => { writeln!(buf, "digraph kiss {{")?; writeln!(buf, "  rankdir=LR;")?; writeln!(buf, "  node [shape=box];")?; }
        VizFormat::Mermaid => { writeln!(buf, "graph LR")?; }
        VizFormat::MarkdownMermaid => { writeln!(buf, "```mermaid")?; writeln!(buf, "graph LR")?; }
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

fn write_graph_for_format(buf: &mut Vec<u8>, graph: &DependencyGraph, prefix: &str, format: VizFormat) -> std::io::Result<()> {
    match format {
        VizFormat::Dot => write_graph_dot(buf, graph, prefix),
        VizFormat::Mermaid | VizFormat::MarkdownMermaid => write_graph_mermaid(buf, graph, prefix),
    }
}

fn write_coarsened_for_format(buf: &mut Vec<u8>, coarse: &CoarsenedGraph, format: VizFormat) -> std::io::Result<()> {
    match format {
        VizFormat::Dot => write_coarsened_dot(buf, coarse),
        VizFormat::Mermaid | VizFormat::MarkdownMermaid => write_coarsened_mermaid(buf, coarse),
    }
}

fn build_py_graph(py_files: &[PathBuf]) -> std::io::Result<DependencyGraph> {
    let results = parse_files(py_files).map_err(|e| std::io::Error::other(e.to_string()))?;
    let parsed: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
    Ok(build_dependency_graph(&parsed))
}

fn build_rs_graph(rs_files: &[PathBuf]) -> DependencyGraph {
    let results = parse_rust_files(rs_files);
    let parsed: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
    build_rust_dependency_graph(&parsed)
}

fn build_coarsened_graph(py_files: &[PathBuf], rs_files: &[PathBuf], zoom: f64) -> std::io::Result<CoarsenedGraph> {
    let mut all_nodes: BTreeSet<String> = BTreeSet::new();
    let mut all_edges: BTreeSet<(String, String)> = BTreeSet::new();
    let mut all_paths: BTreeMap<String, PathBuf> = BTreeMap::new();

    if !py_files.is_empty() {
        let (n, e, p) = collect_graph_nodes_and_edges(&build_py_graph(py_files)?, "py");
        all_nodes.extend(n); all_edges.extend(e); all_paths.extend(p);
    }
    if !rs_files.is_empty() {
        let (n, e, p) = collect_graph_nodes_and_edges(&build_rs_graph(rs_files), "rs");
        all_nodes.extend(n); all_edges.extend(e); all_paths.extend(p);
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
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "No source files found."));
    }

    let format = viz_format_for_path(out_path)?;
    let mut buf: Vec<u8> = Vec::new();
    write_format_header(&mut buf, format)?;

    if zoom >= 1.0 {
        if !py_files.is_empty() { write_graph_for_format(&mut buf, &build_py_graph(&py_files)?, "py", format)?; }
        if !rs_files.is_empty() { write_graph_for_format(&mut buf, &build_rs_graph(&rs_files), "rs", format)?; }
    } else {
        write_coarsened_for_format(&mut buf, &build_coarsened_graph(&py_files, &rs_files, zoom)?, format)?;
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
    fn test_gather_files_by_lang_empty_input() {
        let (py, rs) = kiss::discovery::gather_files_by_lang(&[], None, &[]);
        assert!(py.is_empty());
        assert!(rs.is_empty());
    }

    #[test]
    fn test_run_viz_errors_on_no_source_files() {
        let out_path = Path::new("does-not-matter.dot");
        let err = run_viz(out_path, &[], None, &[], 1.0).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("No source files"));
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
    fn test_target_node_count() {
        assert_eq!(target_node_count(1, 0.5), 1);
        assert_eq!(target_node_count(10, 0.0), 1);
        assert_eq!(target_node_count(10, 1.0), 10);
        assert_eq!(target_node_count(10, 0.5), 6);
    }

    #[test]
    fn test_build_node_index() {
        let nodes = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let idx = build_node_index(&nodes);
        assert_eq!(idx.get("a"), Some(&0));
        assert_eq!(idx.get("b"), Some(&1));
        assert_eq!(idx.get("c"), Some(&2));
    }

    #[test]
    fn test_build_cluster_labels() {
        let nodes = vec!["a".to_string(), "b".to_string()];
        let paths = BTreeMap::new();
        let communities = vec![vec![0, 1]];
        let labels = build_cluster_labels(&nodes, &paths, &communities);
        assert_eq!(labels.len(), 1);
        assert!(labels[0].contains('a'));
    }

    #[test]
    fn test_build_cluster_edges() {
        let nodes = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut edges = BTreeSet::new();
        edges.insert(("a".to_string(), "b".to_string()));
        let communities = vec![vec![0], vec![1, 2]];
        let result = build_cluster_edges(&nodes, &edges, &communities);
        assert!(result.contains(&(0, 1)));
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
    fn test_coarsened_graph_struct() {
        let cg = CoarsenedGraph { labels: vec!["a".into()], edges: BTreeSet::new() };
        assert_eq!(cg.labels.len(), 1);
    }

    #[test]
    fn test_write_coarsened_formats() {
        let cg = CoarsenedGraph { labels: vec!["node".into()], edges: BTreeSet::new() };
        let mut dot_out = Vec::new();
        write_coarsened_dot(&mut dot_out, &cg).unwrap();
        assert!(String::from_utf8(dot_out).unwrap().contains("c0"));
        let mut mmd_out = Vec::new();
        write_coarsened_mermaid(&mut mmd_out, &cg).unwrap();
        assert!(String::from_utf8(mmd_out).unwrap().contains("c0"));
    }

    #[test]
    fn test_coarsen_with_zoom() {
        let nodes: Vec<String> = vec!["a".into(), "b".into(), "c".into()];
        let mut edges = BTreeSet::new();
        edges.insert(("a".to_string(), "b".to_string()));
        let paths = BTreeMap::new();
        let cg = coarsen_with_zoom(&nodes, &edges, &paths, 0.5);
        assert!(!cg.labels.is_empty());
    }

    #[test]
    fn test_leiden_partition() {
        let nodes: Vec<String> = vec!["a".into(), "b".into(), "c".into()];
        let mut edges = BTreeSet::new();
        edges.insert(("a".to_string(), "b".to_string()));
        let communities = leiden_partition(&nodes, &edges);
        assert!(!communities.is_empty());
    }

    #[test]
    fn test_rebuild_intercommunity_weights() {
        let mut edges = BTreeSet::new();
        edges.insert(("a".to_string(), "b".to_string()));
        let mut node_index = HashMap::new();
        node_index.insert("a", 0);
        node_index.insert("b", 1);
        let node_to_comm = vec![0, 1]; // a in comm 0, b in comm 1
        let weights = rebuild_intercommunity_weights(&edges, &node_index, &node_to_comm);
        assert_eq!(weights.get(&(0, 1)), Some(&1));
    }

    #[test]
    fn test_merge_communities_to_target() {
        let nodes: Vec<String> = vec!["a".into(), "b".into(), "c".into()];
        let edges = BTreeSet::new();
        let communities = vec![vec![0], vec![1], vec![2]];
        let merged = merge_communities_to_target(&nodes, &edges, communities, 2);
        assert!(merged.len() <= 2);
    }

    #[test]
    fn test_assign_nodes_to_communities() {
        let communities = vec![vec![0, 1], vec![2]];
        let result = assign_nodes_to_communities(&communities, 3);
        assert_eq!(result, vec![0, 0, 1]);
    }

    #[test]
    fn test_find_best_merge_target() {
        let mut weights = BTreeMap::new();
        weights.insert((0, 1), 5);
        weights.insert((0, 2), 3);
        assert_eq!(find_best_merge_target(&weights, 0), Some(1));
        assert_eq!(find_best_merge_target(&weights, 3), None);
    }

    #[test]
    fn test_node_size_and_display() {
        let paths = BTreeMap::new();
        let (size, name) = node_size_and_display("test", &paths);
        assert_eq!(size, 0);
        assert_eq!(name, "test");
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
    fn test_build_py_graph() {
        // Empty list should produce empty graph
        let result = build_py_graph(&[]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_rs_graph() {
        // Empty list should produce empty graph
        let graph = build_rs_graph(&[]);
        assert!(graph.nodes.is_empty());
    }
}
