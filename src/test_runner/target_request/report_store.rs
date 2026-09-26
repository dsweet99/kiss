use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use super::digest::digest_bytes;
use super::report::TargetReport;
use super::types::TargetRequest;

const SCHEMA: &str = "target-report-store-v3";
const ENTRY_LIMIT: usize = 64;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredReport {
    seq: u64,
    key: String,
    report: TargetReport,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct StoreMeta {
    next_seq: u64,
    keys: Vec<String>,
}

pub(crate) fn publish_if_rows_hold(
    repo_root: &Path,
    request: &TargetRequest,
    report: &TargetReport,
) -> Result<(), String> {
    let recaptured = super::recapture::recapture_report(repo_root, request, report)?;
    publish_report(repo_root, request, &recaptured)
}

pub(crate) fn publish_report(
    repo_root: &Path,
    request: &TargetRequest,
    report: &TargetReport,
) -> Result<(), String> {
    let key = report_key(request, report);
    let dir = store_dir(repo_root);
    fs::create_dir_all(&dir).map_err(|err| format!("target report store: {err}"))?;
    let (seq, observed) = {
        let _lock = lock_store(&dir)?;
        let observed = read_key_pointer(&dir, &key).map(|stored| stored.seq);
        let mut meta = read_meta(&dir);
        let seq = meta.next_seq;
        meta.next_seq += 1;
        write_meta(&dir, &meta)?;
        (seq, observed)
    };
    let entry = StoredReport {
        seq,
        key: key.clone(),
        report: report.clone(),
    };
    write_entry(&dir, &entry)?;
    let _lock = lock_store(&dir)?;
    abort_if_pointer_changed(&dir, &entry, observed)?;
    let mut meta = read_meta(&dir);
    meta.keys.retain(|item| item != &key);
    meta.keys.push(key);
    prune_unlocked(&dir, &mut meta);
    write_meta(&dir, &meta)?;
    write_key_pointer(&dir, &entry)?;
    write_pointer(&dir, &entry)?;
    Ok(())
}

pub(crate) fn load_current_report(repo_root: &Path) -> Option<TargetReport> {
    let dir = store_dir(repo_root);
    if !dir.is_dir() {
        return None;
    }
    // Serialize with publish_report so dual-path pointer reads cannot tear mid-migrate.
    let _lock = lock_store(&dir).ok()?;
    read_pointer(&dir).map(|stored| stored.report)
}

pub(crate) fn load_report_for_identity(
    repo_root: &Path,
    request: &TargetRequest,
    digest: &str,
    complete: bool,
    coverage_all: bool,
    extras: crate::test_runner::language_keyed::LanguageKeyed<&[String]>,
) -> Option<TargetReport> {
    let dir = store_dir(repo_root);
    if !dir.is_dir() {
        return None;
    }
    // Do not lock inside read_key_pointer — publish_report already holds the lock
    // when it calls that helper; lock only at this public load entry point.
    let _lock = lock_store(&dir).ok()?;
    let (runner, gate_policy) = super::report::evaluation_key_tokens(repo_root, coverage_all);
    let key = identity_key(request, digest, complete, &runner, &gate_policy, extras);
    read_key_pointer(&dir, &key).map(|stored| stored.report)
}

fn report_key(request: &TargetRequest, report: &TargetReport) -> String {
    identity_key(
        request,
        &report.stamp.digest,
        report.stamp.complete,
        &report.snapshot.runner,
        &report.snapshot.gate_policy,
        report.snapshot.extras.as_slices(),
    )
}

fn identity_key(
    request: &TargetRequest,
    digest: &str,
    complete: bool,
    runner: &str,
    gate_policy: &str,
    extras: crate::test_runner::language_keyed::LanguageKeyed<&[String]>,
) -> String {
    let mut payload = serde_json::json!({
        "schema": SCHEMA,
        "request": request,
        "digest": digest,
        "complete": complete,
        "runner": runner,
        "gate_policy": gate_policy,
    });
    if !extras.python.is_empty() || !extras.rust.is_empty() {
        payload["extras"] = serde_json::json!({
            "python": extras.python,
            "rust": extras.rust,
        });
    }
    digest_bytes(&serde_json::to_vec(&payload).expect("report key"))
}

fn store_dir(repo_root: &Path) -> PathBuf {
    repo_root
        .join("target")
        .join("kiss-plan")
        .join("target-reports")
}

fn lock_store(dir: &Path) -> Result<File, String> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("lock"))
        .map_err(|err| format!("target report lock: {err}"))?;
    file.lock_exclusive()
        .map_err(|err| format!("target report lock: {err}"))?;
    Ok(file)
}

fn read_meta(dir: &Path) -> StoreMeta {
    fs::read(dir.join("meta.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn write_meta(dir: &Path, meta: &StoreMeta) -> Result<(), String> {
    atomic_json(dir, "meta.json", meta)
}

fn read_pointer(dir: &Path) -> Option<StoredReport> {
    let bytes = fs::read(dir.join("pointer.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_pointer(dir: &Path, entry: &StoredReport) -> Result<(), String> {
    atomic_json(dir, "pointer.json", entry)
}

fn key_pointer_name(key: &str) -> String {
    format!("pointers/{key}.json")
}

fn legacy_key_pointer_name(key: &str) -> String {
    format!("pointers/{}.json", &key[..16.min(key.len())])
}

fn read_key_pointer(dir: &Path, key: &str) -> Option<StoredReport> {
    // Prefer full-key pointer; fall back to legacy 16-hex name with equality so
    // upgraded idle/non-watch loads still reuse pre-fix publishes.
    read_matching_key_pointer(dir, key, &key_pointer_name(key)).or_else(|| {
        read_matching_key_pointer(dir, key, &legacy_key_pointer_name(key))
    })
}

fn read_matching_key_pointer(dir: &Path, key: &str, name: &str) -> Option<StoredReport> {
    let bytes = fs::read(dir.join(name)).ok()?;
    let stored: StoredReport = serde_json::from_slice(&bytes).ok()?;
    (stored.key == key).then_some(stored)
}

fn write_key_pointer(dir: &Path, entry: &StoredReport) -> Result<(), String> {
    atomic_json(dir, &key_pointer_name(&entry.key), entry)?;
    // Drop legacy truncated pointer only when it held this same identity.
    let legacy = legacy_key_pointer_name(&entry.key);
    if read_matching_key_pointer(dir, &entry.key, &legacy).is_some() {
        let _ = fs::remove_file(dir.join(legacy));
    }
    Ok(())
}

fn entry_name(seq: u64, key: &str) -> String {
    format!("entries/{seq:08}_{key}.json")
}

fn write_entry(dir: &Path, entry: &StoredReport) -> Result<(), String> {
    let name = entry_name(entry.seq, &entry.key);
    if let Some(parent) = dir.join(&name).parent() {
        fs::create_dir_all(parent).map_err(|err| format!("target report entry: {err}"))?;
    }
    atomic_json(dir, &name, entry)
}

/// After an unlocked `write_entry`, refuse to publish if another writer moved the
/// key pointer. Drop the orphan entry so prune (which only walks `meta.keys`)
/// cannot leave it forever.
fn abort_if_pointer_changed(
    dir: &Path,
    entry: &StoredReport,
    observed: Option<u64>,
) -> Result<(), String> {
    if read_key_pointer(dir, &entry.key)
        .as_ref()
        .map(|stored| stored.seq)
        != observed
    {
        remove_entry_files_for(dir, entry.seq, &entry.key);
        return Err("concurrent mutation".into());
    }
    Ok(())
}

/// Remove entry files for this seq+key, including legacy truncated filenames that
/// do not embed the full identity key.
fn remove_entry_files_for(dir: &Path, seq: u64, key: &str) {
    let _ = fs::remove_file(dir.join(entry_name(seq, key)));
    let Ok(entries) = fs::read_dir(dir.join("entries")) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(bytes) = fs::read(entry.path()) else {
            continue;
        };
        let Ok(stored) = serde_json::from_slice::<StoredReport>(&bytes) else {
            continue;
        };
        if stored.seq == seq && stored.key == key {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn prune_unlocked(dir: &Path, meta: &mut StoreMeta) {
    while meta.keys.len() > ENTRY_LIMIT {
        let oldest = meta.keys.remove(0);
        if let Ok(entries) = fs::read_dir(dir.join("entries")) {
            for entry in entries.flatten() {
                // Delete by stored identity key so legacy `entries/{seq}_{prefix}.json`
                // files are removed too (filename no longer embeds the full key).
                let Ok(bytes) = fs::read(entry.path()) else {
                    continue;
                };
                let Ok(stored) = serde_json::from_slice::<StoredReport>(&bytes) else {
                    continue;
                };
                if stored.key == oldest {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        let _ = fs::remove_file(dir.join(key_pointer_name(&oldest)));
        // Only drop the shared 16-hex legacy pointer when it still holds this identity;
        // a colliding-prefix peer may own that path after an upgrade mix.
        let legacy = legacy_key_pointer_name(&oldest);
        if read_matching_key_pointer(dir, &oldest, &legacy).is_some() {
            let _ = fs::remove_file(dir.join(legacy));
        }
    }
}

fn atomic_json<T: Serialize>(dir: &Path, name: &str, value: &T) -> Result<(), String> {
    let path = dir.join(name);
    let tmp = dir.join(format!(".{}.tmp", name.replace('/', "_")));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("target report write: {err}"))?;
    }
    let bytes = serde_json::to_vec(value).map_err(|err| format!("target report json: {err}"))?;
    {
        let mut file = File::create(&tmp).map_err(|err| format!("target report write: {err}"))?;
        file.write_all(&bytes)
            .map_err(|err| format!("target report write: {err}"))?;
    }
    fs::rename(tmp, path).map_err(|err| format!("target report publish: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::report::{ReportEvidenceStamp, TargetReport};
    use super::super::scope::ReportScope;
    use super::super::slice::TargetSliceStamp;

    fn stub_report(exit_code: i32) -> TargetReport {
        TargetReport {
            scope: ReportScope {
                regions: Vec::new(),
                selectors: Vec::new(),
                complete: true,
            },
            rows: Vec::new(),
            stamp: TargetSliceStamp::default(),
            exit_code,
            evidence: ReportEvidenceStamp::default(),
            coverage: Default::default(),
            gates: Vec::new(),
            coverage_all: false,
            graph_generation: None,
            snapshot: Default::default(),
        }
    }

    fn colliding_keys() -> (String, String) {
        let prefix = "0123456789abcdef";
        let key_a = format!("{prefix}{}", "a".repeat(48));
        let key_b = format!("{prefix}{}", "b".repeat(48));
        assert_eq!(&key_a[..16], &key_b[..16]);
        assert_ne!(key_a, key_b);
        (key_a, key_b)
    }

    #[test]
    fn key_pointer_does_not_cross_load_on_shared_16_hex_prefix() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("target-reports");
        fs::create_dir_all(dir.join("pointers")).unwrap();
        let (key_a, key_b) = colliding_keys();
        let entry_a = StoredReport {
            seq: 1,
            key: key_a.clone(),
            report: stub_report(0),
        };
        let entry_b = StoredReport {
            seq: 2,
            key: key_b.clone(),
            report: stub_report(1),
        };
        write_key_pointer(&dir, &entry_a).unwrap();
        write_key_pointer(&dir, &entry_b).unwrap();

        // Require coexistence: equality-check alone with a truncated shared
        // pointer path would yield None for A after B overwrites, and a weak
        // "None or matching key" assert would still pass.
        let loaded_a = read_key_pointer(&dir, &key_a).expect("key A pointer must remain");
        assert_eq!(loaded_a.key, key_a);
        assert_eq!(loaded_a.report.exit_code, 0);
        let loaded_b = read_key_pointer(&dir, &key_b).expect("key B pointer");
        assert_eq!(loaded_b.key, key_b);
        assert_eq!(loaded_b.report.exit_code, 1);
    }

    #[test]
    fn prune_does_not_delete_peer_entry_sharing_16_hex_prefix() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("target-reports");
        fs::create_dir_all(&dir).unwrap();
        let (key_a, key_b) = colliding_keys();
        let entry_a = StoredReport {
            seq: 1,
            key: key_a.clone(),
            report: stub_report(0),
        };
        let entry_b = StoredReport {
            seq: 2,
            key: key_b.clone(),
            report: stub_report(1),
        };
        write_entry(&dir, &entry_a).unwrap();
        write_entry(&dir, &entry_b).unwrap();
        write_key_pointer(&dir, &entry_a).unwrap();
        write_key_pointer(&dir, &entry_b).unwrap();

        let mut keys = vec![key_a.clone()];
        keys.extend((0..ENTRY_LIMIT - 1).map(|i| format!("{i:064x}")));
        keys.push(key_b.clone());
        assert_eq!(keys.len(), ENTRY_LIMIT + 1);
        let mut meta = StoreMeta { next_seq: 3, keys };
        prune_unlocked(&dir, &mut meta);

        // Fixed item 3: prune must actually remove the full-key pointer/entry for the
        // oldest identity — peer-survival alone would still pass if pointer removal
        // were dropped.
        assert!(
            read_key_pointer(&dir, &key_a).is_none(),
            "pruning key A must remove A's full-key pointer"
        );
        let key_a_entries: Vec<_> = fs::read_dir(dir.join("entries"))
            .unwrap()
            .flatten()
            .filter_map(|e| {
                let bytes = fs::read(e.path()).ok()?;
                let stored: StoredReport = serde_json::from_slice(&bytes).ok()?;
                (stored.key == key_a).then_some(e.path())
            })
            .collect();
        assert!(
            key_a_entries.is_empty(),
            "pruning key A must remove A's entry file(s); leftover: {key_a_entries:?}"
        );
        assert!(
            read_key_pointer(&dir, &key_b).is_some_and(|s| s.key == key_b),
            "pruning a colliding-prefix peer must not remove key B's pointer"
        );
        let key_b_entries: Vec<_> = fs::read_dir(dir.join("entries"))
            .unwrap()
            .flatten()
            .filter_map(|e| {
                let bytes = fs::read(e.path()).ok()?;
                let stored: StoredReport = serde_json::from_slice(&bytes).ok()?;
                (stored.key == key_b).then_some(stored)
            })
            .collect();
        assert!(
            !key_b_entries.is_empty(),
            "pruning a colliding-prefix peer must not remove key B's entry file"
        );
    }

    #[test]
    fn prune_removes_legacy_truncated_entry_by_stored_key() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("target-reports");
        fs::create_dir_all(dir.join("entries")).unwrap();
        fs::create_dir_all(dir.join("pointers")).unwrap();
        let (key_a, key_b) = colliding_keys();
        let entry_a = StoredReport {
            seq: 1,
            key: key_a.clone(),
            report: stub_report(0),
        };
        let entry_b = StoredReport {
            seq: 2,
            key: key_b.clone(),
            report: stub_report(1),
        };
        // Both identities use pre-fix truncated entry filenames sharing the
        // 16-hex prefix. Filename-prefix deletion would wipe B when pruning A;
        // stored.key equality must keep B.
        let legacy_a_name = format!("entries/{:08}_{}.json", entry_a.seq, &key_a[..16]);
        let legacy_b_name = format!("entries/{:08}_{}.json", entry_b.seq, &key_b[..16]);
        assert_eq!(&key_a[..16], &key_b[..16]);
        atomic_json(&dir, &legacy_a_name, &entry_a).unwrap();
        atomic_json(&dir, &legacy_b_name, &entry_b).unwrap();
        write_key_pointer(&dir, &entry_a).unwrap();
        write_key_pointer(&dir, &entry_b).unwrap();

        let mut keys = vec![key_a.clone()];
        keys.extend((0..ENTRY_LIMIT - 1).map(|i| format!("{i:064x}")));
        keys.push(key_b.clone());
        let mut meta = StoreMeta { next_seq: 3, keys };
        prune_unlocked(&dir, &mut meta);

        assert!(
            !dir.join(&legacy_a_name).exists(),
            "prune must remove legacy truncated entry for A by stored.key"
        );
        assert!(
            dir.join(&legacy_b_name).is_file(),
            "prune(A) must not remove peer B's legacy truncated entry \
             (a name.contains(prefix) mutant would delete it)"
        );
        let key_b_entries: Vec<_> = fs::read_dir(dir.join("entries"))
            .unwrap()
            .flatten()
            .filter_map(|e| {
                let bytes = fs::read(e.path()).ok()?;
                let stored: StoredReport = serde_json::from_slice(&bytes).ok()?;
                (stored.key == key_b).then_some(e.path())
            })
            .collect();
        assert!(
            !key_b_entries.is_empty(),
            "peer B entry must survive prune(A)"
        );
    }

    #[test]
    fn concurrent_mutation_abort_removes_written_entry() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("target-reports");
        fs::create_dir_all(dir.join("entries")).unwrap();
        fs::create_dir_all(dir.join("pointers")).unwrap();
        let key = "c".repeat(64);
        let observed_winner = StoredReport {
            seq: 1,
            key: key.clone(),
            report: stub_report(0),
        };
        write_key_pointer(&dir, &observed_winner).unwrap();
        let observed = read_key_pointer(&dir, &key).map(|s| s.seq);
        assert_eq!(observed, Some(1));

        let lost = StoredReport {
            seq: 2,
            key: key.clone(),
            report: stub_report(1),
        };
        // Lost writer used a pre-fix truncated entry filename — abort must still
        // find and remove it (full-key entry_name alone would miss).
        let legacy_lost = format!("entries/{:08}_{}.json", lost.seq, &key[..16]);
        atomic_json(&dir, &legacy_lost, &lost).unwrap();
        // Peer wins the pointer while we hold no lock (same window as publish_report).
        let peer = StoredReport {
            seq: 3,
            key: key.clone(),
            report: stub_report(2),
        };
        write_entry(&dir, &peer).unwrap();
        write_key_pointer(&dir, &peer).unwrap();

        let err = abort_if_pointer_changed(&dir, &lost, observed).unwrap_err();
        assert_eq!(err, "concurrent mutation");
        // Scan entries/ rather than trusting entry_name alone: a truncated
        // write_entry path plus full-key remove would leave an orphan while
        // !entry_name(...).exists() still passed.
        assert!(
            !dir.join(&legacy_lost).exists(),
            "lost-race legacy truncated entry must be removed"
        );
        let lost_leftovers: Vec<_> = fs::read_dir(dir.join("entries"))
            .unwrap()
            .flatten()
            .filter_map(|e| {
                let bytes = fs::read(e.path()).ok()?;
                let stored: StoredReport = serde_json::from_slice(&bytes).ok()?;
                (stored.seq == lost.seq && stored.key == lost.key).then_some(e.path())
            })
            .collect();
        assert!(
            lost_leftovers.is_empty(),
            "lost-race entry must be removed from entries/; leftover paths: {lost_leftovers:?}"
        );
        assert!(
            dir.join(entry_name(peer.seq, &peer.key)).is_file(),
            "abort must not delete peer entry with same key but different seq \
             (a key-only delete mutant would remove it)"
        );
        assert!(
            read_key_pointer(&dir, &key).is_some_and(|s| s.seq == 3),
            "winning peer pointer must remain"
        );
    }

    #[test]
    fn read_key_pointer_reuses_legacy_16_hex_with_equality() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("target-reports");
        fs::create_dir_all(dir.join("pointers")).unwrap();
        let (key_a, key_b) = colliding_keys();
        let entry_a = StoredReport {
            seq: 1,
            key: key_a.clone(),
            report: stub_report(0),
        };
        // Pre-fix truncated pointer path only.
        atomic_json(&dir, &legacy_key_pointer_name(&key_a), &entry_a).unwrap();
        assert!(
            !dir.join(key_pointer_name(&key_a)).exists(),
            "fixture must be legacy-only"
        );

        let loaded = read_key_pointer(&dir, &key_a).expect("legacy A must load");
        assert_eq!(loaded.report.exit_code, 0);
        assert!(
            read_key_pointer(&dir, &key_b).is_none(),
            "colliding-prefix peer B must not receive A's legacy pointer"
        );

        // Full-key wins when both exist.
        let full_a = StoredReport {
            seq: 2,
            key: key_a.clone(),
            report: stub_report(7),
        };
        atomic_json(&dir, &key_pointer_name(&key_a), &full_a).unwrap();
        assert_eq!(
            read_key_pointer(&dir, &key_a).unwrap().report.exit_code,
            7,
            "full-key pointer must be preferred over legacy"
        );

        // Publishing B must not delete peer A's legacy file.
        fs::remove_file(dir.join(key_pointer_name(&key_a))).unwrap();
        atomic_json(&dir, &legacy_key_pointer_name(&key_a), &entry_a).unwrap();
        let entry_b = StoredReport {
            seq: 3,
            key: key_b.clone(),
            report: stub_report(1),
        };
        write_key_pointer(&dir, &entry_b).unwrap();
        assert!(
            dir.join(legacy_key_pointer_name(&key_a)).is_file(),
            "publishing B must not remove peer A's legacy pointer"
        );
        assert_eq!(read_key_pointer(&dir, &key_a).unwrap().report.exit_code, 0);

        // Migrating A removes matching legacy.
        write_key_pointer(&dir, &full_a).unwrap();
        assert!(dir.join(key_pointer_name(&key_a)).is_file());
        assert!(
            !dir.join(legacy_key_pointer_name(&key_a)).exists(),
            "matching legacy pointer must be removed on full-key publish"
        );
        assert_eq!(read_key_pointer(&dir, &key_a).unwrap().report.exit_code, 7);
    }

    #[test]
    fn prune_does_not_delete_peer_legacy_pointer_sharing_16_hex_prefix() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("target-reports");
        fs::create_dir_all(dir.join("pointers")).unwrap();
        fs::create_dir_all(dir.join("entries")).unwrap();
        let (key_a, key_b) = colliding_keys();
        // A has a full-key pointer (what prune removes by name). B only has the
        // shared legacy 16-hex pointer — unconditional legacy unlink on prune(A)
        // would destroy B's idle-cache upgrade path.
        let entry_a = StoredReport {
            seq: 1,
            key: key_a.clone(),
            report: stub_report(0),
        };
        let entry_b = StoredReport {
            seq: 2,
            key: key_b.clone(),
            report: stub_report(1),
        };
        write_entry(&dir, &entry_a).unwrap();
        write_key_pointer(&dir, &entry_a).unwrap();
        atomic_json(&dir, &legacy_key_pointer_name(&key_b), &entry_b).unwrap();

        let mut keys = vec![key_a.clone()];
        keys.extend((0..ENTRY_LIMIT - 1).map(|i| format!("{i:064x}")));
        keys.push(key_b.clone());
        assert_eq!(keys.len(), ENTRY_LIMIT + 1);
        let mut meta = StoreMeta { next_seq: 3, keys };
        prune_unlocked(&dir, &mut meta);

        assert!(
            read_key_pointer(&dir, &key_a).is_none(),
            "pruned A full-key pointer must be gone"
        );
        assert!(
            dir.join(legacy_key_pointer_name(&key_b)).is_file(),
            "pruning A must not remove peer B's legacy pointer file"
        );
        assert_eq!(
            read_key_pointer(&dir, &key_b).unwrap().report.exit_code,
            1,
            "peer B must still load via legacy pointer after prune(A)"
        );
    }

    #[test]
    fn load_current_report_waits_on_held_exclusive_lock() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = store_dir(tmp.path());
        fs::create_dir_all(&dir).unwrap();
        let entry = StoredReport {
            seq: 1,
            key: "c".repeat(64),
            report: stub_report(7),
        };
        write_pointer(&dir, &entry).unwrap();
        let held = lock_store(&dir).expect("hold exclusive lock");
        let repo = tmp.path().to_path_buf();
        let handle = std::thread::spawn(move || load_current_report(&repo));
        let started = std::time::Instant::now();
        while started.elapsed() < std::time::Duration::from_millis(200) {
            if handle.is_finished() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            !handle.is_finished(),
            "load_current_report must block on an already-held exclusive store lock"
        );
        drop(held);
        let loaded = handle
            .join()
            .expect("load_current_report thread")
            .expect("pointer");
        assert_eq!(loaded.exit_code, 7);
    }

    #[test]
    fn load_report_for_identity_waits_on_held_exclusive_lock() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = store_dir(tmp.path());
        fs::create_dir_all(&dir).unwrap();
        // Empty store is enough: the idle identity loader must flock before miss.
        let held = lock_store(&dir).expect("hold exclusive lock");
        let repo = tmp.path().to_path_buf();
        let request = TargetRequest::default();
        let handle = std::thread::spawn(move || {
            load_report_for_identity(
                &repo,
                &request,
                "digest",
                true,
                false,
                crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
            )
        });
        let started = std::time::Instant::now();
        while started.elapsed() < std::time::Duration::from_millis(200) {
            if handle.is_finished() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            !handle.is_finished(),
            "load_report_for_identity (idle identity path) must block on an \
             already-held exclusive store lock"
        );
        drop(held);
        assert!(
            handle
                .join()
                .expect("load_report_for_identity thread")
                .is_none(),
            "empty store must miss after unlock"
        );
    }
}
