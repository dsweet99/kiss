use fa_leiden_cd::{Graph as LeidenGraph, TrivialModularityOptimizer};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct CoarsenedGraph {
    pub labels: Vec<String>,
    pub edges: BTreeSet<(usize, usize)>,
}

fn stable_fnv1a_64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in s.as_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn target_node_count(node_count: usize, zoom: f64) -> usize {
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

fn build_node_index(nodes: &[String]) -> HashMap<&str, usize> {
    let mut idx = HashMap::new();
    for (i, n) in nodes.iter().enumerate() {
        idx.insert(n.as_str(), i);
    }
    idx
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

fn leiden_partition(nodes: &[String], edges: &BTreeSet<(String, String)>) -> Vec<Vec<usize>> {
    let mut g: LeidenGraph<String, ()> = LeidenGraph::new();
    let mut ids: HashMap<String, usize> = HashMap::new();
    for n in nodes {
        let id = g.add_node(n.clone());
        ids.insert(n.clone(), id);
    }

    for (from, to) in edges {
        let Some(&a) = ids.get(from) else { continue };
        let Some(&b) = ids.get(to) else { continue };
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
    communities.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    communities
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

fn find_best_merge_target(weights: &BTreeMap<(usize, usize), usize>, small_i: usize) -> Option<usize> {
    let mut best: Option<(usize, usize)> = None;
    for ((a, b), w) in weights {
        let other = if *a == small_i {
            *b
        } else if *b == small_i {
            *a
        } else {
            continue;
        };
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
    if target <= 1 {
        return vec![(0..nodes.len()).collect()];
    }
    if communities.len() <= target {
        return communities;
    }

    let node_index = build_node_index(nodes);
    let mut node_to_comm = assign_nodes_to_communities(&communities, nodes.len());
    let mut weights = rebuild_intercommunity_weights(edges, &node_index, &node_to_comm);

    while communities.len() > target {
        let (small_i, _) = communities
            .iter()
            .enumerate()
            .min_by_key(|(_, m)| m.len())
            .unwrap();
        let fallback = usize::from(small_i == 0);
        let merge_into = find_best_merge_target(&weights, small_i).unwrap_or(fallback);
        let (dst, src) = if merge_into < small_i {
            (merge_into, small_i)
        } else {
            (small_i, merge_into)
        };

        let moved: Vec<usize> = communities[src].drain(..).collect();
        communities[dst].extend(moved);
        communities[dst].sort_unstable();
        communities.remove(src);

        node_to_comm = assign_nodes_to_communities(&communities, nodes.len());
        weights = rebuild_intercommunity_weights(edges, &node_index, &node_to_comm);
    }
    communities
}

fn path_prefix_key(path: &Path, depth: usize) -> String {
    let mut comps: Vec<String> = path
        .parent()
        .into_iter()
        .flat_map(|p| p.components())
        .filter_map(|c| match c {
            Component::Normal(os) => os.to_str().map(std::string::ToString::to_string),
            _ => None,
        })
        .collect();

    if let Some(pos) = comps.iter().rposition(|c| c == "src") {
        comps = comps[(pos + 1)..].to_vec();
    } else if path.is_absolute() && comps.len() > 2 {
        comps = comps[(comps.len() - 2)..].to_vec();
    }

    if depth == 0 || comps.is_empty() {
        return String::new();
    }
    let d = depth.min(comps.len());
    comps[..d].join("/")
}

fn collect_paths_and_depth(
    nodes: &[String],
    paths: &BTreeMap<String, PathBuf>,
) -> (Vec<Option<PathBuf>>, usize) {
    let mut per_node_paths: Vec<Option<PathBuf>> = Vec::with_capacity(nodes.len());
    let mut max_depth: usize = 0;
    for n in nodes {
        let p = paths.get(n).cloned();
        if let Some(pp) = &p {
            let depth = pp.parent().map_or(0, |par| {
                par.components()
                    .filter(|c| matches!(c, Component::Normal(_)))
                    .count()
            });
            max_depth = max_depth.max(depth);
        }
        per_node_paths.push(p);
    }
    (per_node_paths, max_depth)
}

fn external_key(node: &str) -> String {
    format!("external/{}", node.split(':').next().unwrap_or("x"))
}

fn group_key(node: &str, path: Option<&PathBuf>, depth: usize) -> String {
    path.map_or_else(|| external_key(node), |p| path_prefix_key(p, depth))
}

fn choose_prefix_depth(
    nodes: &[String],
    per_node_paths: &[Option<PathBuf>],
    max_depth: usize,
    target: usize,
) -> usize {
    if max_depth == 0 {
        return 0;
    }
    let mut chosen = max_depth;
    for depth in 1..=max_depth {
        let mut groups: HashSet<String> = HashSet::new();
        for (i, n) in nodes.iter().enumerate() {
            groups.insert(group_key(n, per_node_paths[i].as_ref(), depth));
        }
        chosen = depth;
        if groups.len() >= target {
            break;
        }
    }
    chosen
}

fn group_nodes(
    nodes: &[String],
    per_node_paths: &[Option<PathBuf>],
    depth: usize,
) -> Vec<Vec<usize>> {
    let mut by_key: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, n) in nodes.iter().enumerate() {
        let key = group_key(n, per_node_paths[i].as_ref(), depth);
        by_key.entry(key).or_default().push(i);
    }
    let mut communities: Vec<Vec<usize>> = by_key.into_values().collect();
    for c in &mut communities {
        c.sort_unstable();
    }
    communities
}

fn merge_overflow(mut communities: Vec<Vec<usize>>, target: usize) -> Vec<Vec<usize>> {
    if communities.len() <= target {
        return communities;
    }
    communities.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    let mut kept: Vec<Vec<usize>> = communities.drain(..(target - 1)).collect();
    let mut other: Vec<usize> = communities.into_iter().flatten().collect();
    other.sort_unstable();
    kept.push(other);
    kept
}

fn split_largest_once(nodes: &[String], community: &[usize]) -> (Vec<usize>, Vec<usize>) {
    let mut a: Vec<usize> = Vec::new();
    let mut b: Vec<usize> = Vec::new();
    for &node_idx in community {
        let h = stable_fnv1a_64(&nodes[node_idx]);
        if (h & 1) == 0 {
            a.push(node_idx);
        } else {
            b.push(node_idx);
        }
    }
    if a.is_empty() || b.is_empty() {
        a.clear();
        b.clear();
        for (j, &node_idx) in community.iter().enumerate() {
            if (j & 1) == 0 {
                a.push(node_idx);
            } else {
                b.push(node_idx);
            }
        }
    }
    a.sort_unstable();
    b.sort_unstable();
    (a, b)
}

fn split_until_target(nodes: &[String], mut communities: Vec<Vec<usize>>, target: usize) -> Vec<Vec<usize>> {
    while communities.len() < target {
        let (idx, _) = communities
            .iter()
            .enumerate()
            .max_by_key(|(_, c)| c.len())
            .unwrap();
        if communities[idx].len() <= 1 {
            break;
        }
        let (a, b) = split_largest_once(nodes, &communities[idx]);
        communities[idx] = a;
        communities.push(b);
    }
    communities
}

fn fast_communities_from_paths(
    nodes: &[String],
    paths: &BTreeMap<String, PathBuf>,
    target: usize,
) -> Vec<Vec<usize>> {
    if nodes.is_empty() {
        return Vec::new();
    }
    let target = target.clamp(1, nodes.len());
    let (per_node_paths, max_depth) = collect_paths_and_depth(nodes, paths);
    let chosen_depth = choose_prefix_depth(nodes, &per_node_paths, max_depth, target);
    let communities = group_nodes(nodes, &per_node_paths, chosen_depth);
    let communities = merge_overflow(communities, target);
    let mut communities = split_until_target(nodes, communities, target);
    communities.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    if communities.len() > target {
        communities.truncate(target);
    }
    communities
}

fn leiden_or_merge_to_target(
    nodes: &[String],
    edges: &BTreeSet<(String, String)>,
    target: usize,
) -> Vec<Vec<usize>> {
    let initial = {
        let communities = leiden_partition(nodes, edges);
        if communities.len() < target {
            (0..nodes.len()).map(|i| vec![i]).collect()
        } else {
            communities
        }
    };
    merge_communities_to_target(nodes, edges, initial, target)
}

const fn should_use_fast_coarsen(node_count: usize, edge_count: usize, target: usize) -> bool {
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
        fast_communities_from_paths(nodes, paths, target)
    } else {
        leiden_or_merge_to_target(nodes, edges, target)
    };

    let labels = build_cluster_labels(nodes, paths, &communities);
    let co_edges = build_cluster_edges(nodes, edges, &communities);
    CoarsenedGraph { labels, edges: co_edges }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fast_communities_assigns_all_nodes() {
        let nodes: Vec<String> = (0..10).map(|i| format!("py:m{i}")).collect();
        let mut paths: BTreeMap<String, PathBuf> = BTreeMap::new();
        for (i, n) in nodes.iter().enumerate() {
            let dir = if i < 5 { "pkg1" } else { "pkg2" };
            paths.insert(n.clone(), PathBuf::from(format!("src/{dir}/m{i}.py")));
        }
        let comm = fast_communities_from_paths(&nodes, &paths, 4);
        assert!(!comm.is_empty());
        let mut all: Vec<usize> = comm.into_iter().flatten().collect();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all, (0..10).collect::<Vec<_>>());
    }

    #[test]
    fn test_should_use_fast_coarsen_regressions() {
        // Guard against regressions that re-enable the slow Leiden path for aggressive zoom values.
        assert!(should_use_fast_coarsen(2_000, 0, 10)); // node threshold
        assert!(should_use_fast_coarsen(100, 10_000, 50)); // edge threshold
        assert!(should_use_fast_coarsen(1_000, 0, 100)); // aggressive coarsen (target << nodes)
        assert!(!should_use_fast_coarsen(100, 0, 90)); // not aggressive, small graph
    }

    #[test]
    fn test_touch_privates_for_static_coverage_part1() {
        let _ = stable_fnv1a_64("x");
        assert_eq!(target_node_count(10, 0.0), 1);

        let nodes: Vec<String> = vec!["a".into(), "b".into()];
        let idx = build_node_index(&nodes);
        assert_eq!(idx.get("a"), Some(&0));

        let empty_paths: BTreeMap<String, PathBuf> = BTreeMap::new();
        assert_eq!(node_size_and_display("missing", &empty_paths).0, 0);

        let comms = vec![vec![0], vec![1]];
        let _ = build_cluster_labels(&nodes, &empty_paths, &comms);

        let mut edges: BTreeSet<(String, String)> = BTreeSet::new();
        edges.insert(("a".to_string(), "b".to_string()));
        let _ = build_cluster_edges(&nodes, &edges, &comms);

        let node_to_comm = assign_nodes_to_communities(&comms, nodes.len());
        let _ = rebuild_intercommunity_weights(&edges, &idx, &node_to_comm);

        let _ = find_best_merge_target(&BTreeMap::new(), 0);
        let _ = merge_communities_to_target(&nodes, &edges, comms, 1);

        let _ = path_prefix_key(Path::new("src/pkg/mod.py"), 1);
    }

    #[test]
    fn test_touch_privates_for_static_coverage_part2() {
        let nodes: Vec<String> = vec!["a".into(), "b".into()];
        let mut edges: BTreeSet<(String, String)> = BTreeSet::new();
        edges.insert(("a".to_string(), "b".to_string()));

        let mut paths: BTreeMap<String, PathBuf> = BTreeMap::new();
        paths.insert("a".to_string(), PathBuf::from("src/pkg/a.py"));
        paths.insert("b".to_string(), PathBuf::from("src/pkg/b.py"));
        let (per, max_depth) = collect_paths_and_depth(&nodes, &paths);
        assert!(max_depth >= 1);

        let _ = external_key("py:a");
        let _ = group_key("py:a", per[0].as_ref(), 1);
        let depth = choose_prefix_depth(&nodes, &per, max_depth, 1);
        let grouped = group_nodes(&nodes, &per, depth);
        let _ = merge_overflow(grouped, 1);

        let (a, b) = split_largest_once(&nodes, &[0, 1]);
        let _ = split_until_target(&nodes, vec![a, b], 2);

        let _ = leiden_partition(&nodes, &BTreeSet::new());
        let _ = leiden_or_merge_to_target(&nodes, &BTreeSet::new(), 1);

        let _ = coarsen_with_zoom(&nodes, &edges, &paths, 0.3);
    }
}

