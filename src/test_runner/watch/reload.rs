use std::path::{Path, PathBuf};
use std::time::Duration;

use kiss::{Config, ConfigLanguage, GateConfig, Language, TestSectionConfig};

use super::filter::WatchPathFilter;
use super::settle::{PathSignature, SettleMachine};
use crate::bin_cli::args::TestInvocation;
use crate::test_runner::RunTestCmdArgs;

#[derive(Debug, Clone)]
pub(crate) struct WatchReloadSeed {
    pub cli_ignore: Vec<String>,
    pub jobs_cli: Option<usize>,
    pub extra: Vec<String>,
    pub coverage_all: bool,
    pub enabled: bool,
    pub config_path: PathBuf,
}

pub(crate) struct WatchLiveConfig {
    pub invocation: TestInvocation,
    pub main_branch_cli: Option<String>,
    pub base_branch_cli: Option<String>,
    pub dry_run: bool,
    pub lang_filter: Option<Language>,
    pub extra: Vec<String>,
    pub python_extra: Vec<String>,
    pub ignore: Vec<String>,
    pub jobs: usize,
    pub config_main_branch: Option<String>,
    pub gate_config: GateConfig,
    pub py_config: Config,
    pub rs_config: Config,
    pub coverage_all: bool,
    pub settle: Duration,
    pub language_tables: kiss::LanguageTablesPresent,
    seed: WatchReloadSeed,
    kissconfig_sig: PathSignature,
    kissconfig_digest: u64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CycleForceFlags {
    pub force_rerun: bool,
    pub force_bad: bool,
    pub metrics: bool,
    pub targets: Vec<String>,
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
        Self {
            invocation: args.invocation.clone(),
            main_branch_cli: args.main_branch_cli.map(str::to_owned),
            base_branch_cli: args.base_branch_cli.map(str::to_owned),
            dry_run: args.dry_run,
            lang_filter: args.lang_filter,
            extra: seed.extra.clone(),
            python_extra: args.python_extra.to_vec(),
            ignore: args.ignore.to_vec(),
            jobs: args.jobs,
            config_main_branch: args.config_main_branch.map(str::to_owned),
            gate_config: args.gate_config.clone(),
            py_config,
            rs_config,
            coverage_all: seed.coverage_all,
            settle,
            language_tables: kiss::LanguageTablesPresent::from_path_or_both(config_path),
            kissconfig_sig: PathSignature::from_path(config_path),
            kissconfig_digest: file_digest(config_path),
            seed,
        }
    }

    pub(crate) fn cycle_args(&self, force: CycleForceFlags) -> RunTestCmdArgs<'_> {
        let invocation = if !force.targets.is_empty() {
            TestInvocation::Targets(force.targets)
        } else {
            self.invocation.clone()
        };
        RunTestCmdArgs {
            invocation,
            main_branch_cli: self.main_branch_cli.as_deref(),
            base_branch_cli: self.base_branch_cli.as_deref(),
            dry_run: self.dry_run,
            force_rerun: force.force_rerun,
            force_bad: force.force_bad,
            metrics: force.metrics,
            jobs: self.jobs,
            extra: &self.extra,
            python_extra: &self.python_extra,
            ignore: &self.ignore,
            lang_filter: self.lang_filter,
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
        *filter = WatchPathFilter::build_with_config(
            repo_root,
            &self.ignore,
            self.lang_filter,
            &self.invocation,
            &self.seed.config_path,
        );
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
        self.ignore = test_cfg.merged_ignore(&self.seed.cli_ignore);
        self.jobs = self.seed.jobs_cli.unwrap_or(test_cfg.num_jobs);
        self.python_extra =
            kiss::effective_python_pytest_args(&test_cfg.pytest_plugins, &self.seed.extra);
        self.config_main_branch = test_cfg.main_branch.clone();
        self.settle = Duration::from_secs_f64(test_cfg.watch_settle_seconds);
        self.language_tables = kiss::LanguageTablesPresent::from_path_or_both(path);
        Ok(())
    }

    pub(crate) fn watched_config_path(&self) -> &Path {
        &self.seed.config_path
    }
}

pub(crate) fn resolve_config_path(repo_root: &Path, config_path: &Path) -> PathBuf {
    if config_path.is_absolute() {
        config_path.to_path_buf()
    } else {
        repo_root.join(config_path)
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
