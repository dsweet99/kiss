use super::stable_fnv1a_64;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

pub(super) fn path_prefix_key(path: &Path, depth: usize) -> String {
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

pub(super) fn collect_paths_and_depth(
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

pub(super) fn external_key(node: &str) -> String {
    format!("external/{}", node.split(':').next().unwrap_or("x"))
}

pub(super) fn group_key(node: &str, path: Option<&PathBuf>, depth: usize) -> String {
    path.map_or_else(|| external_key(node), |p| path_prefix_key(p, depth))
}

pub(crate) fn choose_prefix_depth(
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

pub(crate) fn group_nodes(
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

pub(super) fn merge_overflow(mut communities: Vec<Vec<usize>>, target: usize) -> Vec<Vec<usize>> {
    assert!(target >= 1, "merge_overflow requires target >= 1");
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

pub(super) fn split_largest_once(
    nodes: &[String],
    community: &[usize],
) -> (Vec<usize>, Vec<usize>) {
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

pub(super) fn split_until_target(
    nodes: &[String],
    mut communities: Vec<Vec<usize>>,
    target: usize,
) -> Vec<Vec<usize>> {
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

pub(super) fn fast_communities_from_paths(
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
