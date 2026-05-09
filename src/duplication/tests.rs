use super::*;
use crate::parsing::{create_parser, parse_file};
use crate::rust_parsing::parse_rust_file;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

#[test]
fn test_structs_and_helpers() {
    let c = DuplicationConfig::default();
    assert!(c.shingle_size > 0);
    let chunk = CodeChunk {
        file: "f.py".into(),
        name: "foo".into(),
        start_line: 1,
        end_line: 10,
        normalized: "x".into(),
    };
    assert_eq!(clustering::chunk_key(&chunk).0, PathBuf::from("f.py"));
    let mut uf = clustering::UnionFind::new(5);
    uf.union(0, 1);
    assert_eq!(uf.find(0), uf.find(1));
    let c1 = CodeChunk {
        file: "a.py".into(),
        name: "f".into(),
        start_line: 1,
        end_line: 5,
        normalized: "x".into(),
    };
    let c2 = CodeChunk {
        file: "b.py".into(),
        name: "g".into(),
        start_line: 1,
        end_line: 5,
        normalized: "x".into(),
    };
    let _ = DuplicatePair {
        chunk1: c1.clone(),
        chunk2: c2,
        similarity: 0.9,
    };
    let _ = DuplicateCluster {
        chunks: vec![c1.clone()],
        avg_similarity: 0.8,
    };
    let chunks = vec![
        c1,
        CodeChunk {
            file: "b.py".into(),
            name: "g".into(),
            start_line: 2,
            end_line: 6,
            normalized: "y".into(),
        },
    ];
    assert_eq!(clustering::build_chunk_index(&chunks).len(), 2);
    let mut ps = HashMap::new();
    ps.insert((0, 1), 0.8);
    assert!(clustering::compute_cluster_similarity(&[0, 1], &ps) > 0.0);
    assert!(extraction::is_nontrivial_chunk("a b c d e f g h i j k", 10));
    assert!(!extraction::is_nontrivial_chunk("a b c", 10));
    assert!(!extraction::is_nontrivial_chunk("a b c d e f g h i j k", 3)); // too few lines
}

#[test]
fn test_python_duplication() {
    // Multi-line function (>= 5 lines) to pass MIN_CHUNK_LINES filter
    let code = "def foo():\n    x = 1\n    y = 2\n    z = 3\n    a = 4\n    b = 5\n    return x + y + z + a + b";
    let mut tmp1 = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    let mut tmp2 = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    write!(tmp1, "{code}").unwrap();
    write!(tmp2, "{code}").unwrap();
    let mut parser = create_parser().unwrap();
    let p1 = parse_file(&mut parser, tmp1.path()).unwrap();
    let p2 = parse_file(&mut parser, tmp2.path()).unwrap();
    let pairs = detect_duplicates(&[&p1, &p2], &DuplicationConfig::default());
    assert!(!pairs.is_empty() && (pairs[0].similarity - 1.0).abs() < 0.01);
    let chunks = vec![
        CodeChunk {
            file: "a.py".into(),
            name: "f1".into(),
            start_line: 1,
            end_line: 5,
            normalized: "x y z a b c d e f g".into(),
        },
        CodeChunk {
            file: "b.py".into(),
            name: "f2".into(),
            start_line: 1,
            end_line: 5,
            normalized: "x y z a b c d e f g".into(),
        },
    ];
    let pairs2 = detect_duplicates_from_chunks(&chunks, &DuplicationConfig::default());
    let _ = cluster_duplicates(&pairs2, &chunks);
    let mut tmp3 = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    write!(
        tmp3,
        "def foo():\n    x = 1\n    y = 2\n    z = 3\n    a = 4\n    b = 5\n    c = 6"
    )
    .unwrap();
    let parsed = parse_file(&mut create_parser().unwrap(), tmp3.path()).unwrap();
    let mut chunks = Vec::new();
    extraction::extract_function_chunks(
        parsed.tree.root_node(),
        &parsed.source,
        &parsed.path,
        &mut chunks,
    );
    assert!(!chunks.is_empty());
}

#[test]
fn test_cluster_duplicates_from_chunks_smoke() {
    let chunks = vec![
        CodeChunk {
            file: "a.py".into(),
            name: "f1".into(),
            start_line: 1,
            end_line: 10,
            normalized: "x y z a b c d e f g".into(),
        },
        CodeChunk {
            file: "b.py".into(),
            name: "f2".into(),
            start_line: 1,
            end_line: 10,
            normalized: "x y z a b c d e f g".into(),
        },
    ];
    let clusters = cluster_duplicates_from_chunks(&chunks, &DuplicationConfig::default());
    assert!(!clusters.is_empty());
}

#[test]
fn test_rust_duplication() {
    // Multi-line function (>= 5 lines) to pass MIN_CHUNK_LINES filter
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    write!(tmp, "fn foo() {{\n    let x = 1;\n    let y = 2;\n    let z = 3;\n    let a = 4;\n    let b = 5;\n}}").unwrap();
    let parsed = parse_rust_file(tmp.path()).unwrap();
    assert!(!extract_rust_chunks_for_duplication(&[&parsed]).is_empty());
    let source = "fn bar() {\n    let x = 1;\n    let y = 2;\n    let z = 3;\n    let a = 4;\n    let b = 5;\n}";
    let ast: syn::File = syn::parse_str(source).unwrap();
    let mut chunks = Vec::new();
    extraction::extract_rust_function_chunks(&ast, source, Path::new("test.rs"), &mut chunks);
    extraction::extract_chunks_from_item(&ast.items[0], source, Path::new("test.rs"), &mut chunks);
    if let syn::Item::Fn(f) = &ast.items[0] {
        let start = f.sig.fn_token.span.start().line;
        let end = f.block.brace_token.span.close().end().line;
        extraction::add_rust_function_chunk(
            "bar",
            start,
            end,
            source,
            Path::new("test.rs"),
            &mut chunks,
        );
    }
}

#[test]
fn test_cmp_chunk_key_and_min_chunk() {
    let c1 = CodeChunk {
        file: "a.py".into(),
        name: "f1".into(),
        start_line: 1,
        end_line: 5,
        normalized: "x".into(),
    };
    let c2 = CodeChunk {
        file: "b.py".into(),
        name: "f2".into(),
        start_line: 1,
        end_line: 5,
        normalized: "y".into(),
    };
    // Test cmp_chunk_key from mod.rs
    assert!(cmp_chunk_key(&c1, &c2).is_lt());
    // Test clustering::cmp_chunk_key
    assert!(clustering::cmp_chunk_key(&c1, &c2).is_lt());
    // Test min_chunk_in_cluster
    let cluster = DuplicateCluster {
        chunks: vec![c2.clone(), c1.clone()],
        avg_similarity: 0.9,
    };
    let min = clustering::min_chunk_in_cluster(&cluster).unwrap();
    assert_eq!(min.file, PathBuf::from("a.py"));
    // Test sort_clusters_deterministic
    let mut clusters = vec![cluster];
    clustering::sort_clusters_deterministic(&mut clusters);
    // Test chunks_are_nested
    assert!(!chunks_are_nested(&c1, &c2));
}

#[test]
fn test_cluster_from_pairs() {
    let chunks = vec![
        CodeChunk {
            file: "a.py".into(),
            name: "f1".into(),
            start_line: 1,
            end_line: 10,
            normalized: "x y z a b c d e f g".into(),
        },
        CodeChunk {
            file: "b.py".into(),
            name: "f2".into(),
            start_line: 1,
            end_line: 10,
            normalized: "x y z a b c d e f g".into(),
        },
    ];
    let pairs = vec![(0, 1, 0.95)];
    let clusters = clustering::cluster_from_pairs(&chunks, pairs);
    assert_eq!(clusters.len(), 1);
    assert_eq!(clusters[0].chunks.len(), 2);
}

#[test]
fn test_extract_chunks_for_duplication_direct() {
    let code = "def foo():\n    x = 1\n    y = 2\n    z = 3\n    a = 4\n    b = 5\n    return x";
    let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    write!(tmp, "{code}").unwrap();
    let mut parser = create_parser().unwrap();
    let p = parse_file(&mut parser, tmp.path()).unwrap();
    let chunks = extract_chunks_for_duplication(&[&p]);
    assert!(!chunks.is_empty());
}
