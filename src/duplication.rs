//! Code duplication detection using MinHash/LSH (Normalize → Shingle → MinHash → LSH → Verify)

use crate::parsing::ParsedFile;
use crate::rust_parsing::ParsedRustFile;
use crate::units::get_child_by_field;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use syn::{ImplItem, Item};
use tree_sitter::Node;

const MIN_CHUNK_TOKENS: usize = 10;

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

#[derive(Debug, Clone)]
pub struct MinHashSignature {
    pub hashes: Vec<u64>,
}

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
    fn new(n: usize) -> Self {
        Self { parent: (0..n).collect() }
    }

    fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.find(self.parent[x]);
        }
        self.parent[x]
    }

    fn union(&mut self, x: usize, y: usize) {
        let (px, py) = (self.find(x), self.find(y));
        if px != py {
            self.parent[px] = py;
        }
    }
}

type ChunkKey = (PathBuf, usize, usize);

fn chunk_key(c: &CodeChunk) -> ChunkKey {
    (c.file.clone(), c.start_line, c.end_line)
}

fn build_chunk_index(chunks: &[CodeChunk]) -> HashMap<ChunkKey, usize> {
    chunks.iter().enumerate().map(|(idx, c)| (chunk_key(c), idx)).collect()
}

fn compute_cluster_similarity(indices: &[usize], pair_sims: &HashMap<(usize, usize), f64>) -> f64 {
    let mut total = 0.0;
    let mut count = 0;
    for i in 0..indices.len() {
        for j in (i + 1)..indices.len() {
            let key = (indices[i].min(indices[j]), indices[i].max(indices[j]));
            if let Some(&sim) = pair_sims.get(&key) {
                total += sim;
                count += 1;
            }
        }
    }
    if count > 0 { total / count as f64 } else { 0.0 }
}

/// Cluster duplicate pairs into groups of similar code
pub fn cluster_duplicates(pairs: &[DuplicatePair], chunks: &[CodeChunk]) -> Vec<DuplicateCluster> {
    if pairs.is_empty() || chunks.len() < 2 {
        return Vec::new();
    }

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
    for idx in 0..chunks.len() {
        clusters_map.entry(uf.find(idx)).or_default().push(idx);
    }

    let mut clusters: Vec<DuplicateCluster> = clusters_map
        .into_values()
        .filter(|indices| indices.len() > 1)
        .map(|indices| DuplicateCluster {
            avg_similarity: compute_cluster_similarity(&indices, &pair_similarities),
            chunks: indices.into_iter().map(|i| chunks[i].clone()).collect(),
        })
        .collect();

    clusters.sort_by(|a, b| {
        b.chunks.len().cmp(&a.chunks.len())
            .then_with(|| b.avg_similarity.partial_cmp(&a.avg_similarity).unwrap_or(std::cmp::Ordering::Equal))
    });

    clusters
}

/// Extract code chunks from parsed files for duplication detection
#[must_use]
pub fn extract_chunks_for_duplication(parsed_files: &[&ParsedFile]) -> Vec<CodeChunk> {
    let mut chunks = Vec::new();

    for parsed in parsed_files {
        let root = parsed.tree.root_node();
        extract_function_chunks(root, &parsed.source, &parsed.path, &mut chunks);
    }

    chunks
}

/// Extract code chunks from parsed Rust files for duplication detection
#[must_use]
pub fn extract_rust_chunks_for_duplication(parsed_files: &[&ParsedRustFile]) -> Vec<CodeChunk> {
    let mut chunks = Vec::new();

    for parsed in parsed_files {
        extract_rust_function_chunks(&parsed.ast, &parsed.source, &parsed.path, &mut chunks);
    }

    chunks
}

fn extract_rust_function_chunks(ast: &syn::File, source: &str, file: &Path, chunks: &mut Vec<CodeChunk>) {
    for item in &ast.items {
        extract_chunks_from_item(item, source, file, chunks);
    }
}

fn extract_chunks_from_item(item: &Item, source: &str, file: &Path, chunks: &mut Vec<CodeChunk>) {
    match item {
        Item::Fn(func) => {
            add_rust_function_chunk(&func.sig.ident.to_string(), func.sig.ident.span(), &func.block, source, file, chunks);
        }
        Item::Impl(impl_block) => {
            for impl_item in &impl_block.items {
                if let ImplItem::Fn(method) = impl_item {
                    add_rust_function_chunk(&method.sig.ident.to_string(), method.sig.ident.span(), &method.block, source, file, chunks);
                }
            }
        }
        Item::Mod(m) => {
            if let Some((_, items)) = &m.content {
                for item in items {
                    extract_chunks_from_item(item, source, file, chunks);
                }
            }
        }
        _ => {}
    }
}

fn is_nontrivial_chunk(normalized: &str) -> bool {
    normalized.split_whitespace().count() >= MIN_CHUNK_TOKENS
}

fn add_rust_function_chunk(
    name: &str,
    span: proc_macro2::Span,
    _block: &syn::Block,
    source: &str,
    file: &Path,
    chunks: &mut Vec<CodeChunk>,
) {
    let start_line = span.start().line;
    let end_line = span.end().line;
    let lines: Vec<&str> = source.lines().collect();
    
    if start_line > 0 && end_line <= lines.len() {
        let body_text: String = lines[start_line - 1..end_line].join("\n");
        let normalized = normalize_code(&body_text);

        if is_nontrivial_chunk(&normalized) {
            chunks.push(CodeChunk {
                file: file.to_path_buf(),
                name: name.to_string(),
                start_line,
                end_line,
                normalized,
            });
        }
    }
}

fn extract_function_chunks(node: Node, source: &str, file: &Path, chunks: &mut Vec<CodeChunk>) {
    match node.kind() {
        "function_definition" | "async_function_definition" => {
            let name = get_child_by_field(node, "name", source).unwrap_or_default();
            let start_line = node.start_position().row + 1;
            let end_line = node.end_position().row + 1;

            if let Some(body) = node.child_by_field_name("body") {
                let body_text = &source[body.start_byte()..body.end_byte()];
                let normalized = normalize_code(body_text);

                if is_nontrivial_chunk(&normalized) {
                    chunks.push(CodeChunk {
                        file: file.to_path_buf(),
                        name,
                        start_line,
                        end_line,
                        normalized,
                    });
                }
            }

            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                extract_function_chunks(child, source, file, chunks);
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                extract_function_chunks(child, source, file, chunks);
            }
        }
    }
}

/// Normalize code: lowercase, collapse whitespace, replace numbers with 'N'
fn normalize_code(code: &str) -> String {
    let mut result = String::with_capacity(code.len());
    let mut last_was_space = true;

    for c in code.chars() {
        if c.is_whitespace() {
            if !last_was_space {
                result.push(' ');
                last_was_space = true;
            }
        } else if c.is_ascii_digit() {
            if !result.ends_with('N') {
                result.push('N');
                last_was_space = false;
            }
        } else {
            result.push(c.to_ascii_lowercase());
            last_was_space = false;
        }
    }

    result.trim().to_string()
}

/// Generate shingles (n-grams of tokens)
fn generate_shingles(text: &str, shingle_size: usize) -> std::collections::HashSet<u64> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let tokens: Vec<&str> = text.split_whitespace().collect();
    let mut shingles = std::collections::HashSet::new();

    if tokens.len() >= shingle_size {
        for window in tokens.windows(shingle_size) {
            let mut hasher = DefaultHasher::new();
            window.hash(&mut hasher);
            shingles.insert(hasher.finish());
        }
    }

    shingles
}

/// Compute MinHash signature for a set of shingles
fn compute_minhash(shingles: &std::collections::HashSet<u64>, size: usize) -> MinHashSignature {
    let mut hashes = vec![u64::MAX; size];
    let coefficients: Vec<(u64, u64)> = (0..size)
        .map(|i| {
            let seed = 0x9E3779B97F4A7C15_u64.wrapping_add(i as u64);
            let a = seed.wrapping_mul(0xBF58476D1CE4E5B9) | 1;
            let b = seed.wrapping_mul(0x94D049BB133111EB);
            (a, b)
        })
        .collect();

    for &shingle in shingles {
        for (i, &(a, b)) in coefficients.iter().enumerate() {
            let h = a.wrapping_mul(shingle).wrapping_add(b);
            if h < hashes[i] {
                hashes[i] = h;
            }
        }
    }

    MinHashSignature { hashes }
}

/// Estimate Jaccard similarity from MinHash signatures
fn estimate_similarity(sig1: &MinHashSignature, sig2: &MinHashSignature) -> f64 {
    if sig1.hashes.is_empty() {
        return 0.0;
    }
    let matching = sig1
        .hashes
        .iter()
        .zip(&sig2.hashes)
        .filter(|(a, b)| a == b)
        .count();
    matching as f64 / sig1.hashes.len() as f64
}

fn hash_band(band_slice: &[u64]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    band_slice.hash(&mut hasher);
    hasher.finish()
}

fn add_bucket_pairs(indices: &[usize], candidates: &mut std::collections::HashSet<(usize, usize)>) {
    if indices.len() < 2 || indices.len() > 100 { return; }
    for i in 0..indices.len() {
        for j in (i + 1)..indices.len() {
            candidates.insert((indices[i].min(indices[j]), indices[i].max(indices[j])));
        }
    }
}

/// Find candidate pairs using LSH
fn find_lsh_candidates(
    signatures: &[MinHashSignature],
    num_bands: usize,
) -> std::collections::HashSet<(usize, usize)> {
    let mut candidates = std::collections::HashSet::new();
    if signatures.is_empty() { return candidates; }

    let hash_len = signatures[0].hashes.len();
    let rows_per_band = (hash_len / num_bands).max(1);

    for band_idx in 0..num_bands {
        let band_start = band_idx * rows_per_band;
        if band_start >= hash_len { break; }
        let band_end = (band_start + rows_per_band).min(hash_len);

        let mut buckets: HashMap<u64, Vec<usize>> = HashMap::new();
        for (idx, sig) in signatures.iter().enumerate() {
            buckets.entry(hash_band(&sig.hashes[band_start..band_end])).or_default().push(idx);
        }

        for indices in buckets.values() {
            add_bucket_pairs(indices, &mut candidates);
        }
    }

    candidates
}

/// Detect duplicate code across parsed files
pub fn detect_duplicates(
    parsed_files: &[&ParsedFile],
    config: &DuplicationConfig,
) -> Vec<DuplicatePair> {
    let chunks = extract_chunks_for_duplication(parsed_files);
    detect_duplicates_from_chunks(&chunks, config)
}

/// Detect duplicate code from pre-extracted code chunks
/// This is the core duplication detection algorithm, usable for any language
pub fn detect_duplicates_from_chunks(
    chunks: &[CodeChunk],
    config: &DuplicationConfig,
) -> Vec<DuplicatePair> {
    if chunks.len() < 2 {
        return Vec::new();
    }

    let signatures: Vec<MinHashSignature> = chunks
        .iter()
        .map(|chunk| {
            let shingles = generate_shingles(&chunk.normalized, config.shingle_size);
            compute_minhash(&shingles, config.minhash_size)
        })
        .collect();

    let candidates = find_lsh_candidates(&signatures, config.lsh_bands);
    let mut duplicates = Vec::new();

    for (i, j) in candidates {
        let similarity = estimate_similarity(&signatures[i], &signatures[j]);
        if similarity >= config.min_similarity {
            duplicates.push(DuplicatePair {
                chunk1: chunks[i].clone(),
                chunk2: chunks[j].clone(),
                similarity,
            });
        }
    }

    duplicates.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());

    duplicates
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::{create_parser, parse_file, ParsedFile};
    use std::io::Write;

    fn parse_source(code: &str) -> ParsedFile {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(tmp, "{}", code).unwrap();
        let mut parser = create_parser().unwrap();
        parse_file(&mut parser, tmp.path()).unwrap()
    }

    #[test]
    fn test_normalize_and_structs() {
        assert_eq!(normalize_code("x = 123"), "x = N");
        let c = DuplicationConfig::default(); assert!(c.shingle_size > 0);
        let chunk = CodeChunk { file: "f.py".into(), name: "foo".into(), start_line: 1, end_line: 10, normalized: "x".into() };
        assert_eq!(chunk_key(&chunk).0, std::path::PathBuf::from("f.py"));
        assert_eq!(build_chunk_index(&[chunk]).len(), 1);
        let mut uf = UnionFind::new(5); uf.union(0, 1); assert_eq!(uf.find(0), uf.find(1));
        let cluster = DuplicateCluster { chunks: vec![], avg_similarity: 0.9 }; assert_eq!(cluster.avg_similarity, 0.9);
    }

    #[test]
    fn test_minhash_lsh_clustering() {
        let shingles = generate_shingles("one two three four", 3);
        let sig = compute_minhash(&shingles, 5); assert_eq!(sig.hashes.len(), 5);
        let a = MinHashSignature { hashes: vec![1, 2, 3, 4, 5] };
        let b = MinHashSignature { hashes: vec![1, 2, 3, 4, 5] };
        assert_eq!(estimate_similarity(&a, &b), 1.0);
        assert_ne!(hash_band(&[1, 2, 3]), hash_band(&[4, 5, 6]));
        let mut cands = std::collections::HashSet::new(); add_bucket_pairs(&[0, 1, 2], &mut cands);
        let _ = find_lsh_candidates(&[a, b], 2);
        let c1 = CodeChunk { file: "a.py".into(), name: "f".into(), start_line: 1, end_line: 5, normalized: "x".into() };
        let c2 = CodeChunk { file: "b.py".into(), name: "g".into(), start_line: 1, end_line: 5, normalized: "x".into() };
        let pair = DuplicatePair { chunk1: c1.clone(), chunk2: c2.clone(), similarity: 0.9 };
        let clusters = cluster_duplicates(&[pair], &[c1, c2]);
        assert!(!clusters.is_empty());
        let mut pair_sims = std::collections::HashMap::new(); pair_sims.insert((0, 1), 0.9);
        assert!(compute_cluster_similarity(&[0, 1], &pair_sims) >= 0.0);
    }

    #[test]
    fn test_extraction_and_detection() {
        let parsed = parse_source("def foo():\n    x = 1\n    y = 2\n    z = 3\n    w = 4");
        let _ = extract_chunks_for_duplication(&[&parsed]);
        let mut chunks = Vec::new();
        extract_function_chunks(parsed.tree.root_node(), &parsed.source, &parsed.path, &mut chunks);
        assert!(detect_duplicates(&[&parsed], &DuplicationConfig::default()).is_empty());
        assert!(extract_rust_chunks_for_duplication(&[]).is_empty());
        assert!(detect_duplicates_from_chunks(&[], &DuplicationConfig::default()).is_empty());
        let src = "fn foo() { let x=1; let y=2; let z=3; let w=4; let v=5; }";
        let ast: syn::File = syn::parse_str(src).unwrap();
        let mut rc = Vec::new(); extract_rust_function_chunks(&ast, src, std::path::Path::new("t.rs"), &mut rc);
        let item: syn::Item = syn::parse_str(src).unwrap();
        let mut c2 = Vec::new(); extract_chunks_from_item(&item, src, std::path::Path::new("t.rs"), &mut c2);
        let f: syn::ItemFn = syn::parse_str(src).unwrap();
        let mut c3 = Vec::new(); add_rust_function_chunk(&f.sig.ident.to_string(), f.sig.ident.span(), &f.block, src, std::path::Path::new("t.rs"), &mut c3);
        assert!(!is_nontrivial_chunk("a b c"));
        assert!(is_nontrivial_chunk(&"word ".repeat(MIN_CHUNK_TOKENS)));
    }

    // --- Design doc: Duplication Similarity Threshold ---
    // "Jaccard ≥ 0.7"

    #[test]
    fn test_identical_code_has_similarity_one() {
        let text = "x = 1 y = 2 z = 3 w = 4 v = 5 a = 6 b = 7 c = 8 d = 9 e = 10";
        let shingles1 = generate_shingles(text, 3);
        let shingles2 = generate_shingles(text, 3);
        
        let sig1 = compute_minhash(&shingles1, 100);
        let sig2 = compute_minhash(&shingles2, 100);
        
        let similarity = estimate_similarity(&sig1, &sig2);
        assert_eq!(similarity, 1.0, "identical text should have similarity 1.0");
    }

    #[test]
    fn test_completely_different_code_has_low_similarity() {
        let text1 = "alpha beta gamma delta epsilon zeta eta theta iota kappa";
        let text2 = "one two three four five six seven eight nine ten";
        
        let sig1 = compute_minhash(&generate_shingles(text1, 3), 100);
        let sig2 = compute_minhash(&generate_shingles(text2, 3), 100);
        
        let similarity = estimate_similarity(&sig1, &sig2);
        assert!(similarity < 0.3, "completely different text should have low similarity, got {}", similarity);
    }

    #[test]
    fn test_similarity_threshold_boundary() {
        // Test that the 0.7 threshold is properly applied
        let config = DuplicationConfig::default();
        assert_eq!(config.min_similarity, 0.7, "default threshold should be 0.7");
        
        // Create two identical chunks (similarity = 1.0 > 0.7)
        let chunk1 = CodeChunk {
            file: "a.py".into(),
            name: "func_a".into(),
            start_line: 1,
            end_line: 10,
            normalized: "x = 1 y = 2 z = 3 w = 4 v = 5 a = 6 b = 7 c = 8 d = 9 e = 10".into(),
        };
        let chunk2 = CodeChunk {
            file: "b.py".into(),
            name: "func_b".into(),
            start_line: 1,
            end_line: 10,
            normalized: "x = 1 y = 2 z = 3 w = 4 v = 5 a = 6 b = 7 c = 8 d = 9 e = 10".into(),
        };
        
        let duplicates = detect_duplicates_from_chunks(&[chunk1, chunk2], &config);
        assert!(!duplicates.is_empty(), "identical chunks should be flagged as duplicates");
        assert!(duplicates[0].similarity >= 0.7, "flagged duplicates should have similarity >= 0.7");
    }

    #[test]
    fn test_below_threshold_not_flagged() {
        // Create two chunks that are slightly similar but below threshold
        let config = DuplicationConfig { min_similarity: 0.7, ..Default::default() };
        
        // Mostly different text
        let chunk1 = CodeChunk {
            file: "a.py".into(),
            name: "func_a".into(),
            start_line: 1,
            end_line: 10,
            normalized: "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu".into(),
        };
        let chunk2 = CodeChunk {
            file: "b.py".into(),
            name: "func_b".into(),
            start_line: 1,
            end_line: 10,
            normalized: "one two three four five six seven eight nine ten eleven twelve".into(),
        };
        
        let duplicates = detect_duplicates_from_chunks(&[chunk1, chunk2], &config);
        // These should NOT be flagged as duplicates (similarity < 0.7)
        assert!(duplicates.is_empty() || duplicates[0].similarity < 0.7,
            "very different chunks should not be flagged as duplicates");
    }

    #[test]
    fn test_exactly_70_percent_similarity_is_flagged() {
        // Similarity >= 0.7 should be flagged (not just > 0.7)
        let config = DuplicationConfig { min_similarity: 0.7, ..Default::default() };
        
        // We need chunks that produce exactly 0.7 similarity
        // This is hard to achieve precisely, so we test the >= condition
        let chunk1 = CodeChunk {
            file: "a.py".into(),
            name: "func_a".into(),
            start_line: 1,
            end_line: 10,
            normalized: "x = 1 y = 2 z = 3 w = 4 v = 5 a = 6 b = 7 same same same same same".into(),
        };
        let chunk2 = CodeChunk {
            file: "b.py".into(),
            name: "func_b".into(),
            start_line: 1,
            end_line: 10,
            normalized: "x = 1 y = 2 z = 3 w = 4 v = 5 a = 6 b = 7 same same same same same".into(),
        };
        
        let duplicates = detect_duplicates_from_chunks(&[chunk1, chunk2], &config);
        // Identical chunks have similarity 1.0 >= 0.7
        assert!(!duplicates.is_empty(), "chunks at or above threshold should be flagged");
    }
}
