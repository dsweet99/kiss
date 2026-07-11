use super::{BinaryIdObjectMap, resolve_objects_for_profdata};
use crate::batch_export_tools::{
    ExportTools, objects_satisfy_profile, resolve_export_tools_from_rustc,
};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

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

    assert!(
        format!("{err:?}").contains("no catalog object matched profile binary id `deadbeef`"),
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
    let profraw = find_profraw(&target.join("llvm-cov-target")).expect("profraw");
    merge_profraw(&tools, &profraw, &profdata);
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
    let output = std::process::Command::new("cargo")
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
        .current_dir(fixture)
        .output()
        .expect("cargo llvm-cov test");
    assert!(
        output.status.success(),
        "fixture coverage run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn merge_profraw(tools: &ExportTools, profraw: &Path, profdata: &Path) {
    let merge = std::process::Command::new(&tools.llvm_profdata)
        .args(["merge", "-sparse", profraw.to_str().unwrap(), "-o"])
        .arg(profdata)
        .status()
        .expect("llvm-profdata merge");
    assert!(merge.success());
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

fn find_profraw(root: &Path) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("profraw") {
                return Some(path);
            }
        }
    }
    None
}

#[cfg(unix)]
fn write_executable(path: PathBuf, contents: &str) -> PathBuf {
    std::fs::write(&path, contents).unwrap();
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).unwrap();
    path
}
