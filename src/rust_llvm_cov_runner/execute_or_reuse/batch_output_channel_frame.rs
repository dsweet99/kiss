use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;

use crate::rust_llvm_cov_runner::execute_or_reuse::batch_output_channel_token::TOKEN_LEN;

use crate::rust_llvm_cov_runner::execute_or_reuse::batch_output_channel::{
    FRAME_MAGIC, OutputChannelFrame, OutputStreamKind,
};

pub(crate) fn relay_frame_live(frame: &OutputChannelFrame) {
    match frame.stream {
        OutputStreamKind::Stdout => {
            let _ = io::stdout().lock().write_all(&frame.bytes);
        }
        OutputStreamKind::Stderr => {
            let _ = io::stderr().lock().write_all(&frame.bytes);
        }
    }
}

pub(crate) fn write_frame(
    stream: &mut UnixStream,
    token: &[u8; TOKEN_LEN],
    instance_id: &str,
    sequence: u32,
    stream_kind: OutputStreamKind,
    payload: &[u8],
) -> io::Result<()> {
    let id_bytes = instance_id.as_bytes();
    if id_bytes.len() > u16::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "instance id too long for output channel frame",
        ));
    }
    stream.write_all(FRAME_MAGIC)?;
    stream.write_all(token)?;
    stream.write_all(&sequence.to_le_bytes())?;
    stream.write_all(&[stream_kind as u8])?;
    stream.write_all(&(id_bytes.len() as u16).to_le_bytes())?;
    stream.write_all(id_bytes)?;
    stream.write_all(&(payload.len() as u32).to_le_bytes())?;
    stream.write_all(payload)?;
    stream.flush()?;
    Ok(())
}

pub(crate) fn read_frame(
    stream: &mut UnixStream,
    expected_token: &[u8; TOKEN_LEN],
) -> io::Result<OutputChannelFrame> {
    let mut magic = [0u8; 4];
    read_exact_or_eof(stream, &mut magic)?;
    if &magic != FRAME_MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "output channel frame has invalid magic",
        ));
    }
    let mut token = [0u8; TOKEN_LEN];
    read_exact_or_eof(stream, &mut token)?;
    if token != *expected_token {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "output channel frame has invalid auth token",
        ));
    }
    let mut sequence_bytes = [0u8; 4];
    read_exact_or_eof(stream, &mut sequence_bytes)?;
    let sequence = u32::from_le_bytes(sequence_bytes);
    let mut stream_byte = [0u8; 1];
    read_exact_or_eof(stream, &mut stream_byte)?;
    let stream_kind = match stream_byte[0] {
        0 => OutputStreamKind::Stdout,
        1 => OutputStreamKind::Stderr,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "output channel frame has invalid stream kind",
            ));
        }
    };
    let mut id_len_bytes = [0u8; 2];
    read_exact_or_eof(stream, &mut id_len_bytes)?;
    let id_len = u16::from_le_bytes(id_len_bytes) as usize;
    let mut id_bytes = vec![0u8; id_len];
    read_exact_or_eof(stream, &mut id_bytes)?;
    let instance_id = String::from_utf8(id_bytes).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("output channel instance id is not utf-8: {err}"),
        )
    })?;
    let mut payload_len_bytes = [0u8; 4];
    read_exact_or_eof(stream, &mut payload_len_bytes)?;
    let payload_len = u32::from_le_bytes(payload_len_bytes) as usize;
    let mut bytes = vec![0u8; payload_len];
    read_exact_or_eof(stream, &mut bytes)?;
    Ok(OutputChannelFrame {
        instance_id,
        sequence,
        stream: stream_kind,
        bytes,
    })
}

fn read_exact_or_eof(stream: &mut UnixStream, buf: &mut [u8]) -> io::Result<()> {
    let mut offset = 0;
    while offset < buf.len() {
        match stream.read(&mut buf[offset..]) {
            Ok(0) if offset == 0 => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "output channel eof",
                ));
            }
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "output channel truncated frame",
                ));
            }
            Ok(read) => offset += read,
            Err(err) => return Err(err),
        }
    }
    Ok(())
}
