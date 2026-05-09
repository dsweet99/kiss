use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::analyze_parse::py_parsed_or_log;
use kiss::cli_output::{print_dry_results, print_no_files_message};
use kiss::{
    DuplicatePair, DuplicationConfig, Language, detect_duplicates_from_chunks,
    extract_chunks_for_duplication, extract_rust_chunks_for_duplication, parse_files,
    parse_rust_files,
};

use crate::analyze::focus::gather_files;

/// Inputs for [`run_dry`].
pub struct DryRunParams<'a> {
    pub path: &'a str,
    pub filter_files: &'a [String],
    pub config: &'a DuplicationConfig,
    pub ignore_prefixes: &'a [String],
    pub lang_filter: Option<Language>,
}

pub fn run_dry(p: &DryRunParams<'_>) {
    let DryRunParams {
        path,
        filter_files,
        config,
        ignore_prefixes,
        lang_filter,
    } = p;
    let root = Path::new(path);
    let (py_files, rs_files) = gather_files(root, *lang_filter, ignore_prefixes);

    if py_files.is_empty() && rs_files.is_empty() {
        print_no_files_message(*lang_filter, root);
        return;
    }

    let py_parsed = parse_py_for_dry(&py_files);
    let rs_parsed = parse_rs_for_dry(&rs_files);

    let mut chunks = extract_chunks_for_duplication(&py_parsed.iter().collect::<Vec<_>>());
    chunks.extend(extract_rust_chunks_for_duplication(
        &rs_parsed.iter().collect::<Vec<_>>(),
    ));

    let mut pairs = detect_duplicates_from_chunks(&chunks, config);

    filter_pairs_by_files(&mut pairs, filter_files);

    print_dry_results(&pairs);
}

fn parse_py_for_dry(py_files: &[PathBuf]) -> Vec<kiss::ParsedFile> {
    if py_files.is_empty() {
        Vec::new()
    } else {
        parse_files(py_files)
            .unwrap_or_default()
            .into_iter()
            .filter_map(py_parsed_or_log)
            .collect()
    }
}

fn parse_rs_for_dry(rs_files: &[PathBuf]) -> Vec<kiss::ParsedRustFile> {
    if rs_files.is_empty() {
        Vec::new()
    } else {
        parse_rust_files(rs_files)
            .into_iter()
            .filter_map(Result::ok)
            .collect()
    }
}

fn filter_pairs_by_files(pairs: &mut Vec<DuplicatePair>, filter_files: &[String]) {
    if filter_files.is_empty() {
        return;
    }
    let filters: HashSet<PathBuf> = filter_files
        .iter()
        .map(|f| {
            Path::new(f)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(f))
        })
        .collect();
    pairs.retain(|p| filters.contains(&p.chunk1.file) || filters.contains(&p.chunk2.file));
}

#[cfg(test)]
mod dry_helpers_test {
    use super::{DryRunParams, filter_pairs_by_files, parse_py_for_dry, parse_rs_for_dry};
    use kiss::{DuplicatePair, DuplicationConfig};

    #[test]
    fn empty_inputs_smoke() {
        assert!(parse_py_for_dry(&[]).is_empty());
        assert!(parse_rs_for_dry(&[]).is_empty());
        let mut pairs: Vec<DuplicatePair> = Vec::new();
        filter_pairs_by_files(&mut pairs, &[]);
        assert!(pairs.is_empty());
    }

    #[test]
    fn test_dry_run_params_struct() {
        let config = DuplicationConfig::default();
        let params = DryRunParams {
            path: "/tmp",
            filter_files: &[],
            config: &config,
            ignore_prefixes: &[],
            lang_filter: None,
        };
        assert_eq!(params.path, "/tmp");
        assert!(params.filter_files.is_empty());
    }
}
