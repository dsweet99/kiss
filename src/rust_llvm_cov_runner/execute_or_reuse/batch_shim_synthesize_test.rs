use super::{
    check_aggregate_pool_profile_path_for_run, synthesize_check_aggregate_shim_metadata,
    top_level_mod_names,
};
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_events::{
    BatchCompilerArtifact, BatchEventStream, BatchTestTerminal,
};
use std::path::PathBuf;

fn artifact(
    prefix: &str,
    executable: &str,
    src_path: Option<&str>,
    is_test_harness: bool,
) -> BatchCompilerArtifact {
    BatchCompilerArtifact {
        executable: Some(executable.to_string()),
        filenames: vec![format!("{executable}.rmeta")],
        nextest_binary_id: Some(prefix.to_string()),
        libtest_binary_prefix: Some(prefix.to_string()),
        src_path: src_path.map(str::to_string),
        is_test_harness,
    }
}

fn terminal(full_name: &str, passed: bool) -> BatchTestTerminal {
    BatchTestTerminal {
        full_name: full_name.to_string(),
        test_name: full_name
            .rsplit_once('$')
            .map(|(_, name)| name.to_string())
            .unwrap(),
        passed,
        timed_out: false,
        exec_time_secs: 0.01,
        stdout: None,
        reason: None,
    }
}

#[test]
fn synthesizes_shared_pool_metadata_with_deps_preference() {
    let stream = BatchEventStream {
        compiler_artifacts: vec![
            artifact("kiss-ai::bin/kiss", "/repo/target/debug/kiss", None, true),
            artifact(
                "kiss-ai::bin/kiss",
                "/repo/target/debug/deps/kiss-abc",
                None,
                true,
            ),
            artifact(
                "kiss-ai::kiss",
                "/repo/target/debug/deps/kiss_lib-def",
                None,
                true,
            ),
        ],
        terminal_tests: vec![
            terminal("kiss-ai::bin/kiss$cli::smoke", true),
            terminal("kiss-ai::kiss$config::tests::defaults", true),
        ],
        ..BatchEventStream::default()
    };
    let profile = PathBuf::from("/tmp/instances/pool-%32m.profraw");
    let meta = synthesize_check_aggregate_shim_metadata(
        &stream,
        &profile,
        PathBuf::from("/repo").as_path(),
    )
    .unwrap();
    assert_eq!(meta.len(), 2);
    assert_eq!(meta[0].argv, vec!["/repo/target/debug/deps/kiss-abc"]);
    assert_eq!(meta[1].argv, vec!["/repo/target/debug/deps/kiss_lib-def"]);
    assert_eq!(meta[0].profile_path, profile);
    assert_eq!(meta[1].profile_path, profile);
}

#[test]
fn colliding_lib_and_bin_libtest_prefix_disambiguates_via_src_mods() {
    let tmp = tempfile::tempdir().unwrap();
    let lib_src = tmp.path().join("lib.rs");
    let bin_src = tmp.path().join("main.rs");
    std::fs::write(&lib_src, "pub mod check_cache;\npub mod config;\n").unwrap();
    std::fs::write(&bin_src, "mod analyze;\nmod test_runner;\nmod bin_cli;\n").unwrap();

    let stream = BatchEventStream {
        compiler_artifacts: vec![
            BatchCompilerArtifact {
                executable: Some("/repo/target/debug/deps/kiss-lib".into()),
                filenames: vec![],
                nextest_binary_id: Some("kiss-ai::kiss".into()),
                libtest_binary_prefix: Some("kiss-ai::kiss".into()),
                src_path: Some(lib_src.to_string_lossy().into_owned()),
                is_test_harness: true,
            },
            BatchCompilerArtifact {
                executable: Some("/repo/target/debug/deps/kiss-bin".into()),
                filenames: vec![],
                nextest_binary_id: Some("kiss-ai::bin/kiss".into()),
                libtest_binary_prefix: Some("kiss-ai::kiss".into()),
                src_path: Some(bin_src.to_string_lossy().into_owned()),
                is_test_harness: true,
            },
            BatchCompilerArtifact {
                executable: Some("/repo/target/debug/kiss".into()),
                filenames: vec![],
                nextest_binary_id: Some("kiss-ai::bin/kiss".into()),
                libtest_binary_prefix: Some("kiss-ai::kiss".into()),
                src_path: Some(bin_src.to_string_lossy().into_owned()),
                is_test_harness: false,
            },
        ],
        terminal_tests: vec![
            terminal("kiss-ai::kiss$check_cache::tests::smoke", true),
            terminal(
                "kiss-ai::kiss$analyze::cov_records_cache::tests::round_trip",
                true,
            ),
        ],
        ..BatchEventStream::default()
    };
    let meta = synthesize_check_aggregate_shim_metadata(
        &stream,
        PathBuf::from("/tmp/pool-%32m.profraw").as_path(),
        PathBuf::from("/repo").as_path(),
    )
    .unwrap();
    assert_eq!(meta.len(), 2);
    assert_eq!(meta[0].argv, vec!["/repo/target/debug/deps/kiss-bin"]);
    assert_eq!(meta[1].argv, vec!["/repo/target/debug/deps/kiss-lib"]);
}

#[test]
fn colliding_same_mod_disambiguates_via_fn_body() {
    let tmp = tempfile::tempdir().unwrap();
    let lib_src = tmp.path().join("lib.rs");
    let bin_src = tmp.path().join("main.rs");
    std::fs::write(
        &lib_src,
        "#[cfg(test)]\npub mod cwd_test_lock {\n    pub fn lock() {}\n}\n",
    )
    .unwrap();
    std::fs::write(
        &bin_src,
        "#[cfg(test)]\npub(crate) mod cwd_test_lock {\n    pub fn lock() {}\n    #[test]\n    fn guard_restores_current_directory_during_unwind() {}\n}\n",
    )
    .unwrap();

    let stream = BatchEventStream {
        compiler_artifacts: vec![
            BatchCompilerArtifact {
                executable: Some("/repo/target/debug/deps/kiss-lib".into()),
                filenames: vec![],
                nextest_binary_id: Some("kiss-ai::kiss".into()),
                libtest_binary_prefix: Some("kiss-ai::kiss".into()),
                src_path: Some(lib_src.to_string_lossy().into_owned()),
                is_test_harness: true,
            },
            BatchCompilerArtifact {
                executable: Some("/repo/target/debug/deps/kiss-bin".into()),
                filenames: vec![],
                nextest_binary_id: Some("kiss-ai::bin/kiss".into()),
                libtest_binary_prefix: Some("kiss-ai::kiss".into()),
                src_path: Some(bin_src.to_string_lossy().into_owned()),
                is_test_harness: true,
            },
        ],
        terminal_tests: vec![terminal(
            "kiss-ai::kiss$cwd_test_lock::guard_restores_current_directory_during_unwind",
            true,
        )],
        ..BatchEventStream::default()
    };
    let meta = synthesize_check_aggregate_shim_metadata(
        &stream,
        PathBuf::from("/tmp/pool-%32m.profraw").as_path(),
        PathBuf::from("/repo").as_path(),
    )
    .unwrap();
    assert_eq!(meta.len(), 1);
    assert_eq!(meta[0].argv, vec!["/repo/target/debug/deps/kiss-bin"]);
}

#[test]
fn pool_profile_path_uses_online_merge_pattern() {
    let run_path = check_aggregate_pool_profile_path_for_run(
        PathBuf::from("/tmp/target").as_path(),
        PathBuf::from("/tmp/cache/runs/run-abc").as_path(),
    );
    assert_eq!(
        run_path,
        PathBuf::from("/tmp/target/run-abc-pool-%32m.profraw")
    );
}

#[test]
fn missing_executable_for_libtest_prefix_errors() {
    let stream = BatchEventStream {
        compiler_artifacts: vec![artifact("other::other", "/tmp/other", None, true)],
        terminal_tests: vec![terminal("kiss-ai::kiss$missing", true)],
        ..BatchEventStream::default()
    };
    let err = synthesize_check_aggregate_shim_metadata(
        &stream,
        PathBuf::from("/tmp/pool-%32m.profraw").as_path(),
        PathBuf::from("/repo").as_path(),
    )
    .unwrap_err();
    assert!(format!("{err:?}").contains("kiss-ai::kiss"));
}

#[test]
fn top_level_mod_names_reads_pub_and_private_mods() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("main.rs");
    std::fs::write(
        &path,
        "// mod ignored_comment;\npub mod analyze;\nmod test_runner;\npub(crate) mod rust_units;\nfn main() {}\n",
    )
    .unwrap();
    let mods = top_level_mod_names(&path);
    assert!(mods.contains("analyze"));
    assert!(mods.contains("test_runner"));
    assert!(mods.contains("rust_units"));
    assert!(!mods.contains("ignored_comment"));
}
