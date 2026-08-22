use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::analyze_parse::{ParseAllTimedParams, parse_all_timed};
use kiss::cli_output::{print_dry_results, print_no_files_message};
use kiss::{
    Config, DuplicatePair, DuplicationConfig, Language, detect_duplicates_from_chunks,
    extract_chunks_for_duplication_with_roles, extract_rust_chunks_for_duplication_with_roles,
};

use crate::analyze::focus::gather_files;

pub struct DryRunParams<'a> {
    pub path: &'a str,
    pub filter_files: &'a [String],
    pub config: &'a DuplicationConfig,
    pub ignore_prefixes: &'a [String],
    pub lang_filter: Option<Language>,
    pub language_tables: kiss::LanguageTablesPresent,
}

pub fn run_dry(p: &DryRunParams<'_>) -> i32 {
    let DryRunParams {
        path,
        filter_files,
        config,
        ignore_prefixes,
        lang_filter,
        language_tables,
    } = p;
    let root = Path::new(path);
    let (py_files, rs_files) = gather_files(root, *lang_filter, ignore_prefixes);
    if let Err(code) =
        crate::bin_cli::util::reject_unconfigured_languages(&py_files, &rs_files, *language_tables)
    {
        return code;
    }

    if py_files.is_empty() && rs_files.is_empty() {
        print_no_files_message(*lang_filter, root);
        return 0;
    }

    let (result, _) = match parse_all_timed(ParseAllTimedParams {
        py_files: &py_files,
        rs_files: &rs_files,
        py_config: &Config::python_defaults(),
        rs_config: &Config::rust_defaults(),
        show_timing: false,
    }) {
        Ok(ok) => ok,
        Err(err) => {
            eprintln!("{err}");
            return 1;
        }
    };

    let mut chunks = extract_chunks_for_duplication_with_roles(
        &result.py_parsed.iter().collect::<Vec<_>>(),
        Some(&result.roles),
    );
    chunks.extend(extract_rust_chunks_for_duplication_with_roles(
        &result.rs_parsed.iter().collect::<Vec<_>>(),
        Some(&result.roles),
    ));

    let mut pairs = detect_duplicates_from_chunks(&chunks, config);

    filter_pairs_by_files(&mut pairs, filter_files);

    print_dry_results(&pairs);
    0
}

#[cfg(test)]
fn parse_py_for_dry(py_files: &[PathBuf]) -> Vec<kiss::ParsedFile> {
    assert!(py_files.is_empty());
    Vec::new()
}

#[cfg(test)]
fn parse_rs_for_dry(rs_files: &[PathBuf]) -> Vec<kiss::ParsedRustFile> {
    assert!(rs_files.is_empty());
    Vec::new()
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
    use super::{DryRunParams, filter_pairs_by_files, parse_py_for_dry, parse_rs_for_dry, run_dry};
    use kiss::{DuplicatePair, DuplicationConfig};

    impl<'a> DryRunParams<'a> {
        fn witness(config: &'a DuplicationConfig) -> Self {
            Self {
                path: "/tmp",
                filter_files: &[],
                config,
                ignore_prefixes: &[],
                lang_filter: None,
                language_tables: kiss::LanguageTablesPresent::both(),
            }
        }
    }

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
            language_tables: kiss::LanguageTablesPresent::both(),
        };
        assert_eq!(params.path, "/tmp");
        assert!(params.filter_files.is_empty());
    }

    #[test]
    fn witness_dry_run_params_assoc() {
        let config = DuplicationConfig::default();
        let _ = DryRunParams::witness(&config);
    }

    #[test]
    fn run_dry_handles_empty_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let config = DuplicationConfig::default();
        run_dry(&DryRunParams {
            path: tmp.path().to_str().unwrap(),
            filter_files: &[],
            config: &config,
            ignore_prefixes: &[],
            lang_filter: None,
            language_tables: kiss::LanguageTablesPresent::both(),
        });
    }

    #[test]
    fn filter_pairs_by_files_keeps_matching_chunk() {
        let tmp = tempfile::tempdir().unwrap();
        let kept = tmp.path().join("kept.py");
        std::fs::write(&kept, "def k():\n    return 1\n").unwrap();
        let chunk = kiss::CodeChunk {
            file: kept.clone(),
            name: "k".into(),
            start_line: 1,
            end_line: 2,
            normalized: "def k".into(),
        };
        let other = kiss::CodeChunk {
            file: tmp.path().join("other.py"),
            name: "o".into(),
            start_line: 1,
            end_line: 2,
            normalized: "def o".into(),
        };
        let mut pairs = vec![kiss::DuplicatePair {
            chunk1: chunk,
            chunk2: other,
            similarity: 1.0,
        }];
        filter_pairs_by_files(&mut pairs, &[kept.to_string_lossy().into_owned()]);
        assert_eq!(pairs.len(), 1);
    }

    #[test]
    fn run_dry_parses_python_file() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("alpha.py"), "def alpha():\n    return 1\n").unwrap();
        let config = DuplicationConfig::default();
        let code = run_dry(&DryRunParams {
            path: tmp.path().to_str().unwrap(),
            filter_files: &[],
            config: &config,
            ignore_prefixes: &[],
            lang_filter: Some(kiss::Language::Python),
            language_tables: kiss::LanguageTablesPresent::both(),
        });
        assert_eq!(code, 0);
    }
}
