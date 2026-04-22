use crate::minhash::{
    compute_minhash, estimate_similarity, find_lsh_candidates, generate_shingles,
};
use crate::parsing::ParsedFile;
use rayon::prelude::*;
use std::cmp::Ordering;

mod clustering;
mod extraction;

#[cfg(test)]
mod tests;

pub use crate::minhash::MinHashSignature;
pub use clustering::{DuplicateCluster, cluster_duplicates};
pub use extraction::{
    CodeChunk, extract_chunks_for_duplication, extract_rust_chunks_for_duplication,
};

pub struct DuplicationConfig {
    pub minhash_size: usize,
    pub shingle_size: usize,
    pub lsh_bands: usize,
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

#[derive(Debug)]
pub struct DuplicatePair {
    pub chunk1: CodeChunk,
    pub chunk2: CodeChunk,
    pub similarity: f64,
}

pub(crate) use clustering::cmp_chunk_key;

fn chunks_are_nested(a: &CodeChunk, b: &CodeChunk) -> bool {
    a.file == b.file
        && ((a.start_line <= b.start_line && b.end_line <= a.end_line)
            || (b.start_line <= a.start_line && a.end_line <= b.end_line))
}

pub fn detect_duplicates(
    parsed_files: &[&ParsedFile],
    config: &DuplicationConfig,
) -> Vec<DuplicatePair> {
    detect_duplicates_from_chunks(&extract_chunks_for_duplication(parsed_files), config)
}

pub fn detect_duplicates_from_chunks(
    chunks: &[CodeChunk],
    config: &DuplicationConfig,
) -> Vec<DuplicatePair> {
    if chunks.len() < 2 {
        return Vec::new();
    }
    // Hot path: computing shingles + MinHash per chunk. Parallelize, but keep a stable
    // signature ordering (par_iter() over slices is indexed, so Vec preserves order).
    let signatures: Vec<MinHashSignature> = chunks
        .par_iter()
        .map(|c| {
            compute_minhash(
                &generate_shingles(&c.normalized, config.shingle_size),
                config.minhash_size,
            )
        })
        .collect();
    let candidates = find_lsh_candidates(&signatures, config.lsh_bands);
    let mut duplicates: Vec<DuplicatePair> = candidates
        .into_iter()
        .filter(|&(i, j)| !chunks_are_nested(&chunks[i], &chunks[j]))
        .filter_map(|(i, j)| {
            let similarity = estimate_similarity(&signatures[i], &signatures[j]);
            (similarity >= config.min_similarity).then(|| DuplicatePair {
                chunk1: chunks[i].clone(),
                chunk2: chunks[j].clone(),
                similarity,
            })
        })
        .collect();
    duplicates.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .unwrap_or(Ordering::Equal)
            .then_with(|| cmp_chunk_key(&a.chunk1, &b.chunk1))
            .then_with(|| cmp_chunk_key(&a.chunk2, &b.chunk2))
    });
    duplicates
}

/// Optimized duplication pipeline used by `kiss check`.
///
/// This produces the same `DuplicateCluster` output as
/// `cluster_duplicates(&detect_duplicates_from_chunks(chunks, config), chunks)` but avoids
/// cloning `CodeChunk` values for intermediate pairs/candidates.
#[must_use]
pub fn cluster_duplicates_from_chunks(
    chunks: &[CodeChunk],
    config: &DuplicationConfig,
) -> Vec<DuplicateCluster> {
    if chunks.len() < 2 {
        return Vec::new();
    }

    let signatures: Vec<MinHashSignature> = chunks
        .par_iter()
        .map(|c| {
            compute_minhash(
                &generate_shingles(&c.normalized, config.shingle_size),
                config.minhash_size,
            )
        })
        .collect();

    let candidates: Vec<(usize, usize)> = find_lsh_candidates(&signatures, config.lsh_bands)
        .into_iter()
        .collect();

    // Compute similarity in parallel; only keep pairs that pass threshold.
    // Skip nested pairs (parent function vs its inner closure in the same file).
    let good_pairs: Vec<(usize, usize, f64)> = candidates
        .par_iter()
        .filter(|&&(i, j)| !chunks_are_nested(&chunks[i], &chunks[j]))
        .filter_map(|&(i, j)| {
            let similarity = estimate_similarity(&signatures[i], &signatures[j]);
            (similarity >= config.min_similarity).then_some((i, j, similarity))
        })
        .collect();

    if good_pairs.is_empty() {
        return Vec::new();
    }

    clustering::cluster_from_pairs(chunks, good_pairs)
}
