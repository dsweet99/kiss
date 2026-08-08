use super::*;
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::time::{Duration, SystemTime};
use tempfile::tempdir;

fn set_mtime_age(path: &Path, age: Duration) {
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .saturating_sub(age.as_secs()) as i64;
    let times = [
        libc::timespec {
            tv_sec: secs,
            tv_nsec: 0,
        },
        libc::timespec {
            tv_sec: secs,
            tv_nsec: 0,
        },
    ];
    let c_path = CString::new(path.as_os_str().as_bytes()).unwrap();
    let rc = unsafe { libc::utimensat(libc::AT_FDCWD, c_path.as_ptr(), times.as_ptr(), 0) };
    assert_eq!(rc, 0, "utimensat {}", path.display());
}

#[test]
fn sweep_removes_stale_entry_state_tmp_and_nested_tmps() {
    let tmp = tempdir().unwrap();
    let cache = tmp.path();
    let nested = cache.join("reverse_line_index").join("snapshots");
    fs::create_dir_all(&nested).unwrap();
    let entry_tmp = cache.join(".entry_state.1.2.tmp");
    let nested_tmp = nested.join(".meta.json.3.4.tmp");
    let keep = cache.join("entry_state.json");
    fs::write(&entry_tmp, b"{}\n").unwrap();
    fs::write(&nested_tmp, b"{}\n").unwrap();
    fs::write(&keep, b"{}\n").unwrap();
    set_mtime_age(&entry_tmp, Duration::from_secs(120));
    set_mtime_age(&nested_tmp, Duration::from_secs(120));

    sweep_orphaned_publication_tmps(cache).unwrap();

    assert!(!entry_tmp.exists());
    assert!(!nested_tmp.exists());
    assert!(keep.exists());
}

#[test]
fn sweep_keeps_fresh_publication_tmps() {
    let tmp = tempdir().unwrap();
    let cache = tmp.path();
    let fresh = cache.join(".population.1.2.tmp");
    fs::write(&fresh, b"{}\n").unwrap();

    sweep_orphaned_publication_tmps(cache).unwrap();

    assert!(fresh.exists(), "in-flight tmp must survive sweep");
}

#[test]
fn sweep_missing_cache_root_is_ok() {
    let tmp = tempdir().unwrap();
    sweep_orphaned_publication_tmps(&tmp.path().join("missing")).unwrap();
}
