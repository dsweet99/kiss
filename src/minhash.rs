//! MinHash/LSH algorithms for code similarity detection

use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct MinHashSignature {
    pub hashes: Vec<u64>,
}

/// Normalize code: lowercase, collapse whitespace, replace numbers with 'N'
pub fn normalize_code(code: &str) -> String {
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
pub fn generate_shingles(text: &str, shingle_size: usize) -> HashSet<u64> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let tokens: Vec<&str> = text.split_whitespace().collect();
    let mut shingles = HashSet::new();

    if tokens.len() >= shingle_size {
        for window in tokens.windows(shingle_size) {
            let mut hasher = DefaultHasher::new();
            window.hash(&mut hasher);
            shingles.insert(hasher.finish());
        }
    }

    shingles
}

/// Compute `MinHash` signature for a set of shingles
pub fn compute_minhash(shingles: &HashSet<u64>, size: usize) -> MinHashSignature {
    let mut hashes = vec![u64::MAX; size];
    let coefficients: Vec<(u64, u64)> = (0..size)
        .map(|i| {
            let seed = 0x9E37_79B9_7F4A_7C15_u64.wrapping_add(i as u64);
            let a = seed.wrapping_mul(0xBF58_476D_1CE4_E5B9) | 1;
            let b = seed.wrapping_mul(0x94D0_49BB_1331_11EB);
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

/// Estimate Jaccard similarity from `MinHash` signatures
pub fn estimate_similarity(sig1: &MinHashSignature, sig2: &MinHashSignature) -> f64 {
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

fn add_bucket_pairs(indices: &[usize], candidates: &mut HashSet<(usize, usize)>) {
    if indices.len() < 2 || indices.len() > 100 {
        return;
    }
    for i in 0..indices.len() {
        for j in (i + 1)..indices.len() {
            candidates.insert((indices[i].min(indices[j]), indices[i].max(indices[j])));
        }
    }
}

/// Find candidate pairs using LSH
pub fn find_lsh_candidates(signatures: &[MinHashSignature], num_bands: usize) -> HashSet<(usize, usize)> {
    let mut candidates = HashSet::new();
    if signatures.is_empty() {
        return candidates;
    }

    let hash_len = signatures[0].hashes.len();
    let rows_per_band = (hash_len / num_bands).max(1);

    for band_idx in 0..num_bands {
        let band_start = band_idx * rows_per_band;
        if band_start >= hash_len {
            break;
        }
        let band_end = (band_start + rows_per_band).min(hash_len);

        let mut buckets: HashMap<u64, Vec<usize>> = HashMap::new();
        for (idx, sig) in signatures.iter().enumerate() {
            buckets
                .entry(hash_band(&sig.hashes[band_start..band_end]))
                .or_default()
                .push(idx);
        }

        for indices in buckets.values() {
            add_bucket_pairs(indices, &mut candidates);
        }
    }

    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_code() {
        assert_eq!(normalize_code("x = 123"), "x = N");
        assert_eq!(normalize_code("  hello   world  "), "hello world");
    }

    #[test]
    fn test_shingles() {
        let text = "a b c d e";
        let shingles = generate_shingles(text, 3);
        assert!(!shingles.is_empty());
    }

    #[test]
    fn test_minhash_identical() {
        let shingles = generate_shingles("the quick brown fox", 2);
        let sig1 = compute_minhash(&shingles, 100);
        let sig2 = compute_minhash(&shingles, 100);
        assert!((estimate_similarity(&sig1, &sig2) - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_minhash_different() {
        let shingles1 = generate_shingles("the quick brown fox", 2);
        let shingles2 = generate_shingles("completely different text here", 2);
        let sig1 = compute_minhash(&shingles1, 100);
        let sig2 = compute_minhash(&shingles2, 100);
        assert!(estimate_similarity(&sig1, &sig2) < 0.5);
    }

    #[test]
    fn test_lsh_candidates() {
        let shingles = generate_shingles("some sample text here", 2);
        let sig = compute_minhash(&shingles, 100);
        let signatures = vec![sig.clone(), sig.clone(), sig];
        let candidates = find_lsh_candidates(&signatures, 20);
        assert!(!candidates.is_empty());
    }
}

