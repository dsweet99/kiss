use std::collections::BTreeMap;
use std::fs;

use crate::batch_runner_resolve::{
    resolve_delegated_runners, runner_map_fingerprint, write_runner_map,
};
use crate::test_support::{
    make_executable, runner_resolve_base_request, runner_resolve_toml_string,
};

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

    let mut req = runner_resolve_base_request(repo.path());
    let host = resolve_delegated_runners(&req).unwrap().host_platform;
    fs::write(
        cargo_home.path().join("config.toml"),
        format!(
            "[target.{}]\nrunner = [{}]\n",
            host,
            runner_resolve_toml_string(&wrapper.to_string_lossy())
        ),
    )
    .unwrap();
    req.env.insert(
        "CARGO_HOME".to_string(),
        cargo_home.path().to_string_lossy().to_string(),
    );

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

    let base = runner_resolve_base_request(repo.path());
    let host = resolve_delegated_runners(&base).unwrap().host_platform;
    fs::write(
        repo.path().join(".cargo/config.toml"),
        format!(
            "[target.'cfg(unix)']\nrunner = [{}, \"--from-cfg\"]\n",
            runner_resolve_toml_string(&wrapper.to_string_lossy())
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

    let mut req = runner_resolve_base_request(repo.path());
    let host = resolve_delegated_runners(&req).unwrap().host_platform;
    fs::write(
        repo.path().join(".cargo/config.toml"),
        format!(
            "[target.{}]\nrunner = [{}]\n",
            host,
            runner_resolve_toml_string(&repo_runner.to_string_lossy())
        ),
    )
    .unwrap();
    req.cargo_args = vec![
        "--config".to_string(),
        format!(
            "[target.{}]\nrunner = [{}]\n",
            host,
            runner_resolve_toml_string(&cli_runner.to_string_lossy())
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
