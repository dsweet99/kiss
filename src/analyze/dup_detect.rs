use kiss::code_roles::{SourceRoleIndex, is_test_only_file};
use kiss::{
    DuplicateCluster, DuplicationConfig, ParsedFile, ParsedRustFile,
    cluster_duplicates_from_chunks, extract_chunks_for_duplication_with_roles,
    extract_rust_chunks_for_duplication_with_roles,
};

pub fn detect_py_duplicates(
    parsed: &[ParsedFile],
    min_similarity: f64,
    roles: &SourceRoleIndex,
) -> Vec<DuplicateCluster> {
    let config = DuplicationConfig {
        min_similarity,
        ..Default::default()
    };
    let refs: Vec<_> = parsed
        .iter()
        .filter(|p| !is_test_only_file(roles, &p.path))
        .collect();
    cluster_duplicates_from_chunks(
        &extract_chunks_for_duplication_with_roles(&refs, Some(roles)),
        &config,
    )
}

pub fn detect_rs_duplicates(
    parsed: &[ParsedRustFile],
    min_similarity: f64,
    roles: &SourceRoleIndex,
) -> Vec<DuplicateCluster> {
    let config = DuplicationConfig {
        min_similarity,
        ..Default::default()
    };
    let refs: Vec<_> = parsed
        .iter()
        .filter(|p| !is_test_only_file(roles, &p.path))
        .collect();
    cluster_duplicates_from_chunks(
        &extract_rust_chunks_for_duplication_with_roles(&refs, Some(roles)),
        &config,
    )
}
