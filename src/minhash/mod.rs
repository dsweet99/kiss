use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

// Default MinHash size - precompute coefficients for common case
const DEFAULT_MINHASH_SIZE: usize = 100;

// Precomputed coefficients for universal hash family (avoids regeneration per chunk)
static DEFAULT_COEFFICIENTS: LazyLock<[(u64, u64); DEFAULT_MINHASH_SIZE]> = LazyLock::new(|| {
    let mut coeffs = [(0u64, 0u64); DEFAULT_MINHASH_SIZE];
    for (i, coeff) in coeffs.iter_mut().enumerate() {
        let seed = 0x9E37_79B9_7F4A_7C15_u64.wrapping_add(i as u64);
        let a = seed.wrapping_mul(0xBF58_476D_1CE4_E5B9) | 1;
        let b = seed.wrapping_mul(0x94D0_49BB_1331_11EB);
        *coeff = (a, b);
    }
    coeffs
});

#[derive(Debug, Clone)]
pub struct MinHashSignature {
    pub hashes: Vec<u64>,
}

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

pub fn generate_shingles(text: &str, shingle_size: usize) -> HashSet<u64> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    if shingle_size == 0 {
        return HashSet::new();
    }

    let tokens: Vec<&str> = text.split_whitespace().collect();
    // Upper bound: one shingle per token window.
    let approx = tokens.len().saturating_sub(shingle_size).saturating_add(1);
    let mut shingles = HashSet::with_capacity(approx);

    if tokens.len() >= shingle_size {
        for window in tokens.windows(shingle_size) {
            let mut hasher = DefaultHasher::new();
            window.hash(&mut hasher);
            shingles.insert(hasher.finish());
        }
    }

    shingles
}

pub fn compute_minhash<S: std::hash::BuildHasher>(
    shingles: &HashSet<u64, S>,
    size: usize,
) -> MinHashSignature {
    let mut hashes = vec![u64::MAX; size];

    // Use precomputed coefficients for default size, compute dynamically for custom sizes
    // Coefficients for universal hash family: h(x) = (a*x + b)
    // Constants from SplitMix64/xxHash with good avalanche properties
    if size == DEFAULT_MINHASH_SIZE {
        for &shingle in shingles {
            for (i, &(a, b)) in DEFAULT_COEFFICIENTS.iter().enumerate() {
                let h = a.wrapping_mul(shingle).wrapping_add(b);
                if h < hashes[i] {
                    hashes[i] = h;
                }
            }
        }
    } else {
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
    }

    MinHashSignature { hashes }
}

pub fn estimate_similarity(sig1: &MinHashSignature, sig2: &MinHashSignature) -> f64 {
    let max_len = sig1.hashes.len().max(sig2.hashes.len());
    if max_len == 0 {
        return 0.0;
    }
    let matching = sig1
        .hashes
        .iter()
        .zip(&sig2.hashes)
        .filter(|(a, b)| a == b)
        .count();
    // Safe: matching <= max_len which is typically 100, result is 0.0-1.0
    #[allow(clippy::cast_precision_loss)]
    let sim = matching as f64 / max_len as f64;
    sim
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

pub fn find_lsh_candidates(
    signatures: &[MinHashSignature],
    num_bands: usize,
) -> HashSet<(usize, usize)> {
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
#[path = "minhash_test.rs"]
mod tests;
