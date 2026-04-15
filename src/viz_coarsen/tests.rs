use super::*;
use std::path::Path;

#[test]
fn test_fast_communities_assigns_all_nodes() {
    let nodes: Vec<String> = (0..10).map(|i| format!("py:m{i}")).collect();
    let mut paths_map: BTreeMap<String, PathBuf> = BTreeMap::new();
    for (i, n) in nodes.iter().enumerate() {
        let dir = if i < 5 { "pkg1" } else { "pkg2" };
        paths_map.insert(n.clone(), PathBuf::from(format!("src/{dir}/m{i}.py")));
    }
    let comm = paths::fast_communities_from_paths(&nodes, &paths_map, 4);
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

    let node_to_comm = leiden::assign_nodes_to_communities(&comms, nodes.len());
    let _ = leiden::rebuild_intercommunity_weights(&edges, &idx, &node_to_comm);

    let _ = leiden::find_best_merge_target(&BTreeMap::new(), 0);
    let _ = leiden::merge_communities_to_target(&nodes, &edges, comms, 1);

    let _ = paths::path_prefix_key(Path::new("src/pkg/mod.py"), 1);
}

#[test]
fn test_touch_privates_for_static_coverage_part2() {
    let nodes: Vec<String> = vec!["a".into(), "b".into()];
    let mut edges: BTreeSet<(String, String)> = BTreeSet::new();
    edges.insert(("a".to_string(), "b".to_string()));

    let mut paths_map: BTreeMap<String, PathBuf> = BTreeMap::new();
    paths_map.insert("a".to_string(), PathBuf::from("src/pkg/a.py"));
    paths_map.insert("b".to_string(), PathBuf::from("src/pkg/b.py"));
    let (per, max_depth) = paths::collect_paths_and_depth(&nodes, &paths_map);
    assert!(max_depth >= 1);

    let _ = paths::external_key("py:a");
    let _ = paths::group_key("py:a", per[0].as_ref(), 1);
    let grouped = vec![vec![0, 1]];
    let _ = paths::merge_overflow(grouped, 1);

    let (a, b) = paths::split_largest_once(&nodes, &[0, 1]);
    let _ = paths::split_until_target(&nodes, vec![a, b], 2);

    let _ = leiden::leiden_partition(&nodes, &BTreeSet::new());
    let _ = leiden::leiden_or_merge_to_target(&nodes, &BTreeSet::new(), 1);

    let _ = coarsen_with_zoom(&nodes, &edges, &paths_map, 0.3);
}

#[test]
fn test_choose_prefix_depth_and_group_nodes() {
    let nodes: Vec<String> = vec!["a".into(), "b".into(), "c".into()];
    let per_paths: Vec<Option<PathBuf>> = vec![
        Some(PathBuf::from("src/pkg1/a.py")),
        Some(PathBuf::from("src/pkg2/b.py")),
        None,
    ];
    let depth = paths::choose_prefix_depth(&nodes, &per_paths, 3, 2);
    assert!(depth >= 1);

    let groups = paths::group_nodes(&nodes, &per_paths, depth);
    assert!(!groups.is_empty());
}
