use std::thread;

use super::{
    OUTPUT_CHANNEL_SOCKET_ENV, OUTPUT_CHANNEL_TOKEN_ENV, OutputChannelServer, OutputStreamKind,
    create_output_channel_config, send_output_frames,
};

#[test]
fn output_channel_round_trips_frames_with_auth() {
    let tmp = tempfile::tempdir().unwrap();
    let config = create_output_channel_config(tmp.path(), false).unwrap();
    let server = OutputChannelServer::start(config.clone()).unwrap();
    send_output_frames(&config, "alpha", b"out-a", b"err-a").unwrap();
    send_output_frames(&config, "beta", b"out-b", b"").unwrap();
    thread::sleep(std::time::Duration::from_millis(20));
    let frames = server.stop();
    assert_eq!(frames.len(), 3);
    let alpha_stdout: Vec<_> = frames
        .iter()
        .filter(|frame| frame.instance_id == "alpha" && frame.stream == OutputStreamKind::Stdout)
        .map(|frame| frame.bytes.as_slice())
        .collect();
    let alpha_stderr: Vec<_> = frames
        .iter()
        .filter(|frame| frame.instance_id == "alpha" && frame.stream == OutputStreamKind::Stderr)
        .map(|frame| frame.bytes.as_slice())
        .collect();
    let beta_stdout: Vec<_> = frames
        .iter()
        .filter(|frame| frame.instance_id == "beta" && frame.stream == OutputStreamKind::Stdout)
        .map(|frame| frame.bytes.as_slice())
        .collect();
    assert_eq!(alpha_stdout, vec![b"out-a".as_slice()]);
    assert_eq!(alpha_stderr, vec![b"err-a".as_slice()]);
    assert_eq!(beta_stdout, vec![b"out-b".as_slice()]);
}

#[test]
fn output_channel_rejects_invalid_token() {
    let tmp = tempfile::tempdir().unwrap();
    let config = create_output_channel_config(tmp.path(), false).unwrap();
    let server = OutputChannelServer::start(config.clone()).unwrap();
    let mut bad = config.clone();
    bad.token[0] ^= 0xff;
    let _ = send_output_frames(&bad, "alpha", b"x", b"");
    thread::sleep(std::time::Duration::from_millis(20));
    let frames = server.stop();
    assert!(frames.is_empty());
}

#[test]
fn apply_output_channel_env_sets_socket_and_token() {
    let tmp = tempfile::tempdir().unwrap();
    let config = create_output_channel_config(tmp.path(), true).unwrap();
    let mut env = std::collections::BTreeMap::new();
    super::apply_output_channel_env(&mut env, &config);
    assert_eq!(
        env.get(super::OUTPUT_CHANNEL_SOCKET_ENV),
        Some(&config.socket_path.to_string_lossy().to_string())
    );
    let token_hex = env
        .get(OUTPUT_CHANNEL_TOKEN_ENV)
        .expect("token env present");
    assert_eq!(token_hex.len(), 32);
    // SAFETY: test-only env mutation restored below.
    unsafe {
        for (key, value) in &env {
            std::env::set_var(key, value);
        }
    }
    let loaded = super::output_channel_config_from_env().expect("config from env");
    assert_eq!(loaded.socket_path, config.socket_path);
    assert_eq!(loaded.token, config.token);
    // SAFETY: test-only env cleanup.
    unsafe {
        std::env::remove_var(OUTPUT_CHANNEL_SOCKET_ENV);
        std::env::remove_var(OUTPUT_CHANNEL_TOKEN_ENV);
    }
}
