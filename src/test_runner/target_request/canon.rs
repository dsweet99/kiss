use std::path::{Path, PathBuf};

use super::types::{OperandExpr, TargetFocus, TargetRequest};

pub(crate) fn canonicalize_target_request(
    mut request: TargetRequest,
    repo_root: Option<&Path>,
) -> TargetRequest {
    request.ignore = kiss::normalize_ignore_prefixes(&request.ignore);
    request.ignore.sort();
    request.ignore.dedup();
    if let TargetFocus::Operands(operands) = &mut request.focus {
        *operands = canonicalize_operands(operands, repo_root);
    }
    request
}

fn canonicalize_operands(operands: &[OperandExpr], repo_root: Option<&Path>) -> Vec<OperandExpr> {
    let mut out: Vec<OperandExpr> = operands
        .iter()
        .map(|operand| OperandExpr {
            raw: canonicalize_operand_raw(&operand.raw, repo_root),
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

fn canonicalize_operand_raw(raw: &str, repo_root: Option<&Path>) -> String {
    let normalized = colon_to_nodeid(raw);
    let (path_part, tail) = split_operand(&normalized);
    let path_part = normalize_path_spelling(path_part, repo_root);
    match tail {
        Some(tail) => format!("{path_part}::{tail}"),
        None => path_part,
    }
}

fn split_operand(raw: &str) -> (&str, Option<&str>) {
    match raw.split_once("::") {
        Some((path, tail)) => (path, Some(tail)),
        None => (raw, None),
    }
}

fn path_has_source_ext(path_part: &str) -> bool {
    Path::new(path_part)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("py") || ext.eq_ignore_ascii_case("rs"))
}

fn colon_to_nodeid(raw: &str) -> String {
    if raw.contains("::") {
        return raw.to_string();
    }
    match raw.rsplit_once(':') {
        Some((path, name))
            if !name.is_empty() && !name.contains('/') && path_has_source_ext(path) =>
        {
            format!("{path}::{name}")
        }
        _ => raw.to_string(),
    }
}

fn normalize_path_spelling(path_part: &str, repo_root: Option<&Path>) -> String {
    let unified = path_part.replace('\\', "/");
    let trimmed = unified.trim_start_matches("./");
    let stripped = match repo_root {
        Some(root) => strip_repo_prefix(trimmed, root),
        None => trimmed.to_string(),
    };
    strip_dir_slash(&stripped)
}

fn strip_repo_prefix(path_part: &str, repo_root: &Path) -> String {
    let candidate = PathBuf::from(path_part);
    if !candidate.is_absolute() {
        return path_part.to_string();
    }
    let Ok(root) = repo_root.canonicalize() else {
        return path_part.to_string();
    };
    candidate
        .strip_prefix(&root)
        .or_else(|_| candidate.strip_prefix(repo_root))
        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path_part.to_string())
}

fn strip_dir_slash(path_part: &str) -> String {
    if path_part.len() > 1 && path_part.ends_with('/') {
        path_part.trim_end_matches('/').to_string()
    } else {
        path_part.to_string()
    }
}
