use std::io::{self, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use super::FRAME_MAGIC;
use super::{
    OUTPUT_CHANNEL_TMP_DIR, OutputChannelConfig, OutputChannelFrame, OutputChannelServer,
    OutputStreamKind, create_output_channel_config, read_connection_frames, read_frame,
    send_output_frames, write_frame,
};

#[test]
fn output_channel_binds_short_socket_for_long_run_root() {
    let long_root = PathBuf::from("/tmp").join("x".repeat(200));
    let config = create_output_channel_config(&long_root, false).unwrap();
    assert!(
        config.socket_path.starts_with(OUTPUT_CHANNEL_TMP_DIR),
        "socket must live under {OUTPUT_CHANNEL_TMP_DIR}, got {}",
        config.socket_path.display()
    );
    assert!(
        config.socket_path.as_os_str().len() < 108,
        "socket path must fit SUN_LEN: {}",
        config.socket_path.display()
    );
    let server = OutputChannelServer::start(config.clone()).unwrap();
    send_output_frames(&config, "long", b"ok", b"").unwrap();
    thread::sleep(std::time::Duration::from_millis(20));
    let frames = server.stop();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].bytes, b"ok");
}

#[test]
fn output_channel_live_relay_collects_stderr_frames() {
    let tmp = tempfile::tempdir().unwrap();
    let config = create_output_channel_config(tmp.path(), true).unwrap();
    let server = OutputChannelServer::start(config.clone()).unwrap();
    send_output_frames(&config, "live", b"", b"live-err").unwrap();
    thread::sleep(std::time::Duration::from_millis(20));
    let frames = server.stop();
    assert_eq!(frames.len(), 1);
    assert_eq!(
        frames[0],
        OutputChannelFrame {
            instance_id: "live".to_string(),
            sequence: 0,
            stream: OutputStreamKind::Stderr,
            bytes: b"live-err".to_vec(),
        }
    );
}

#[test]
fn output_channel_live_relay_collects_stdout_frames() {
    let tmp = tempfile::tempdir().unwrap();
    let config = create_output_channel_config(tmp.path(), true).unwrap();
    let server = OutputChannelServer::start(config.clone()).unwrap();
    assert!(config.relay_live);
    send_output_frames(&config, "live", b"live-out", b"").unwrap();
    thread::sleep(std::time::Duration::from_millis(20));
    let frames = server.stop();
    assert_eq!(frames[0].stream, OutputStreamKind::Stdout);
    assert_eq!(frames[0].bytes, b"live-out");
}

#[test]
fn output_channel_rejects_malformed_frames() {
    let tmp = tempfile::tempdir().unwrap();
    let config = create_output_channel_config(tmp.path(), false).unwrap();
    let server = OutputChannelServer::start(config.clone()).unwrap();
    let mut bad_magic = UnixStream::connect(&config.socket_path).unwrap();
    bad_magic.write_all(b"BAD!").unwrap();
    drop(bad_magic);
    let mut bad_stream = UnixStream::connect(&config.socket_path).unwrap();
    bad_stream.write_all(FRAME_MAGIC).unwrap();
    bad_stream.write_all(&config.token).unwrap();
    bad_stream.write_all(&0u32.to_le_bytes()).unwrap();
    bad_stream.write_all(&[99]).unwrap();
    drop(bad_stream);
    thread::sleep(std::time::Duration::from_millis(20));
    let frames = server.stop();
    assert!(frames.is_empty());
}

#[test]
fn output_channel_server_reports_malformed_frame_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let config = create_output_channel_config(tmp.path(), false).unwrap();
    let server = OutputChannelServer::start(config.clone()).unwrap();
    let mut bad_magic = UnixStream::connect(&config.socket_path).unwrap();
    bad_magic.write_all(b"BAD!").unwrap();
    drop(bad_magic);
    thread::sleep(std::time::Duration::from_millis(20));

    let stopped = server.stop_with_errors();

    assert!(stopped.frames.is_empty());
    assert!(
        stopped
            .errors
            .iter()
            .any(|error| error.contains("invalid magic"))
    );
}

#[test]
fn read_frame_parses_valid_stdout_frame() {
    let (mut server, mut client) = UnixStream::pair().unwrap();
    let token = [7u8; 16];
    write_frame(
        &mut client,
        &token,
        "inst",
        3,
        OutputStreamKind::Stdout,
        b"payload",
    )
    .unwrap();
    let frame = read_frame(&mut server, &token).unwrap();
    assert_eq!(frame.instance_id, "inst");
    assert_eq!(frame.sequence, 3);
    assert_eq!(frame.stream, OutputStreamKind::Stdout);
    assert_eq!(frame.bytes, b"payload");
}

#[test]
fn read_frame_reports_invalid_magic_stream_and_truncation() {
    let token = [9u8; 16];
    let (mut server, mut client) = UnixStream::pair().unwrap();
    client.write_all(b"NOPE").unwrap();
    assert!(read_frame(&mut server, &token).is_err());

    let (mut server, mut client) = UnixStream::pair().unwrap();
    write_frame(&mut client, &token, "x", 0, OutputStreamKind::Stderr, b"z").unwrap();
    let frame = read_frame(&mut server, &token).unwrap();
    assert_eq!(frame.stream, OutputStreamKind::Stderr);

    let (mut server, mut client) = UnixStream::pair().unwrap();
    client.write_all(FRAME_MAGIC).unwrap();
    client.write_all(&token).unwrap();
    drop(client);
    assert!(matches!(
        read_frame(&mut server, &token),
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof
    ));
}

#[test]
fn output_channel_config_clone_and_stream_debug() {
    let tmp = tempfile::tempdir().unwrap();
    let config = create_output_channel_config(tmp.path(), false).unwrap();
    let cloned = config.clone();
    assert_eq!(config.socket_path, cloned.socket_path);
    assert!(format!("{:?}", OutputStreamKind::Stdout).contains("Stdout"));
    for kind in OutputStreamKind::witness_values() {
        assert!(!format!("{kind:?}").is_empty());
    }
    let _ = OutputChannelConfig::witness();
}

#[test]
fn read_frame_rejects_bad_token() {
    let (mut server, mut client) = UnixStream::pair().unwrap();
    let good = [1u8; 16];
    let bad = [2u8; 16];
    write_frame(&mut client, &good, "x", 0, OutputStreamKind::Stdout, b"a").unwrap();
    assert!(read_frame(&mut server, &bad).is_err());
}

#[test]
fn read_connection_frames_drains_valid_connection() {
    let tmp = tempfile::tempdir().unwrap();
    let config = create_output_channel_config(tmp.path(), true).unwrap();
    let frames = Arc::new(Mutex::new(Vec::new()));
    let (mut server, mut client) = UnixStream::pair().unwrap();
    write_frame(
        &mut client,
        &config.token,
        "inst",
        0,
        OutputStreamKind::Stdout,
        b"hi",
    )
    .unwrap();
    drop(client);
    read_connection_frames(&mut server, &config, &frames).unwrap();
    let stored = frames.lock().unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].bytes, b"hi");
}

#[test]
fn output_channel_server_witness_frames_starts_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let config = create_output_channel_config(tmp.path(), false).unwrap();
    let server = OutputChannelServer::start(config).unwrap();
    assert_eq!(server.witness_frames(), 0);
    let _ = server.stop();
}

#[test]
fn output_channel_chunks_large_payloads() {
    let tmp = tempfile::tempdir().unwrap();
    let config = create_output_channel_config(tmp.path(), false).unwrap();
    let server = OutputChannelServer::start(config.clone()).unwrap();
    let stdout = vec![b'a'; 9000];
    let stderr = vec![b'b'; 5000];
    send_output_frames(&config, "large", &stdout, &stderr).unwrap();
    thread::sleep(std::time::Duration::from_millis(30));
    let frames = server.stop();
    let stdout_bytes: Vec<u8> = frames
        .iter()
        .filter(|frame| frame.stream == OutputStreamKind::Stdout)
        .flat_map(|frame| frame.bytes.clone())
        .collect();
    let stderr_bytes: Vec<u8> = frames
        .iter()
        .filter(|frame| frame.stream == OutputStreamKind::Stderr)
        .flat_map(|frame| frame.bytes.clone())
        .collect();
    assert_eq!(stdout_bytes, stdout);
    assert_eq!(stderr_bytes, stderr);
    assert!(
        frames.len() > 2,
        "large payloads must be split into multiple frames"
    );
}

#[test]
fn output_channel_client_connect_send_and_shutdown() {
    let tmp = tempfile::tempdir().unwrap();
    let config = create_output_channel_config(tmp.path(), false).unwrap();
    let server = OutputChannelServer::start(config.clone()).unwrap();
    let mut client = super::OutputChannelClient::connect(&config).unwrap();
    client
        .send_chunk("direct", OutputStreamKind::Stdout, b"one")
        .unwrap();
    client
        .send_chunk("direct", OutputStreamKind::Stderr, b"two")
        .unwrap();
    client.shutdown().unwrap();
    thread::sleep(std::time::Duration::from_millis(20));
    let frames = server.stop();
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0].bytes, b"one");
    assert_eq!(frames[1].bytes, b"two");
}

#[test]
fn chunk_output_splits_large_buffers() {
    let bytes: Vec<u8> = (0..10_000).map(|index| (index % 251) as u8).collect();
    let chunks: Vec<_> = super::chunk_output(&bytes).collect();
    assert!(chunks.len() > 1);
    let joined: Vec<u8> = chunks
        .into_iter()
        .flat_map(|chunk| chunk.to_vec())
        .collect();
    assert_eq!(joined, bytes);
}

#[test]
fn output_channel_stop_reports_frames_and_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let config = create_output_channel_config(tmp.path(), false).unwrap();
    let server = OutputChannelServer::start(config.clone()).unwrap();
    let mut bad_magic = UnixStream::connect(&config.socket_path).unwrap();
    bad_magic.write_all(b"BAD!").unwrap();
    drop(bad_magic);
    thread::sleep(std::time::Duration::from_millis(20));
    let stopped = server.stop_with_errors();
    assert!(stopped.frames.is_empty());
    assert!(!stopped.errors.is_empty());
    assert!(format!("{stopped:?}").contains("frames"));
}

#[test]
fn output_channel_rejects_truncated_frame() {
    let tmp = tempfile::tempdir().unwrap();
    let config = create_output_channel_config(tmp.path(), false).unwrap();
    let server = OutputChannelServer::start(config.clone()).unwrap();
    let mut stream = UnixStream::connect(&config.socket_path).unwrap();
    stream.write_all(FRAME_MAGIC).unwrap();
    stream.write_all(&config.token).unwrap();
    drop(stream);
    thread::sleep(std::time::Duration::from_millis(20));
    let frames = server.stop();
    assert!(frames.is_empty());
}
