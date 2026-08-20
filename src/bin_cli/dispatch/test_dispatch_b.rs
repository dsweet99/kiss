use super::*;

#[test]
fn public_dispatch_routes_analyze_and_tool_groups() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("sample.py"), "def value():\n    return 1\n").unwrap();
    let orig_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = GateConfig::default();
    let test = TestSectionConfig::default();

    assert_eq!(
        dispatch(
            Cli {
                config: None,
                lang: Some(kiss::Language::Python),
                defaults: true,
                command: Commands::Clamp { ignore: vec![] },
            },
            &py,
            &rs,
            &gate,
            &test,
        ),
        0
    );
    assert_ne!(
        dispatch(
            Cli {
                config: None,
                lang: Some(kiss::Language::Python),
                defaults: true,
                command: Commands::Stats {
                    paths: vec![".".to_string()],
                    all: Some(1),
                    table: true,
                    ignore: vec![],
                },
            },
            &py,
            &rs,
            &gate,
            &test,
        ),
        2
    );
    assert_ne!(
        dispatch(
            Cli {
                config: None,
                lang: Some(kiss::Language::Python),
                defaults: true,
                command: Commands::Mimic {
                    paths: vec![".".to_string()],
                    out: None,
                    ignore: vec![],
                },
            },
            &py,
            &rs,
            &gate,
            &test,
        ),
        2
    );
    assert_eq!(
        dispatch(
            Cli {
                config: None,
                lang: None,
                defaults: true,
                command: Commands::Config,
            },
            &py,
            &rs,
            &gate,
            &test,
        ),
        0
    );
    assert_ne!(
        dispatch(
            Cli {
                config: None,
                lang: Some(kiss::Language::Python),
                defaults: true,
                command: Commands::Dry {
                    path: ".".to_string(),
                    filter_files: vec![],
                    shingle_size: 3,
                    minhash_size: 100,
                    lsh_bands: 20,
                    min_similarity: 0.9,
                    ignore: vec![],
                },
            },
            &py,
            &rs,
            &gate,
            &test,
        ),
        2
    );
    assert_ne!(
        dispatch(
            Cli {
                config: None,
                lang: Some(kiss::Language::Python),
                defaults: true,
                command: Commands::Mv {
                    query: "sample.py::value".to_string(),
                    new_name: "new_value".to_string(),
                    paths: vec![".".to_string()],
                    to: None,
                    dry_run: true,
                    json: false,
                    ignore: vec![],
                },
            },
            &py,
            &rs,
            &gate,
            &test,
        ),
        2
    );
    assert_eq!(
        dispatch(
            Cli {
                config: None,
                lang: None,
                defaults: true,
                command: Commands::Test {
                    operands: vec!["all".to_string()],
                    main_branch: None,
                    base_branch: None,
                    dry_run: true,
                    force: false,
                    force_bad: false,
                    metrics: false,
                    coverage_all: false,
                    watch: false,
                    jobs: None,
                    ignore: vec![],
                    extra: vec![],
                },
            },
            &py,
            &rs,
            &gate,
            &test,
        ),
        2
    );

    std::env::set_current_dir(orig_dir).unwrap();
}

#[test]
fn dispatch_test_command_routes_valid_test_mode() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    let orig_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();

    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = kiss::GateConfig::default();
    let cfg = super::TriConfig {
        py: &py,
        rs: &rs,
        gate: &gate,
    };
    let code = super::dispatch_test_command(
        None,
        true,
        None,
        Commands::Test {
            operands: vec!["commit".to_string()],
            main_branch: None,
            base_branch: None,
            dry_run: true,
            force: false,
            force_bad: false,
            metrics: false,
            coverage_all: false,
            watch: false,
            jobs: None,
            ignore: vec![],
            extra: vec![],
        },
        &cfg,
        &TestSectionConfig::default(),
    );

    std::env::set_current_dir(orig_dir).unwrap();
    assert_eq!(code, 1);
}

#[test]
fn dispatch_test_command_rejects_removed_validate_selection() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    let orig_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();

    let py = kiss::Config::python_defaults();
    let rs = kiss::Config::rust_defaults();
    let gate = kiss::GateConfig::default();
    let cfg = super::TriConfig {
        py: &py,
        rs: &rs,
        gate: &gate,
    };
    let code = super::dispatch_test_command(
        Some(kiss::Language::Python),
        true,
        None,
        Commands::Test {
            operands: vec!["validate-selection".to_string()],
            main_branch: None,
            base_branch: None,
            dry_run: true,
            force: false,
            force_bad: false,
            metrics: false,
            coverage_all: false,
            watch: false,
            jobs: None,
            ignore: vec![],
            extra: vec![],
        },
        &cfg,
        &TestSectionConfig::default(),
    );

    std::env::set_current_dir(orig_dir).unwrap();
    assert_eq!(code, 2);
}
