#[cfg(debug_assertions)]
use std::fs::{self, OpenOptions};
use std::io;
#[cfg(debug_assertions)]
use std::io::Write;
use std::path::Path;
#[cfg(debug_assertions)]
use std::path::PathBuf;
use std::process;
#[cfg(debug_assertions)]
use std::thread;
#[cfg(debug_assertions)]
use std::time::Duration;
#[cfg(debug_assertions)]
use std::time::Instant;
use std::time::{SystemTime, UNIX_EPOCH};

/// Process-id + nanosecond suffix for uniquely named temporary files.
pub fn unique_process_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{}.{}", process::id(), nanos)
}

#[cfg(debug_assertions)]
const SCHEMA_VERSION: u32 = 1;
#[cfg(any(debug_assertions, test))]
const BARRIER_DIR_ENV: &str = "KISS_QA_PUBLICATION_BARRIER_DIR";
#[cfg(any(debug_assertions, test))]
const BARRIER_TARGET_ENV: &str = "KISS_QA_PUBLICATION_BARRIER_TARGET";
#[cfg(debug_assertions)]
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(25);
#[cfg(debug_assertions)]
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg(debug_assertions)]
#[derive(Clone, Copy)]
struct WaitPolicy {
    poll_interval: Duration,
    timeout: Duration,
}

#[cfg(not(debug_assertions))]
#[derive(Clone, Copy)]
struct WaitPolicy;

#[cfg(debug_assertions)]
impl WaitPolicy {
    const fn default() -> Self {
        Self {
            poll_interval: DEFAULT_POLL_INTERVAL,
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

#[cfg(not(debug_assertions))]
impl WaitPolicy {
    const fn default() -> Self {
        Self
    }
}

pub fn after_sync_before_rename(
    artifact: &str,
    temporary_path: &Path,
    final_path: &Path,
) -> io::Result<()> {
    wait_if_targeted(
        artifact,
        "after_sync_before_rename",
        temporary_path,
        final_path,
        WaitPolicy::default(),
    )
}

pub fn after_rename(artifact: &str, temporary_path: &Path, final_path: &Path) -> io::Result<()> {
    wait_if_targeted(
        artifact,
        "after_rename",
        temporary_path,
        final_path,
        WaitPolicy::default(),
    )
}

#[cfg(not(debug_assertions))]
fn wait_if_targeted(
    _artifact: &str,
    _phase: &str,
    _temporary_path: &Path,
    _final_path: &Path,
    _policy: WaitPolicy,
) -> io::Result<()> {
    Ok(())
}

#[cfg(debug_assertions)]
fn wait_if_targeted(
    artifact: &str,
    phase: &str,
    temporary_path: &Path,
    final_path: &Path,
    policy: WaitPolicy,
) -> io::Result<()> {
    let Some(dir) = configured_barrier_dir()? else {
        return Ok(());
    };
    let Ok(target) = std::env::var(BARRIER_TARGET_ENV) else {
        return Ok(());
    };
    if target != format!("{artifact}:{phase}") {
        return Ok(());
    }

    let operation_id = operation_id(artifact, phase, temporary_path);
    let ready_path = dir.join(format!("{operation_id}.ready.json"));
    let release_path = dir.join(format!("{operation_id}.release.json"));
    ensure_child_path(&dir, &ready_path)?;
    ensure_child_path(&dir, &release_path)?;
    publish_ready_record(
        &dir,
        &operation_id,
        artifact,
        phase,
        temporary_path,
        final_path,
        &ready_path,
    )?;
    wait_for_release_record(&release_path, &operation_id, artifact, phase, policy)
}

#[cfg(debug_assertions)]
fn configured_barrier_dir() -> io::Result<Option<PathBuf>> {
    let Ok(raw) = std::env::var(BARRIER_DIR_ENV) else {
        return Ok(None);
    };
    let dir = PathBuf::from(raw);
    let canonical = dir.canonicalize()?;
    if !canonical.is_dir() {
        return Err(io::Error::other(format!(
            "publication barrier dir is not a directory: {}",
            canonical.display()
        )));
    }
    Ok(Some(canonical))
}

#[cfg(debug_assertions)]
fn publish_ready_record(
    dir: &Path,
    operation_id: &str,
    artifact: &str,
    phase: &str,
    temporary_path: &Path,
    final_path: &Path,
    ready_path: &Path,
) -> io::Result<()> {
    let tmp_path = dir.join(format!(".{operation_id}.ready.tmp"));
    ensure_child_path(dir, &tmp_path)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp_path)?;
    write!(
        file,
        "{{\n  \"schema_version\": {SCHEMA_VERSION},\n  \"operation_id\": \"{}\",\n  \"artifact\": \"{}\",\n  \"phase\": \"{}\",\n  \"temporary_path\": \"{}\",\n  \"final_path\": \"{}\"\n}}\n",
        json_escape(operation_id),
        json_escape(artifact),
        json_escape(phase),
        json_escape(&temporary_path.to_string_lossy()),
        json_escape(&final_path.to_string_lossy())
    )?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp_path, ready_path)
}

#[cfg(debug_assertions)]
fn wait_for_release_record(
    release_path: &Path,
    operation_id: &str,
    artifact: &str,
    phase: &str,
    policy: WaitPolicy,
) -> io::Result<()> {
    let deadline = Instant::now() + policy.timeout;
    loop {
        match read_release_record(release_path) {
            Ok(Some(record)) => {
                validate_release_record(&record, operation_id, artifact, phase)?;
                return Ok(());
            }
            Ok(None) => {}
            Err(err) => return Err(err),
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "timed out waiting for publication barrier release {} for {artifact}:{phase}",
                    release_path.display()
                ),
            ));
        }
        thread::sleep(policy.poll_interval);
    }
}

#[cfg(debug_assertions)]
fn read_release_record(path: &Path) -> io::Result<Option<ReleaseRecord>> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(io::Error::other(format!(
                "publication barrier release path is a symlink: {}",
                path.display()
            )));
        }
        Ok(meta) if !meta.is_file() => {
            return Err(io::Error::other(format!(
                "publication barrier release path is not a file: {}",
                path.display()
            )));
        }
        Ok(_) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    }
    let text = fs::read_to_string(path)?;
    let schema_version = json_number_field(&text, "schema_version")?;
    let operation_id = json_string_field(&text, "operation_id")?;
    let artifact = json_string_field(&text, "artifact")?;
    let phase = json_string_field(&text, "phase")?;
    Ok(Some(ReleaseRecord {
        schema_version,
        operation_id,
        artifact,
        phase,
    }))
}

#[cfg(debug_assertions)]
fn validate_release_record(
    record: &ReleaseRecord,
    operation_id: &str,
    artifact: &str,
    phase: &str,
) -> io::Result<()> {
    if record.schema_version != SCHEMA_VERSION
        || record.operation_id != operation_id
        || record.artifact != artifact
        || record.phase != phase
    {
        return Err(io::Error::other(format!(
            "malformed publication barrier release record for {artifact}:{phase}"
        )));
    }
    Ok(())
}

#[cfg(debug_assertions)]
#[derive(Debug)]
struct ReleaseRecord {
    schema_version: u32,
    operation_id: String,
    artifact: String,
    phase: String,
}

#[cfg(debug_assertions)]
fn ensure_child_path(dir: &Path, path: &Path) -> io::Result<()> {
    if path.parent() != Some(dir) || path.file_name().is_none() {
        return Err(io::Error::other(format!(
            "publication barrier path escaped barrier dir: {}",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(debug_assertions)]
fn operation_id(artifact: &str, phase: &str, temporary_path: &Path) -> String {
    let basename = temporary_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("tmp");
    format!(
        "{}-{}-{}-{}-{}",
        sanitize_component(artifact),
        sanitize_component(phase),
        process::id(),
        unique_nanos(),
        sanitize_component(basename)
    )
}

#[cfg(any(debug_assertions, test))]
fn unique_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

#[cfg(debug_assertions)]
fn sanitize_component(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "x".to_string()
    } else {
        sanitized
    }
}

#[cfg(debug_assertions)]
fn json_escape(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out
}

#[cfg(debug_assertions)]
fn json_number_field(text: &str, field: &str) -> io::Result<u32> {
    let marker = format!("\"{field}\"");
    let start = text
        .find(&marker)
        .ok_or_else(|| io::Error::other(format!("missing JSON field {field}")))?;
    let after_marker = &text[start + marker.len()..];
    let colon = after_marker
        .find(':')
        .ok_or_else(|| io::Error::other(format!("missing JSON colon for {field}")))?;
    let digits = after_marker[colon + 1..]
        .trim_start()
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        return Err(io::Error::other(format!(
            "invalid JSON number field {field}"
        )));
    }
    digits
        .parse::<u32>()
        .map_err(|err| io::Error::other(format!("invalid JSON number field {field}: {err}")))
}

#[cfg(debug_assertions)]
fn json_string_field(text: &str, field: &str) -> io::Result<String> {
    let marker = format!("\"{field}\"");
    let start = text
        .find(&marker)
        .ok_or_else(|| io::Error::other(format!("missing JSON field {field}")))?;
    let after_marker = &text[start + marker.len()..];
    let colon = after_marker
        .find(':')
        .ok_or_else(|| io::Error::other(format!("missing JSON colon for {field}")))?;
    let value = after_marker[colon + 1..].trim_start();
    let mut chars = value.chars();
    if chars.next() != Some('"') {
        return Err(io::Error::other(format!(
            "invalid JSON string field {field}"
        )));
    }
    let mut out = String::new();
    let mut escaped = false;
    for ch in chars {
        if escaped {
            out.push(decode_json_escape(ch));
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Ok(out);
        } else {
            out.push(ch);
        }
    }
    Err(io::Error::other(format!(
        "unterminated JSON string field {field}"
    )))
}

#[cfg(debug_assertions)]
fn decode_json_escape(ch: char) -> char {
    match ch {
        '"' => '"',
        '\\' => '\\',
        '/' => '/',
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        other => other,
    }
}

#[cfg(test)]
mod tests;
