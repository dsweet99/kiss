//! Integration tests for the kiss library API

use kiss::*;
use std::path::Path;

#[test]
fn finds_python_files_in_test_directory() {
    let root = Path::new("tests/fake_python");
    let files = find_python_files(root);
    assert!(!files.is_empty(), "Should find Python files");
    assert!(files.iter().all(|p| p.extension().unwrap() == "py"), "All files should be .py");
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
    let path = Path::new("tests/fake_python/clean_utils.py");
    let parsed = parse_file(&mut parser, path).expect("should parse");
    assert_eq!(parsed.path, path);
    assert!(!parsed.source.is_empty());
    assert!(!parsed.tree.root_node().has_error());
}

#[test]
fn parses_all_test_files_without_errors() {
    let files = find_python_files(Path::new("tests/fake_python"));
    let results = parse_files(&files).expect("parser should initialize");
    for result in &results {
        let parsed = result.as_ref().expect("all files should parse");
        assert!(!parsed.tree.root_node().has_error(), "Parse errors in {}", parsed.path.display());
    }
}

#[test]
fn extracts_code_units_from_clean_utils() {
    let mut parser = create_parser().expect("parser should initialize");
    let parsed = parse_file(&mut parser, Path::new("tests/fake_python/clean_utils.py")).expect("should parse");
    let units = extract_code_units(&parsed);

    let modules: Vec<_> = units.iter().filter(|u| u.kind == CodeUnitKind::Module).collect();
    let functions: Vec<_> = units.iter().filter(|u| u.kind == CodeUnitKind::Function).collect();
    let classes: Vec<_> = units.iter().filter(|u| u.kind == CodeUnitKind::Class).collect();
    let methods: Vec<_> = units.iter().filter(|u| u.kind == CodeUnitKind::Method).collect();

    assert_eq!(modules.len(), 1);
    assert_eq!(modules[0].name, "clean_utils");
    assert_eq!(functions.len(), 3);
    assert!(functions.iter().any(|f| f.name == "calculate_average"));
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].name, "Counter");
    assert_eq!(methods.len(), 4);
}

#[test]
fn extracts_many_methods_from_god_class() {
    let mut parser = create_parser().expect("parser should initialize");
    let parsed = parse_file(&mut parser, Path::new("tests/fake_python/god_class.py")).expect("should parse");
    let units = extract_code_units(&parsed);
    let methods: Vec<_> = units.iter().filter(|u| u.kind == CodeUnitKind::Method).collect();
    assert!(methods.len() > 20, "Expected >20 methods, got {}", methods.len());
}

#[test]
fn computes_file_metrics_for_god_class() {
    let mut parser = create_parser().expect("parser should initialize");
    let parsed = parse_file(&mut parser, Path::new("tests/fake_python/god_class.py")).expect("should parse");
    let metrics = compute_file_metrics(&parsed);
    assert!(metrics.lines > 200, "Expected >200 lines, got {}", metrics.lines);
    assert_eq!(metrics.classes, 1);
    assert!(metrics.imports > 5, "Expected >5 imports, got {}", metrics.imports);
}

#[test]
fn computes_class_metrics_for_god_class() {
    let mut parser = create_parser().expect("parser should initialize");
    let parsed = parse_file(&mut parser, Path::new("tests/fake_python/god_class.py")).expect("should parse");
    let class_node = find_first_node_of_kind(parsed.tree.root_node(), "class_definition").expect("should find class");
    let metrics = compute_class_metrics(class_node);
    assert!(metrics.methods > thresholds::METHODS_PER_CLASS);
}

#[test]
fn computes_function_metrics() {
    let mut parser = create_parser().expect("parser should initialize");
    let parsed = parse_file(&mut parser, Path::new("tests/fake_python/clean_utils.py")).expect("should parse");
    let func_node = find_first_node_of_kind(parsed.tree.root_node(), "function_definition").expect("should find function");
    let metrics = compute_function_metrics(func_node, &parsed.source);
    assert_eq!(metrics.arguments, 1);
    assert!(metrics.statements >= 2);
    assert!(metrics.returns >= 1);
}

fn find_first_node_of_kind<'a>(node: tree_sitter::Node<'a>, kind: &str) -> Option<tree_sitter::Node<'a>> {
    if node.kind() == kind { return Some(node); }
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
    let parsed_god = parse_file(&mut parser, Path::new("tests/fake_python/god_class.py")).expect("should parse");
    let parsed_files: Vec<&ParsedFile> = vec![&parsed_god];
    let graph = build_dependency_graph(&parsed_files);
    assert!(graph.nodes.len() > 1, "Should have multiple nodes in graph");
    assert!(graph.nodes.contains_key("god_class"));
    let metrics = graph.module_metrics("god_class");
    assert!(metrics.fan_out > 3, "Expected fan_out > 3, got {}", metrics.fan_out);
}

#[test]
fn computes_cyclomatic_complexity() {
    let mut parser = create_parser().expect("parser should initialize");
    let parsed = parse_file(&mut parser, Path::new("tests/fake_python/deeply_nested.py")).expect("should parse");
    let func_node = find_first_node_of_kind(parsed.tree.root_node(), "function_definition").expect("should find function");
    let complexity = compute_cyclomatic_complexity(func_node);
    assert!(complexity > 5, "Expected complexity > 5, got {}", complexity);
}

#[test]
fn detects_duplicate_code() {
    let mut parser = create_parser().expect("parser should initialize");
    let parsed = parse_file(&mut parser, Path::new("tests/fake_python/user_service.py")).expect("should parse");
    let parsed_files: Vec<&ParsedFile> = vec![&parsed];
    let duplicates = detect_duplicates(&parsed_files, &DuplicationConfig::default());
    assert!(!duplicates.is_empty(), "Should detect duplicates in user_service.py");
    assert!(duplicates[0].similarity > 0.7, "Expected similarity > 0.7, got {}", duplicates[0].similarity);
}

#[test]
fn handles_empty_python_file() {
    use tempfile::TempDir;
    use std::fs;

    let tmp = TempDir::new().unwrap();
    let empty_py = tmp.path().join("empty.py");
    fs::write(&empty_py, "").unwrap();

    let mut parser = create_parser().expect("parser should initialize");
    let parsed = parse_file(&mut parser, &empty_py).expect("should parse empty file");
    assert_eq!(parsed.source, "");
    let units = extract_code_units(&parsed);
    assert!(units.len() <= 1);
    let file_metrics = compute_file_metrics(&parsed);
    assert_eq!(file_metrics.lines, 0);
}

#[test]
fn handles_empty_rust_file() {
    use tempfile::NamedTempFile;
    use std::io::Write;

    let mut tmp = NamedTempFile::with_suffix(".rs").unwrap();
    write!(tmp, "").unwrap();
    let parsed = parse_rust_file(tmp.path()).expect("should parse empty Rust file");
    assert_eq!(parsed.source, "");
    assert!(parsed.ast.items.is_empty());
}

#[test]
fn analyze_empty_python_file_no_violations() {
    use tempfile::TempDir;
    use std::fs;

    let tmp = TempDir::new().unwrap();
    let empty_py = tmp.path().join("empty.py");
    fs::write(&empty_py, "").unwrap();

    let mut parser = create_parser().expect("parser should initialize");
    let parsed = parse_file(&mut parser, &empty_py).expect("should parse");
    let violations = analyze_file(&parsed, &Config::default());
    assert!(violations.is_empty());
}

#[test]
fn analyze_empty_rust_file_no_violations() {
    use tempfile::NamedTempFile;
    use std::io::Write;

    let mut tmp = NamedTempFile::with_suffix(".rs").unwrap();
    write!(tmp, "").unwrap();
    let parsed = parse_rust_file(tmp.path()).expect("should parse");
    let violations = analyze_rust_file(&parsed, &Config::default());
    assert!(violations.is_empty());
}

