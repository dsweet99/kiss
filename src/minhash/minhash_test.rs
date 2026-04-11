use std::collections::HashSet;

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

#[test]
fn test_minhash_signature_struct() {
    let sig = MinHashSignature {
        hashes: vec![1, 2, 3],
    };
    assert_eq!(sig.hashes.len(), 3);
}

#[test]
fn test_hash_band() {
    let band = [1u64, 2, 3];
    let h1 = super::hash_band(&band);
    let h2 = super::hash_band(&band);
    assert_eq!(h1, h2);
}

#[test]
fn test_add_bucket_pairs() {
    let indices = vec![0, 1, 2];
    let mut candidates = HashSet::new();
    super::add_bucket_pairs(&indices, &mut candidates);
    assert!(candidates.contains(&(0, 1)));
    assert!(candidates.contains(&(0, 2)));
    assert!(candidates.contains(&(1, 2)));
}

#[test]
fn test_add_bucket_pairs_single() {
    let indices = vec![0];
    let mut candidates = HashSet::new();
    super::add_bucket_pairs(&indices, &mut candidates);
    assert!(candidates.is_empty());
}

// === Bug-hunting tests ===

#[test]
fn test_generate_shingles_zero_size_returns_empty() {
    // shingle_size=0 is degenerate; should return empty set, not panic.
    // windows(0) panics in Rust, so this exposes a missing guard.
    let shingles = generate_shingles("hello world test", 0);
    assert!(shingles.is_empty());
}

#[test]
fn test_estimate_similarity_is_symmetric() {
    // Similarity should be the same regardless of argument order.
    let sig1 = MinHashSignature {
        hashes: vec![1, 2, 3, 4, 5],
    };
    let sig2 = MinHashSignature {
        hashes: vec![1, 2, 3],
    };
    let sim_ab = estimate_similarity(&sig1, &sig2);
    let sim_ba = estimate_similarity(&sig2, &sig1);
    assert!(
        (sim_ab - sim_ba).abs() < f64::EPSILON,
        "Similarity should be symmetric: {sim_ab} vs {sim_ba}"
    );
}
