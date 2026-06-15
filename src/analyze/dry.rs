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
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{DryRunParams, filter_pairs_by_files, parse_py_for_dry, parse_rs_for_dry, run_dry};
    use kiss::{CodeChunk, DuplicatePair, DuplicationConfig};

    #[test]
    fn parse_empty_file_lists_without_work() {
        assert!(parse_py_for_dry(&[]).is_empty());
        assert!(parse_rs_for_dry(&[]).is_empty());
    }

    #[test]
    fn run_dry_empty_directory_leaves_project_unchanged() {
        let temp = tempfile::tempdir().unwrap();
        let config = DuplicationConfig::default();
        let params = DryRunParams {
            path: temp.path().to_str().unwrap(),
            filter_files: &[],
            config: &config,
            ignore_prefixes: &[],
            lang_filter: None,
        };

        run_dry(&params);

        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
    }

    #[test]
    fn run_dry_with_duplicate_python_files_preserves_sources() {
        let temp = tempfile::tempdir().unwrap();
        let source = "\
def duplicated():
    alpha = 1
    beta = 2
    gamma = 3
    delta = 4
    epsilon = 5
    return alpha + beta + gamma + delta + epsilon
";
        let first = temp.path().join("first.py");
        let second = temp.path().join("second.py");
        fs::write(&first, source).unwrap();
        fs::write(&second, source).unwrap();
        let config = DuplicationConfig::default();
        let params = DryRunParams {
            path: temp.path().to_str().unwrap(),
            filter_files: &[],
            config: &config,
            ignore_prefixes: &[],
            lang_filter: None,
        };

        run_dry(&params);

        assert_eq!(fs::read_to_string(first).unwrap(), source);
        assert_eq!(fs::read_to_string(second).unwrap(), source);
    }

    #[test]
    fn filter_pairs_keeps_pairs_that_touch_canonical_filter() {
        let temp = tempfile::tempdir().unwrap();
        let selected = temp.path().join("selected.py");
        fs::write(&selected, "def selected():\n    return 1\n").unwrap();
        let other = temp.path().join("other.py");
        let mut pairs = vec![pair(&selected, &other)];
        let filters = vec![selected.to_string_lossy().into_owned()];

        filter_pairs_by_files(&mut pairs, &filters);

        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].chunk1.file, selected);
    }

    #[test]
    fn filter_pairs_drops_pairs_that_match_no_filters() {
        let temp = tempfile::tempdir().unwrap();
        let selected = temp.path().join("selected.py");
        let unselected = temp.path().join("unselected.py");
        let other = temp.path().join("other.py");
        fs::write(&selected, "def selected():\n    return 1\n").unwrap();
        let mut pairs = vec![pair(&unselected, &other)];
        let filters = vec![selected.to_string_lossy().into_owned()];

        filter_pairs_by_files(&mut pairs, &filters);

        assert!(pairs.is_empty());
    }

    #[test]
    fn filter_pairs_uses_original_filter_when_canonicalize_fails() {
        let mut pairs = vec![pair("virtual.py", "other.py")];
        let filters = vec!["virtual.py".to_string()];

        filter_pairs_by_files(&mut pairs, &filters);

        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].chunk1.file, PathBuf::from("virtual.py"));
    }

    fn pair(first: impl AsRef<Path>, second: impl AsRef<Path>) -> DuplicatePair {
        DuplicatePair {
            chunk1: chunk(first),
            chunk2: chunk(second),
            similarity: 1.0,
        }
    }

    fn chunk(file: impl AsRef<Path>) -> CodeChunk {
        CodeChunk {
            file: file.as_ref().to_path_buf(),
            name: "duplicate".to_string(),
            start_line: 1,
            end_line: 6,
            normalized: "alpha beta gamma delta epsilon zeta eta theta iota kappa".to_string(),
        }
    }
}
