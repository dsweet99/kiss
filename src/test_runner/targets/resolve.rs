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
use super::resolve_hydrate::{hydrate_python_models, python_nodeids_for_model};
use crate::test_runner::workspace_selector_cache::load_cached_python_workspace_selectors;
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

impl TargetSelectionQuery {
    pub(crate) fn direct(&self, language: Language) -> &BTreeSet<String> {
        match language {
            Language::Python => &self.direct_python,
            Language::Rust => &self.direct_rust,
        }
    }

    pub(crate) fn source_files(&self, language: Language) -> Vec<PathBuf> {
        let (files, lines) = match language {
            Language::Python => (&self.python_files, &self.python_lines),
            Language::Rust => (&self.rust_files, &self.rust_lines),
        };
        files.iter().chain(lines.keys()).cloned().collect()
    }
}

#[cfg(test)]
pub(crate) fn resolve_target_operands(
    repo_root: &Path,
    operands: &[String],
    lang_filter: Option<Language>,
    ignore: &[String],
    pytest_args: &[String],
) -> Result<TargetSelectionQuery, String> {
    resolve_target_operands_with(repo_root, operands, lang_filter, ignore, pytest_args, &[])
}

pub(crate) fn resolve_target_operands_with(
    repo_root: &Path,
    operands: &[String],
    lang_filter: Option<Language>,
    ignore: &[String],
    pytest_args: &[String],
    skip_python_collect: &[PathBuf],
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
    let mut pending: Vec<(ParsedTestTarget, PathBuf)> = Vec::new();
    let mut rust_universe = None;
    for raw in operands {
        if !seen_raw.insert(raw.clone()) {
            continue;
        }
        ingest_operand(
            repo_root,
            raw,
            lang_filter,
            ignore,
            &mut IngestState {
                query: &mut query,
                models: &mut models,
                rust_universe: &mut rust_universe,
                pending: &mut pending,
            },
        )?;
    }
    suppress_directory_production_tests(&mut models, skip_python_collect);
    hydrate_python_models(
        repo_root,
        &mut models,
        pytest_args,
        python_selector_cache.as_deref(),
    )?;
    for (parsed, abs) in pending {
        let model = models.get_mut(&abs).expect("model inserted above");
        if parsed.language == Language::Python {
            attach_python_tests(
                repo_root,
                model,
                pytest_args,
                python_selector_cache.as_deref(),
            )?;
        }
        apply_parsed_target(&mut query, model, &parsed, &abs, &mut roles)?;
        if parsed.language == Language::Python && python_selector_cache.is_none() {
            python_selector_cache =
                load_cached_python_workspace_selectors(repo_root, ignore, pytest_args);
        }
    }
    flush_unresolved_universes(&mut query, repo_root, ignore, pytest_args)?;
    Ok(query)
}

struct IngestState<'a> {
    query: &'a mut TargetSelectionQuery,
    models: &'a mut BTreeMap<PathBuf, SourceModel>,
    rust_universe: &'a mut Option<BTreeSet<String>>,
    pending: &'a mut Vec<(ParsedTestTarget, PathBuf)>,
}

fn ingest_operand(
    repo_root: &Path,
    raw: &str,
    lang_filter: Option<Language>,
    ignore: &[String],
    state: &mut IngestState<'_>,
) -> Result<(), String> {
    let parsed = parse_test_target(raw)?;
    let abs = canonicalize_target_path(repo_root, &parsed)?;
    reject_ignored_target(repo_root, &abs, ignore, &parsed.raw)?;
    reject_lang_mismatch(lang_filter, parsed.language, &parsed.raw)?;
    if let Some(nodeid) = parsed.python_nodeid.as_deref() {
        insert_direct(
            state.query,
            Language::Python,
            relative_python_nodeid(repo_root, nodeid),
        );
        return Ok(());
    }
    if !state.models.contains_key(&abs) {
        let mut model = load_source_model(&abs, parsed.language)?;
        qualify_rust_model(repo_root, &mut model, state.rust_universe);
        state.models.insert(abs.clone(), model);
    }
    state.pending.push((parsed, abs));
    Ok(())
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
        let lines = model.target_lines_for_definition(def);
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

fn relative_python_nodeid(repo_root: &Path, requested: &str) -> String {
    let (requested_file, Some(requested_tail)) = kiss::split_selector(requested) else {
        return requested.to_string();
    };
    let Some(requested_rel) = nodeid_file_rel(repo_root, requested_file) else {
        return requested.to_string();
    };
    format!("{requested_rel}::{requested_tail}")
}

fn nodeid_file_rel(repo_root: &Path, file: &str) -> Option<String> {
    let path = Path::new(file);
    if path.is_absolute() {
        return repo_relative(repo_root, path);
    }
    Some(file.replace('\\', "/"))
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

fn suppress_directory_production_tests(
    models: &mut BTreeMap<PathBuf, SourceModel>,
    skip_python_collect: &[PathBuf],
) {
    if skip_python_collect.is_empty() {
        return;
    }
    let skip: Vec<PathBuf> = skip_python_collect
        .iter()
        .map(|path| path.canonicalize().unwrap_or_else(|_| path.clone()))
        .collect();
    for (abs, model) in models.iter_mut() {
        if model.language != Language::Python {
            continue;
        }
        let canon = abs.canonicalize().unwrap_or_else(|_| abs.clone());
        if !skip.iter().any(|path| path == abs || path == &canon) {
            continue;
        }
        model.direct_tests.clear();
        for def in &mut model.definitions {
            def.is_unit_test = false;
            def.test_selector = None;
        }
    }
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
    let nodeids = python_nodeids_for_model(repo_root, model, &rel, pytest_args, cached_selectors)?;
    attach_python_nodeids(model, &nodeids, &rel);

    model.direct_tests.retain(|test| !test.selector.is_empty());
    for def in &mut model.definitions {
        if def.is_unit_test && def.test_selector.as_ref().is_none_or(String::is_empty) {
            def.is_unit_test = false;
        }
    }
    Ok(())
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
            "target '{raw}' is matched by an --ignore prefix and cannot be requested"
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
#[path = "resolve_path.rs"]
mod resolve_path;
#[path = "resolve_universe.rs"]
mod resolve_universe;
use resolve_insert::{insert_direct, insert_file, insert_lines, language_label, repo_relative};
use resolve_path::canonicalize_target_path;
use resolve_universe::{flush_unresolved_universes, qualify_rust_model};
