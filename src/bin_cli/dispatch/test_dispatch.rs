use super::handlers;
use super::options;
use super::{dispatch, dispatch_analyze, dispatch_tools};
use crate::bin_cli::args::{Cli, Commands};
use crate::bin_cli::run::run_with_cli;
use kiss::GateConfig;
use kiss::TestSectionConfig;

#[test]
fn dispatch_routes_supported_commands_to_handlers() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("sample.py"), "def old():\n    return 1\n").unwrap();
    let out_dot = tmp.path().join("out.dot");
    let orig_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = GateConfig::default();
    let test = TestSectionConfig::default();
    let cfg = options::TriConfig {
        py: &py,
        rs: &rs,
        gate: &gate,
    };
    call_handler_dispatchers(&cfg, &test, out_dot);
    call_router_dispatchers(&py, &rs, &gate, &test, &cfg);
    std::env::set_current_dir(orig_dir).unwrap();
}

fn call_handler_dispatchers(
    cfg: &options::TriConfig<'_>,
    test: &TestSectionConfig,
    out_dot: std::path::PathBuf,
) {
    let _ = handlers::dispatch_check(options::CheckDispatchOptions {
        lang: None,
        paths: vec![".".to_string()],
        ignore: vec![],
        timing: false,
        cfg,
    });
    let _ = handlers::dispatch_stats(options::StatsDispatchOptions {
        lang: None,
        paths: vec![".".to_string()],
        all: None,
        table: false,
        ignore: vec![],
        cfg,
    });
    let _ = handlers::dispatch_mimic(options::MimicDispatchOptions {
        lang: None,
        paths: vec![".".to_string()],
        out: None,
        ignore: vec![],
    });
    assert_eq!(handlers::dispatch_clamp(None, vec![]), 0);
    let _ = handlers::dispatch_dry(options::DryDispatchOptions {
        lang: None,
        path: ".".to_string(),
        filter_files: vec![],
        shingle_size: 3,
        minhash_size: 100,
        lsh_bands: 20,
        min_similarity: 0.9,
        ignore: vec![],
    });
    assert_eq!(
        handlers::dispatch_rules(options::RulesDispatchOptions {
            lang: None,
            defaults: true,
            cfg,
        }),
        0
    );
    assert_eq!(
        handlers::dispatch_config(options::ConfigDispatchOptions {
            defaults: true,
            config: None,
            cfg,
            test_cfg: test,
        }),
        0
    );
    let _ = handlers::dispatch_viz(options::VizDispatchOptions {
        lang: None,
        out: out_dot,
        paths: vec![".".to_string()],
        zoom: 1.0,
        num_nodes: None,
        ignore: vec![],
    });
    let _ = handlers::dispatch_shrink(options::ShrinkDispatchOptions {
        lang: None,
        target: Some("files=1".to_string()),
        paths: vec![".".to_string()],
        ignore: vec![],
        cfg,
    });
    let _ = handlers::dispatch_test(options::TestDispatchOptions {
        lang: None,
        invocation: crate::bin_cli::args::TestInvocation::Commit,
        main_branch: None,
        base_branch: None,
        dry_run: true,
        force: false,
        force_bad: false,        metrics: false,
        coverage_all: false,
        watch: false,
        jobs: None,
        ignore: vec![],
        extra: vec![],
        test_cfg: test,
        cfg,
    });
    let _ = handlers::dispatch_mv(options::MvDispatchOptions {
        lang: None,
        query: "sample.py::old".to_string(),
        new_name: "new".to_string(),
        paths: vec![".".to_string()],
        to: None,
        mv_flags: options::MvOutputFlags {
            dry_run: true,
            json: false,
        },
        ignore: vec![],
    });
}

fn call_router_dispatchers(
    py: &kiss::Config,
    rs: &kiss::Config,
    gate: &GateConfig,
    test: &TestSectionConfig,
    cfg: &options::TriConfig<'_>,
) {
    let _ = dispatch_analyze(
        None,
        Commands::Check {
            paths: vec![".".to_string()],
            ignore: vec![],
            timing: false,
        },
        cfg,
        test,
    );
    assert_eq!(
        dispatch_tools(None, true, None, Commands::Rules, cfg, test),
        0
    );
    assert_eq!(
        dispatch(
            Cli {
                config: None,
                lang: None,
                defaults: true,
                command: Commands::Rules,
            },
            py,
            rs,
            gate,
            test,
        ),
        0
    );
    assert_eq!(
        run_with_cli(Cli {
            config: None,
            lang: None,
            defaults: true,
            command: Commands::Rules,
        }),
        0
    );
}

#[test]
fn dispatch_test_command_rejects_invalid_modes_before_running_tests() {
    let test = TestSectionConfig::default();
    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = kiss::GateConfig::default();
    let cfg = super::TriConfig {
        py: &py,
        rs: &rs,
        gate: &gate,
    };

    assert_eq!(
        super::dispatch_test_command(
            None,
            Commands::Test {
                operands: vec!["all".to_string()],
                main_branch: None,
                base_branch: None,
                dry_run: true,
                force: false,
        force_bad: false,                metrics: false,
                coverage_all: false,
                watch: false,
                watch_bg: false,
                jobs: None,
                ignore: vec![],
                extra: vec![],
            },
            &cfg,
            &test,
        ),
        2
    );
    assert_eq!(
        super::dispatch_test_command(None, Commands::Rules, &cfg, &test),
        2
    );
}

#[test]
fn dispatch_private_routers_reject_commands_from_the_other_group() {
    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = GateConfig::default();
    let test = TestSectionConfig::default();
    let cfg = options::TriConfig {
        py: &py,
        rs: &rs,
        gate: &gate,
    };

    assert_eq!(dispatch_analyze(None, Commands::Rules, &cfg, &test), 2);
    assert_eq!(
        dispatch_tools(
            None,
            true,
            None,
            Commands::Check {
                paths: vec![".".to_string()],
                ignore: vec![],
                timing: false,
            },
            &cfg,
            &test,
        ),
        2
    );
}

#[test]
fn dispatch_private_routers_cover_additional_command_variants() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("sample.py"), "def old():\n    return 1\n").unwrap();
    let orig_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = GateConfig::default();
    let test = TestSectionConfig::default();
    let cfg = options::TriConfig {
        py: &py,
        rs: &rs,
        gate: &gate,
    };

    let _ = dispatch_analyze(
        None,
        Commands::Stats {
            paths: vec![".".to_string()],
            all: None,
            table: false,
            ignore: vec![],
        },
        &cfg,
        &test,
    );
    let _ = dispatch_analyze(
        None,
        Commands::Mimic {
            paths: vec![".".to_string()],
            out: None,
            ignore: vec![],
        },
        &cfg,
        &test,
    );
    assert_eq!(
        dispatch_analyze(None, Commands::Clamp { ignore: vec![] }, &cfg, &test),
        0
    );
    let _ = dispatch_tools(
        None,
        true,
        None,
        Commands::Dry {
            path: ".".to_string(),
            filter_files: vec![],
            shingle_size: 3,
            minhash_size: 100,
            lsh_bands: 20,
            min_similarity: 0.9,
            ignore: vec![],
        },
        &cfg,
        &test,
    );
    let _ = dispatch_tools(
        None,
        true,
        None,
        Commands::Mv {
            query: "sample.py::old".to_string(),
            new_name: "new".to_string(),
            paths: vec![".".to_string()],
            to: None,
            dry_run: true,
            json: false,
            ignore: vec![],
        },
        &cfg,
        &test,
    );

    std::env::set_current_dir(orig_dir).unwrap();
}

#[test]
fn dispatch_test_rejects_watch_with_dry_run() {
    let test = TestSectionConfig::default();
    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = kiss::GateConfig::default();
    let cfg = super::TriConfig {
        py: &py,
        rs: &rs,
        gate: &gate,
    };
    assert_eq!(
        super::dispatch_test_command(
            None,
            Commands::Test {
                operands: vec![".".to_string()],
                main_branch: None,
                base_branch: None,
                dry_run: true,
                force: false,
                force_bad: false,
                metrics: false,
                coverage_all: false,
                watch: true,
                watch_bg: false,
                jobs: None,
                ignore: vec![],
                extra: vec![],
            },
            &cfg,
            &test,
        ),
        2
    );
}

