use super::definitions::RustCodeDefinition;
use super::is_rust_test_file;
use crate::rust_parsing::ParsedRustFile;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use syn::visit::Visit;
use syn::Expr;

const MAX_CLI_ROUTE_WITNESS_DEFS_PER_FILE: usize = 12;

/// Runtime-heavy workflow bodies inside route-attested trees: credit via direct witnesses only.
fn cli_route_bulk_credit_excluded(path: &Path) -> bool {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    stem.ends_with("_session")
        || stem.ends_with("_loop")
        || stem.ends_with("_flow")
        || stem == "behavior"
}

fn path_has_cli_component(path: &Path) -> bool {
    path.components()
        .any(|c| matches!(c, std::path::Component::Normal(s) if s == "cli"))
}

fn cli_path_components(path: &Path) -> Vec<&str> {
    path.components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect()
}

fn cli_token_variants(token: &str) -> Vec<String> {
    let stripped = token.strip_prefix("--").unwrap_or(token);
    let underscored = stripped.replace('-', "_");
    let dashed = stripped.replace('_', "-");
    [stripped.to_string(), underscored, dashed]
        .into_iter()
        .collect()
}

fn cli_token_matches_segment(token: &str, segment: &str) -> bool {
    if segment == token {
        return true;
    }
    for candidate in cli_token_variants(token) {
        if candidate.len() < 3 {
            continue;
        }
        if segment == candidate.as_str()
            || segment.starts_with(&format!("{candidate}_"))
            || segment.ends_with(&format!("_{candidate}"))
            || segment.contains(&format!("_{candidate}_"))
        {
            return true;
        }
    }
    false
}

fn cli_flag_prefix(token: &str) -> Option<&str> {
    let stripped = token.strip_prefix("--").unwrap_or(token);
    if stripped.contains('-') || stripped.contains('_') {
        return stripped
            .split(['-', '_'])
            .next()
            .filter(|p| !p.is_empty());
    }
    None
}

fn is_top_level_cli_file(path: &Path) -> bool {
    let comps = cli_path_components(path);
    let Some(cli_idx) = comps.iter().position(|&c| c == "cli") else {
        return false;
    };
    comps.len() == cli_idx + 2
}

fn token_matches_nested_cli_path(token: &str, path: &Path) -> bool {
    let comps = cli_path_components(path);
    let Some(cli_idx) = comps.iter().position(|&c| c == "cli") else {
        return false;
    };
    comps[cli_idx + 1..]
        .iter()
        .any(|&seg| cli_token_matches_segment(token, seg))
}

fn token_matches_top_level_cli_file(token: &str, path: &Path) -> bool {
    if !is_top_level_cli_file(path) {
        return false;
    }
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if stem.ends_with("_session") {
        return token.len() >= 3 && !token.starts_with('-');
    }
    cli_token_matches_segment(token, stem)
        || cli_token_variants(token)
            .iter()
            .any(|v| cli_token_matches_segment(v, stem))
}

fn token_matches_cli_path(token: &str, path: &Path) -> bool {
    if token_matches_nested_cli_path(token, path) {
        return true;
    }
    if token_matches_top_level_cli_file(token, path) {
        return true;
    }
    if let Some(prefix) = cli_flag_prefix(token) {
        let comps = cli_path_components(path);
        let Some(cli_idx) = comps.iter().position(|&c| c == "cli") else {
            return false;
        };
        return comps[cli_idx + 1..]
            .iter()
            .any(|&seg| seg.starts_with(prefix));
    }
    false
}

fn collect_cli_top_level_stems(definitions: &[RustCodeDefinition]) -> HashSet<String> {
    definitions
        .iter()
        .filter_map(|d| {
            let path = crate::rust_include::canonical_path(&d.file);
            if !is_top_level_cli_file(&path) {
                return None;
            }
            path.file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_string)
        })
        .collect()
}

/// Top-level dispatch entry when a sibling `{stem}_{token}` module exists (structural co-dispatch).
fn top_level_co_dispatch_match(
    token: &str,
    path: &Path,
    sibling_stems: &HashSet<String>,
) -> bool {
    if !is_top_level_cli_file(path) || cli_route_bulk_credit_excluded(path) {
        return false;
    }
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    sibling_stems.iter().any(|s| {
        cli_token_matches_segment(token, s) && (s.starts_with(stem) || stem.starts_with(s))
    })
}

fn nested_cli_route_bulk_credit_excluded(path: &Path) -> bool {
    !is_top_level_cli_file(path) && cli_route_bulk_credit_excluded(path)
}

fn file_matches_cli_route_with_context(
    path: &Path,
    tokens: &HashSet<String>,
    sibling_stems: &HashSet<String>,
) -> bool {
    if nested_cli_route_bulk_credit_excluded(path) {
        return false;
    }
    if !path_has_cli_component(path) {
        return false;
    }
    tokens.iter().any(|t| {
        token_matches_cli_path(t, path) || top_level_co_dispatch_match(t, path, sibling_stems)
    })
}

#[cfg(test)]
pub(crate) fn file_matches_cli_route(path: &Path, tokens: &HashSet<String>) -> bool {
    file_matches_cli_route_with_context(path, tokens, &HashSet::new())
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
    let sibling_stems = collect_cli_top_level_stems(definitions);
    definitions
        .iter()
        .map(|d| crate::rust_include::canonical_path(&d.file))
        .filter(|p| file_matches_cli_route_with_context(p, &tokens, &sibling_stems))
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
    let sibling_stems = collect_cli_top_level_stems(definitions);
    let mut by_file: HashMap<PathBuf, Vec<&RustCodeDefinition>> = HashMap::new();
    for d in definitions {
        let key = crate::rust_include::canonical_path(&d.file);
        if file_matches_cli_route_with_context(&key, &tokens, &sibling_stems) {
            by_file.entry(key).or_default().push(d);
        }
    }
    for defs in by_file.values() {
        if defs.len() > MAX_CLI_ROUTE_WITNESS_DEFS_PER_FILE {
            continue;
        }
        let file = crate::rust_include::canonical_path(&defs[0].file);
        if cli_route_bulk_credit_excluded(&file) {
            continue;
        }
        for d in defs {
            refs.insert(d.name.clone());
        }
    }
}

#[cfg(test)]
#[path = "tests_calibration_route.rs"]
mod tests_calibration_route;
