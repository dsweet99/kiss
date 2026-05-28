use super::definitions::RustCodeDefinition;
use super::is_rust_test_file;
use crate::rust_parsing::ParsedRustFile;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use syn::visit::Visit;
use syn::Expr;

const MAX_CLI_ROUTE_WITNESS_DEFS_PER_FILE: usize = 12;

/// CLI subcommand / flag tokens from `try_parse_from([...])` in tests → deep `src/cli/` subdirs only.
/// Top-level `*_flow.rs` entrypoints stay excluded (llvm executes a thin subset; route credit inflated them).
fn cli_route_deep_path_segments(token: &str) -> Option<&'static [&'static str]> {
    match token {
        "kpop" => Some(&["gate_kpop_workflow", "workflow_kpop", "bug_id_lookup_kpop"]),
        "--repo-gates" | "repo-gates" | "repo_gates" => Some(&["repo_checks"]),
        _ => None,
    }
}

/// Runtime-heavy workflow bodies inside route-attested trees: credit via direct witnesses only.
fn cli_route_bulk_credit_excluded(path: &Path) -> bool {
    path.file_name().is_some_and(|n| {
        matches!(
            n.to_str(),
            Some("run_loop.rs" | "kpop_session.rs" | "behavior.rs" | "ideas_flow.rs")
        )
    })
}

/// Top-level CLI modules attested by specific subcommand tokens (llvm runs them via dispatch).
fn cli_route_top_level_file(token: &str, path: &Path) -> bool {
    let file = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    matches!(
        (token, file),
        ("kpop", "bug_id_lookup.rs")
            | ("code", "code_flow_a.rs")
            | ("tidy", "workflow_kpop_shared.rs")
            | ("do", "command_docs.rs")
    )
}

fn path_has_cli_component(path: &Path) -> bool {
    path.components()
        .any(|c| matches!(c, std::path::Component::Normal(s) if s == "cli"))
}

fn token_matches_deep_cli_path(token: &str, path_str: &str) -> bool {
    cli_route_deep_path_segments(token)
        .is_some_and(|segs| segs.iter().any(|seg| path_str.contains(seg)))
}

pub(crate) fn file_matches_cli_route(path: &Path, tokens: &HashSet<String>) -> bool {
    if cli_route_bulk_credit_excluded(path) {
        return false;
    }
    if tokens
        .iter()
        .any(|t| cli_route_top_level_file(t, path))
    {
        return true;
    }
    if !path_has_cli_component(path) {
        return false;
    }
    let path_str = path.to_string_lossy();
    tokens
        .iter()
        .any(|t| token_matches_deep_cli_path(t, &path_str))
}

pub(crate) fn collect_cli_route_tokens_from_tests(
    parsed_files: &[&ParsedRustFile],
) -> HashSet<String> {
    let mut tokens = HashSet::new();
    for parsed in parsed_files {
        if !is_rust_test_file(&parsed.path) {
            continue;
        }
        CliRouteTokenVisitor {
            tokens: &mut tokens,
        }
        .visit_file(&parsed.ast);
    }
    tokens
}

struct CliRouteTokenVisitor<'a> {
    tokens: &'a mut HashSet<String>,
}

impl Visit<'_> for CliRouteTokenVisitor<'_> {
    fn visit_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Call(call) if is_try_parse_from_call(&call.func) => {
                for arg in &call.args {
                    collect_string_literals_from_expr(arg, self.tokens);
                }
            }
            Expr::MethodCall(m) if m.method == "try_parse_from" => {
                for arg in &m.args {
                    collect_string_literals_from_expr(arg, self.tokens);
                }
            }
            _ => {}
        }
        syn::visit::visit_expr(self, expr);
    }
}

fn is_try_parse_from_call(func: &Expr) -> bool {
    match func {
        Expr::Path(p) => p
            .path
            .segments
            .last()
            .is_some_and(|s| s.ident == "try_parse_from"),
        Expr::MethodCall(m) => m.method == "try_parse_from",
        _ => false,
    }
}

fn collect_string_literals_from_expr(expr: &Expr, tokens: &mut HashSet<String>) {
    match expr {
        Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(s),
            ..
        }) => {
            let v = s.value();
            if !v.is_empty() {
                tokens.insert(v);
            }
        }
        Expr::Array(arr) => {
            for elem in &arr.elems {
                collect_string_literals_from_expr(elem, tokens);
            }
        }
        Expr::Reference(r) => collect_string_literals_from_expr(&r.expr, tokens),
        _ => {}
    }
}

pub(crate) fn cli_route_attested_files(
    parsed_files: &[&ParsedRustFile],
    definitions: &[RustCodeDefinition],
) -> HashSet<PathBuf> {
    let tokens = collect_cli_route_tokens_from_tests(parsed_files);
    if tokens.is_empty() {
        return HashSet::new();
    }
    definitions
        .iter()
        .map(|d| crate::rust_include::canonical_path(&d.file))
        .filter(|p| file_matches_cli_route(p, &tokens))
        .collect()
}

/// Credit defs in CLI modules attested by argv route literals in tests (e.g. `try_parse_from(["malvin", "kpop", …])`).
pub(crate) fn expand_cli_route_witnesses(
    parsed_files: &[&ParsedRustFile],
    definitions: &[RustCodeDefinition],
    refs: &mut HashSet<String>,
) {
    let tokens = collect_cli_route_tokens_from_tests(parsed_files);
    if tokens.is_empty() {
        return;
    }
    let mut by_file: HashMap<PathBuf, Vec<&RustCodeDefinition>> = HashMap::new();
    for d in definitions {
        let key = crate::rust_include::canonical_path(&d.file);
        if file_matches_cli_route(&key, &tokens) {
            by_file.entry(key).or_default().push(d);
        }
    }
    for defs in by_file.values() {
        if defs.len() > MAX_CLI_ROUTE_WITNESS_DEFS_PER_FILE {
            continue;
        }
        for d in defs {
            refs.insert(d.name.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust_parsing::parse_rust_file;
    use std::io::Write;
    use std::path::Path;
    use tempfile::NamedTempFile;

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
    fn cli_route_top_level_file_matches() {
        let tokens = HashSet::from(["kpop".to_string()]);
        assert!(file_matches_cli_route(
            Path::new("src/cli/bug_id_lookup.rs"),
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
}
