//! kiss - Python code-quality metrics tool

// Modules
pub mod config;
pub mod counts;
pub mod discovery;
pub mod duplication;
pub mod graph;
pub mod parsing;
pub mod stats;
pub mod test_refs;
pub mod units;

// Re-export main types and functions for easy access
pub use config::{thresholds, Config};
pub use counts::{
    analyze_file, compute_class_metrics, compute_file_metrics, compute_function_metrics,
    ClassMetrics, FileMetrics, FunctionMetrics, Violation,
};
pub use discovery::find_python_files;
pub use duplication::{
    cluster_duplicates, detect_duplicates, extract_chunks_for_duplication, CodeChunk,
    DuplicateCluster, DuplicatePair, DuplicationConfig, MinHashSignature,
};
pub use graph::{
    analyze_graph, build_dependency_graph, compute_cyclomatic_complexity, CycleInfo,
    DependencyGraph, ModuleGraphMetrics,
};
pub use parsing::{create_parser, parse_file, parse_files, ParseError, ParsedFile};
pub use stats::{
    compute_summaries, format_stats_table, generate_config_toml, MetricStats, PercentileSummary,
};
pub use test_refs::{analyze_test_refs, is_test_file, CodeDefinition, TestRefAnalysis};
pub use units::{extract_code_units, CodeUnit, CodeUnitKind};

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn finds_python_files_in_test_directory() {
        let root = Path::new("tests/fake_code");
        let files = find_python_files(root);

        assert!(!files.is_empty(), "Should find Python files");
        assert!(
            files.iter().all(|p| p.extension().unwrap() == "py"),
            "All files should be .py"
        );
    }

    #[test]
    fn returns_empty_for_nonexistent_directory() {
        let root = Path::new("nonexistent_directory");
        let files = find_python_files(root);

        assert!(files.is_empty());
    }

    #[test]
    fn parses_python_file_successfully() {
        let mut parser = create_parser().expect("parser should initialize");
        let path = Path::new("tests/fake_code/clean_utils.py");
        let parsed = parse_file(&mut parser, path).expect("should parse");

        assert_eq!(parsed.path, path);
        assert!(!parsed.source.is_empty());
        assert!(!parsed.tree.root_node().has_error());
    }

    #[test]
    fn parses_all_test_files_without_errors() {
        let files = find_python_files(Path::new("tests/fake_code"));
        let results = parse_files(&files).expect("parser should initialize");

        for result in &results {
            let parsed = result.as_ref().expect("all files should parse");
            assert!(
                !parsed.tree.root_node().has_error(),
                "Parse errors in {}",
                parsed.path.display()
            );
        }
    }

    #[test]
    fn extracts_code_units_from_clean_utils() {
        let mut parser = create_parser().expect("parser should initialize");
        let parsed = parse_file(&mut parser, Path::new("tests/fake_code/clean_utils.py"))
            .expect("should parse");
        let units = extract_code_units(&parsed);

        // Should have: 1 module, 3 functions, 1 class, 4 methods
        let modules: Vec<_> = units.iter().filter(|u| u.kind == CodeUnitKind::Module).collect();
        let functions: Vec<_> = units.iter().filter(|u| u.kind == CodeUnitKind::Function).collect();
        let classes: Vec<_> = units.iter().filter(|u| u.kind == CodeUnitKind::Class).collect();
        let methods: Vec<_> = units.iter().filter(|u| u.kind == CodeUnitKind::Method).collect();

        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].name, "clean_utils");

        assert_eq!(functions.len(), 3);
        assert!(functions.iter().any(|f| f.name == "calculate_average"));
        assert!(functions.iter().any(|f| f.name == "clamp"));
        assert!(functions.iter().any(|f| f.name == "is_valid_email"));

        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].name, "Counter");

        // Counter has: __init__, increment, decrement, value (property)
        assert_eq!(methods.len(), 4);
        assert!(methods.iter().any(|m| m.name == "__init__"));
        assert!(methods.iter().any(|m| m.name == "increment"));
        assert!(methods.iter().any(|m| m.name == "decrement"));
        assert!(methods.iter().any(|m| m.name == "value"));
    }

    #[test]
    fn extracts_many_methods_from_god_class() {
        let mut parser = create_parser().expect("parser should initialize");
        let parsed = parse_file(&mut parser, Path::new("tests/fake_code/god_class.py"))
            .expect("should parse");
        let units = extract_code_units(&parsed);

        let classes: Vec<_> = units.iter().filter(|u| u.kind == CodeUnitKind::Class).collect();
        let methods: Vec<_> = units.iter().filter(|u| u.kind == CodeUnitKind::Method).collect();

        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].name, "ApplicationManager");

        // God class has many methods - should be > 20
        assert!(
            methods.len() > 20,
            "Expected >20 methods, got {}",
            methods.len()
        );
    }

    #[test]
    fn computes_file_metrics_for_god_class() {
        let mut parser = create_parser().expect("parser should initialize");
        let parsed = parse_file(&mut parser, Path::new("tests/fake_code/god_class.py"))
            .expect("should parse");

        let metrics = compute_file_metrics(&parsed);

        // god_class.py has many lines, 1 class, and many imports
        assert!(metrics.lines > 200, "Expected >200 lines, got {}", metrics.lines);
        assert_eq!(metrics.classes, 1);
        assert!(metrics.imports > 5, "Expected >5 imports, got {}", metrics.imports);
    }

    #[test]
    fn computes_class_metrics_for_god_class() {
        let mut parser = create_parser().expect("parser should initialize");
        let parsed = parse_file(&mut parser, Path::new("tests/fake_code/god_class.py"))
            .expect("should parse");

        // Find the class node
        let root = parsed.tree.root_node();
        let class_node = find_first_node_of_kind(root, "class_definition").expect("should find class");

        let metrics = compute_class_metrics(class_node);

        // ApplicationManager has >20 methods (violates threshold)
        assert!(
            metrics.methods > thresholds::METHODS_PER_CLASS,
            "Expected >{} methods, got {}",
            thresholds::METHODS_PER_CLASS,
            metrics.methods
        );
    }

    #[test]
    fn computes_function_metrics() {
        let mut parser = create_parser().expect("parser should initialize");
        let parsed = parse_file(&mut parser, Path::new("tests/fake_code/clean_utils.py"))
            .expect("should parse");

        // Find calculate_average function
        let root = parsed.tree.root_node();
        let func_node = find_first_node_of_kind(root, "function_definition").expect("should find function");

        let metrics = compute_function_metrics(func_node, &parsed.source);

        // calculate_average has 1 argument (numbers)
        assert_eq!(metrics.arguments, 1);
        // Has 2 statements (if and return)
        assert!(metrics.statements >= 2);
        // Has 1 return
        assert!(metrics.returns >= 1);
    }

    // Helper for tests
    fn find_first_node_of_kind<'a>(node: tree_sitter::Node<'a>, kind: &str) -> Option<tree_sitter::Node<'a>> {
        if node.kind() == kind {
            return Some(node);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = find_first_node_of_kind(child, kind) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn builds_dependency_graph() {
        let mut parser = create_parser().expect("parser should initialize");
        let parsed_god = parse_file(&mut parser, Path::new("tests/fake_code/god_class.py"))
            .expect("should parse");

        let parsed_files: Vec<&ParsedFile> = vec![&parsed_god];
        let graph = build_dependency_graph(&parsed_files);

        // god_class.py imports json, os, smtplib, sqlite3, datetime, etc.
        assert!(graph.nodes.len() > 1, "Should have multiple nodes in graph");

        // Check god_class module exists
        assert!(graph.nodes.contains_key("god_class"));

        // god_class has many dependencies (high fan-out)
        let metrics = graph.module_metrics("god_class");
        assert!(metrics.fan_out > 3, "Expected fan_out > 3, got {}", metrics.fan_out);
    }

    #[test]
    fn computes_cyclomatic_complexity() {
        let mut parser = create_parser().expect("parser should initialize");
        let parsed = parse_file(&mut parser, Path::new("tests/fake_code/deeply_nested.py"))
            .expect("should parse");

        // Find a function with lots of branches
        let root = parsed.tree.root_node();
        let func_node = find_first_node_of_kind(root, "function_definition").expect("should find function");

        let complexity = compute_cyclomatic_complexity(func_node);

        // deeply_nested.py has high complexity functions
        assert!(complexity > 5, "Expected complexity > 5, got {}", complexity);
    }

    #[test]
    fn detects_duplicate_code() {
        let mut parser = create_parser().expect("parser should initialize");
        let parsed = parse_file(&mut parser, Path::new("tests/fake_code/user_service.py"))
            .expect("should parse");

        let parsed_files: Vec<&ParsedFile> = vec![&parsed];
        let duplicates = detect_duplicates(&parsed_files, &DuplicationConfig::default());

        // user_service.py has create_user and create_admin which are similar
        assert!(
            !duplicates.is_empty(),
            "Should detect duplicates in user_service.py"
        );

        // The similarity should be high (>70%)
        assert!(
            duplicates[0].similarity > 0.7,
            "Expected similarity > 0.7, got {}",
            duplicates[0].similarity
        );
    }
}
