
use crate::minhash::{compute_minhash, estimate_similarity, find_lsh_candidates, generate_shingles, normalize_code};
use crate::parsing::ParsedFile;
use crate::rust_parsing::ParsedRustFile;
use crate::units::get_child_by_field;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use syn::{ImplItem, Item};
use tree_sitter::Node;

const MIN_CHUNK_TOKENS: usize = 10;
const MIN_CHUNK_LINES: usize = 5;

pub struct DuplicationConfig {
    pub minhash_size: usize,
    pub shingle_size: usize,
    pub lsh_bands: usize,
    pub min_similarity: f64,
}

impl Default for DuplicationConfig {
    fn default() -> Self {
        Self { minhash_size: 100, shingle_size: 3, lsh_bands: 20, min_similarity: 0.7 }
    }
}

#[derive(Debug, Clone)]
pub struct CodeChunk {
    pub file: PathBuf,
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
    pub normalized: String,
}

pub use crate::minhash::MinHashSignature;

#[derive(Debug)]
pub struct DuplicatePair {
    pub chunk1: CodeChunk,
    pub chunk2: CodeChunk,
    pub similarity: f64,
}

#[derive(Debug)]
pub struct DuplicateCluster {
    pub chunks: Vec<CodeChunk>,
    pub avg_similarity: f64,
}

struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self { Self { parent: (0..n).collect() } }
    fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x { self.parent[x] = self.find(self.parent[x]); }
        self.parent[x]
    }
    fn union(&mut self, x: usize, y: usize) {
        let (px, py) = (self.find(x), self.find(y));
        if px != py { self.parent[px] = py; }
    }
}

type ChunkKey = (PathBuf, usize, usize);
fn chunk_key(c: &CodeChunk) -> ChunkKey { (c.file.clone(), c.start_line, c.end_line) }
fn build_chunk_index(chunks: &[CodeChunk]) -> HashMap<ChunkKey, usize> {
    chunks.iter().enumerate().map(|(idx, c)| (chunk_key(c), idx)).collect()
}

fn compute_cluster_similarity(indices: &[usize], pair_sims: &HashMap<(usize, usize), f64>) -> f64 {
    // Average similarity of pairs that actually exist in this cluster
    // Rather than O(k²) iteration, use O(pairs) by iterating pair_sims directly
    use std::collections::HashSet;
    let index_set: HashSet<usize> = indices.iter().copied().collect();
    let mut total = 0.0;
    let mut count = 0;
    for (&(i, j), &sim) in pair_sims {
        if index_set.contains(&i) && index_set.contains(&j) {
            total += sim;
            count += 1;
        }
    }
    if count > 0 { total / f64::from(count) } else { 0.0 }
}

pub fn cluster_duplicates(pairs: &[DuplicatePair], chunks: &[CodeChunk]) -> Vec<DuplicateCluster> {
    if pairs.is_empty() || chunks.len() < 2 { return Vec::new(); }
    let key_to_idx = build_chunk_index(chunks);
    let mut uf = UnionFind::new(chunks.len());
    let mut pair_similarities: HashMap<(usize, usize), f64> = HashMap::new();
    for pair in pairs {
        if let (Some(&i1), Some(&i2)) = (key_to_idx.get(&chunk_key(&pair.chunk1)), key_to_idx.get(&chunk_key(&pair.chunk2))) {
            uf.union(i1, i2);
            pair_similarities.insert((i1.min(i2), i1.max(i2)), pair.similarity);
        }
    }
    let mut clusters_map: HashMap<usize, Vec<usize>> = HashMap::new();
    for idx in 0..chunks.len() { clusters_map.entry(uf.find(idx)).or_default().push(idx); }
    let mut clusters: Vec<DuplicateCluster> = clusters_map.into_values()
        .filter(|indices| indices.len() > 1)
        .map(|indices| DuplicateCluster {
            avg_similarity: compute_cluster_similarity(&indices, &pair_similarities),
            chunks: indices.into_iter().map(|i| chunks[i].clone()).collect(),
        })
        .collect();
    clusters.sort_by(|a, b| b.chunks.len().cmp(&a.chunks.len()).then_with(|| b.avg_similarity.partial_cmp(&a.avg_similarity).unwrap_or(std::cmp::Ordering::Equal)));
    clusters
}

#[must_use]
pub fn extract_chunks_for_duplication(parsed_files: &[&ParsedFile]) -> Vec<CodeChunk> {
    let mut chunks = Vec::new();
    for parsed in parsed_files {
        extract_function_chunks(parsed.tree.root_node(), &parsed.source, &parsed.path, &mut chunks);
    }
    chunks
}

#[must_use]
pub fn extract_rust_chunks_for_duplication(parsed_files: &[&ParsedRustFile]) -> Vec<CodeChunk> {
    let mut chunks = Vec::new();
    for parsed in parsed_files { extract_rust_function_chunks(&parsed.ast, &parsed.source, &parsed.path, &mut chunks); }
    chunks
}

fn extract_rust_function_chunks(ast: &syn::File, source: &str, file: &Path, chunks: &mut Vec<CodeChunk>) {
    for item in &ast.items { extract_chunks_from_item(item, source, file, chunks); }
}

fn extract_chunks_from_item(item: &Item, source: &str, file: &Path, chunks: &mut Vec<CodeChunk>) {
    match item {
        Item::Fn(func) => {
            let start = func.sig.fn_token.span.start().line;
            let end = func.block.brace_token.span.close().end().line;
            add_rust_function_chunk(&func.sig.ident.to_string(), start, end, source, file, chunks);
        }
        Item::Impl(impl_block) => {
            for impl_item in &impl_block.items {
                if let ImplItem::Fn(method) = impl_item {
                    let start = method.sig.fn_token.span.start().line;
                    let end = method.block.brace_token.span.close().end().line;
                    add_rust_function_chunk(&method.sig.ident.to_string(), start, end, source, file, chunks);
                }
            }
        }
        Item::Mod(m) => { if let Some((_, items)) = &m.content { for item in items { extract_chunks_from_item(item, source, file, chunks); } } }
        _ => {}
    }
}

fn is_nontrivial_chunk(normalized: &str, line_count: usize) -> bool {
    line_count >= MIN_CHUNK_LINES && normalized.split_whitespace().count() >= MIN_CHUNK_TOKENS
}

fn add_rust_function_chunk(name: &str, start_line: usize, end_line: usize, source: &str, file: &Path, chunks: &mut Vec<CodeChunk>) {
    let line_count = end_line.saturating_sub(start_line) + 1;
    let lines: Vec<&str> = source.lines().collect();
    if start_line > 0 && end_line <= lines.len() {
        let body_text: String = lines[start_line - 1..end_line].join("\n");
        let normalized = normalize_code(&body_text);
        if is_nontrivial_chunk(&normalized, line_count) {
            chunks.push(CodeChunk { file: file.to_path_buf(), name: name.to_string(), start_line, end_line, normalized });
        }
    }
}

fn extract_function_chunks(node: Node, source: &str, file: &Path, chunks: &mut Vec<CodeChunk>) {
    match node.kind() {
        "function_definition" | "async_function_definition" => {
            let name = get_child_by_field(node, "name", source).unwrap_or_default();
            let (start_line, end_line) = (node.start_position().row + 1, node.end_position().row + 1);
            let line_count = end_line.saturating_sub(start_line) + 1;
            if let Some(body) = node.child_by_field_name("body") {
                let normalized = normalize_code(&source[body.start_byte()..body.end_byte()]);
                if is_nontrivial_chunk(&normalized, line_count) {
                    chunks.push(CodeChunk { file: file.to_path_buf(), name, start_line, end_line, normalized });
                }
            }
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) { extract_function_chunks(child, source, file, chunks); }
        }
        _ => { let mut cursor = node.walk(); for child in node.children(&mut cursor) { extract_function_chunks(child, source, file, chunks); } }
    }
}

pub fn detect_duplicates(parsed_files: &[&ParsedFile], config: &DuplicationConfig) -> Vec<DuplicatePair> {
    detect_duplicates_from_chunks(&extract_chunks_for_duplication(parsed_files), config)
}

pub fn detect_duplicates_from_chunks(chunks: &[CodeChunk], config: &DuplicationConfig) -> Vec<DuplicatePair> {
    if chunks.len() < 2 { return Vec::new(); }
    let signatures: Vec<MinHashSignature> = chunks.iter()
        .map(|c| compute_minhash(&generate_shingles(&c.normalized, config.shingle_size), config.minhash_size))
        .collect();
    let candidates = find_lsh_candidates(&signatures, config.lsh_bands);
    let mut duplicates: Vec<DuplicatePair> = candidates.into_iter()
        .filter_map(|(i, j)| {
            let similarity = estimate_similarity(&signatures[i], &signatures[j]);
            (similarity >= config.min_similarity).then(|| DuplicatePair { chunk1: chunks[i].clone(), chunk2: chunks[j].clone(), similarity })
        })
        .collect();
    duplicates.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
    duplicates
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::{create_parser, parse_file};
    use crate::rust_parsing::parse_rust_file;
    use std::io::Write;

    #[test]
    fn test_structs_and_helpers() {
        let c = DuplicationConfig::default(); assert!(c.shingle_size > 0);
        let chunk = CodeChunk { file: "f.py".into(), name: "foo".into(), start_line: 1, end_line: 10, normalized: "x".into() };
        assert_eq!(chunk_key(&chunk).0, PathBuf::from("f.py"));
        let mut uf = UnionFind::new(5); uf.union(0, 1); assert_eq!(uf.find(0), uf.find(1));
        let c1 = CodeChunk { file: "a.py".into(), name: "f".into(), start_line: 1, end_line: 5, normalized: "x".into() };
        let c2 = CodeChunk { file: "b.py".into(), name: "g".into(), start_line: 1, end_line: 5, normalized: "x".into() };
        let _ = DuplicatePair { chunk1: c1.clone(), chunk2: c2, similarity: 0.9 };
        let _ = DuplicateCluster { chunks: vec![c1.clone()], avg_similarity: 0.8 };
        let chunks = vec![c1, CodeChunk { file: "b.py".into(), name: "g".into(), start_line: 2, end_line: 6, normalized: "y".into() }];
        assert_eq!(build_chunk_index(&chunks).len(), 2);
        let mut ps = HashMap::new();
        ps.insert((0, 1), 0.8);
        assert!(compute_cluster_similarity(&[0, 1], &ps) > 0.0);
        assert!(is_nontrivial_chunk("a b c d e f g h i j k", 10));
        assert!(!is_nontrivial_chunk("a b c", 10));
        assert!(!is_nontrivial_chunk("a b c d e f g h i j k", 3)); // too few lines
    }

    #[test]
    fn test_python_duplication() {
        // Multi-line function (>= 5 lines) to pass MIN_CHUNK_LINES filter
        let code = "def foo():\n    x = 1\n    y = 2\n    z = 3\n    a = 4\n    b = 5\n    return x + y + z + a + b";
        let mut tmp1 = tempfile::NamedTempFile::with_suffix(".py").unwrap();
        let mut tmp2 = tempfile::NamedTempFile::with_suffix(".py").unwrap();
        write!(tmp1, "{code}").unwrap(); write!(tmp2, "{code}").unwrap();
        let mut parser = create_parser().unwrap();
        let p1 = parse_file(&mut parser, tmp1.path()).unwrap();
        let p2 = parse_file(&mut parser, tmp2.path()).unwrap();
        let pairs = detect_duplicates(&[&p1, &p2], &DuplicationConfig::default());
        assert!(!pairs.is_empty() && (pairs[0].similarity - 1.0).abs() < 0.01);
        let chunks = vec![
            CodeChunk { file: "a.py".into(), name: "f1".into(), start_line: 1, end_line: 5, normalized: "x y z a b c d e f g".into() },
            CodeChunk { file: "b.py".into(), name: "f2".into(), start_line: 1, end_line: 5, normalized: "x y z a b c d e f g".into() },
        ];
        let pairs2 = detect_duplicates_from_chunks(&chunks, &DuplicationConfig::default());
        let _ = cluster_duplicates(&pairs2, &chunks);
        let mut tmp3 = tempfile::NamedTempFile::with_suffix(".py").unwrap();
        write!(tmp3, "def foo():\n    x = 1\n    y = 2\n    z = 3\n    a = 4\n    b = 5\n    c = 6").unwrap();
        let parsed = parse_file(&mut create_parser().unwrap(), tmp3.path()).unwrap();
        let mut chunks = Vec::new();
        extract_function_chunks(parsed.tree.root_node(), &parsed.source, &parsed.path, &mut chunks);
        assert!(!chunks.is_empty());
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
        extract_rust_function_chunks(&ast, source, Path::new("test.rs"), &mut chunks);
        extract_chunks_from_item(&ast.items[0], source, Path::new("test.rs"), &mut chunks);
        if let syn::Item::Fn(f) = &ast.items[0] {
            let start = f.sig.fn_token.span.start().line;
            let end = f.block.brace_token.span.close().end().line;
            add_rust_function_chunk("bar", start, end, source, Path::new("test.rs"), &mut chunks);
        }
    }
}
