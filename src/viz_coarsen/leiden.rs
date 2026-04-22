use super::build_node_index;
use fa_leiden_cd::{Graph as LeidenGraph, TrivialModularityOptimizer};
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub(super) fn leiden_partition(
    nodes: &[String],
    edges: &BTreeSet<(String, String)>,
) -> Vec<Vec<usize>> {
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

pub(super) fn assign_nodes_to_communities(
    communities: &[Vec<usize>],
    node_count: usize,
) -> Vec<usize> {
    let mut node_to_comm = vec![0; node_count];
    for (ci, members) in communities.iter().enumerate() {
        for &m in members {
            node_to_comm[m] = ci;
        }
    }
    node_to_comm
}

pub(super) fn rebuild_intercommunity_weights(
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

pub(super) fn find_best_merge_target(
    weights: &BTreeMap<(usize, usize), usize>,
    small_i: usize,
) -> Option<usize> {
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

pub(super) fn merge_communities_to_target(
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

pub(super) fn leiden_or_merge_to_target(
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
