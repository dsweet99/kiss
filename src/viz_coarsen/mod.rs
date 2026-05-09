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

pub(crate) fn cluster_title(member_paths: &[Option<PathBuf>]) -> String {
    let known: Vec<&std::path::Path> = member_paths.iter().filter_map(|p| p.as_deref()).collect();
    if known.is_empty() {
        return "external".to_string();
    }

    let key_components: Vec<Vec<String>> = known
        .iter()
        .map(|p| {
            paths::path_prefix_key(p, usize::MAX)
                .split('/')
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        })
        .collect();

    let min_len = key_components.iter().map(Vec::len).min().unwrap_or(0);
    let mut common: Vec<String> = Vec::new();
    for i in 0..min_len {
        let first = &key_components[0][i];
        if key_components.iter().all(|c| &c[i] == first) {
            common.push(first.clone());
        } else {
            break;
        }
    }

    if common.is_empty() {
        "mixed".to_string()
    } else {
        common.join("/")
    }
}

pub(crate) fn build_cluster_labels(
    nodes: &[String],
    paths: &BTreeMap<String, PathBuf>,
    communities: &[Vec<usize>],
) -> Vec<String> {
    let mut labels = Vec::with_capacity(communities.len());
    for members in communities {
        let member_paths: Vec<Option<PathBuf>> = members
            .iter()
            .map(|&idx| paths.get(&nodes[idx]).cloned())
            .collect();
        let title = cluster_title(&member_paths);
        let count = members.len();
        let suffix = if count == 1 { "node" } else { "nodes" };
        // Single line: embedded `\n` inside `c0["..."]` breaks many Mermaid parsers (labels
        // split across source lines; rendered graph shows only generic ids like `c0`).
        labels.push(format!("{title} ({count} {suffix})"));
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
pub fn coarsen_with_target(
    nodes: &[String],
    edges: &BTreeSet<(String, String)>,
    paths: &BTreeMap<String, PathBuf>,
    target: usize,
) -> CoarsenedGraph {
    let target = target.max(1);
    if target == 1 {
        return CoarsenedGraph {
            labels: vec![format!("codebase ({} nodes)", nodes.len())],
            edges: BTreeSet::new(),
        };
    }

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

#[must_use]
pub fn coarsen_with_zoom(
    nodes: &[String],
    edges: &BTreeSet<(String, String)>,
    paths: &BTreeMap<String, PathBuf>,
    zoom: f64,
) -> CoarsenedGraph {
    let target = if zoom <= 0.0 {
        1
    } else {
        target_node_count(nodes.len(), zoom)
    };
    coarsen_with_target(nodes, edges, paths, target)
}
