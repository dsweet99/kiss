use super::{BinaryIdObjectMap, resolve_objects_for_profdata};
use crate::execute_or_reuse::batch_export_tools::{
    ExportTools, objects_satisfy_profile, resolve_export_tools_from_rustc,
};
use crate::test_support::write_executable;
use std::ffi::OsStr;
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
fn resolve_objects_for_profdata_resolves_profile_binary_ids() {
    let tools = resolve_export_tools_from_rustc(OsStr::new("rustc")).unwrap();
    let target = std::env::temp_dir().join(format!("kiss-export-minimal-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&target);
    run_export_contract_fixture(&target);
    let profdata = target.join("instance.profdata");
    let deps = target.join("llvm-cov-target/debug/deps");
    let integration = find_deps_artifact(&deps, |name| {
        name.starts_with("integration-") && !name.ends_with(".d")
    })
    .expect("integration test binary");
    let rlib = find_deps_artifact(&deps, |name| name.starts_with("libexport_contract_runner-"))
        .expect("runner rlib");
    let catalog = vec![integration.clone(), rlib.clone()];
    let seed = vec![integration.clone(), rlib];
    let map = BinaryIdObjectMap::build(&tools, &catalog).expect("binary id map");
    let integration_id = map
        .lookup_by_object(&integration)
        .expect("integration build id");
    let profraw =
        find_profraw_for_binary_id(&tools, &target.join("llvm-cov-target"), integration_id)
            .expect("integration profraw");
    merge_profraw(&tools, &profraw, &profdata);
    let resolved = resolve_objects_for_profdata(&tools, &profdata, &catalog, &seed, Some(&map))
        .expect("resolved");
    assert_eq!(resolved, vec![integration]);
    assert!(objects_satisfy_profile(&tools, &profdata, &resolved));
}

fn run_export_contract_fixture(target: &Path) {
    let fixture = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/export_contract"
    ));
    let mut command = std::process::Command::new("cargo");
    scrub_enclosing_coverage_environment(&mut command);
    command
        .args([
            "llvm-cov",
            "test",
            "-p",
            "export-contract-runner",
            "--manifest-path",
            &fixture.join("Cargo.toml").to_string_lossy(),
            "--",
            "--test-threads=1",
            "invokes_helper_in_process",
        ])
        .env("CARGO_TARGET_DIR", target)
        .env(
            "RUSTFLAGS",
            "-Cinstrument-coverage -Clink-arg=-Wl,--build-id=sha1",
        )
        .current_dir(fixture);
    let output = command.output().expect("cargo llvm-cov test");
    assert!(
        output.status.success(),
        "fixture coverage run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
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

fn merge_profraws(tools: &ExportTools, profraws: &[PathBuf], profdata: &Path) {
    let mut command = std::process::Command::new(&tools.llvm_profdata);
    command.arg("merge").arg("-sparse");
    for profraw in profraws {
        command.arg(profraw);
    }
    command.arg("-o").arg(profdata);
    let merge = command.status().expect("llvm-profdata merge");
    assert!(merge.success());
}

fn merge_profraw(tools: &ExportTools, profraw: &Path, profdata: &Path) {
    merge_profraws(tools, &[profraw.to_path_buf()], profdata);
}

fn find_deps_artifact(deps: &Path, matches: impl Fn(&str) -> bool + Clone) -> Option<PathBuf> {
    std::fs::read_dir(deps)
        .ok()?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(&matches)
        })
}

fn find_profraw_for_binary_id(
    tools: &ExportTools,
    root: &Path,
    expected_id: &str,
) -> Option<PathBuf> {
    let tmp_profdata = std::env::temp_dir().join(format!(
        "kiss-export-profraw-probe-{}-{}.profdata",
        std::process::id(),
        expected_id
    ));
    for profraw in find_all_profraws(root) {
        merge_profraw(tools, &profraw, &tmp_profdata);
        let ids = crate::execute_or_reuse::batch_export_tools::read_profdata_binary_ids(tools, &tmp_profdata).ok()?;
        if ids == [expected_id] {
            let _ = std::fs::remove_file(&tmp_profdata);
            return Some(profraw);
        }
    }
    let _ = std::fs::remove_file(&tmp_profdata);
    None
}

fn find_all_profraws(root: &Path) -> Vec<PathBuf> {
    let mut profraws = Vec::new();
    collect_profraws(root, &mut profraws);
    profraws.sort();
    profraws
}

fn collect_profraws(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_profraws(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("profraw") {
            out.push(path);
        }
    }
}

#[allow(dead_code)]
fn find_profraw(root: &Path) -> Option<PathBuf> {
    find_all_profraws(root).into_iter().next()
}
