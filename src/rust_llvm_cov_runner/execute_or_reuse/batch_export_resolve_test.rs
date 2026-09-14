use super::{BinaryIdObjectMap, resolve_objects_for_profdata};
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_export_tools::{
    ExportTools, objects_satisfy_profile,
};
use crate::rust_llvm_cov_runner::test_support::write_executable;
use std::path::{Path, PathBuf};

const ENCLOSING_COVERAGE_ENV_KEYS: &[&str] = &[
    "LLVM_PROFILE_FILE",
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
    "RUSTFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "RUSTDOCFLAGS",
    "CARGO_TARGET_DIR",
    "CARGO_LLVM_COV_TARGET_DIR",
    "CARGO_LLVM_COV_BUILD_DIR",
];

#[test]
#[cfg(unix)]
fn binary_id_object_map_prefers_deps_path_for_duplicate_build_ids() {
    let tmp = tempfile::tempdir().unwrap();
    let root_binary = tmp.path().join("kiss");
    let deps_binary = tmp.path().join("deps").join("kiss-hash");
    std::fs::create_dir_all(deps_binary.parent().unwrap()).unwrap();
    std::fs::write(&root_binary, b"binary").unwrap();
    std::fs::write(&deps_binary, b"binary").unwrap();
    let llvm_readobj = write_executable(
        tmp.path().join("llvm-readobj"),
        "#!/bin/sh\nprintf 'Build ID: deadbeef\\n'\nexit 0\n",
    );
    let tools = ExportTools {
        llvm_profdata: PathBuf::from("/bin/false"),
        llvm_cov: PathBuf::from("/bin/false"),
        llvm_readobj,
    };
    let map = BinaryIdObjectMap::build(&tools, &[root_binary.clone(), deps_binary.clone()])
        .expect("duplicate build ids should dedupe");
    assert_eq!(map.lookup("deadbeef"), Some(&deps_binary));

    let root_only = tmp.path().join("other").join("kiss");
    std::fs::create_dir_all(root_only.parent().unwrap()).unwrap();
    std::fs::write(&root_only, b"binary").unwrap();
    let map_root_first =
        BinaryIdObjectMap::build(&tools, &[root_only.clone(), deps_binary.clone()])
            .expect("duplicate build ids should dedupe");
    assert_eq!(map_root_first.lookup("deadbeef"), Some(&deps_binary));

    let deps_first = BinaryIdObjectMap::build(&tools, &[deps_binary.clone(), root_only.clone()])
        .expect("duplicate build ids should dedupe");
    assert_eq!(deps_first.lookup("deadbeef"), Some(&deps_binary));
}

#[test]
#[cfg(unix)]
fn binary_id_object_map_rejects_ambiguous_duplicate_build_ids() {
    let tmp = tempfile::tempdir().unwrap();
    let first = tmp.path().join("bin-a").join("kiss");
    let second = tmp.path().join("bin-b").join("kiss");
    std::fs::create_dir_all(first.parent().unwrap()).unwrap();
    std::fs::create_dir_all(second.parent().unwrap()).unwrap();
    std::fs::write(&first, b"binary").unwrap();
    std::fs::write(&second, b"binary").unwrap();
    let llvm_readobj = write_executable(
        tmp.path().join("llvm-readobj"),
        "#!/bin/sh\nprintf 'Build ID: cafebabe\\n'\nexit 0\n",
    );
    let tools = ExportTools {
        llvm_profdata: PathBuf::from("/bin/false"),
        llvm_cov: PathBuf::from("/bin/false"),
        llvm_readobj,
    };
    let err = BinaryIdObjectMap::build(&tools, &[first.clone(), second.clone()])
        .expect_err("duplicate build ids outside deps should error");
    assert!(
        format!("{err:?}").contains("ambiguous catalog objects"),
        "unexpected error: {err:?}"
    );
}

#[test]
#[cfg(unix)]
fn binary_id_object_map_rejects_duplicate_build_ids_in_deps() {
    let tmp = tempfile::tempdir().unwrap();
    let first = tmp.path().join("deps").join("kiss-a");
    let second = tmp.path().join("deps").join("kiss-b");
    std::fs::create_dir_all(first.parent().unwrap()).unwrap();
    std::fs::write(&first, b"binary").unwrap();
    std::fs::write(&second, b"binary").unwrap();
    let llvm_readobj = write_executable(
        tmp.path().join("llvm-readobj"),
        "#!/bin/sh\nprintf 'Build ID: feedface\\n'\nexit 0\n",
    );
    let tools = ExportTools {
        llvm_profdata: PathBuf::from("/bin/false"),
        llvm_cov: PathBuf::from("/bin/false"),
        llvm_readobj,
    };
    let err = BinaryIdObjectMap::build(&tools, &[first.clone(), second.clone()])
        .expect_err("duplicate build ids in deps should error");
    assert!(
        format!("{err:?}").contains("ambiguous catalog objects"),
        "unexpected error: {err:?}"
    );
}

#[test]
fn binary_id_object_map_builds_from_catalog() {
    let tools = ExportTools {
        llvm_profdata: PathBuf::from("/bin/false"),
        llvm_cov: PathBuf::from("/bin/false"),
        llvm_readobj: PathBuf::from("/bin/false"),
    };
    let map = BinaryIdObjectMap::build(&tools, &[]).expect("empty map");
    assert!(map.lookup("missing").is_none());
}

#[test]
#[cfg(unix)]
fn binary_id_object_map_build_with_jobs_matches_serial() {
    let tmp = tempfile::tempdir().unwrap();
    let first = tmp.path().join("deps").join("a");
    let second = tmp.path().join("deps").join("b");
    std::fs::create_dir_all(first.parent().unwrap()).unwrap();
    std::fs::write(&first, b"a").unwrap();
    std::fs::write(&second, b"b").unwrap();
    let llvm_readobj = write_executable(
        tmp.path().join("llvm-readobj"),
        "#!/bin/sh\ncase \"$2\" in *b) printf 'Build ID: bbbbbbbb\\n' ;; *) printf 'Build ID: aaaaaaaa\\n' ;; esac\nexit 0\n",
    );
    let tools = ExportTools {
        llvm_profdata: PathBuf::from("/bin/false"),
        llvm_cov: PathBuf::from("/bin/false"),
        llvm_readobj,
    };
    let catalog = vec![first.clone(), second.clone()];
    let serial = BinaryIdObjectMap::build(&tools, &catalog).expect("serial");
    let parallel = BinaryIdObjectMap::build_with_jobs(&tools, &catalog, 4).expect("parallel");
    assert_eq!(serial.lookup("aaaaaaaa"), parallel.lookup("aaaaaaaa"));
    assert_eq!(serial.lookup("bbbbbbbb"), parallel.lookup("bbbbbbbb"));
    assert_eq!(serial.lookup("aaaaaaaa"), Some(&first));
    assert_eq!(serial.lookup("bbbbbbbb"), Some(&second));
}

#[test]
fn resolve_objects_for_profdata_requires_seed_objects() {
    let tools = ExportTools {
        llvm_profdata: PathBuf::from("/bin/false"),
        llvm_cov: PathBuf::from("/bin/false"),
        llvm_readobj: PathBuf::from("/bin/false"),
    };
    assert!(
        resolve_objects_for_profdata(
            &tools,
            Path::new("/tmp/missing.profdata"),
            &[PathBuf::from("/tmp/catalog.o")],
            &[],
            None,
        )
        .is_err()
    );
}

#[test]
fn resolve_objects_for_profdata_requires_binary_id_map() {
    let tools = ExportTools {
        llvm_profdata: PathBuf::from("/bin/false"),
        llvm_cov: PathBuf::from("/bin/false"),
        llvm_readobj: PathBuf::from("/bin/false"),
    };
    let err = resolve_objects_for_profdata(
        &tools,
        Path::new("/tmp/missing.profdata"),
        &[PathBuf::from("/tmp/catalog.o")],
        &[PathBuf::from("/tmp/seed.o")],
        None,
    )
    .expect_err("missing map");
    assert!(
        format!("{err:?}").contains("binary-id object map"),
        "unexpected error: {err:?}"
    );
}

#[test]
#[cfg(unix)]
fn resolve_objects_for_profdata_rejects_unresolved_profile_binary_ids() {
    let tmp = tempfile::tempdir().unwrap();
    let llvm_profdata = write_executable(
        tmp.path().join("llvm-profdata"),
        "#!/bin/sh\nif [ \"$1\" = show ]; then printf 'Binary IDs:\\ndeadbeef\\n'; exit 0; fi\nexit 1\n",
    );
    let llvm_cov = write_executable(
        tmp.path().join("llvm-cov"),
        "#!/bin/sh\nif echo \"$@\" | grep -q -- -instr-profile; then exit 1; fi\nprintf '{\"data\":[{\"binary_ids\":[\"cafebabe\"]}]}'\nexit 0\n",
    );
    let llvm_readobj = write_executable(
        tmp.path().join("llvm-readobj"),
        "#!/bin/sh\nprintf 'Build ID: cafebabe\\n'\nexit 0\n",
    );
    let tools = ExportTools {
        llvm_profdata,
        llvm_cov,
        llvm_readobj,
    };
    let profdata = tmp.path().join("instance.profdata");
    let seed = tmp.path().join("seed-object");
    std::fs::write(&profdata, b"profile").unwrap();
    std::fs::write(&seed, b"object").unwrap();

    let err = resolve_objects_for_profdata(
        &tools,
        &profdata,
        &[],
        std::slice::from_ref(&seed),
        Some(&BinaryIdObjectMap::default()),
    )
    .expect_err("unresolved nonempty binary id should be an error");

    let message = format!("{err:?}");
    assert!(
        message.contains("no catalog object matched profile binary id `deadbeef`")
            || message.contains("seed-filtered object resolve produced no objects"),
        "unexpected error: {err:?}"
    );
}

#[test]
#[cfg(unix)]
fn resolve_objects_for_profdata_resolves_profile_binary_ids() {
    // Synthetic tools only: a real cargo llvm-cov fixture exceeds the execute_or_reuse
    // 60s SLA under parallel `kiss test` load (observed 96s). Export-contract modules
    // already exercise real llvm tooling; this asserts selective id→object resolve.
    let tmp = tempfile::tempdir().unwrap();
    let deps = tmp.path().join("deps");
    std::fs::create_dir_all(&deps).unwrap();
    let integration = deps.join("integration-hash");
    let rlib = deps.join("libexport_contract_runner-hash.rlib");
    std::fs::write(&integration, b"integration").unwrap();
    std::fs::write(&rlib, b"rlib").unwrap();
    let llvm_profdata = write_executable(
        tmp.path().join("llvm-profdata"),
        "#!/bin/sh\nif [ \"$1\" = show ]; then printf 'Binary IDs:\\naaaaaaaa\\n'; exit 0; fi\nexit 1\n",
    );
    let llvm_cov = write_executable(
        tmp.path().join("llvm-cov"),
        "#!/bin/sh\nif echo \"$@\" | grep -q -- -check-binary-ids; then exit 0; fi\nexit 1\n",
    );
    let llvm_readobj = write_executable(
        tmp.path().join("llvm-readobj"),
        "#!/bin/sh\ncase \"$2\" in *integration*) printf 'Build ID: aaaaaaaa\\n' ;; *) printf 'Build ID: bbbbbbbb\\n' ;; esac\nexit 0\n",
    );
    let tools = ExportTools {
        llvm_profdata,
        llvm_cov,
        llvm_readobj,
    };
    let profdata = tmp.path().join("instance.profdata");
    std::fs::write(&profdata, b"profile").unwrap();
    let catalog = vec![integration.clone(), rlib.clone()];
    let seed = vec![integration.clone(), rlib];
    let map = BinaryIdObjectMap::build(&tools, &catalog).expect("binary id map");
    let resolved = resolve_objects_for_profdata(&tools, &profdata, &catalog, &seed, Some(&map))
        .expect("resolved");
    assert_eq!(resolved, vec![integration]);
    assert!(objects_satisfy_profile(&tools, &profdata, &resolved));
}

#[test]
fn export_contract_fixture_scrubs_enclosing_coverage_environment() {
    let mut command = std::process::Command::new("cargo");
    scrub_enclosing_coverage_environment(&mut command);

    for key in ENCLOSING_COVERAGE_ENV_KEYS {
        assert_eq!(
            command
                .get_envs()
                .find(|(candidate, _)| candidate == key)
                .unwrap()
                .1,
            None,
            "{key} must not leak into the nested coverage command"
        );
    }
}

fn scrub_enclosing_coverage_environment(command: &mut std::process::Command) {
    for key in ENCLOSING_COVERAGE_ENV_KEYS {
        command.env_remove(key);
    }
}
