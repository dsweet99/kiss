//! Integration witnesses for per-file `test_coverage` gate (executable-call mode).

use kiss::check_universe_cache::{
    CachedCoverageItem, CachedDuplicateCluster, CachedFileCoverage, FullCheckCache,
};
use kiss::units::{CodeUnitKind, extract_code_units};
use kiss::{
    ConfigError, create_parser, is_in_test_directory, is_test_file, parse_file, parse_files,
};
use std::collections::BTreeMap;
use std::path::Path;

#[test]
fn gate_config_error_display_is_executable() {
    for err in [
        ConfigError::UnknownKey {
            key: "k".into(),
            section: "gate".into(),
        },
        ConfigError::UnknownSection {
            section: "x".into(),
            hint: Some("gate".into()),
        },
        ConfigError::UnknownSection {
            section: "x".into(),
            hint: None,
        },
        ConfigError::InvalidValue {
            key: "k".into(),
            message: "m".into(),
        },
        ConfigError::ParseError {
            message: "m".into(),
        },
        ConfigError::IoError {
            path: "p".into(),
            message: "m".into(),
        },
    ] {
        assert!(!err.to_string().is_empty());
    }
}

#[test]
fn gate_parse_errors_display_is_executable() {
    for err in [
        kiss::ParseError::ParserInitError,
        kiss::ParseError::ParseFailed,
        kiss::ParseError::IoError(std::io::Error::new(std::io::ErrorKind::NotFound, "missing")),
    ] {
        assert!(!err.to_string().is_empty());
    }
}

#[test]
fn gate_rust_parse_errors_display_is_executable() {
    let syn_err = match syn::parse_file("fn (") {
        Ok(_) => panic!("expected syn parse failure"),
        Err(err) => err,
    };
    for err in [
        kiss::RustParseError::IoError(std::io::Error::new(std::io::ErrorKind::NotFound, "missing")),
        kiss::RustParseError::SynError(syn_err),
    ] {
        assert!(!err.to_string().is_empty());
    }
}

#[test]
fn gate_test_detection_helpers_are_executable() {
    assert!(is_test_file(Path::new("test_api.py")));
    assert!(is_test_file(Path::new("api_test.py")));
    assert!(is_test_file(Path::new("conftest.py")));
    assert!(!is_test_file(Path::new("api.py")));
    assert!(is_in_test_directory(Path::new("pkg/tests/api.py")));
    assert!(is_in_test_directory(Path::new("pkg/test/helpers.py")));
    assert!(!is_in_test_directory(Path::new("pkg/source/api.py")));
    assert!(!is_test_file(Path::new("")));
    assert!(!is_in_test_directory(Path::new("")));
}

#[test]
fn gate_code_unit_kind_methods_are_executable() {
    for kind in [
        CodeUnitKind::Function,
        CodeUnitKind::Method,
        CodeUnitKind::Class,
        CodeUnitKind::Module,
        CodeUnitKind::Struct,
        CodeUnitKind::Enum,
        CodeUnitKind::TraitImplMethod,
    ] {
        assert!(!kind.as_str().is_empty());
        assert!(!kind.to_string().is_empty());
    }
}

#[test]
fn gate_extract_code_units_runs_on_fixture() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("sample.py");
    std::fs::write(&path, "def sample():\n    return 1\n").unwrap();
    let mut parser = create_parser().unwrap();
    let parsed = parse_file(&mut parser, &path).unwrap();
    let units = extract_code_units(&parsed);
    assert!(units.iter().any(|u| u.name == "sample"));
}

#[test]
fn gate_check_universe_cache_types_round_trip() {
    let item = CachedCoverageItem {
        file: "a.py".into(),
        name: "fn".into(),
        line: 3,
    };
    let (path, name, line) = item.into_tuple();
    assert_eq!(path, Path::new("a.py"));
    assert_eq!(name, "fn");
    assert_eq!(line, 3);

    let fc = CachedFileCoverage {
        file: "a.py".into(),
        pct: 91,
    };
    let (path, pct) = fc.into_tuple();
    assert_eq!(path, Path::new("a.py"));
    assert_eq!(pct, 91);

    let cluster = CachedDuplicateCluster {
        chunks: vec![],
        avg_similarity: 0.5,
    };
    assert!(cluster.chunks.is_empty());

    let _cache = FullCheckCache {
        fingerprint: String::new(),
        py_stats: None,
        rs_stats: None,
        py_paths: vec![],
        focus_paths: vec![],
        focus_restrict: false,
        rs_paths: vec![],
        py_file_count: 0,
        rs_file_count: 0,
        code_unit_count: 0,
        statement_count: 0,
        graph_nodes: 0,
        graph_edges: 0,
        base_violations: vec![],
        graph_violations: vec![],
        coverage_violations: vec![],
        py_duplicates: vec![],
        rs_duplicates: vec![],
        definitions: vec![],
        unreferenced: vec![],
        weighted_file_pcts: vec![],
        file_content_digests: vec![],
        rslip_fingerprint: String::new(),
        rust_coverage_fingerprint: String::new(),
    };
}

#[test]
fn gate_rslip_public_api_exercises_discovery_and_types() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("test_sample.py"),
        "def test_sample():\n    assert True\n",
    )
    .unwrap();
    std::fs::write(tmp.path().join("app.py"), "VALUE = 1\n").unwrap();

    let digest = rslip::content_digest(b"app");
    assert_ne!(digest, rslip::content_digest(b"other"));
    let normalized = rslip::normalize_path(tmp.path(), Path::new("app.py"));
    assert_eq!(normalized, "app.py");
    assert!(rslip::db_path(tmp.path()).ends_with(".kiss/rslip.json"));

    let files = rslip::discover_repo_files(tmp.path()).unwrap();
    assert!(files.iter().any(|f| f.path == "test_sample.py"));
    assert!(files.iter().any(|f| f.path == "app.py"));
    assert!(matches!(
        files[0].role,
        rslip::FileRole::Source | rslip::FileRole::Test
    ));

    let db = rslip::Database {
        schema_version: rslip::SCHEMA_VERSION,
        rslip_version: rslip::RSLIP_VERSION.to_string(),
        config_fingerprints: BTreeMap::new(),
        files: BTreeMap::new(),
        tests: BTreeMap::new(),
        source_to_covering_tests: BTreeMap::new(),
    };
    rslip::write_database_atomic(tmp.path(), &db).unwrap();
    let loaded = rslip::load_database(tmp.path()).unwrap().unwrap();
    assert_eq!(loaded.schema_version, rslip::SCHEMA_VERSION);
}

#[test]
fn gate_analyze_rust_test_refs_exercises_reference_pipeline() {
    let path = Path::new("tests/fake_rust/syntactic_witness_lib.rs");
    let parsed = kiss::parse_rust_file(path).unwrap();
    let analysis = kiss::analyze_rust_test_refs(&[&parsed], None);
    assert!(!analysis.definitions.is_empty());
}

#[test]
fn gate_rslip_query_covering_tests_smoke() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("app.py"), "def app():\n    return 1\n").unwrap();
    std::fs::write(
        tmp.path().join("test_app.py"),
        "def test_app():\n    assert 1\n",
    )
    .unwrap();
    let file_records = rslip::discover_repo_files(tmp.path()).unwrap();
    let files = file_records
        .iter()
        .map(|file| (file.path.clone(), file.clone()))
        .collect();
    let db = rslip::Database {
        schema_version: rslip::SCHEMA_VERSION,
        rslip_version: rslip::RSLIP_VERSION.to_string(),
        config_fingerprints: rslip::config_fingerprints(&file_records),
        files,
        tests: BTreeMap::new(),
        source_to_covering_tests: BTreeMap::from([(
            "app.py".to_string(),
            vec!["test_app.py::test_app".to_string()],
        )]),
    };
    rslip::write_database_atomic(tmp.path(), &db).unwrap();
    let covering =
        kiss::rslip::query_covering_tests(tmp.path(), &[tmp.path().join("app.py")]).unwrap();
    assert_eq!(covering.len(), 1);
    assert!(covering[0].0.ends_with("test_app.py"));
}

#[test]
fn gate_build_dependency_graph_is_executable() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("mod.py");
    std::fs::write(&path, "import os\n").unwrap();
    let parsed_all = parse_files(&[path]).unwrap();
    let parsed: Vec<&kiss::ParsedFile> = parsed_all
        .iter()
        .filter_map(|result| result.as_ref().ok())
        .collect();
    let graph = kiss::build_dependency_graph(&parsed);
    assert!(!graph.nodes.is_empty());
}

#[test]
fn gate_rule_defs_format_is_executable() {
    let config = kiss::Config::default();
    let gate = kiss::GateConfig::default();
    for rule in kiss::RULES {
        assert!(!rule.format(&config, &gate).is_empty());
        let _ = rule.applies_to_python();
        let _ = rule.applies_to_rust();
    }
    assert!(!kiss::RuleCategory::Functions.python_heading().is_empty());
    assert!(!kiss::RuleCategory::Functions.rust_heading().is_empty());
}

#[test]
fn gate_rust_import_extraction_is_executable() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("lib.rs");
    std::fs::write(&path, "use std::collections::HashMap;\npub fn ok() {}\n").unwrap();
    let parsed_all = kiss::parse_rust_files(&[path]);
    let parsed: Vec<&kiss::ParsedRustFile> = parsed_all
        .iter()
        .filter_map(|result| result.as_ref().ok())
        .collect();
    let graph = kiss::build_rust_dependency_graph(&parsed);
    assert!(!graph.nodes.is_empty());
}
