use crate::analyze_cache::fnv1a64;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

mod leiden;
mod paths;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone)]
pub struct CoarsenedGraph {
    pub labels: Vec<String>,
    pub edges: BTreeSet<(usize, usize)>,
}

pub(crate) fn stable_fnv1a_64(s: &str) -> u64 {
    fnv1a64(0xcbf2_9ce4_8422_2325, s.as_bytes())
}

pub(crate) fn target_node_count(node_count: usize, zoom: f64) -> usize {
    if node_count <= 1 {
        return 1;
    }
    let z = zoom.clamp(0.0, 1.0);
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

pub(crate) fn build_node_index(nodes: &[String]) -> HashMap<&str, usize> {
    let mut idx = HashMap::new();
    for (i, n) in nodes.iter().enumerate() {
        idx.insert(n.as_str(), i);
    }
    idx
}

pub(crate) fn node_size_and_display(
    name: &str,
    paths: &BTreeMap<String, PathBuf>,
) -> (u64, String) {
    paths.get(name).map_or_else(
        || (0, name.to_string()),
        |p| {
            let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
            (size, p.display().to_string())
        },
    )
}

pub(crate) fn build_cluster_labels(
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
                if i > 0 {
                    label.push('\n');
                }
                label.push_str(&s);
            }
        } else {
            for (i, (_, s)) in sized.into_iter().take(3).enumerate() {
                if i > 0 {
                    label.push('\n');
                }
                label.push_str(&s);
            }
            let _ = write!(label, "\n[{} more]", total - 3);
        }
        labels.push(label);
    }
    labels
}

pub(crate) fn build_cluster_edges(
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

pub(crate) const fn should_use_fast_coarsen(
    node_count: usize,
    edge_count: usize,
    target: usize,
) -> bool {
    let aggressive_coarsen = target.saturating_mul(2) < node_count;
    aggressive_coarsen || node_count >= 1_500 || edge_count >= 7_500
}

#[must_use]
pub fn coarsen_with_zoom(
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
        // Caller should handle zoom==1 fast-path.
    }

    let target = target_node_count(nodes.len(), zoom);

    let use_fast = should_use_fast_coarsen(nodes.len(), edges.len(), target);

    let communities = if use_fast {
        paths::fast_communities_from_paths(nodes, paths, target)
    } else {
        leiden::leiden_or_merge_to_target(nodes, edges, target)
    };

    let labels = build_cluster_labels(nodes, paths, &communities);
    let co_edges = build_cluster_edges(nodes, edges, &communities);
    CoarsenedGraph {
        labels,
        edges: co_edges,
    }
}
