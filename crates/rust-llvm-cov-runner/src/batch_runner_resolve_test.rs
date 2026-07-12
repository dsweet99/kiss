use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::batch_plan::RustCoverageBatchRequest;
use crate::batch_runner_resolve::placeholder_delegated_runner_fields;
use crate::batch_runner_resolve::{
    delegated_runner_for_platform, resolve_delegated_runners, runner_map_fingerprint,
    write_runner_map,
};
use crate::test_support::make_executable;

#[test]
fn resolved_delegated_runners_struct_round_trips() {
    let map = BTreeMap::from([("host".to_string(), vec!["/bin/true".to_string()])]);
    let left = crate::batch_runner_resolve::ResolvedDelegatedRunners {
        map: map.clone(),
        host_platform: "host".to_string(),
    };
    let right = left.clone();
    assert_eq!(left, right);
    assert_eq!(
        left.map.get("host").unwrap(),
        &vec!["/bin/true".to_string()]
    );
}

#[test]
fn delegated_runner_for_platform_selects_map_entry() {
    let map = BTreeMap::from([("host".to_string(), vec!["/bin/true".to_string()])]);
    assert_eq!(
        delegated_runner_for_platform(&map, "host").map(|argv| argv.to_vec()),
        Some(vec!["/bin/true".to_string()])
    );
    assert!(delegated_runner_for_platform(&map, "missing").is_none());
}

#[test]
fn resolve_delegated_runners_returns_host_platform_and_map() {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .unwrap();
    fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    let req = base_request(repo.path());
    let resolved = resolve_delegated_runners(&req).unwrap();
    let clone = resolved.clone();
    assert_eq!(clone.host_platform, resolved.host_platform);
    assert_eq!(clone.map, resolved.map);
    assert!(!resolved.host_platform.is_empty());
    assert!(resolved.map.contains_key(&resolved.host_platform));
    let _ = format!("{resolved:?}");
}

#[test]
fn resolve_delegated_runners_reads_repository_target_runner() {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join(".cargo")).unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .unwrap();
    fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    let wrapper = repo.path().join("wrapper.sh");
    fs::write(&wrapper, "#!/bin/sh\nexec \"$@\"\n").unwrap();
    make_executable(&wrapper);

    let base = base_request(repo.path());
    let host = resolve_delegated_runners(&base).unwrap().host_platform;
    fs::write(
        repo.path().join(".cargo/config.toml"),
        format!(
            "[target.{}]\nrunner = [{}]\n",
            host,
            toml_string(&wrapper.to_string_lossy())
        ),
    )
    .unwrap();

    let resolved = resolve_delegated_runners(&base).unwrap();
    let runner = resolved
        .map
        .get(&resolved.host_platform)
        .expect("host runner");
    assert_eq!(
        runner.first().map(String::as_str),
        Some(wrapper.to_string_lossy().as_ref())
    );
}

#[test]
fn resolve_delegated_runners_honors_cargo_config_cli_runner_override() {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .unwrap();
    fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    let wrapper = repo.path().join("cli-runner.sh");
    fs::write(&wrapper, "#!/bin/sh\nexec \"$@\"\n").unwrap();
    make_executable(&wrapper);

    let mut req = base_request(repo.path());
    let host = resolve_delegated_runners(&req).unwrap().host_platform;
    req.cargo_args = vec![
        "--config".to_string(),
        format!(
            "[target.{}]\nrunner = [{}]\n",
            host,
            toml_string(&wrapper.to_string_lossy())
        ),
    ];
    let resolved = resolve_delegated_runners(&req).unwrap();
    assert_eq!(
        resolved.map.get(&host),
        Some(&vec![wrapper.to_string_lossy().to_string()])
    );
}

#[test]
fn resolve_delegated_runners_honors_cargo_config_cli_dotted_runner_override() {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .unwrap();
    fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    let wrapper = repo.path().join("dotted-runner.sh");
    fs::write(&wrapper, "#!/bin/sh\nexec \"$@\"\n").unwrap();
    make_executable(&wrapper);

    let mut req = base_request(repo.path());
    let host = resolve_delegated_runners(&req).unwrap().host_platform;
    req.cargo_args = vec![format!(
        "--config=target.{}.runner = [{}, \"--from-dotted\"]",
        host,
        toml_string(&wrapper.to_string_lossy())
    )];

    let resolved = resolve_delegated_runners(&req).unwrap();
    assert_eq!(
        resolved.map.get(&host),
        Some(&vec![
            wrapper.to_string_lossy().to_string(),
            "--from-dotted".to_string(),
        ])
    );
}

#[test]
fn resolve_delegated_runners_honors_cargo_config_file_runner_override() {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .unwrap();
    fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    let wrapper = repo.path().join("file-runner.sh");
    fs::write(&wrapper, "#!/bin/sh\nexec \"$@\"\n").unwrap();
    make_executable(&wrapper);

    let mut req = base_request(repo.path());
    let host = resolve_delegated_runners(&req).unwrap().host_platform;
    fs::write(
        repo.path().join("runner-config.toml"),
        format!(
            "[target.{}]\nrunner = [{}, \"--from-file\"]\n",
            host,
            toml_string(&wrapper.to_string_lossy())
        ),
    )
    .unwrap();
    req.cargo_args = vec!["--config".to_string(), "runner-config.toml".to_string()];

    let resolved = resolve_delegated_runners(&req).unwrap();
    assert_eq!(
        resolved.map.get(&host),
        Some(&vec![
            wrapper.to_string_lossy().to_string(),
            "--from-file".to_string(),
        ])
    );
}

#[test]
fn resolve_delegated_runners_preserves_relative_runner_with_args_and_build_target() {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join(".cargo")).unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::create_dir_all(repo.path().join("tools with spaces")).unwrap();
    fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .unwrap();
    fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    let wrapper = repo.path().join("tools with spaces").join("runner.sh");
    fs::write(&wrapper, "#!/bin/sh\nexec \"$@\"\n").unwrap();
    make_executable(&wrapper);

    let base = base_request(repo.path());
    let host = resolve_delegated_runners(&base).unwrap().host_platform;
    fs::write(
        repo.path().join(".cargo/config.toml"),
        format!(
            "[build]\ntarget = {}\n[target.{}]\nrunner = [{}, \"arg with space\", \"--flag\"]\n",
            toml_string(&host),
            host,
            toml_string("./tools with spaces/runner.sh")
        ),
    )
    .unwrap();

    let resolved = resolve_delegated_runners(&base).unwrap();
    assert_eq!(resolved.host_platform, host);
    let resolved_wrapper = repo.path().join("./tools with spaces/runner.sh");
    assert_eq!(
        resolved.map.get(&host),
        Some(&vec![
            resolved_wrapper.to_string_lossy().to_string(),
            "arg with space".to_string(),
            "--flag".to_string(),
        ])
    );
}

#[test]
fn resolve_delegated_runners_reads_runner_from_cargo_home_env() {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .unwrap();
    fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();

    let cargo_home = tempfile::tempdir().unwrap();
    let wrapper = cargo_home.path().join("env-runner.sh");
    fs::write(&wrapper, "#!/bin/sh\nexec \"$@\"\n").unwrap();
    make_executable(&wrapper);

    let mut req = base_request(repo.path());
    let host = resolve_delegated_runners(&req).unwrap().host_platform;
    fs::write(
        cargo_home.path().join("config.toml"),
        format!(
            "[target.{}]\nrunner = [{}]\n",
            host,
            toml_string(&wrapper.to_string_lossy())
        ),
    )
    .unwrap();
    req.env
        .insert("CARGO_HOME".to_string(), cargo_home.path().to_string_lossy().to_string());

    let resolved = resolve_delegated_runners(&req).unwrap();
    assert_eq!(
        resolved.map.get(&host),
        Some(&vec![wrapper.to_string_lossy().to_string()])
    );
}

#[test]
fn resolve_delegated_runners_reads_cfg_target_runner_section() {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join(".cargo")).unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .unwrap();
    fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    let wrapper = repo.path().join("cfg-runner.sh");
    fs::write(&wrapper, "#!/bin/sh\nexec \"$@\"\n").unwrap();
    make_executable(&wrapper);

    let base = base_request(repo.path());
    let host = resolve_delegated_runners(&base).unwrap().host_platform;
    fs::write(
        repo.path().join(".cargo/config.toml"),
        format!(
            "[target.'cfg(unix)']\nrunner = [{}, \"--from-cfg\"]\n",
            toml_string(&wrapper.to_string_lossy())
        ),
    )
    .unwrap();

    let resolved = resolve_delegated_runners(&base).unwrap();
    assert_eq!(
        resolved.map.get(&host),
        Some(&vec![
            wrapper.to_string_lossy().to_string(),
            "--from-cfg".to_string(),
        ])
    );
}

#[test]
fn resolve_delegated_runners_cli_config_overrides_repository_runner() {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join(".cargo")).unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .unwrap();
    fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    let repo_runner = repo.path().join("repo-runner.sh");
    fs::write(&repo_runner, "#!/bin/sh\nexec \"$@\"\n").unwrap();
    make_executable(&repo_runner);
    let cli_runner = repo.path().join("cli-runner.sh");
    fs::write(&cli_runner, "#!/bin/sh\nexec \"$@\"\n").unwrap();
    make_executable(&cli_runner);

    let mut req = base_request(repo.path());
    let host = resolve_delegated_runners(&req).unwrap().host_platform;
    fs::write(
        repo.path().join(".cargo/config.toml"),
        format!(
            "[target.{}]\nrunner = [{}]\n",
            host,
            toml_string(&repo_runner.to_string_lossy())
        ),
    )
    .unwrap();
    req.cargo_args = vec![
        "--config".to_string(),
        format!(
            "[target.{}]\nrunner = [{}]\n",
            host,
            toml_string(&cli_runner.to_string_lossy())
        ),
    ];

    let resolved = resolve_delegated_runners(&req).unwrap();
    assert_eq!(
        resolved.map.get(&host),
        Some(&vec![cli_runner.to_string_lossy().to_string()])
    );
}

#[test]
fn runner_map_fingerprint_changes_when_delegated_runner_changes() {
    let mut left = BTreeMap::from([("host".to_string(), vec!["a".to_string()])]);
    let mut right = left.clone();
    assert_eq!(
        runner_map_fingerprint(&left),
        runner_map_fingerprint(&right)
    );
    right.insert("host".to_string(), vec!["b".to_string()]);
    assert_ne!(
        runner_map_fingerprint(&left),
        runner_map_fingerprint(&right)
    );
    left.insert("other".to_string(), vec![]);
    assert_ne!(
        runner_map_fingerprint(&left),
        runner_map_fingerprint(&right)
    );
}

#[test]
fn write_runner_map_round_trips() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("runner-map.json");
    let map = BTreeMap::from([(
        "x86_64-unknown-linux-gnu".to_string(),
        vec!["/bin/true".to_string()],
    )]);
    write_runner_map(&path, &map).unwrap();
    let loaded = crate::batch_runner_resolve::read_runner_map(&path).unwrap();
    assert_eq!(loaded, map);
}

fn base_request(repo: &Path) -> RustCoverageBatchRequest {
    let (delegated_runners, runner_map_fingerprint, host_platform) =
        placeholder_delegated_runner_fields();
    RustCoverageBatchRequest {
        cwd: repo.to_path_buf(),
        source_root: repo.to_path_buf(),
        cargo: PathBuf::from("cargo"),
        cache_root: repo.join(".kiss/rust_llvm_cov_cache"),
        logical_selectors: vec!["alpha".to_string()],
        cargo_args: Vec::new(),
        test_args: Vec::new(),
        env: BTreeMap::new(),
        force_rerun: false,
        jobs: 1,
        generated_config: repo.join(".kiss/rust_llvm_cov_cache/runs/run-test/nextest.toml"),
        population_publication_selectors: None,
        delegated_runners,
        runner_map_fingerprint,
        host_platform,
    }
}

fn toml_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}
