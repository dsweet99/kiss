use std::collections::HashSet;

use super::*;

#[test]
fn test_normalize_code() {
    assert_eq!(normalize_code("x = 123"), "x = 123");
    assert_eq!(normalize_code("  hello   world  "), "hello world");
    assert_ne!(
        normalize_code("result = compute(123, 456)"),
        normalize_code("result = compute(999, 111)")
    );
}

#[test]
fn test_normalize_code_handles_newlines_and_default_minhash_size() {
    assert_eq!(normalize_code("hello\n\tworld"), "hello world");
    assert_eq!(normalize_code(""), "");

    let shingles = generate_shingles("one two three four", 2);
    let sig = compute_minhash(&shingles, DEFAULT_MINHASH_SIZE);
    assert_eq!(sig.hashes.len(), DEFAULT_MINHASH_SIZE);
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

#[test]
fn test_custom_minhash_size_and_empty_similarity() {
    let shingles = generate_shingles("the quick brown fox jumps", 2);
    let sig = compute_minhash(&shingles, 7);
    assert_eq!(sig.hashes.len(), 7);
    assert!(sig.hashes.iter().any(|hash| *hash != u64::MAX));

    let empty_a = MinHashSignature { hashes: Vec::new() };
    let empty_b = MinHashSignature { hashes: Vec::new() };
    assert_eq!(estimate_similarity(&empty_a, &empty_b), 0.0);
}

#[test]
fn test_lsh_candidates_handles_empty_and_too_many_bucket_members() {
    assert!(find_lsh_candidates(&[], 20).is_empty());

    let sig = MinHashSignature {
        hashes: vec![1, 2, 3],
    };
    let signatures = vec![sig; 101];
    assert!(
        find_lsh_candidates(&signatures, 1).is_empty(),
        "oversized buckets are ignored to avoid quadratic blowups"
    );
}

#[test]
fn test_generate_shingles_zero_size_returns_empty() {
    let shingles = generate_shingles("hello world test", 0);
    assert!(shingles.is_empty());
}

#[test]
fn test_estimate_similarity_is_symmetric() {
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

#[test]
fn test_minhash_edge_cases_cover_empty_and_short_bands() {
    assert_eq!(normalize_code("MIXED\tCase"), "mixed case");
    assert!(generate_shingles("one two", 3).is_empty());
    assert_eq!(DEFAULT_COEFFICIENTS.len(), DEFAULT_MINHASH_SIZE);
    assert!(DEFAULT_COEFFICIENTS.iter().all(|(a, _)| a % 2 == 1));

    let empty = HashSet::<u64>::new();
    let sig = compute_minhash(&empty, 3);
    assert_eq!(sig.hashes, vec![u64::MAX; 3]);

    let short = MinHashSignature { hashes: vec![7, 8] };
    let candidates = find_lsh_candidates(&[short.clone(), short], 5);
    assert!(candidates.contains(&(0, 1)));
}

#[test]
fn test_minhash_covers_whitespace_non_ascii_and_band_boundaries() {
    assert_eq!(normalize_code(" \n\t "), "");
    assert_eq!(normalize_code("CAFÉ  VALUE"), "cafÉ value");

    let empty = HashSet::<u64>::new();
    let default_empty = compute_minhash(&empty, DEFAULT_MINHASH_SIZE);
    assert_eq!(default_empty.hashes, vec![u64::MAX; DEFAULT_MINHASH_SIZE]);

    let partial_a = MinHashSignature {
        hashes: vec![1, 2, 3, 4],
    };
    let partial_b = MinHashSignature {
        hashes: vec![1, 9, 3, 8],
    };
    assert_eq!(estimate_similarity(&partial_a, &partial_b), 0.5);

    let mut reversed = HashSet::new();
    super::add_bucket_pairs(&[2, 0], &mut reversed);
    assert!(reversed.contains(&(0, 2)));

    let sig = MinHashSignature { hashes: vec![42] };
    let candidates = find_lsh_candidates(&[sig.clone(), sig], 10);
    assert_eq!(candidates, HashSet::from([(0, 1)]));
}
