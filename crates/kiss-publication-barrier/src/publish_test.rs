use super::test_support::{EnvGuard, temp_dir};
use super::*;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
#[cfg(debug_assertions)]
use std::thread;
#[cfg(debug_assertions)]
use std::time::{Duration, Instant};

#[test]
fn publish_atomically_writes_final_and_removes_tmp() {
    let _env = EnvGuard::set(None, None);
    let dir = temp_dir();
    let final_path = dir.join("out.json");
    let tmp_path = dir.join("out.tmp");
    publish_atomically("artifact", &final_path, &tmp_path, |file| {
        file.write_all(b"payload\n")
    })
    .unwrap();
    assert_eq!(fs::read(&final_path).unwrap(), b"payload\n");
    assert!(!tmp_path.exists());
}

#[test]
fn publish_atomically_rejects_mismatched_parents() {
    let _env = EnvGuard::set(None, None);
    let dir = temp_dir();
    let other = dir.join("other");
    fs::create_dir(&other).unwrap();
    let err = publish_atomically(
        "artifact",
        &dir.join("out.json"),
        &other.join("out.tmp"),
        |_| Ok(()),
    )
    .expect_err("mismatched parents must fail");
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn publish_atomically_create_new_collision_leaves_final_untouched() {
    let _env = EnvGuard::set(None, None);
    let dir = temp_dir();
    let final_path = dir.join("out.json");
    let tmp_path = dir.join("out.tmp");
    fs::write(&final_path, b"prior\n").unwrap();
    fs::write(&tmp_path, b"stale\n").unwrap();
    let err = publish_atomically("artifact", &final_path, &tmp_path, |file| {
        file.write_all(b"new\n")
    })
    .expect_err("existing tmp must fail create_new");
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read(&final_path).unwrap(), b"prior\n");
    assert_eq!(fs::read(&tmp_path).unwrap(), b"stale\n");
}

#[test]
fn publish_atomically_writer_error_leaves_no_final() {
    let _env = EnvGuard::set(None, None);
    let dir = temp_dir();
    let final_path = dir.join("out.json");
    let tmp_path = dir.join("out.tmp");
    let err = publish_atomically("artifact", &final_path, &tmp_path, |_| {
        Err(io::Error::other("write failed"))
    })
    .expect_err("writer error must propagate");
    assert!(err.to_string().contains("write failed"));
    assert!(!final_path.exists());
}

#[test]
fn publish_atomically_writer_error_preserves_prior_final() {
    let _env = EnvGuard::set(None, None);
    let dir = temp_dir();
    let final_path = dir.join("out.json");
    let tmp_path = dir.join("out.tmp");
    fs::write(&final_path, b"prior\n").unwrap();
    let _ = publish_atomically("artifact", &final_path, &tmp_path, |_| {
        Err(io::Error::other("write failed"))
    });
    assert_eq!(fs::read(&final_path).unwrap(), b"prior\n");
}

#[test]
fn publish_atomically_rename_failure_removes_tmp() {
    let _env = EnvGuard::set(None, None);
    let dir = temp_dir();
    let final_path = dir.join("out.json");
    fs::create_dir(&final_path).unwrap();
    let tmp_path = dir.join("out.tmp");
    let err = publish_atomically("artifact", &final_path, &tmp_path, |file| {
        file.write_all(b"payload\n")
    })
    .expect_err("rename onto directory must fail");
    assert!(!tmp_path.exists(), "tmp should be best-effort removed: {err}");
}

#[cfg(debug_assertions)]
fn write_matching_release(dir: &Path, operation_id: &str, phase: &str) {
    let release = dir.join(format!("{operation_id}.release.json"));
    fs::write(
        release,
        format!(
            "{{\"schema_version\":1,\"operation_id\":\"{}\",\"artifact\":\"artifact\",\"phase\":\"{phase}\"}}\n",
            json_escape(operation_id)
        ),
    )
    .unwrap();
}

#[cfg(debug_assertions)]
fn release_ready_phases(barrier: &Path) -> (bool, bool) {
    let mut saw_after_sync = false;
    let mut saw_after_rename = false;
    for entry in fs::read_dir(barrier).unwrap().filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".ready.json") {
            continue;
        }
        let text = fs::read_to_string(entry.path()).unwrap();
        let operation_id = json_string_field(&text, "operation_id").unwrap();
        let phase = json_string_field(&text, "phase").unwrap();
        if phase == "after_sync_before_rename" {
            saw_after_sync = true;
            unsafe {
                std::env::set_var(BARRIER_TARGET_ENV, "artifact:after_rename");
            }
            write_matching_release(barrier, &operation_id, &phase);
        } else if phase == "after_rename" {
            saw_after_rename = true;
            write_matching_release(barrier, &operation_id, &phase);
        }
    }
    (saw_after_sync, saw_after_rename)
}

#[cfg(debug_assertions)]
#[test]
fn publish_atomically_fires_both_qa_barrier_phases() {
    let dir = temp_dir();
    let barrier = temp_dir();
    let final_path = dir.join("out.json");
    let tmp_path = dir.join("out.tmp");
    let _env = EnvGuard::set(Some(&barrier), Some("artifact:after_sync_before_rename"));
    let barrier_for_thread = barrier.clone();
    let handle = thread::spawn(move || {
        let mut saw_after_sync = false;
        let mut saw_after_rename = false;
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            let (sync, rename) = release_ready_phases(&barrier_for_thread);
            saw_after_sync |= sync;
            saw_after_rename |= rename;
            if saw_after_sync && saw_after_rename {
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        assert!(saw_after_sync, "expected after_sync_before_rename ready");
        assert!(saw_after_rename, "expected after_rename ready");
    });
    publish_atomically("artifact", &final_path, &tmp_path, |file| {
        file.write_all(b"payload\n")
    })
    .unwrap();
    handle.join().unwrap();
    assert_eq!(fs::read(&final_path).unwrap(), b"payload\n");
}

fn file_calls_after_sync_hook(text: &str) -> bool {
    text.contains("kiss_publication_barrier::after_sync_before_rename")
        || text.lines().any(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with("//")
                && (trimmed.contains("after_sync_before_rename(")
                    || trimmed.contains("after_sync_before_rename::"))
        })
}

fn collect_forbidden_publish_hook_callers(
    dir: &Path,
    barrier_src: &Path,
    offenders: &mut Vec<String>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if path.is_dir() {
            if matches!(
                name.as_str(),
                "target" | ".git" | "node_modules" | ".kiss" | "__pycache__"
            ) {
                continue;
            }
            collect_forbidden_publish_hook_callers(&path, barrier_src, offenders);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        if name.ends_with("_test.rs") {
            continue;
        }
        if let Ok(canon) = path.canonicalize()
            && canon.starts_with(barrier_src)
        {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap_or_default();
        if file_calls_after_sync_hook(&text) {
            offenders.push(path.display().to_string());
        }
    }
}

#[test]
fn open_publish_tmp_retries_when_parent_is_missing() {
    let dir = temp_dir();
    let nested = dir.join("nested");
    let tmp_path = nested.join("out.tmp");
    // Parent absent → first create_new yields NotFound; helper recreates and retries.
    let mut file = open_publish_tmp("artifact", &tmp_path, &nested).unwrap();
    file.write_all(b"payload\n").unwrap();
    drop(file);
    assert_eq!(fs::read(&tmp_path).unwrap(), b"payload\n");
}

#[test]
fn sync_publish_parent_ok_when_parent_missing() {
    let missing = temp_dir().join("gone");
    sync_publish_parent("artifact", &missing).unwrap();
}

#[test]
fn publish_atomically_ignores_stale_missing_barrier_dir() {
    let dir = temp_dir();
    let missing_barrier = dir.join("no-such-barrier");
    let _env = EnvGuard::set(Some(&missing_barrier), Some("artifact:after_rename"));
    let final_path = dir.join("out.json");
    let tmp_path = dir.join("out.tmp");
    publish_atomically("artifact", &final_path, &tmp_path, |file| {
        file.write_all(b"payload\n")
    })
    .unwrap();
    assert_eq!(fs::read(&final_path).unwrap(), b"payload\n");
}

#[test]
fn production_sources_do_not_call_after_sync_before_rename_directly() {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let barrier_src = workspace
        .join("crates/kiss-publication-barrier/src")
        .canonicalize()
        .unwrap();
    let mut offenders = Vec::new();
    collect_forbidden_publish_hook_callers(&workspace, &barrier_src, &mut offenders);
    assert!(
        offenders.is_empty(),
        "production publishers must use publish_atomically only; forbidden after_sync_before_rename in:\n{}",
        offenders.join("\n")
    );
}
