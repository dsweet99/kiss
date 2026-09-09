use std::path::{Path, PathBuf};

use kiss::Language;

#[derive(Debug)]
pub(crate) enum ExpandedTargetPlan {
    All,
    Files(Vec<String>),
}

pub(crate) fn expand_target_operands(
    repo_root: &Path,
    targets: &[String],
    ignore: &[String],
    lang_filter: Option<Language>,
) -> Result<ExpandedTargetPlan, String> {
    let root_canon = repo_root
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize repository root: {e}"))?;
    let mut file_operands = Vec::new();
    let mut saw_repo_root = false;

    for raw in targets {
        if is_file_or_symbol_operand(raw) {
            reject_missing_source_file(repo_root, raw, ignore, lang_filter)?;
            file_operands.push(raw.clone());
            continue;
        }
        let candidate = resolve_candidate(repo_root, raw);
        let abs = candidate
            .canonicalize()
            .map_err(|_| format!("target '{raw}': path not found at {}", candidate.display()))?;
        if abs.is_file() {
            return Err(format!(
                "target '{raw}': {} is not a .py/.rs source file or directory",
                abs.display()
            ));
        }
        if !abs.is_dir() {
            return Err(format!(
                "target '{raw}': {} is not a directory",
                abs.display()
            ));
        }
        if abs == root_canon {
            saw_repo_root = true;
            continue;
        }
        append_directory_sources(&mut file_operands, raw, &abs, ignore, lang_filter)?;
    }

    if saw_repo_root {
        if targets.len() != 1 || !file_operands.is_empty() {
            return Err("repository root cannot be mixed with additional targets".to_string());
        }
        return Ok(ExpandedTargetPlan::All);
    }
    if file_operands.is_empty() {
        return Ok(ExpandedTargetPlan::Files(Vec::new()));
    }
    Ok(ExpandedTargetPlan::Files(file_operands))
}

fn is_file_or_symbol_operand(raw: &str) -> bool {
    let path_part = raw.split_once("::").map_or(raw, |(path, _)| path);
    Path::new(path_part)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("py") || ext.eq_ignore_ascii_case("rs"))
}

fn reject_missing_source_file(
    repo_root: &Path,
    raw: &str,
    ignore: &[String],
    lang_filter: Option<Language>,
) -> Result<(), String> {
    let path_part = raw.split_once("::").map_or(raw, |(path, _)| path);
    let candidate = resolve_candidate(repo_root, path_part);
    match candidate.canonicalize() {
        Ok(abs) if abs.is_file() => {
            reject_file_operand_filters(repo_root, raw, path_part, &abs, ignore, lang_filter)
        }
        _ => Err(format!(
            "target '{raw}': file not found at {}",
            candidate.display()
        )),
    }
}

fn reject_file_operand_filters(
    repo_root: &Path,
    raw: &str,
    path_part: &str,
    abs: &Path,
    ignore: &[String],
    lang_filter: Option<Language>,
) -> Result<(), String> {
    if let Some((filter, language)) = lang_mismatch(path_part, lang_filter) {
        return Err(format!(
            "target '{raw}' is {} but --lang selects only {}",
            language.label(),
            filter.label()
        ));
    }
    let root = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    let rel = abs.strip_prefix(&root).unwrap_or(Path::new(path_part));
    if kiss::path_ignored_by_prefixes(&rel.to_string_lossy(), ignore)
        || kiss::path_ignored_by_prefixes(path_part, ignore)
    {
        return Err(format!(
            "target '{raw}' is covered by --ignore prefix and cannot be requested"
        ));
    }
    Ok(())
}

fn lang_mismatch(path_part: &str, lang_filter: Option<Language>) -> Option<(Language, Language)> {
    let filter = lang_filter?;
    let language = Language::from_path(Path::new(path_part))?;
    (language != filter).then_some((filter, language))
}

fn resolve_candidate(repo_root: &Path, raw: &str) -> PathBuf {
    let path = Path::new(raw);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root.join(path)
    }
}

fn append_directory_sources(
    file_operands: &mut Vec<String>,
    raw: &str,
    abs: &Path,
    ignore: &[String],
    lang_filter: Option<Language>,
) -> Result<(), String> {
    let path_arg = abs.to_string_lossy().into_owned();
    let (py_files, rs_files) =
        kiss::gather_files_by_lang(std::slice::from_ref(&path_arg), None, ignore);
    if py_files.is_empty() && rs_files.is_empty() {
        return Err(format!("directory '{raw}' expands to zero source files"));
    }
    let files: Vec<_> = match lang_filter {
        Some(Language::Python) => py_files,
        Some(Language::Rust) => rs_files,
        None => py_files.into_iter().chain(rs_files).collect(),
    };
    for path in files {
        file_operands.push(path.to_string_lossy().into_owned());
    }
    Ok(())
}
