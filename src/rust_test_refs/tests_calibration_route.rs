use super::RustCodeDefinition;
use super::*;
use crate::rust_parsing::parse_rust_file;
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

#[test]
fn cli_route_inventory_subdir_matches() {
    let tokens = HashSet::from(["inventory".to_string()]);
    assert!(file_matches_cli_route(
        Path::new("src/cli/inventory/list.rs"),
        &tokens
    ));
}

#[test]
fn cli_route_token_variant_underscore_dash() {
    let tokens = HashSet::from(["repo_gates".to_string()]);
    assert!(file_matches_cli_route(
        Path::new("src/cli/repo_checks/mod.rs"),
        &tokens
    ));
}

#[test]
fn cli_route_rejects_unrelated_cli_subtree() {
    let tokens = HashSet::from(["inventory".to_string()]);
    assert!(!file_matches_cli_route(
        Path::new("src/cli/billing/list.rs"),
        &tokens
    ));
}

#[test]
fn cli_route_helper_functions_cover_branches() {
    assert_eq!(
        cli_token_variants("--repo-gates"),
        vec!["repo-gates", "repo_gates", "repo-gates"]
    );
    assert!(cli_token_matches_segment("kpop", "gate_kpop_workflow"));
    assert!(cli_token_matches_segment("inventory", "inventory"));
    assert_eq!(cli_flag_prefix("--repo-gates"), Some("repo"));
    assert!(is_top_level_cli_file(Path::new("src/cli/code_flow_a.rs")));
    assert!(!is_top_level_cli_file(Path::new("src/cli/inventory/list.rs")));
    assert!(token_matches_nested_cli_path(
        "inventory",
        Path::new("src/cli/inventory/list.rs")
    ));
    assert!(token_matches_top_level_cli_file(
        "code",
        Path::new("src/cli/code_flow_a.rs")
    ));
    assert!(token_matches_cli_path(
        "inventory",
        Path::new("src/cli/inventory/list.rs")
    ));
    let comps = cli_path_components(Path::new("src/cli/inventory/list.rs"));
    assert_eq!(comps, vec!["src", "cli", "inventory", "list.rs"]);
    let stems = HashSet::from(["bug_id_lookup_kpop".to_string()]);
    assert!(file_matches_cli_route_with_context(
        Path::new("src/cli/bug_id_lookup.rs"),
        &HashSet::from(["kpop".to_string()]),
        &stems
    ));
    assert!(!file_matches_cli_route_with_context(
        Path::new("src/lib.rs"),
        &HashSet::from(["kpop".to_string()]),
        &stems
    ));
}

#[test]
fn cli_route_maps_kpop_to_gate_workflow() {
    let tokens = HashSet::from(["kpop".to_string()]);
    assert!(file_matches_cli_route(
        Path::new("src/cli/gate_kpop_workflow/params.rs"),
        &tokens
    ));
}

#[test]
fn cli_route_maps_repo_gates_flag() {
    let tokens = HashSet::from(["--repo-gates".to_string()]);
    assert!(file_matches_cli_route(
        Path::new("src/cli/repo_checks/gate_run.rs"),
        &tokens
    ));
}

#[test]
fn cli_route_excludes_bulk_credit_files() {
    let tokens = HashSet::from(["kpop".to_string()]);
    assert!(!file_matches_cli_route(
        Path::new("src/cli/gate_kpop_workflow/run_loop.rs"),
        &tokens
    ));
}

#[test]
fn cli_route_flag_prefix_matches_repo_checks() {
    let tokens = HashSet::from(["--repo-gates".to_string()]);
    assert!(file_matches_cli_route(
        Path::new("src/cli/repo_checks/gate_run.rs"),
        &tokens
    ));
    assert!(!file_matches_cli_route(Path::new("src/lib.rs"), &tokens));
}

#[test]
fn cli_route_top_level_stem_contains_token() {
    let tokens = HashSet::from(["code".to_string()]);
    assert!(file_matches_cli_route(
        Path::new("src/cli/code_flow_a.rs"),
        &tokens
    ));
}

    #[test]
    fn cli_route_co_dispatch_credits_shared_stem_entry() {
        let defs = vec![
            RustCodeDefinition {
                name: "lookup".into(),
                kind: crate::units::CodeUnitKind::Function,
                file: PathBuf::from("src/cli/bug_id_lookup.rs"),
                line: 1,
                end_line: 5,
                impl_for_type: None,
            },
            RustCodeDefinition {
                name: "run".into(),
                kind: crate::units::CodeUnitKind::Function,
                file: PathBuf::from("src/cli/bug_id_lookup_kpop.rs"),
                line: 1,
                end_line: 5,
                impl_for_type: None,
            },
        ];
        let mut f = NamedTempFile::with_suffix("_test.rs").unwrap();
        write!(
            f,
            "fn t() {{ let _ = Cli::try_parse_from([\"malvin\", \"kpop\"]); }}"
        )
        .unwrap();
        let parsed = parse_rust_file(f.path()).unwrap();
        let mut refs = HashSet::new();
        expand_cli_route_witnesses(&[&parsed], &defs, &mut refs);
        assert!(refs.contains("lookup"));
    }

    #[test]
    fn cli_route_top_level_kpop_sibling_matches() {
    let tokens = HashSet::from(["kpop".to_string()]);
    assert!(file_matches_cli_route(
        Path::new("src/cli/bug_id_lookup_kpop.rs"),
        &tokens
    ));
}

#[test]
fn cli_route_rejects_non_cli_paths() {
    let tokens = HashSet::from(["kpop".to_string()]);
    assert!(!file_matches_cli_route(Path::new("src/lib.rs"), &tokens));
}

#[test]
fn cli_route_deep_segments_cover_repo_gates_aliases() {
    let tokens = HashSet::from(["repo-gates".to_string(), "repo_gates".to_string()]);
    assert!(file_matches_cli_route(
        Path::new("src/cli/repo_checks/mod.rs"),
        &tokens
    ));
}

#[test]
fn collect_tokens_from_try_parse_from() {
    let mut f = NamedTempFile::with_suffix("_test.rs").unwrap();
    write!(
        f,
        "fn t() {{ let _ = Cli::try_parse_from([\"malvin\", \"kpop\", \"--doc\"]); }}"
    )
    .unwrap();
    let parsed = parse_rust_file(f.path()).unwrap();
    let tokens = collect_cli_route_tokens_from_tests(&[&parsed]);
    assert!(tokens.contains("kpop"));
    assert!(tokens.contains("malvin"));
}

#[test]
fn collect_tokens_from_method_try_parse_from() {
    let mut f = NamedTempFile::with_suffix("_test.rs").unwrap();
    write!(
        f,
        "fn t() {{ let _ = cli.try_parse_from([\"malvin\", \"tidy\"]); }}"
    )
    .unwrap();
    let parsed = parse_rust_file(f.path()).unwrap();
    let tokens = collect_cli_route_tokens_from_tests(&[&parsed]);
    assert!(tokens.contains("tidy"));
}

#[test]
fn cli_route_attested_files_filters_definitions() {
    let defs = vec![
        RustCodeDefinition {
            name: "run".into(),
            kind: crate::units::CodeUnitKind::Function,
            file: PathBuf::from("src/cli/gate_kpop_workflow/run.rs"),
            line: 1,
            end_line: 10,
            impl_for_type: None,
        },
        RustCodeDefinition {
            name: "other".into(),
            kind: crate::units::CodeUnitKind::Function,
            file: PathBuf::from("src/lib.rs"),
            line: 1,
            end_line: 10,
            impl_for_type: None,
        },
    ];
    let mut f = NamedTempFile::with_suffix("_test.rs").unwrap();
    write!(
        f,
        "fn t() {{ let _ = Cli::try_parse_from([\"malvin\", \"kpop\"]); }}"
    )
    .unwrap();
    let parsed = parse_rust_file(f.path()).unwrap();
    let attested = cli_route_attested_files(&[&parsed], &defs);
    assert_eq!(attested.len(), 1);
    assert!(attested.contains(&PathBuf::from("src/cli/gate_kpop_workflow/run.rs")));
}

#[test]
fn expand_cli_route_witnesses_credits_attested_defs() {
    let defs = vec![RustCodeDefinition {
        name: "helper".into(),
        kind: crate::units::CodeUnitKind::Function,
        file: PathBuf::from("src/cli/repo_checks/gate_run.rs"),
        line: 1,
        end_line: 5,
        impl_for_type: None,
    }];
    let mut f = NamedTempFile::with_suffix("_test.rs").unwrap();
    write!(
        f,
        "fn t() {{ let _ = Cli::try_parse_from([\"malvin\", \"--repo-gates\"]); }}"
    )
    .unwrap();
    let parsed = parse_rust_file(f.path()).unwrap();
    let mut refs = HashSet::new();
    expand_cli_route_witnesses(&[&parsed], &defs, &mut refs);
    assert!(refs.contains("helper"));
}

/// principles.md: CLI argv→path routing must not rely on benchmark token tables.
#[test]
fn cli_route_subcommand_witnesses_follow_structure_not_benchmark_table() {
    let mut test_rs = NamedTempFile::with_suffix("_test.rs").unwrap();
    write!(
        test_rs,
        "fn t() {{ let _ = Cli::try_parse_from([\"app\", \"inventory\"]); }}"
    )
    .unwrap();
    let parsed = parse_rust_file(test_rs.path()).unwrap();
    let defs = vec![RustCodeDefinition {
        name: "list_items".into(),
        kind: crate::units::CodeUnitKind::Function,
        file: PathBuf::from("src/cli/inventory/list.rs"),
        line: 1,
        end_line: 10,
        impl_for_type: None,
    }];
    let mut refs = HashSet::new();
    expand_cli_route_witnesses(&[&parsed], &defs, &mut refs);
    assert!(
        refs.contains("list_items"),
        "CLI subcommand token should structurally credit same-named src/cli/ subtree"
    );
}

#[test]
fn expand_cli_route_witnesses_skips_large_files() {
    let file = PathBuf::from("src/cli/gate_kpop_workflow/big.rs");
    let defs: Vec<RustCodeDefinition> = (0..13)
        .map(|i| RustCodeDefinition {
            name: format!("f{i}"),
            kind: crate::units::CodeUnitKind::Function,
            file: file.clone(),
            line: i + 1,
            end_line: i + 2,
            impl_for_type: None,
        })
        .collect();
    let mut f = NamedTempFile::with_suffix("_test.rs").unwrap();
    write!(
        f,
        "fn t() {{ let _ = Cli::try_parse_from([\"malvin\", \"kpop\"]); }}"
    )
    .unwrap();
    let parsed = parse_rust_file(f.path()).unwrap();
    let mut refs = HashSet::new();
    expand_cli_route_witnesses(&[&parsed], &defs, &mut refs);
    assert!(refs.is_empty());
}
