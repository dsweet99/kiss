use super::stress_break_kiss::parse;
use kiss::Config;
use kiss::graph::{DependencyGraph, analyze_graph};
use kiss::minhash::{MinHashSignature, compute_minhash, estimate_similarity, generate_shingles};
use std::fmt::Write as _;

#[test]
fn h4_fully_connected_graph_100_nodes() {
    let n = 100;
    let mut g = DependencyGraph::new();

    for i in 0..n {
        let from = format!("mod_{i}");
        g.get_or_create_node(&from);
        g.paths
            .insert(from.clone(), std::path::PathBuf::from(format!("{from}.py")));
        for j in 0..n {
            if i != j {
                g.add_dependency(&from, &format!("mod_{j}"));
            }
        }
    }

    let config = Config::python_defaults();
    let viols = analyze_graph(&g, &config);

    let metrics = g.module_metrics("mod_0");
    assert_eq!(
        metrics.fan_out,
        n - 1,
        "mod_0 should have fan_out = {}, got {}",
        n - 1,
        metrics.fan_out
    );
    assert_eq!(
        metrics.indirect_dependencies, 0,
        "mod_0: all deps are direct (fan_out = total_reachable), indirect should be 0 (got {})",
        metrics.indirect_dependencies
    );

    assert!(
        viols.iter().any(|v| v.metric == "cycle_size"),
        "Fully connected graph should have cycle violations"
    );
}

#[test]
fn h4_long_chain_graph_200_deep() {
    let n = 200;
    let mut g = DependencyGraph::new();
    for i in 0..n {
        let name = format!("mod_{i}");
        g.get_or_create_node(&name);
        g.paths
            .insert(name.clone(), std::path::PathBuf::from(format!("{name}.py")));
        if i > 0 {
            g.add_dependency(&format!("mod_{}", i - 1), &name);
        }
    }

    let metrics = g.module_metrics("mod_0");
    assert_eq!(
        metrics.indirect_dependencies,
        n - 2,
        "Head of chain has fan_out=1 and reaches {} nodes, indirect should be {} (got {})",
        n - 1,
        n - 2,
        metrics.indirect_dependencies
    );
    assert_eq!(
        metrics.dependency_depth,
        n - 1,
        "Longest chain from mod_0 should be {}",
        n - 1
    );

    let tail = g.module_metrics(&format!("mod_{}", n - 1));
    assert_eq!(tail.indirect_dependencies, 0);
    assert_eq!(tail.fan_in, 1);
}

#[test]
fn h5_same_file_two_paths_in_graph() {
    let mut g = DependencyGraph::new();
    g.get_or_create_node("utils");
    g.get_or_create_node("pkg.utils");
    g.paths
        .insert("utils".into(), std::path::PathBuf::from("src/utils.py"));
    g.paths
        .insert("pkg.utils".into(), std::path::PathBuf::from("src/utils.py"));

    g.get_or_create_node("main");
    g.paths
        .insert("main".into(), std::path::PathBuf::from("src/main.py"));
    g.add_dependency("main", "utils");

    let config = Config::python_defaults();
    let viols = analyze_graph(&g, &config);

    let orphan_viols: Vec<_> = viols
        .iter()
        .filter(|v| v.metric == "orphan_module")
        .collect();

    assert!(
        !orphan_viols.iter().any(|v| v.unit_name == "pkg.utils"),
        "Phantom orphan for 'pkg.utils' should be suppressed (same path as connected 'utils')"
    );
}

#[test]
fn h6_factory_closure_not_self_duplicate() {
    let code = "\
def mk_mk_likelihood(noise_transform_type, mk_covar_module):
    def _mk_likelihood(train_X, train_Y, train_Yvar):
        noise_transform = None
        if noise_transform_type is not None:
            noise_transform = get_noise_outcome_transform(
                noise_transform_type,
                train_X.shape[-2],
                m=train_Y.shape[-1],
                batch_shape=train_X.shape[:-2],
            )
        noise_model = SingleTaskGP(
            train_X=train_X,
            train_Y=train_Yvar,
            train_Yvar=train_Yvar.clone(),
            covar_module=mk_covar_module(train_X, train_Y, train_Yvar),
            outcome_transform=noise_transform,
        )
        return GaussianLikelihoodBase(HeteroskedasticNoise(noise_model))

    return _mk_likelihood
";

    let p = parse(code);
    let parsed_refs = vec![&p];
    let chunks = kiss::extract_chunks_for_duplication(&parsed_refs);

    let config = kiss::DuplicationConfig::default();
    let clusters = kiss::cluster_duplicates_from_chunks(&chunks, &config);

    assert!(
        clusters.is_empty(),
        "Factory function returning a closure should not be flagged as self-duplicate, \
         but got {} cluster(s): {:?}",
        clusters.len(),
        clusters
            .iter()
            .map(|c| c
                .chunks
                .iter()
                .map(|ch| format!("{}:{}-{}", ch.name, ch.start_line, ch.end_line))
                .collect::<Vec<_>>())
            .collect::<Vec<_>>()
    );
}

#[test]
fn h2_duplication_pipeline_with_near_identical_functions() {
    let mut code = String::new();
    for i in 0..20 {
        let _ = write!(
            code,
            "def func_{i}(data):\n    result = process(data, 42)\n    validated = check(result)\n    transformed = convert(validated)\n    output = finalize(transformed)\n    return output\n\n"
        );
    }

    let p = parse(&code);
    let parsed_refs = vec![&p];
    let chunks = kiss::extract_chunks_for_duplication(&parsed_refs);

    assert!(
        chunks.len() >= 15,
        "Expected at least 15 chunks from 20 near-identical functions, got {}",
        chunks.len()
    );

    let sigs: Vec<MinHashSignature> = chunks
        .iter()
        .map(|c| {
            let shingles = generate_shingles(&c.normalized, 3);
            compute_minhash(&shingles, 100)
        })
        .collect();

    if sigs.len() >= 2 {
        let sim = estimate_similarity(&sigs[0], &sigs[1]);
        assert!(
            sim > 0.9,
            "Identical bodies should have >90% similarity, got {sim}"
        );
    }

    let mut code_vary = String::new();
    for (name, n) in [("compute_a", "1"), ("compute_b", "9")] {
        let _ = write!(
            code_vary,
            "def {name}(data):\n    result = process(data, {n})\n    validated = check(result)\n    transformed = convert(validated)\n    output = finalize(transformed)\n    return output\n\n"
        );
    }
    let p2 = parse(&code_vary);
    let chunks2 = kiss::extract_chunks_for_duplication(&[&p2]);
    assert!(
        chunks2.len() >= 2,
        "numeric-only pair must yield extractable chunks, got {}",
        chunks2.len()
    );
    let config = kiss::DuplicationConfig::default();
    let pairs = kiss::detect_duplicates(&[&p2], &config);
    assert!(
        pairs.is_empty(),
        "numeric-only literal differences must not be duplicate pairs, got {pairs:?}"
    );

    let mut code_index = String::new();
    for i in 0..5 {
        let _ = write!(
            code_index,
            "def func_{i}(data):\n    result = process(data, {i})\n    validated = check(result)\n    transformed = convert(validated)\n    output = finalize(transformed)\n    return output\n\n"
        );
    }
    let p3 = parse(&code_index);
    let pairs_index = kiss::detect_duplicates(&[&p3], &config);
    assert!(
        pairs_index.is_empty(),
        "functions differing only by numeric literal index must not duplicate, got {pairs_index:?}"
    );
}
