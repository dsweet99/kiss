use std::path::Path;

use crate::analyze;
use crate::analyze::DryRunParams;
use crate::bin_cli::check_cmd::{CheckCommandArgs, run_check_command};
use crate::bin_cli::mimic::run_mimic;
use crate::bin_cli::shrink::{RunShrinkArgs, ShrinkFullContext, run_shrink};
use crate::bin_cli::stats::{RunStatsArgs, run_stats};
use crate::bin_cli::util::{merge_check_ignore_prefixes, validate_min_similarity, validate_paths};
use crate::rules::{run_config, run_rules};
use crate::viz::{VizCoarsen, run_viz};
use kiss::{Language, normalize_ignore_prefixes};

use super::options::{
    CheckDispatchOptions, ConfigDispatchOptions, DryDispatchOptions, MimicDispatchOptions,
    MvDispatchOptions, RulesDispatchOptions, ShrinkDispatchOptions, StatsDispatchOptions,
    TestDispatchOptions, VizDispatchOptions,
};

pub(in crate::bin_cli::dispatch) fn dispatch_check(o: CheckDispatchOptions<'_>) -> i32 {
    let args = CheckCommandArgs {
        paths: &o.paths,
        lang_filter: o.lang,
        py_config: o.cfg.py,
        rs_config: o.cfg.rs,
        gate_config: o.cfg.gate,
        bypass_gate: o.bypass_gate,
        ignore: &o.ignore,
        timing: o.timing,
        jobs: o.jobs,
    };
    run_check_command(&args)
}

pub(in crate::bin_cli::dispatch) fn dispatch_stats(o: StatsDispatchOptions) -> i32 {
    let ignore = normalize_ignore_prefixes(&o.ignore);
    run_stats(RunStatsArgs {
        paths: &o.paths,
        lang_filter: o.lang,
        ignore: &ignore,
        all: o.all,
        table: o.table,
        py_config: o.cfg.py,
        rs_config: o.cfg.rs,
        gate_config: o.cfg.gate,
    });
    0
}

pub(in crate::bin_cli::dispatch) fn dispatch_mimic(o: MimicDispatchOptions) -> i32 {
    let ignore = normalize_ignore_prefixes(&o.ignore);
    run_mimic(&o.paths, o.out.as_deref(), o.lang, &ignore);
    0
}

pub(in crate::bin_cli::dispatch) fn dispatch_clamp(
    lang: Option<Language>,
    ignore: Vec<String>,
) -> i32 {
    let ignore = merge_check_ignore_prefixes(&ignore);
    run_mimic(
        &[".".to_string()],
        Some(Path::new(".kissconfig")),
        lang,
        &ignore,
    );
    0
}

pub(in crate::bin_cli::dispatch) fn dispatch_dry(o: DryDispatchOptions) -> i32 {
    let ignore = normalize_ignore_prefixes(&o.ignore);
    if let Err(msg) = validate_min_similarity(o.min_similarity) {
        eprintln!("Error: {msg}");
        return 1;
    }
    let config = kiss::DuplicationConfig {
        shingle_size: o.shingle_size,
        minhash_size: o.minhash_size,
        lsh_bands: o.lsh_bands,
        min_similarity: o.min_similarity,
    };
    let params = DryRunParams {
        path: o.path.as_str(),
        filter_files: &o.filter_files,
        config: &config,
        ignore_prefixes: &ignore,
        lang_filter: o.lang,
    };
    analyze::run_dry(&params);
    0
}

pub(in crate::bin_cli::dispatch) fn dispatch_rules(o: RulesDispatchOptions<'_>) -> i32 {
    run_rules(o.cfg.py, o.cfg.rs, o.cfg.gate, o.lang, o.defaults);
    0
}

pub(in crate::bin_cli::dispatch) fn dispatch_config(o: ConfigDispatchOptions<'_>) -> i32 {
    run_config(
        o.cfg.py,
        o.cfg.rs,
        o.cfg.gate,
        o.config.as_ref(),
        o.defaults,
    );
    0
}

pub(in crate::bin_cli::dispatch) fn dispatch_viz(o: VizDispatchOptions) -> i32 {
    let ignore = normalize_ignore_prefixes(&o.ignore);
    validate_paths(&o.paths);
    let coarsen = o
        .num_nodes
        .map_or(VizCoarsen::Zoom(o.zoom), VizCoarsen::NumNodes);
    if let Err(e) = run_viz(&o.out, &o.paths, o.lang, &ignore, coarsen) {
        eprintln!("Error: {e}");
        return 1;
    }
    0
}

pub(in crate::bin_cli::dispatch) fn dispatch_shrink(o: ShrinkDispatchOptions<'_>) -> i32 {
    let ctx = ShrinkFullContext {
        lang_filter: o.lang,
        py_config: o.cfg.py,
        rs_config: o.cfg.rs,
        gate_config: o.cfg.gate,
    };
    run_shrink(RunShrinkArgs {
        target: o.target,
        paths: &o.paths,
        ignore: &o.ignore,
        ctx: &ctx,
    })
}

pub(in crate::bin_cli::dispatch) fn dispatch_test(o: TestDispatchOptions<'_>) -> i32 {
    use crate::bin_cli::test_cmd::{TestCommandArgs, run_test_command};
    run_test_command(TestCommandArgs {
        mode: o.mode,
        main_branch: o.main_branch.as_deref(),
        base_branch: o.base_branch.as_deref(),
        dry_run: o.dry_run,
        ignore: &o.ignore,
        extra: &o.extra,
        lang_filter: o.lang,
        jobs: o.jobs,
        test_cfg: o.test_cfg,
    })
}

pub(in crate::bin_cli::dispatch) fn dispatch_mv(o: MvDispatchOptions) -> i32 {
    let ignore = normalize_ignore_prefixes(&o.ignore);
    let opts = kiss::symbol_mv::MvOptions {
        query: o.query,
        new_name: o.new_name,
        paths: o.paths,
        to: o.to,
        dry_run: o.mv_flags.dry_run,
        json: o.mv_flags.json,
        lang_filter: o.lang,
        ignore,
    };
    kiss::symbol_mv::run_mv_command(opts)
}

#[cfg(test)]
mod tests {
    use super::super::options::TriConfig;
    use super::*;

    struct DispatchFixture {
        py: kiss::Config,
        rs: kiss::Config,
        gate: kiss::GateConfig,
    }

    impl DispatchFixture {
        fn new() -> Self {
            Self {
                py: kiss::Config::python_defaults(),
                rs: kiss::Config::rust_defaults(),
                gate: kiss::GateConfig::default(),
            }
        }

        fn with_cfg<R>(&self, f: impl FnOnce(&TriConfig<'_>) -> R) -> R {
            let cfg = TriConfig {
                py: &self.py,
                rs: &self.rs,
                gate: &self.gate,
            };
            f(&cfg)
        }
    }

    #[test]
    fn dispatch_check_passes_empty_directory_when_gate_is_bypassed() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().to_string_lossy().to_string();
        let fix = DispatchFixture::new();

        let status = fix.with_cfg(|cfg| {
            dispatch_check(CheckDispatchOptions {
                lang: None,
                paths: vec![path],
                bypass_gate: true,
                ignore: vec![],
                timing: false,
                jobs: Some(1),
                cfg,
            })
        });

        assert_eq!(status, 0);
    }

    #[test]
    fn dispatch_dry_rejects_invalid_similarity_without_scanning() {
        let status = dispatch_dry(DryDispatchOptions {
            lang: None,
            path: ".".to_string(),
            filter_files: vec![],
            shingle_size: 3,
            minhash_size: 100,
            lsh_bands: 20,
            min_similarity: 1.5,
            ignore: vec![],
        });

        assert_eq!(status, 1);
    }

    #[test]
    fn dispatch_dry_accepts_tiny_source_directory() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.py"), "def same():\n    return 1\n").unwrap();

        let status = dispatch_dry(DryDispatchOptions {
            lang: Some(Language::Python),
            path: tmp.path().to_string_lossy().to_string(),
            filter_files: vec![],
            shingle_size: 3,
            minhash_size: 16,
            lsh_bands: 4,
            min_similarity: 0.9,
            ignore: vec![],
        });

        assert_eq!(status, 0);
    }

    #[test]
    fn dispatch_stats_and_mimic_accept_tiny_source_directory() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("sample.py"),
            "def sample():\n    return 1\n",
        )
        .unwrap();
        let path = tmp.path().to_string_lossy().to_string();
        let fix = DispatchFixture::new();

        let stats_status = fix.with_cfg(|cfg| {
            dispatch_stats(StatsDispatchOptions {
                lang: None,
                paths: vec![path.clone()],
                all: None,
                table: false,
                ignore: vec![],
                cfg,
            })
        });
        let mimic_status = dispatch_mimic(MimicDispatchOptions {
            lang: None,
            paths: vec![path],
            out: None,
            ignore: vec![],
        });

        assert_eq!(stats_status, 0);
        assert_eq!(mimic_status, 0);
    }

    #[test]
    fn dispatch_config_and_rules_report_success() {
        let fix = DispatchFixture::new();

        let config_status = fix.with_cfg(|cfg| {
            dispatch_config(ConfigDispatchOptions {
                defaults: true,
                config: None,
                cfg,
            })
        });
        let rules_status = fix.with_cfg(|cfg| {
            dispatch_rules(RulesDispatchOptions {
                lang: Some(Language::Rust),
                defaults: true,
                cfg,
            })
        });

        assert_eq!(config_status, 0);
        assert_eq!(rules_status, 0);
    }

    #[test]
    fn dispatch_viz_writes_graph_for_tiny_source_directory() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("a.py"),
            "import math\n\ndef sample():\n    return math.sqrt(4)\n",
        )
        .unwrap();
        let out = tmp.path().join("graph.mmd");

        let status = dispatch_viz(VizDispatchOptions {
            lang: Some(Language::Python),
            out: out.clone(),
            paths: vec![tmp.path().to_string_lossy().to_string()],
            zoom: 1.0,
            num_nodes: None,
            ignore: vec![],
        });

        assert_eq!(status, 0);
        assert!(out.exists());
    }

    #[test]
    fn dispatch_shrink_without_state_reports_failure() {
        let tmp = tempfile::TempDir::new().unwrap();
        let fix = DispatchFixture::new();

        let status = fix.with_cfg(|cfg| {
            dispatch_shrink(ShrinkDispatchOptions {
                lang: None,
                target: None,
                paths: vec![tmp.path().to_string_lossy().to_string()],
                ignore: vec![],
                cfg,
            })
        });

        assert_eq!(status, 1);
    }
}
