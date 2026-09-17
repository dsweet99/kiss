use crate::code_roles::{RoleBuildError, is_test_only_file};
use crate::discovery::{Language, gather_files_by_lang};
use crate::duplication::{
    DuplicationConfig, cluster_duplicates, detect_duplicates_from_chunks,
    extract_chunks_for_duplication_with_roles, extract_rust_chunks_for_duplication_with_roles,
};
use crate::gate_config::GateConfig;
use crate::lang_analysis::parse_then_classify;
use crate::parsing::ParsedFile;
use crate::rust_parsing::ParsedRustFile;

pub fn infer_gate_config_for_paths(
    paths: &[String],
    lang: Option<Language>,
    ignore: &[String],
) -> Result<GateConfig, RoleBuildError> {
    let (py_files, rs_files) = gather_files_by_lang(paths, lang, ignore);
    let mut gate = GateConfig::default();

    let (py_parsed, rs_parsed, roles) = parse_then_classify(&py_files, &rs_files)?;
    let py_prod: Vec<ParsedFile> = py_parsed
        .into_iter()
        .filter(|p| !is_test_only_file(&roles, &p.path))
        .collect();
    let rs_prod: Vec<ParsedRustFile> = rs_parsed
        .into_iter()
        .filter(|p| !is_test_only_file(&roles, &p.path))
        .collect();

    gate.duplication_enabled =
        !has_reportable_duplicates(&py_prod, &rs_prod, Some(&roles), gate.min_similarity);
    gate.comment_removal_enabled =
        !crate::has_non_doc_comments_with_roles(&py_prod, &rs_prod, Some(&roles));
    Ok(gate)
}

pub(crate) fn has_reportable_duplicates(
    py_parsed: &[ParsedFile],
    rs_parsed: &[ParsedRustFile],
    roles: Option<&crate::code_roles::SourceRoleIndex>,
    min_similarity: f64,
) -> bool {
    let config = DuplicationConfig {
        min_similarity,
        ..Default::default()
    };
    let py_refs: Vec<&ParsedFile> = py_parsed.iter().collect();
    let rs_refs: Vec<&ParsedRustFile> = rs_parsed.iter().collect();
    let mut chunks = extract_chunks_for_duplication_with_roles(&py_refs, roles);
    chunks.extend(extract_rust_chunks_for_duplication_with_roles(
        &rs_refs, roles,
    ));
    if chunks.len() < 2 {
        return false;
    }
    let pairs = detect_duplicates_from_chunks(&chunks, &config);
    !cluster_duplicates(&pairs, &chunks).is_empty()
}

#[cfg(test)]
mod infer_gate_test {
    use super::{has_reportable_duplicates, infer_gate_config_for_paths};
    use crate::discovery::Language;
    use tempfile::TempDir;

    #[test]
    fn infer_enables_comment_removal_for_clean_python() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("only.py"), "def foo():\n    return 1\n").unwrap();
        let gate = infer_gate_config_for_paths(
            &[tmp.path().to_string_lossy().into_owned()],
            Some(Language::Python),
            &[],
        )
        .unwrap();
        assert!(gate.duplication_enabled);
        assert!(gate.comment_removal_enabled);
        assert!(!has_reportable_duplicates(&[], &[], None, 0.8));
    }

    #[test]
    fn infer_turns_comment_removal_off_when_comments_exist() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("noted.py"),
            "# keep\ndef foo():\n    return 1\n",
        )
        .unwrap();
        let gate = infer_gate_config_for_paths(
            &[tmp.path().to_string_lossy().into_owned()],
            Some(Language::Python),
            &[],
        )
        .unwrap();
        assert!(!gate.comment_removal_enabled);
    }
}
