use kiss::{
    DuplicateCluster, DuplicationConfig, ParsedFile, ParsedRustFile,
    cluster_duplicates_from_chunks, extract_chunks_for_duplication,
    extract_rust_chunks_for_duplication,
};

pub fn detect_py_duplicates(parsed: &[ParsedFile], min_similarity: f64) -> Vec<DuplicateCluster> {
    let config = DuplicationConfig {
        min_similarity,
        ..Default::default()
    };
    cluster_duplicates_from_chunks(
        &extract_chunks_for_duplication(&parsed.iter().collect::<Vec<_>>()),
        &config,
    )
}

pub fn detect_rs_duplicates(
    parsed: &[ParsedRustFile],
    min_similarity: f64,
) -> Vec<DuplicateCluster> {
    let config = DuplicationConfig {
        min_similarity,
        ..Default::default()
    };
    cluster_duplicates_from_chunks(
        &extract_rust_chunks_for_duplication(&parsed.iter().collect::<Vec<_>>()),
        &config,
    )
}
