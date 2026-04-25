//! Shared `kiss mv` integration-test helpers.

use super::mv_oracles::{OracleBundle, run_post_move_oracles_from_root};
use crate::symbol_mv_matrix::{ScenarioSpec, fixture_root};
use kiss::symbol_mv::{MvOptions, MvRequest, parse_mv_query, plan_edits};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

#[derive(Debug)]
pub struct ScenarioRun {
    temp_dir: TempDir,
    pub root: PathBuf,
    pub scenario: ScenarioSpec,
}

impl ScenarioRun {
    pub fn from_fixture(scenario: ScenarioSpec) -> Result<Self, String> {
        let temp_dir = TempDir::new().map_err(|err| err.to_string())?;
        copy_dir_contents(fixture_root(scenario.fixture), temp_dir.path())?;
        Ok(Self {
            root: temp_dir.path().to_path_buf(),
            scenario,
            temp_dir,
        })
    }

    pub fn into_parts(self) -> (TempDir, PathBuf, ScenarioSpec) {
        (self.temp_dir, self.root, self.scenario)
    }
}

#[derive(Debug)]
pub struct AppliedScenario {
    _temp_dir: Option<TempDir>,
    _original_temp_dir: TempDir,
    #[allow(dead_code)]
    pub name: String,
    pub root: PathBuf,
    pub original_root: PathBuf,
    pub inverse: Option<MvOptions>,
    untouched_files: Vec<(PathBuf, Vec<u8>)>,
    language: kiss::Language,
}

#[derive(Debug)]
pub struct HeavyOutcome {
    pub scenario_name: String,
    #[allow(dead_code)]
    pub root: PathBuf,
    #[allow(dead_code)]
    pub move_count: usize,
    pub post_oracles: OracleBundle,
}

impl AppliedScenario {
    pub fn locality_ok(&self) -> bool {
        self.untouched_files.iter().all(|(path, expected)| {
            fs::read(path)
                .map(|current| current == *expected)
                .unwrap_or(false)
        })
    }

    pub fn apply_inverse(&self) -> Result<Self, String> {
        let inverse = self
            .inverse
            .clone()
            .ok_or_else(|| "missing inverse move".to_string())?;
        apply_options(self.language, &inverse, &self.root, &self.original_root)
    }
}

pub fn apply_scenario(spec: &ScenarioSpec) -> Result<AppliedScenario, String> {
    let run = ScenarioRun::from_fixture(*spec)?;
    let opts = build_options(run.root.as_path(), spec, None);
    let (temp_dir, root, scenario) = run.into_parts();
    run_and_build_applied(
        &opts,
        &root,
        &root,
        spec.name.to_string(),
        scenario.language,
        Some(temp_dir),
    )
}

pub fn run_post_move_oracles(run: &AppliedScenario) -> OracleBundle {
    run_post_move_oracles_from_root(run.language, &run.root)
}

pub fn snapshot_tree(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut entries = Vec::new();
    collect_snapshot_entries(root, root, &mut entries).expect("snapshot tree");
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

pub fn apply_move_sequence(spec: &ScenarioSpec, depth: usize) -> Result<HeavyOutcome, String> {
    let applied = apply_scenario(spec)?;
    let mut next = applied.inverse.clone();
    for _ in 1..depth {
        if let Some(opts) = next.clone() {
            if kiss::symbol_mv::run_mv_command(opts.clone()) != 0 {
                return Err("kiss mv sequence step failed".to_string());
            }
            next = Some(build_inverse_options(&opts, &applied.root)?);
        }
    }
    Ok(HeavyOutcome {
        scenario_name: spec.name.to_string(),
        root: applied.root.clone(),
        move_count: depth,
        post_oracles: run_post_move_oracles_from_root(spec.language, &applied.root),
    })
}

pub fn write_failure_artifacts(outcome: &HeavyOutcome) -> Result<PathBuf, String> {
    let dir = PathBuf::from("target/mv-failures").join(&outcome.scenario_name);
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    fs::write(dir.join("scenario.txt"), format!("{outcome:#?}")).map_err(|err| err.to_string())?;
    Ok(dir)
}

fn apply_options(
    language: kiss::Language,
    opts: &MvOptions,
    root: &Path,
    original_root: &Path,
) -> Result<AppliedScenario, String> {
    run_and_build_applied(
        opts,
        root,
        original_root,
        opts.query.clone(),
        language,
        None,
    )
}

fn run_and_build_applied(
    opts: &MvOptions,
    root: &Path,
    snapshot_source: &Path,
    name: String,
    language: kiss::Language,
    keep_temp_dir: Option<TempDir>,
) -> Result<AppliedScenario, String> {
    let original_temp_dir = TempDir::new().map_err(|err| err.to_string())?;
    copy_dir_contents(snapshot_source, original_temp_dir.path())?;
    let touched_rel_paths = planned_touched_paths(opts)?;
    if kiss::symbol_mv::run_mv_command(opts.clone()) != 0 {
        return Err("kiss mv command failed".to_string());
    }
    let untouched_files = snapshot_tree(original_temp_dir.path())
        .into_iter()
        .filter_map(|(rel, contents)| {
            (!touched_rel_paths.contains(rel.as_str()))
                .then_some((root.join(Path::new(&rel)), contents))
        })
        .collect();
    let original_root = original_temp_dir.path().to_path_buf();
    Ok(AppliedScenario {
        _temp_dir: keep_temp_dir,
        _original_temp_dir: original_temp_dir,
        name,
        root: root.to_path_buf(),
        original_root,
        inverse: Some(build_inverse_options(opts, root)?),
        untouched_files,
        language,
    })
}

fn build_options(root: &Path, spec: &ScenarioSpec, override_query: Option<String>) -> MvOptions {
    let query = override_query.unwrap_or_else(|| absolute_query(root, spec.query));
    MvOptions {
        query,
        new_name: spec.new_name.to_string(),
        paths: vec![root.display().to_string()],
        to: spec.destination.map(|dest| root.join(dest)),
        dry_run: false,
        json: false,
        lang_filter: Some(spec.language),
        ignore: vec![],
    }
}

fn absolute_query(root: &Path, relative_query: &str) -> String {
    let (path, symbol) = relative_query.split_once("::").expect("query format");
    format!("{}::{}", root.join(path).display(), symbol)
}

fn build_inverse_options(opts: &MvOptions, root: &Path) -> Result<MvOptions, String> {
    let parsed = parse_mv_query(&opts.query)?;
    let original_name = parsed.old_name().to_string();
    let current_path = opts.to.clone().unwrap_or_else(|| parsed.path.clone());
    let current_symbol = if let Some(_member) = parsed.member {
        format!("{}.{}", parsed.symbol, opts.new_name)
    } else {
        opts.new_name.clone()
    };
    let inverse_query = format!("{}::{}", current_path.display(), current_symbol);
    let inverse_to = if opts.to.is_some() {
        Some(root.join(relative_to_root(root, &parsed.path)?))
    } else {
        None
    };
    Ok(MvOptions {
        query: inverse_query,
        new_name: original_name,
        paths: vec![root.display().to_string()],
        to: inverse_to,
        dry_run: false,
        json: false,
        lang_filter: opts.lang_filter,
        ignore: vec![],
    })
}

fn planned_touched_paths(opts: &MvOptions) -> Result<BTreeSet<String>, String> {
    let query = parse_mv_query(&opts.query)?;
    let req = MvRequest {
        query,
        new_name: opts.new_name.clone(),
        paths: opts.paths.clone(),
        to: opts.to.clone(),
        ignore: opts.ignore.clone(),
    };
    let plan = plan_edits(&req);
    let root = Path::new(&opts.paths[0]);
    plan.files
        .iter()
        .map(|path| relative_to_root(root, path))
        .collect::<Result<BTreeSet<_>, _>>()
}

fn relative_to_root(root: &Path, path: &Path) -> Result<String, String> {
    path.strip_prefix(root)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .map_err(|err| err.to_string())
}

fn collect_snapshot_entries(
    root: &Path,
    current: &Path,
    entries: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), String> {
    let mut children = BTreeMap::new();
    for entry in fs::read_dir(current).map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        children.insert(entry.file_name(), entry.path());
    }
    for (name, path) in children {
        if should_skip_snapshot_path(&name) {
            continue;
        }
        let file_type = fs::metadata(&path)
            .map_err(|err| err.to_string())?
            .file_type();
        if file_type.is_dir() {
            collect_snapshot_entries(root, &path, entries)?;
        } else if file_type.is_file() {
            entries.push((
                relative_to_root(root, &path)?,
                fs::read(&path).map_err(|err| err.to_string())?,
            ));
        }
    }
    Ok(())
}

fn should_skip_snapshot_path(name: &OsStr) -> bool {
    matches!(
        name.to_string_lossy().as_ref(),
        "__pycache__" | ".pytest_cache" | "target" | "Cargo.lock"
    )
}

fn copy_dir_contents(src: &Path, dst: &Path) -> Result<(), String> {
    for entry in fs::read_dir(src).map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let file_type = entry.file_type().map_err(|err| err.to_string())?;
        if file_type.is_dir() {
            fs::create_dir_all(&dst_path).map_err(|err| err.to_string())?;
            copy_dir_contents(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path).map_err(|err| err.to_string())?;
        }
    }
    Ok(())
}
