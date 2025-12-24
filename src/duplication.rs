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
            if !last_was_space && !result.ends_with('N') {
                result.push('N');
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
    let matching = sig1
        .hashes
        .iter()
        .zip(&sig2.hashes)
        .filter(|(a, b)| a == b)
        .count();
    matching as f64 / sig1.hashes.len() as f64
}

/// Find candidate pairs using LSH
fn find_lsh_candidates(
    signatures: &[MinHashSignature],
    num_bands: usize,
) -> std::collections::HashSet<(usize, usize)> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut candidates = std::collections::HashSet::new();

    if signatures.is_empty() {
        return candidates;
    }

    let rows_per_band = (signatures[0].hashes.len() / num_bands).max(1);

    for band_idx in 0..num_bands {
        let band_start = band_idx * rows_per_band;
        let band_end = (band_start + rows_per_band).min(signatures[0].hashes.len());

        if band_start >= signatures[0].hashes.len() {
            break;
        }

        // Build buckets for this band
        let mut buckets: HashMap<u64, Vec<usize>> = HashMap::new();

        for (idx, sig) in signatures.iter().enumerate() {
            let band_slice = &sig.hashes[band_start..band_end];
            let mut hasher = DefaultHasher::new();
            band_slice.hash(&mut hasher);
            let band_hash = hasher.finish();

            buckets.entry(band_hash).or_default().push(idx);
        }

        // Generate candidate pairs from buckets
        for indices in buckets.values() {
            if indices.len() < 2 || indices.len() > 100 {
                continue;
            }
            for i in 0..indices.len() {
                for j in (i + 1)..indices.len() {
                    let (a, b) = (indices[i].min(indices[j]), indices[i].max(indices[j]));
                    candidates.insert((a, b));
                }
            }
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
        assert!(!normalized.contains('1'));
        assert!(!normalized.contains('2'));
        assert!(normalized.contains('n') || normalized.contains('N') || !normalized.chars().any(|c| c.is_ascii_digit()));
    }
}

