use std::collections::BTreeMap;
use std::fs;

use crate::batch_runner_resolve::{delegated_runner_for_platform, resolve_delegated_runners};
use crate::test_support::{
    make_executable, runner_resolve_base_request, runner_resolve_toml_string,
};

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
    let req = runner_resolve_base_request(repo.path());
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

    let base = runner_resolve_base_request(repo.path());
    let host = resolve_delegated_runners(&base).unwrap().host_platform;
    fs::write(
        repo.path().join(".cargo/config.toml"),
        format!(
            "[target.{}]\nrunner = [{}]\n",
            host,
            runner_resolve_toml_string(&wrapper.to_string_lossy())
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

    let mut req = runner_resolve_base_request(repo.path());
    let host = resolve_delegated_runners(&req).unwrap().host_platform;
    req.cargo_args = vec![
        "--config".to_string(),
        format!(
            "[target.{}]\nrunner = [{}]\n",
            host,
            runner_resolve_toml_string(&wrapper.to_string_lossy())
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

    let mut req = runner_resolve_base_request(repo.path());
    let host = resolve_delegated_runners(&req).unwrap().host_platform;
    req.cargo_args = vec![format!(
        "--config=target.{}.runner = [{}, \"--from-dotted\"]",
        host,
        runner_resolve_toml_string(&wrapper.to_string_lossy())
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
fn resolve_delegated_runners_honors_plain_string_cargo_config_runner_override() {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .unwrap();
    fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    let wrapper = repo.path().join("plain-runner.sh");
    fs::write(&wrapper, "#!/bin/sh\nexec \"$@\"\n").unwrap();
    make_executable(&wrapper);

    let mut req = runner_resolve_base_request(repo.path());
    let host = resolve_delegated_runners(&req).unwrap().host_platform;
    fs::write(
        repo.path().join("runner-config.toml"),
        format!(
            "[target.{}]\nrunner = {}\n",
            host,
            runner_resolve_toml_string(&wrapper.to_string_lossy())
        ),
    )
    .unwrap();
    req.cargo_args = vec!["--config".to_string(), "runner-config.toml".to_string()];

    let resolved = resolve_delegated_runners(&req).unwrap();
    assert_eq!(
        resolved.map.get(&host),
        Some(&vec![wrapper.to_string_lossy().to_string()])
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

    let mut req = runner_resolve_base_request(repo.path());
    let host = resolve_delegated_runners(&req).unwrap().host_platform;
    fs::write(
        repo.path().join("runner-config.toml"),
        format!(
            "[target.{}]\nrunner = [{}, \"--from-file\"]\n",
            host,
            runner_resolve_toml_string(&wrapper.to_string_lossy())
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

    let base = runner_resolve_base_request(repo.path());
    let host = resolve_delegated_runners(&base).unwrap().host_platform;
    fs::write(
        repo.path().join(".cargo/config.toml"),
        format!(
            "[build]\ntarget = {}\n[target.{}]\nrunner = [{}, \"arg with space\", \"--flag\"]\n",
            runner_resolve_toml_string(&host),
            host,
            runner_resolve_toml_string("./tools with spaces/runner.sh")
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
