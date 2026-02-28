use kiss::graph::{DependencyGraph, analyze_graph};
use kiss::minhash::{
    MinHashSignature, compute_minhash, estimate_similarity, generate_shingles, normalize_code,
};
use kiss::parsing::{create_parser, parse_file};
use kiss::py_metrics::{compute_file_metrics, compute_function_metrics};
use kiss::{Config, extract_chunks_for_duplication};
use std::fmt::Write as _;
use std::io::Write;

fn parse(code: &str) -> kiss::ParsedFile {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    write!(tmp, "{code}").unwrap();
    parse_file(&mut create_parser().unwrap(), tmp.path()).unwrap()
}

fn get_func_node(p: &kiss::ParsedFile) -> tree_sitter::Node<'_> {
    p.tree.root_node().child(0).unwrap()
}

// ═══════════════════════════════════════════════════════════════
// H1: Partial parse errors — tree-sitter ERROR nodes silently
//     corrupt metrics instead of being detected.
// ═══════════════════════════════════════════════════════════════

#[test]
fn h1_syntax_error_in_function_body_still_produces_metrics() {
    // Missing colon after `if True` — tree-sitter recovers but AST is mangled.
    let code = "def foo():\n    x = 1\n    if True\n        y = 2\n    return x, y\n";
    let p = parse(code);
    let root = p.tree.root_node();

    // Check if tree-sitter produced any ERROR nodes
    let has_error = has_error_node(root);
    assert!(
        has_error,
        "Expected tree-sitter to produce ERROR nodes for broken syntax"
    );

    // kiss still computes metrics on this broken AST — verify it doesn't panic
    let func = get_func_node(&p);
    let m = compute_function_metrics(func, &p.source);

    // The real question: are these metrics *trustworthy*?
    // With the broken `if True` (no colon), tree-sitter may misparse the body.
    // Statements could be undercounted if `y = 2` lands inside an ERROR node.
    // We just verify it doesn't panic and returns *something*.
    assert!(m.statements >= 1, "Should have at least 1 statement");
}

#[test]
fn h1_error_node_in_return_corrupts_return_value_count() {
    // Return statement with syntax error: `return (a b)` — missing comma
    let code = "def foo():\n    return (a b)\n";
    let p = parse(code);

    let has_error = has_error_node(p.tree.root_node());
    assert!(has_error, "Expected ERROR node for `return (a b)`");

    let func = get_func_node(&p);
    let m = compute_function_metrics(func, &p.source);
    // kiss should ideally report 0 or flag the error, not silently miscount.
    // This test documents the current behavior.
    let _ = m.max_return_values;
}

#[test]
fn h1_unclosed_string_corrupts_entire_function() {
    // Unclosed triple-quote swallows subsequent code
    let code = "def foo():\n    x = '''\n    y = 1\n    return y\n\ndef bar():\n    return 1\n";
    let p = parse(code);
    let fm = compute_file_metrics(&p);

    // The unclosed string may swallow `bar` — file metrics may undercount functions
    // This test documents the behavior rather than asserting correctness.
    let _ = fm;
}

fn has_error_node(node: tree_sitter::Node) -> bool {
    if node.is_error() || node.is_missing() {
        return true;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if has_error_node(child) {
            return true;
        }
    }
    false
}

// ═══════════════════════════════════════════════════════════════
// H2: Normalization collisions — functions differing only in
//     numeric literals are falsely flagged as duplicates.
// ═══════════════════════════════════════════════════════════════

#[test]
fn h2_numeric_normalization_creates_false_duplicates() {
    // Two functions with completely different numeric constants
    // but identical structure should normalize identically.
    let code_a = "result = compute(123, 456, 789)\nif result > 100:\n    return result * 2\n";
    let code_b = "result = compute(999, 111, 222)\nif result > 500:\n    return result * 3\n";

    let norm_a = normalize_code(code_a);
    let norm_b = normalize_code(code_b);

    // After normalization, digits → N, so these become identical
    assert_eq!(
        norm_a, norm_b,
        "Numeric-only differences should normalize identically"
    );

    // Therefore shingles and minhash will be identical → 100% similarity
    let shingles_a = generate_shingles(&norm_a, 3);
    let shingles_b = generate_shingles(&norm_b, 3);
    let sig_a = compute_minhash(&shingles_a, 100);
    let sig_b = compute_minhash(&shingles_b, 100);
    let sim = estimate_similarity(&sig_a, &sig_b);

    assert!(
        (sim - 1.0).abs() < f64::EPSILON,
        "Functions differing only in numbers should be 100% similar, got {sim}"
    );
}

#[test]
fn h2_single_variable_rename_drops_similarity() {
    // Two genuinely duplicated functions where one variable is renamed.
    // With shingle_size=3, each renamed token poisons 3 shingles.
    let code_a =
        "x = get_data()\ny = transform(x)\nz = validate(y)\nresult = process(z)\nreturn result\n";
    let code_b =
        "x = get_data()\ny = transform(x)\nz = validate(y)\noutput = process(z)\nreturn output\n";

    let norm_a = normalize_code(code_a);
    let norm_b = normalize_code(code_b);

    let shingles_a = generate_shingles(&norm_a, 3);
    let shingles_b = generate_shingles(&norm_b, 3);
    let sig_a = compute_minhash(&shingles_a, 100);
    let sig_b = compute_minhash(&shingles_b, 100);
    let sim = estimate_similarity(&sig_a, &sig_b);

    // These are clearly duplicates, but a single rename may push similarity below 0.7
    assert!(
        sim > 0.5,
        "Near-duplicate with single rename should still have >50% similarity, got {sim}"
    );
}

// ═══════════════════════════════════════════════════════════════
// H3: Megafunction — large generated functions stress the
//     metric walkers and duplication pipeline.
// ═══════════════════════════════════════════════════════════════

#[test]
fn h3_function_with_1000_statements() {
    let mut code = String::from("def mega():\n");
    for i in 0..1000 {
        let _ = writeln!(code, "    x_{i} = {i}");
    }
    code.push_str("    return x_0\n");

    let p = parse(&code);
    let func = get_func_node(&p);
    let m = compute_function_metrics(func, &p.source);

    assert_eq!(
        m.statements, 1001,
        "Should count 1000 assignments + 1 return"
    );
    assert!(
        m.local_variables >= 1000,
        "Should track at least 1000 local variables, got {}",
        m.local_variables
    );
}

#[test]
fn h3_deeply_nested_indentation() {
    // 50 levels of nesting — tests the indentation walker
    let mut code = String::from("def deep():\n");
    let mut indent = String::from("    ");
    for i in 0..50 {
        let _ = writeln!(code, "{indent}if x_{i}:");
        indent.push_str("    ");
    }
    let _ = writeln!(code, "{indent}return 1");

    let p = parse(&code);
    let func = get_func_node(&p);
    let m = compute_function_metrics(func, &p.source);

    assert_eq!(
        m.max_indentation, 51,
        "Should report 51 levels of indentation (1 base + 50 ifs)"
    );
}

#[test]
fn h3_function_with_100_return_statements() {
    let mut code = String::from("def many_returns(x):\n");
    for i in 0..100 {
        let _ = writeln!(code, "    if x == {i}:\n        return {i}");
    }

    let p = parse(&code);
    let func = get_func_node(&p);
    let m = compute_function_metrics(func, &p.source);

    assert_eq!(m.returns, 100, "Should count 100 return statements");
}

#[test]
fn h3_file_with_500_functions() {
    let mut code = String::new();
    for i in 0..500 {
        let _ = write!(code, "def f_{i}():\n    return {i}\n\n");
    }

    let p = parse(&code);
    let fm = compute_file_metrics(&p);
    assert_eq!(fm.functions, 500, "Should count 500 functions");
}

// ═══════════════════════════════════════════════════════════════
// H4: Fully-connected import graph — tests graph analysis
//     performance and correctness with dense graphs.
// ═══════════════════════════════════════════════════════════════

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
    let viols = analyze_graph(&g, &config, true);

    let metrics = g.module_metrics("mod_0");
    assert_eq!(
        metrics.fan_out,
        n - 1,
        "mod_0 should have fan_out = {}, got {}",
        n - 1,
        metrics.fan_out
    );
    // Fully connected: every node is a direct neighbor, so indirect = 0
    assert_eq!(
        metrics.indirect_dependencies, 0,
        "mod_0: all deps are direct (fan_out = total_reachable), indirect should be 0 (got {})",
        metrics.indirect_dependencies
    );

    // Should produce cycle violations (one giant SCC)
    assert!(
        viols.iter().any(|v| v.metric == "cycle_size"),
        "Fully connected graph should have cycle violations"
    );
}

#[test]
fn h4_long_chain_graph_200_deep() {
    // Linear chain: mod_0 → mod_1 → ... → mod_199
    // Tests dependency_depth calculation
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
    // Linear chain: fan_out=1, total_reachable=n-1, indirect = n-2
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

// ═══════════════════════════════════════════════════════════════
// H5: Symlink / duplicate path — same file discovered under
//     two different paths creates phantom graph nodes.
// ═══════════════════════════════════════════════════════════════

#[test]
fn h5_same_file_two_paths_in_graph() {
    // Simulates what happens when the same module appears under two names
    let mut g = DependencyGraph::new();
    g.get_or_create_node("utils");
    g.get_or_create_node("pkg.utils"); // same file, different qualified name
    g.paths
        .insert("utils".into(), std::path::PathBuf::from("src/utils.py"));
    g.paths
        .insert("pkg.utils".into(), std::path::PathBuf::from("src/utils.py"));

    // Module that imports "utils" — creates edge to one name but not the other
    g.get_or_create_node("main");
    g.paths
        .insert("main".into(), std::path::PathBuf::from("src/main.py"));
    g.add_dependency("main", "utils");

    let config = Config::python_defaults();
    let viols = analyze_graph(&g, &config, true);

    // After fix: "pkg.utils" shares a path with "utils" (which has edges),
    // so it should NOT be flagged as orphan.
    let orphan_viols: Vec<_> = viols
        .iter()
        .filter(|v| v.metric == "orphan_module")
        .collect();

    assert!(
        !orphan_viols.iter().any(|v| v.unit_name == "pkg.utils"),
        "Phantom orphan for 'pkg.utils' should be suppressed (same path as connected 'utils')"
    );
}

// ═══════════════════════════════════════════════════════════════
// H6: Factory-closure self-duplication — a factory function
//     returning a closure should NOT be flagged as a duplicate
//     of its own inner function.
// ═══════════════════════════════════════════════════════════════

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
    let chunks = extract_chunks_for_duplication(&parsed_refs);

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

// ═══════════════════════════════════════════════════════════════
// Bonus: Duplication pipeline with many similar small functions
// ═══════════════════════════════════════════════════════════════

#[test]
fn h2_duplication_pipeline_with_near_identical_functions() {
    // 20 functions that differ only by a number — all should cluster together
    let mut code = String::new();
    for i in 0..20 {
        let _ = write!(
            code,
            "def func_{i}(data):\n    result = process(data, {i})\n    validated = check(result)\n    transformed = convert(validated)\n    output = finalize(transformed)\n    return output\n\n"
        );
    }

    let p = parse(&code);
    let parsed_refs = vec![&p];
    let chunks = extract_chunks_for_duplication(&parsed_refs);

    // All 20 functions should produce chunks (they have >= 5 lines)
    assert!(
        chunks.len() >= 15,
        "Expected at least 15 chunks from 20 near-identical functions, got {}",
        chunks.len()
    );

    // Compute signatures and check pairwise similarity
    let sigs: Vec<MinHashSignature> = chunks
        .iter()
        .map(|c| {
            let norm = normalize_code(&c.normalized);
            let shingles = generate_shingles(&norm, 3);
            compute_minhash(&shingles, 100)
        })
        .collect();

    // All pairs should have very high similarity (numeric-only diffs)
    if sigs.len() >= 2 {
        let sim = estimate_similarity(&sigs[0], &sigs[1]);
        assert!(
            sim > 0.9,
            "Near-identical functions should have >90% similarity, got {sim}"
        );
    }
}
