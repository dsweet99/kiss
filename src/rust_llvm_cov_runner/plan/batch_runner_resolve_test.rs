use std::collections::BTreeMap;
use std::fs;

use crate::rust_llvm_cov_runner::RustLlvmCovError;
use crate::rust_llvm_cov_runner::plan::batch_runner_resolve::{
    delegated_runner_for_platform, placeholder_delegated_runner_fields, read_runner_map,
    resolve_batch_request_runners, resolve_delegated_runners, runner_map_fingerprint,
    write_runner_map,
};
use crate::rust_llvm_cov_runner::test_support::{
    make_executable, runner_resolve_base_request, runner_resolve_toml_string,
};

#[test]
fn resolved_delegated_runners_struct_round_trips() {
    let map = BTreeMap::from([("host".to_string(), vec!["/bin/true".to_string()])]);
    let left = crate::rust_llvm_cov_runner::plan::batch_runner_resolve::ResolvedDelegatedRunners {
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
fn placeholder_delegated_runner_fields_are_fingerprint_consistent() {
    let (map, fingerprint, host) = placeholder_delegated_runner_fields();
    assert_eq!(host, "x86_64-unknown-linux-gnu");
    assert_eq!(fingerprint, runner_map_fingerprint(&map));
    assert_eq!(delegated_runner_for_platform(&map, &host), Some(&[][..]));
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
fn runner_resolve_cache_normalizes_duplicate_path_entries() {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .unwrap();
    fs::write(repo.path().join("src/lib.rs"), "pub fn x() {}\n").unwrap();

    let separator = if cfg!(windows) { ';' } else { ':' };
    let mut req = runner_resolve_base_request(repo.path());
    req.env.insert(
        "PATH".to_string(),
        ["/first", "/second"].join(&separator.to_string()),
    );
    resolve_delegated_runners(&req).unwrap();
    req.env.insert(
        "PATH".to_string(),
        ["/first", "/second", "/first"].join(&separator.to_string()),
    );

    assert!(super::try_runner_resolve_cache(&req).is_some());
}

#[test]
fn resolve_batch_request_runners_populates_mutable_request_fields() {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .unwrap();
    fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();

    let mut req = runner_resolve_base_request(repo.path());
    resolve_batch_request_runners(&mut req).unwrap();

    assert!(!req.host_platform.is_empty());
    assert!(req.delegated_runners.contains_key(&req.host_platform));
    assert_eq!(
        req.runner_map_fingerprint,
        runner_map_fingerprint(&req.delegated_runners)
    );
}

#[test]
fn runner_map_io_reports_decode_error_and_missing_parent() {
    let tmp = tempfile::tempdir().unwrap();
    let bad_json = tmp.path().join("bad.json");
    fs::write(&bad_json, b"not json").unwrap();
    let err = read_runner_map(&bad_json).unwrap_err();
    assert!(matches!(err, RustLlvmCovError::InvalidRequest(message) if message.contains("decode")));

    let err = write_runner_map(std::path::Path::new(""), &BTreeMap::new()).unwrap_err();
    assert!(matches!(
        err,
        RustLlvmCovError::InvalidRequest(message) if message.contains("no parent")
    ));
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

#[test]
fn resolve_delegated_runners_rejects_invalid_inline_runner_config() {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .unwrap();
    fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();

    let mut req = runner_resolve_base_request(repo.path());
    req.cargo_args = vec![
        "--config".to_string(),
        "[target.\"x86_64-unknown-linux-gnu\"]\nrunner = 1\n".to_string(),
    ];

    let err = resolve_delegated_runners(&req).unwrap_err();
    assert!(matches!(
        err,
        RustLlvmCovError::InvalidRequest(message)
            if message.contains("runner must be a string or string list")
    ));
}

#[test]
fn resolve_delegated_runners_rejects_runner_list_with_non_string_item() {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .unwrap();
    fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();

    let mut req = runner_resolve_base_request(repo.path());
    let host = resolve_delegated_runners(&req).unwrap().host_platform;
    req.cargo_args = vec![format!(
        "--config=[target.{}]\nrunner = [\"/bin/echo\", 1]\n",
        host
    )];

    let err = resolve_delegated_runners(&req).unwrap_err();
    assert!(matches!(
        err,
        RustLlvmCovError::InvalidRequest(message)
            if message.contains("runner list must contain only strings")
    ));
}

#[test]
fn resolve_delegated_runners_rejects_missing_cargo_config_file() {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .unwrap();
    fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();

    let mut req = runner_resolve_base_request(repo.path());
    req.cargo_args = vec!["--config".to_string(), "does-not-exist.toml".to_string()];

    let err = resolve_delegated_runners(&req).unwrap_err();
    assert!(matches!(
        err,
        RustLlvmCovError::InvalidRequest(message)
            if message.contains("failed to read Cargo --config file")
    ));
}

#[test]
fn resolve_delegated_runners_accepts_explicit_target_argument_forms() {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .unwrap();
    fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();

    let base = runner_resolve_base_request(repo.path());
    let host = resolve_delegated_runners(&base).unwrap().host_platform;
    let mut req = base.clone();
    req.cargo_args = vec![
        "--target".to_string(),
        host.clone(),
        format!("--target={host}"),
    ];

    let resolved = resolve_delegated_runners(&req).unwrap();
    assert_eq!(resolved.host_platform, host);
    assert!(resolved.map.contains_key(&host));
}
