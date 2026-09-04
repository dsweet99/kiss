use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use kiss::Language;
use kiss::code_roles::{
    CodeRole, SourcePosition, SourceSpan, contains_file, is_python_test_module_path,
    is_test_only_file,
};

use super::model::{SourceModel, load_source_model};
use super::model_python::attach_python_nodeids;
use super::parse::{ParsedTestTarget, parse_test_target};
use crate::test_runner::runners::collect_python_nodeids_for_targets;
use crate::test_runner::workspace_selector_cache::{
    load_cached_python_workspace_selectors, python_selectors_for_rel_path,
};
#[derive(Clone, Debug, Default)]
pub(crate) struct TargetSelectionQuery {
    pub direct_python: BTreeSet<String>,
    pub direct_rust: BTreeSet<String>,
    pub python_files: BTreeSet<PathBuf>,
    pub rust_files: BTreeSet<PathBuf>,
    pub python_lines: BTreeMap<PathBuf, BTreeSet<u32>>,
    pub rust_lines: BTreeMap<PathBuf, BTreeSet<u32>>,
    unresolved_python_test_module: bool,
    unresolved_rust_test_module: bool,
}

pub(crate) fn resolve_target_operands(
    repo_root: &Path,
    operands: &[String],
    lang_filter: Option<Language>,
    ignore: &[String],
    pytest_args: &[String],
) -> Result<TargetSelectionQuery, String> {
    let mut query = TargetSelectionQuery::default();
    let mut seen_raw = BTreeSet::new();
    let mut models: BTreeMap<PathBuf, SourceModel> = BTreeMap::new();
    let mut roles = RolesOnDemand {
        loaded: Default::default(),
        parsed: Default::default(),
    };
    let mut python_selector_cache =
        load_python_target_cache(repo_root, ignore, pytest_args, lang_filter);
    for raw in operands {
        if !seen_raw.insert(raw.clone()) {
            continue;
        }
        let parsed = parse_test_target(raw)?;
        let abs = canonicalize_target_path(repo_root, &parsed)?;
        reject_ignored_target(repo_root, &abs, ignore, &parsed.raw)?;
        reject_lang_mismatch(lang_filter, parsed.language, &parsed.raw)?;

        if let Some(nodeid) = explicit_python_test_selector(repo_root, &parsed, &abs) {
            insert_direct(&mut query, Language::Python, nodeid);
            continue;
        }
        if !models.contains_key(&abs) {
            let mut model = load_source_model(&abs, parsed.language)?;
            if parsed.language == Language::Python {
                attach_python_tests(
                    repo_root,
                    &mut model,
                    pytest_args,
                    python_selector_cache.as_deref(),
                )?;
            }
            models.insert(abs.clone(), model);
        }
        let model = models.get(&abs).expect("model inserted above");
        apply_parsed_target(&mut query, model, &parsed, &abs, &mut roles)?;
        if parsed.language == Language::Python && python_selector_cache.is_none() {
            python_selector_cache =
                load_cached_python_workspace_selectors(repo_root, ignore, pytest_args);
        }
    }
    flush_unresolved_universes(&mut query, repo_root, ignore, pytest_args)?;
    Ok(query)
}

fn explicit_python_test_selector(
    repo_root: &Path,
    parsed: &ParsedTestTarget,
    abs: &Path,
) -> Option<String> {
    if parsed.language != Language::Python {
        return None;
    }
    if let Some(nodeid) = &parsed.python_nodeid {
        return Some(nodeid.clone());
    }
    if !is_python_test_module_path(abs) {
        return None;
    }
    let rel = repo_relative(repo_root, abs)?;
    match (&parsed.symbol, parsed.member.as_deref()) {
        (Some(name), None) => Some(format!("{rel}::{name}")),
        (Some(class), Some(method)) => Some(format!("{rel}::{class}::{method}")),
        (None, _) => None,
    }
}

struct RolesOnDemand {
    loaded: kiss::code_roles::SourceRoleIndex,
    parsed: BTreeSet<PathBuf>,
}

impl RolesOnDemand {
    fn get(&mut self, path: &Path) -> Result<&kiss::code_roles::SourceRoleIndex, String> {
        if contains_file(&self.loaded, path) {
            return Ok(&self.loaded);
        }
        if self.parsed.insert(path.to_path_buf()) {
            let started = std::time::Instant::now();
            let roles = crate::test_runner::runners::roles_for_changed_paths(&[path.to_path_buf()])
                .map_err(|err| format!("error: kiss test: {err}"))?;
            self.loaded.merge_from(roles);
            crate::test_runner::emit_stage_time("python_target_roles", started.elapsed());
        }
        Ok(&self.loaded)
    }
}

fn apply_parsed_target(
    query: &mut TargetSelectionQuery,
    model: &SourceModel,
    parsed: &ParsedTestTarget,
    abs: &Path,
    roles: &mut RolesOnDemand,
) -> Result<(), String> {
    if let Some(nodeid) = &parsed.python_nodeid {
        if model.direct_tests.iter().any(|t| t.selector == *nodeid) {
            insert_direct(query, Language::Python, nodeid.clone());
            return Ok(());
        }
        return Err(format!(
            "unknown pytest nodeid '{}' in {}",
            parsed.raw,
            abs.display()
        ));
    }
    match (&parsed.symbol, parsed.member.as_deref()) {
        (None, _) => apply_file_operand(query, model, abs, roles)?,
        (Some(name), member) => {
            apply_symbol_target(query, model, parsed, abs, name, member, roles)?
        }
    }
    Ok(())
}

fn apply_file_operand(
    query: &mut TargetSelectionQuery,
    model: &SourceModel,
    abs: &Path,
    roles: &mut RolesOnDemand,
) -> Result<(), String> {
    let before_py = query.direct_python.len();
    let before_rs = query.direct_rust.len();
    for test in &model.direct_tests {
        if test.selector.is_empty() {
            continue;
        }
        insert_direct(query, model.language, test.selector.clone());
    }
    if is_python_test_module_path(abs) || is_test_only_file(roles.get(abs)?, abs) {
        match model.language {
            Language::Python if query.direct_python.len() == before_py => {
                query.unresolved_python_test_module = true;
            }
            Language::Rust if query.direct_rust.len() == before_rs => {
                query.unresolved_rust_test_module = true;
            }
            _ => {}
        }
        return Ok(());
    }
    insert_file(query, model.language, abs);
    Ok(())
}

fn apply_symbol_target(
    query: &mut TargetSelectionQuery,
    model: &SourceModel,
    parsed: &ParsedTestTarget,
    abs: &Path,
    name: &str,
    member: Option<&str>,
    roles: &mut RolesOnDemand,
) -> Result<(), String> {
    let def = model.find_definition(name, member)?;
    if !def.is_unit_test {
        if roles.get(abs)?.role_for_span(abs, definition_span(def)) == CodeRole::TestOnly {
            return Ok(());
        }
        let lines = model.coverage_lines_for_definition(def);
        if !lines.is_empty() {
            insert_lines(query, model.language, abs, lines);
        }
        return Ok(());
    }
    let selectors = unit_test_selectors_for_def(model, name, member);
    if selectors.is_empty() {
        return Err(format!(
            "unit test '{}' in {} has no selector",
            parsed.raw,
            abs.display()
        ));
    }
    for selector in selectors {
        insert_direct(query, model.language, selector);
    }
    Ok(())
}

fn definition_span(def: &super::model::NamedDefinition) -> SourceSpan {
    SourceSpan::new(
        SourcePosition::new(def.start_line as usize, 0),
        SourcePosition::new(def.end_line.saturating_add(1) as usize, 0),
    )
}

fn unit_test_selectors_for_def(
    model: &SourceModel,
    name: &str,
    member: Option<&str>,
) -> Vec<String> {
    model
        .direct_tests
        .iter()
        .filter(|test| match member {
            Some(method) => test.owner.as_deref() == Some(name) && test.name == method,
            None => test.owner.is_none() && test.name == name,
        })
        .map(|test| test.selector.clone())
        .filter(|selector| !selector.is_empty())
        .collect()
}

fn load_python_target_cache(
    repo_root: &Path,
    ignore: &[String],
    pytest_args: &[String],
    lang_filter: Option<Language>,
) -> Option<Vec<String>> {
    let started = std::time::Instant::now();
    let cache = (lang_filter != Some(Language::Rust))
        .then(|| load_cached_python_workspace_selectors(repo_root, ignore, pytest_args))
        .flatten();
    if lang_filter != Some(Language::Rust) {
        crate::test_runner::emit_stage_time(
            if cache.is_some() {
                "python_target_cache"
            } else {
                "python_target_cache_miss"
            },
            started.elapsed(),
        );
    }
    cache
}

fn attach_python_tests(
    repo_root: &Path,
    model: &mut SourceModel,
    pytest_args: &[String],
    cached_selectors: Option<&[String]>,
) -> Result<(), String> {
    if model.direct_tests.is_empty() && !is_python_test_module_path(&model.path) {
        return Ok(());
    }
    let rel =
        repo_relative(repo_root, &model.path).unwrap_or_else(|| model.path.display().to_string());
    let nodeids = match cached_selectors {
        Some(cached) => {
            crate::test_runner::emit_stage_time(
                "python_target_selectors",
                std::time::Duration::ZERO,
            );
            python_selectors_for_rel_path(cached, &rel)
        }
        None => collect_python_nodeids_for_targets(
            repo_root,
            Some(std::slice::from_ref(&model.path)),
            pytest_args,
        )?,
    };
    attach_python_nodeids(model, &nodeids, &rel);

    model.direct_tests.retain(|test| !test.selector.is_empty());
    for def in &mut model.definitions {
        if def.is_unit_test && def.test_selector.as_ref().is_none_or(String::is_empty) {
            def.is_unit_test = false;
        }
    }
    Ok(())
}

fn canonicalize_target_path(
    repo_root: &Path,
    parsed: &ParsedTestTarget,
) -> Result<PathBuf, String> {
    let candidate = if parsed.path.is_absolute() {
        parsed.path.clone()
    } else {
        repo_root.join(&parsed.path)
    };
    let abs = candidate.canonicalize().map_err(|_| {
        format!(
            "target '{}': file not found at {}",
            parsed.raw,
            candidate.display()
        )
    })?;
    if !abs.is_file() {
        return Err(format!(
            "target '{}': {} is not a regular file",
            parsed.raw,
            abs.display()
        ));
    }
    let root = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    if !abs.starts_with(&root) {
        return Err(format!(
            "target '{}': path escapes repository root",
            parsed.raw
        ));
    }
    Ok(abs)
}

fn reject_ignored_target(
    repo_root: &Path,
    abs: &Path,
    ignore: &[String],
    raw: &str,
) -> Result<(), String> {
    let Some(rel) = repo_relative(repo_root, abs) else {
        return Ok(());
    };
    if kiss::path_ignored_by_prefixes(&rel, ignore) {
        return Err(format!(
            "target '{raw}' is covered by --ignore prefix and cannot be requested"
        ));
    }
    Ok(())
}

fn reject_lang_mismatch(
    lang_filter: Option<Language>,
    language: Language,
    raw: &str,
) -> Result<(), String> {
    if let Some(filter) = lang_filter
        && filter != language
    {
        return Err(format!(
            "target '{raw}' is {} but --lang selects only {}",
            language_label(language),
            language_label(filter)
        ));
    }
    Ok(())
}

#[path = "resolve_insert.rs"]
mod resolve_insert;
#[path = "resolve_universe.rs"]
mod resolve_universe;
use resolve_insert::{insert_direct, insert_file, insert_lines, language_label, repo_relative};
use resolve_universe::flush_unresolved_universes;
