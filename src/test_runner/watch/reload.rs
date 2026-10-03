use std::path::{Path, PathBuf};
use std::time::Duration;

use kiss::{Config, ConfigLanguage, GateConfig, Language, TestSectionConfig};

use super::filter::WatchPathFilter;
use super::settle::{PathSignature, SettleMachine};
use crate::test_runner::RunTestCmdArgs;
use crate::test_runner::language_keyed::LanguageKeyed;

#[derive(Debug, Clone)]
pub(crate) struct WatchReloadSeed {
    pub cli_ignore: Vec<String>,
    pub jobs_cli: Option<usize>,
    /// Raw CLI `--extra` seed (not language-keyed); live config holds `LanguageKeyed` extras.
    pub extra: Vec<String>,
    pub coverage_all: bool,
    pub enabled: bool,
    pub config_path: PathBuf,
}

pub(crate) struct WatchLiveConfig {
    pub target_request: crate::test_runner::target_request::TargetRequest,
    pub main_branch_cli: Option<String>,
    pub base_branch_cli: Option<String>,
    pub dry_run: bool,
    pub extras: LanguageKeyed<Vec<String>>,
    pub jobs: usize,
    pub config_main_branch: Option<String>,
    pub gate_config: GateConfig,
    pub py_config: Config,
    pub rs_config: Config,
    /// Session seed from CLI / reload; consumers must use [`Self::effective_coverage_all`].
    coverage_all: bool,
    /// Per-cycle nudge overlay; consumers must use [`Self::effective_coverage_all`].
    nudge_coverage_all: bool,
    pub settle: Duration,
    pub language_tables: kiss::LanguageTablesPresent,
    seed: WatchReloadSeed,
    kissconfig_sig: PathSignature,
    kissconfig_digest: u64,
    cycle_filters: Option<CycleFilterOverride>,
    drift: ConfigDrift,
}

struct ConfigDrift {
    files: Vec<(PathBuf, u64)>,
    outdated: bool,
    rerun_done: bool,
}

#[derive(Debug, Clone)]
struct CycleFilterOverride {
    lang_filter: Option<Language>,
    ignore: Vec<String>,
    extras: LanguageKeyed<Vec<String>>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CycleForceFlags {
    pub force_rerun: bool,
    pub force_bad: bool,
    pub metrics: bool,
    pub target_request: crate::test_runner::target_request::TargetRequest,
}

impl WatchLiveConfig {
    pub(crate) fn from_args(
        args: &RunTestCmdArgs<'_>,
        settle: Duration,
        seed: WatchReloadSeed,
        py_config: Config,
        rs_config: Config,
        config_path: &Path,
    ) -> Self {
        let target_request = crate::test_runner::target_request::request_from_run_args(args);
        Self {
            target_request,
            main_branch_cli: args.main_branch_cli.map(str::to_owned),
            base_branch_cli: args.base_branch_cli.map(str::to_owned),
            dry_run: args.dry_run,
            extras: LanguageKeyed {
                rust: seed.extra.clone(),
                python: args.extras.python.to_vec(),
            },
            jobs: args.jobs,
            config_main_branch: args.config_main_branch.map(str::to_owned),
            gate_config: args.gate_config.clone(),
            py_config,
            rs_config,
            coverage_all: seed.coverage_all,
            nudge_coverage_all: false,
            settle,
            language_tables: kiss::LanguageTablesPresent::from_path_or_both(config_path),
            kissconfig_sig: PathSignature::from_path(config_path),
            kissconfig_digest: file_digest(config_path),
            seed,
            cycle_filters: None,
            drift: ConfigDrift::empty(),
        }
    }

    pub(super) fn poll_config_drift(&mut self, repo_root: &Path) -> bool {
        if self.seed.enabled || self.drift.rerun_done {
            return false;
        }
        if self.drift.files.is_empty() {
            self.drift.files = ConfigDrift::paths(repo_root, &self.seed.config_path);
            return false;
        }
        if self.drift.outdated {
            return true;
        }
        let changed = self
            .drift
            .files
            .iter()
            .any(|(path, digest)| file_digest(path) != *digest);
        if changed {
            self.drift.outdated = true;
        }
        changed
    }

    pub(super) fn config_outdated(&self) -> bool {
        self.drift.outdated
    }

    pub(super) fn config_rerun_pending(&self) -> bool {
        self.drift.outdated && !self.drift.rerun_done && !self.seed.enabled
    }

    pub(super) fn finish_config_rerun(&mut self) {
        if self.drift.outdated {
            self.drift.rerun_done = true;
        }
    }

    pub(crate) fn apply_nudge_filters(
        &mut self,
        lang_filter: Option<Language>,
        ignore: Vec<String>,
        extras: LanguageKeyed<Vec<String>>,
    ) {
        if lang_filter.is_none() && ignore.is_empty() && extras.both_empty() {
            self.cycle_filters = None;
            return;
        }
        self.cycle_filters = Some(CycleFilterOverride {
            lang_filter,
            ignore,
            extras,
        });
    }

    pub(crate) fn clear_nudge_filters(&mut self) {
        self.cycle_filters = None;
    }

    /// Effective “coverage-all for this cycle?” — OR of session seed and nudge overlay.
    pub(crate) fn effective_coverage_all(&self) -> bool {
        self.coverage_all || self.nudge_coverage_all
    }

    pub(crate) fn set_nudge_coverage_all(&mut self, value: bool) {
        self.nudge_coverage_all = value;
    }

    fn cycle_request(
        &self,
        force: &CycleForceFlags,
    ) -> crate::test_runner::target_request::TargetRequest {
        force.target_request.clone()
    }

    pub(crate) fn cycle_args(&self, force: CycleForceFlags) -> RunTestCmdArgs<'_> {
        let (lang_filter, extras, ignore) = match &self.cycle_filters {
            Some(over) => (
                over.lang_filter.or(self.target_request.language()),
                LanguageKeyed {
                    rust: if over.extras.rust.is_empty() {
                        self.extras.rust.as_slice()
                    } else {
                        over.extras.rust.as_slice()
                    },
                    python: if over.extras.python.is_empty() {
                        self.extras.python.as_slice()
                    } else {
                        over.extras.python.as_slice()
                    },
                },
                if over.ignore.is_empty() {
                    self.target_request.ignore.as_slice()
                } else {
                    over.ignore.as_slice()
                },
            ),
            None => (
                self.target_request.language(),
                self.extras.as_slices(),
                self.target_request.ignore.as_slice(),
            ),
        };
        let mut request = self.cycle_request(&force);
        request.set_language(lang_filter);
        if !ignore.is_empty() {
            request.ignore = ignore.to_vec();
        }
        let invocation = crate::test_runner::target_request::to_compat_invocation(&request);
        RunTestCmdArgs {
            invocation,
            target_request: request,
            main_branch_cli: self.main_branch_cli.as_deref(),
            base_branch_cli: self.base_branch_cli.as_deref(),
            dry_run: self.dry_run,
            force_rerun: force.force_rerun,
            force_bad: force.force_bad,
            metrics: force.metrics,
            coverage_all: self.effective_coverage_all(),
            jobs: self.jobs,
            extras,
            config_main_branch: self.config_main_branch.as_deref(),
            gate_config: self.gate_config.clone(),
        }
    }

    pub(crate) fn maybe_reload(
        &mut self,
        repo_root: &Path,
        machine: &mut SettleMachine,
        filter: &mut WatchPathFilter,
    ) -> Result<bool, String> {
        if !self.seed.enabled {
            return Ok(false);
        }
        let path = resolve_config_path(repo_root, &self.seed.config_path);
        let sig = PathSignature::from_path(&path);
        let digest = file_digest(&path);
        if sig == self.kissconfig_sig && digest == self.kissconfig_digest {
            return Ok(false);
        }
        self.apply_reload_from_path(&path)?;
        self.kissconfig_sig = sig;
        self.kissconfig_digest = digest;
        machine.set_settle(self.settle);
        *filter = self.path_filter(repo_root);
        crate::test_runner::emit_test_progress("kiss test: Reloaded .kissconfig");
        Ok(true)
    }

    fn apply_reload_from_path(&mut self, path: &Path) -> Result<(), String> {
        let test_cfg = TestSectionConfig::try_load_path_only(path).map_err(|e| e.to_string())?;
        let (gate_config, py_config, rs_config) = if path.exists() {
            (
                GateConfig::try_load_from(path).map_err(|e| e.to_string())?,
                Config::try_load_from(path, ConfigLanguage::Python).map_err(|e| e.to_string())?,
                Config::try_load_from(path, ConfigLanguage::Rust).map_err(|e| e.to_string())?,
            )
        } else {
            (
                GateConfig::default(),
                Config::python_defaults(),
                Config::rust_defaults(),
            )
        };
        self.gate_config = gate_config;
        self.py_config = py_config;
        self.rs_config = rs_config;
        self.target_request.ignore = test_cfg.merged_ignore(&self.seed.cli_ignore);
        self.jobs = self.seed.jobs_cli.unwrap_or(test_cfg.num_jobs);
        self.extras.python =
            kiss::effective_python_pytest_args(&test_cfg.pytest_plugins, &self.seed.extra);
        self.config_main_branch = test_cfg.main_branch.clone();
        self.settle = Duration::from_secs_f64(test_cfg.watch_settle_seconds);
        self.language_tables = kiss::LanguageTablesPresent::from_path_or_both(path);
        Ok(())
    }

    pub(crate) fn watched_config_path(&self) -> &Path {
        &self.seed.config_path
    }

    pub(crate) fn path_filter(&self, repo_root: &Path) -> WatchPathFilter {
        WatchPathFilter::build_with_config(
            repo_root,
            &self.target_request.ignore,
            self.target_request.language(),
            &self.target_request,
            self.watched_config_path(),
        )
    }
}

pub(crate) fn resolve_config_path(repo_root: &Path, config_path: &Path) -> PathBuf {
    if config_path.is_absolute() {
        config_path.to_path_buf()
    } else {
        repo_root.join(config_path)
    }
}

impl ConfigDrift {
    fn empty() -> Self {
        Self {
            files: Vec::new(),
            outdated: false,
            rerun_done: false,
        }
    }

    fn paths(repo_root: &Path, config_path: &Path) -> Vec<(PathBuf, u64)> {
        let config = if config_path.is_absolute() {
            config_path.to_path_buf()
        } else {
            repo_root.join(config_path)
        };
        [
            config,
            repo_root.join("pyproject.toml"),
            repo_root.join("Cargo.toml"),
        ]
        .into_iter()
        .map(|path| {
            let digest = file_digest(&path);
            (path, digest)
        })
        .collect()
    }
}

fn file_digest(path: &Path) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0100_0000_01b3;
    match std::fs::read(path) {
        Ok(bytes) => bytes
            .iter()
            .fold(OFFSET, |acc, b| (acc ^ u64::from(*b)).wrapping_mul(PRIME)),
        Err(_) => 0,
    }
}

#[cfg(test)]
#[path = "reload_test.rs"]
mod tests;
