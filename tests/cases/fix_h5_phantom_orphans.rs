use kiss::Config;
use kiss::graph::{DependencyGraph, analyze_graph};

/// Bug H5: If the same file path appears under two different module names
/// in the dependency graph, `analyze_graph` should deduplicate by path
/// and not produce phantom orphan violations.
#[test]
fn same_path_two_module_names_no_phantom_orphan() {
    let mut g = DependencyGraph::new();

    // Same file registered under two qualified names
    let path = std::path::PathBuf::from("src/utils.py");
    g.get_or_create_node("utils");
    g.paths.insert("utils".into(), path.clone());
    g.get_or_create_node("pkg.utils");
    g.paths.insert("pkg.utils".into(), path);

    // Another module that imports "utils" (only one of the two names)
    g.get_or_create_node("main");
    g.paths
        .insert("main".into(), std::path::PathBuf::from("src/main.py"));
    g.add_dependency("main", "utils");

    let viols = analyze_graph(&g, &Config::python_defaults(), true);

    // "pkg.utils" should NOT be flagged as orphan since it's the same file as "utils"
    let orphan_viols: Vec<_> = viols
        .iter()
        .filter(|v| v.metric == "orphan_module")
        .collect();
    assert!(
        orphan_viols.is_empty(),
        "No orphan violations expected when duplicate paths exist, but got: {:?}",
        orphan_viols
            .iter()
            .map(|v| &v.unit_name)
            .collect::<Vec<_>>()
    );
}

#[test]
fn different_paths_still_flags_real_orphan() {
    let mut g = DependencyGraph::new();

    g.get_or_create_node("utils");
    g.paths
        .insert("utils".into(), std::path::PathBuf::from("src/utils.py"));
    g.get_or_create_node("orphan");
    g.paths
        .insert("orphan".into(), std::path::PathBuf::from("src/orphan.py"));
    g.get_or_create_node("main");
    g.paths
        .insert("main".into(), std::path::PathBuf::from("src/main.py"));
    g.add_dependency("main", "utils");

    let viols = analyze_graph(&g, &Config::python_defaults(), true);

    let orphan_viols: Vec<_> = viols
        .iter()
        .filter(|v| v.metric == "orphan_module")
        .collect();
    assert!(
        orphan_viols.iter().any(|v| v.unit_name == "orphan"),
        "Real orphan should still be flagged"
    );
}
