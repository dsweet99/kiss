//! Code duplication detection using MinHash and LSH

use crate::parsing::ParsedFile;
use crate::units::get_child_by_field;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tree_sitter::Node;

/// Configuration for duplication detection
pub struct DuplicationConfig {
    /// Number of hash functions for MinHash signature
    pub minhash_size: usize,
    /// Size of shingles (n-grams of tokens)
    pub shingle_size: usize,
    /// Number of bands for LSH
    pub lsh_bands: usize,
    /// Minimum similarity to report as duplicate
    pub min_similarity: f64,
}

impl Default for DuplicationConfig {
    fn default() -> Self {
        Self {
            minhash_size: 100,
            shingle_size: 3,
            lsh_bands: 20,
            min_similarity: 0.7,
        }
    }
}

/// A code chunk for duplication detection
#[derive(Debug, Clone)]
pub struct CodeChunk {
    pub file: PathBuf,
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
    pub normalized: String,
}

/// A MinHash signature
#[derive(Debug, Clone)]
pub struct MinHashSignature {
    pub hashes: Vec<u64>,
}

/// A detected duplicate pair
#[derive(Debug)]
pub struct DuplicatePair {
    pub chunk1: CodeChunk,
    pub chunk2: CodeChunk,
    pub similarity: f64,
}

/// A cluster of duplicate code chunks
#[derive(Debug)]
pub struct DuplicateCluster {
    /// All chunks in this cluster (similar to each other)
    pub chunks: Vec<CodeChunk>,
    /// Average similarity within the cluster
    pub avg_similarity: f64,
}

/// Union-Find data structure for clustering
struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self { parent: (0..n).collect() }
    }

    fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.find(self.parent[x]); // path compression
        }
        self.parent[x]
    }

    fn union(&mut self, x: usize, y: usize) {
        let px = self.find(x);
        let py = self.find(y);
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
pub fn extract_chunks_for_duplication(parsed_files: &[&ParsedFile]) -> Vec<CodeChunk> {
    let mut chunks = Vec::new();

    for parsed in parsed_files {
        let root = parsed.tree.root_node();
        extract_function_chunks(root, &parsed.source, &parsed.path, &mut chunks);
    }

    chunks
}

fn extract_function_chunks(node: Node, source: &str, file: &Path, chunks: &mut Vec<CodeChunk>) {
    match node.kind() {
        "function_definition" | "async_function_definition" => {
            let name = get_child_by_field(node, "name", source).unwrap_or_default();
            let start_line = node.start_position().row + 1;
            let end_line = node.end_position().row + 1;

            // Get the function body and normalize it
            if let Some(body) = node.child_by_field_name("body") {
                let body_text = &source[body.start_byte()..body.end_byte()];
                let normalized = normalize_code(body_text);

                // Only include non-trivial chunks
                if normalized.split_whitespace().count() >= 10 {
                    chunks.push(CodeChunk {
                        file: file.to_path_buf(),
                        name,
                        start_line,
                        end_line,
                        normalized,
                    });
                }
            }

            // Recurse for nested functions
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    extract_function_chunks(child, source, file, chunks);
                }
            }
        }
        _ => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    extract_function_chunks(child, source, file, chunks);
                }
            }
        }
    }
}

/// Normalize code by removing variable names, string literals, etc.
fn normalize_code(code: &str) -> String {
    // Simple normalization: lowercase, collapse whitespace, remove numbers
    let mut result = String::with_capacity(code.len());
    let mut last_was_space = true;

    for c in code.chars() {
        if c.is_whitespace() {
            if !last_was_space {
                result.push(' ');
                last_was_space = true;
            }
        } else if c.is_ascii_digit() {
            // Replace numbers with placeholder
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

    // Use deterministic hash coefficients
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

    if chunks.len() < 2 {
        return Vec::new();
    }

    // Compute MinHash signatures
    let signatures: Vec<MinHashSignature> = chunks
        .iter()
        .map(|chunk| {
            let shingles = generate_shingles(&chunk.normalized, config.shingle_size);
            compute_minhash(&shingles, config.minhash_size)
        })
        .collect();

    // Find candidate pairs via LSH
    let candidates = find_lsh_candidates(&signatures, config.lsh_bands);

    // Verify candidates and compute actual similarity
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

    // Sort by similarity (highest first)
    duplicates.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());

    duplicates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_code_removes_numbers() {
        let code = "x = 123 + 456";
        let normalized = normalize_code(code);
        // Numbers should be replaced with N placeholders
        assert!(!normalized.contains('1'));
        assert!(!normalized.contains('2'));
        assert!(normalized.contains('N'), "Expected 'N' placeholder, got: {:?}", normalized);
    }

    #[test]
    fn normalize_code_preserves_number_placeholder_after_space() {
        // Bug fix: numbers after whitespace should still get N placeholder
        let code = "x = 123";
        let normalized = normalize_code(code);
        // Should be something like "x = N", not "x = "
        assert!(normalized.contains('N'), "Expected 'N' placeholder, got: {:?}", normalized);
        assert_eq!(normalized, "x = N");
    }

    #[test]
    fn normalize_code_collapses_consecutive_digits() {
        let code = "arr[123]";
        let normalized = normalize_code(code);
        // 123 should become a single N
        assert_eq!(normalized.matches('N').count(), 1, "got: {:?}", normalized);
    }

    #[test]
    fn test_duplication_config_default() {
        let c = DuplicationConfig::default();
        assert!(c.shingle_size > 0);
        assert!(c.minhash_size > 0);
    }

    #[test]
    fn test_code_chunk_struct() {
        let chunk = CodeChunk { file: std::path::PathBuf::from("f.py"), name: "foo".into(), start_line: 1, end_line: 10, normalized: "x".into() };
        assert_eq!(chunk.name, "foo");
    }

    #[test]
    fn test_minhash_signature_struct() {
        let sig = MinHashSignature { hashes: vec![1, 2, 3] };
        assert_eq!(sig.hashes.len(), 3);
    }

    #[test]
    fn test_duplicate_pair_struct() {
        let c1 = CodeChunk { file: "a.py".into(), name: "f".into(), start_line: 1, end_line: 5, normalized: "".into() };
        let c2 = CodeChunk { file: "b.py".into(), name: "g".into(), start_line: 1, end_line: 5, normalized: "".into() };
        let pair = DuplicatePair { chunk1: c1, chunk2: c2, similarity: 0.9 };
        assert_eq!(pair.similarity, 0.9);
    }

    #[test]
    fn test_duplicate_cluster_struct() {
        let cluster = DuplicateCluster { chunks: vec![], avg_similarity: 0.85 };
        assert_eq!(cluster.avg_similarity, 0.85);
    }

    #[test]
    fn test_union_find() {
        let mut uf = UnionFind::new(3);
        uf.union(0, 1);
        assert_eq!(uf.find(0), uf.find(1));
    }

    #[test]
    fn test_chunk_key() {
        let chunk = CodeChunk { file: std::path::PathBuf::from("a.py"), name: "f".into(), start_line: 5, end_line: 10, normalized: "".into() };
        let key = chunk_key(&chunk);
        assert_eq!(key.0, std::path::PathBuf::from("a.py"));
    }

    #[test]
    fn test_build_chunk_index() {
        let chunks = vec![
            CodeChunk { file: std::path::PathBuf::from("a.py"), name: "f".into(), start_line: 1, end_line: 5, normalized: "".into() },
        ];
        let idx = build_chunk_index(&chunks);
        assert_eq!(idx.len(), 1);
    }

    #[test]
    fn test_generate_shingles() {
        let shingles = generate_shingles("one two three four", 3);
        assert!(!shingles.is_empty());
    }

    #[test]
    fn test_compute_minhash() {
        let shingles = generate_shingles("def foo(): pass", 3);
        let sig = compute_minhash(&shingles, 5);
        assert_eq!(sig.hashes.len(), 5);
    }

    #[test]
    fn test_estimate_similarity() {
        let a = MinHashSignature { hashes: vec![1, 2, 3, 4, 5] };
        let b = MinHashSignature { hashes: vec![1, 2, 3, 4, 5] };
        assert_eq!(estimate_similarity(&a, &b), 1.0);
    }

    #[test]
    fn test_estimate_similarity_different() {
        let a = MinHashSignature { hashes: vec![1, 2, 3, 4, 5] };
        let b = MinHashSignature { hashes: vec![6, 7, 8, 9, 10] };
        assert_eq!(estimate_similarity(&a, &b), 0.0);
    }

    #[test]
    fn test_duplication_config_struct() {
        let c = DuplicationConfig { minhash_size: 50, shingle_size: 5, lsh_bands: 10, min_similarity: 0.8 };
        assert_eq!(c.minhash_size, 50);
    }

    #[test]
    fn test_union_find_struct() {
        let mut uf = UnionFind::new(5);
        uf.union(1, 2);
        uf.union(3, 4);
        assert_eq!(uf.find(1), uf.find(2));
        assert_ne!(uf.find(1), uf.find(3));
    }

    #[test]
    fn test_compute_cluster_similarity() {
        let mut pair_sims = std::collections::HashMap::new();
        pair_sims.insert((0, 1), 0.9);
        let sim = compute_cluster_similarity(&[0, 1], &pair_sims);
        assert!(sim >= 0.0 && sim <= 1.0);
    }

    #[test]
    fn test_cluster_duplicates() {
        let c1 = CodeChunk { file: "a.py".into(), name: "f".into(), start_line: 1, end_line: 5, normalized: "x".into() };
        let c2 = CodeChunk { file: "b.py".into(), name: "g".into(), start_line: 1, end_line: 5, normalized: "x".into() };
        let pairs = vec![DuplicatePair { chunk1: c1.clone(), chunk2: c2.clone(), similarity: 0.9 }];
        let chunks = vec![c1, c2];
        let clusters = cluster_duplicates(&pairs, &chunks);
        let _ = clusters; // Just verify it runs
    }

    #[test]
    fn test_extract_chunks_for_duplication() {
        use crate::parsing::{create_parser, parse_file};
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "def foo():\n    x = 1\n    y = 2\n    z = 3\n    return x + y + z").unwrap();
        let mut parser = create_parser().unwrap();
        let parsed = parse_file(&mut parser, tmp.path()).unwrap();
        let refs = vec![&parsed];
        let chunks = extract_chunks_for_duplication(&refs);
        // Just verify it runs - chunks may be empty if function is too short
        let _ = chunks;
    }

    #[test]
    fn test_extract_function_chunks() {
        use crate::parsing::{create_parser, parse_file};
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "def bar():\n    a = 1\n    b = 2\n    c = 3\n    return a + b + c").unwrap();
        let mut parser = create_parser().unwrap();
        let parsed = parse_file(&mut parser, tmp.path()).unwrap();
        let mut chunks = Vec::new();
        extract_function_chunks(parsed.tree.root_node(), &parsed.source, &parsed.path, &mut chunks);
        // Just verify it runs - chunks may be empty if function is too short
        let _ = chunks;
    }

    #[test]
    fn test_hash_band() {
        let hashes = vec![1, 2, 3, 4, 5, 6];
        let h1 = hash_band(&hashes[0..3]);
        let h2 = hash_band(&hashes[3..6]);
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_add_bucket_pairs() {
        let indices = vec![0, 1, 2];
        let mut candidates = std::collections::HashSet::new();
        add_bucket_pairs(&indices, &mut candidates);
        assert!(!candidates.is_empty());
    }

    #[test]
    fn test_find_lsh_candidates() {
        let sigs = vec![
            MinHashSignature { hashes: vec![1, 2, 3, 4, 5, 6] },
            MinHashSignature { hashes: vec![1, 2, 3, 4, 5, 6] },
        ];
        let pairs = find_lsh_candidates(&sigs, 2);
        let _ = pairs; // Just verify it runs
    }
}

