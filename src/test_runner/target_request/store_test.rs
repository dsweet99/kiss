use super::plan_store::{
    TARGET_PLAN_ENTRY_LIMIT, load_current, load_entry, load_plan_for_identity, plan_key_for_test,
    publish,
};
use super::projection::SliceProjection;
use super::slice::{TargetSliceStamp, stamp_from_projection};
use super::types::{TargetFocus, TargetRequest};

fn request(tag: &str) -> TargetRequest {
    TargetRequest {
        focus: TargetFocus::Workspace,
        lang: None,
        ignore: vec![tag.to_string()],
    }
}

fn stamp(tag: &str) -> TargetSliceStamp {
    stamp_from_projection(
        &SliceProjection::Workspace {
            selectors: vec![tag.to_string()],
            sources: vec!["a.py".into()],
        },
        true,
    )
}

#[test]
fn publish_is_stable_for_the_same_key() {
    let tmp = tempfile::TempDir::new().unwrap();
    let first = publish(tmp.path(), &request("a"), &stamp("a")).unwrap();
    let second = publish(tmp.path(), &request("a"), &stamp("a")).unwrap();
    assert_eq!(first, second);
    assert_eq!(load_current(tmp.path()).as_ref(), Some(&first));
    assert_eq!(load_entry(tmp.path(), &first.key).as_ref(), Some(&first));
}

#[test]
fn store_prunes_above_entry_limit() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut last = None;
    for i in 0..(TARGET_PLAN_ENTRY_LIMIT + 3) {
        last = Some(
            publish(
                tmp.path(),
                &request(&format!("k{i}")),
                &stamp(&format!("s{i}")),
            )
            .unwrap(),
        );
    }
    let current = load_current(tmp.path()).unwrap();
    assert_eq!(current, last.unwrap());
    let dir = tmp.path().join("target/kiss-plan/target-plans/entries");
    let count = std::fs::read_dir(dir).unwrap().count();
    assert!(count <= TARGET_PLAN_ENTRY_LIMIT, "{count}");
}

#[test]
fn incomplete_and_complete_stamps_are_different_keys() {
    let tmp = tempfile::TempDir::new().unwrap();
    let proj = SliceProjection::Workspace {
        selectors: vec!["t".into()],
        sources: vec!["a.py".into()],
    };
    let complete = stamp_from_projection(&proj, true);
    let incomplete = stamp_from_projection(&proj, false);
    let a = publish(tmp.path(), &request("same"), &complete).unwrap();
    let b = publish(tmp.path(), &request("same"), &incomplete).unwrap();
    assert_ne!(a.key, b.key);
    assert_eq!(
        load_plan_for_identity(tmp.path(), &request("same"), &complete).as_ref(),
        Some(&a)
    );
}

#[test]
fn runner_identity_changes_plan_key() {
    let req = request("same");
    let stamp = stamp("s");
    let a = plan_key_for_test(&req, &stamp, "runner-a");
    let b = plan_key_for_test(&req, &stamp, "runner-b");
    assert_ne!(a, b);
    assert_eq!(a, plan_key_for_test(&req, &stamp, "runner-a"));
}

#[test]
fn plan_entry_files_use_full_key_not_16_hex_prefix() {
    let tmp = tempfile::TempDir::new().unwrap();
    let published = publish(tmp.path(), &request("full-key"), &stamp("full-key")).unwrap();
    let entries = tmp.path().join("target/kiss-plan/target-plans/entries");
    let names: Vec<_> = std::fs::read_dir(&entries)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let full_name = format!("{:08}_{}.json", published.seq, published.key);
    assert!(
        names.iter().any(|n| n == &full_name),
        "entry filename must be the full-key path; got {names:?}"
    );
    let prefix = &published.key[..16.min(published.key.len())];
    let truncated = format!("{:08}_{}.json", published.seq, prefix);
    assert!(
        !names.iter().any(|n| n == &truncated),
        "must not publish a truncated 16-hex-only entry name; got {names:?}"
    );
}

#[test]
fn plan_prune_removes_legacy_truncated_entry_path() {
    let tmp = tempfile::TempDir::new().unwrap();
    let first = publish(tmp.path(), &request("legacy-prune"), &stamp("legacy-prune")).unwrap();
    let entries = tmp.path().join("target/kiss-plan/target-plans/entries");
    let full_name = format!("{:08}_{}.json", first.seq, first.key);
    let legacy_name = format!(
        "{:08}_{}.json",
        first.seq,
        &first.key[..16.min(first.key.len())]
    );
    assert_ne!(full_name, legacy_name);
    // Pre-fix truncated sibling left beside the full-key publish.
    std::fs::copy(entries.join(&full_name), entries.join(&legacy_name)).unwrap();
    assert!(entries.join(&legacy_name).is_file());

    for i in 0..TARGET_PLAN_ENTRY_LIMIT {
        publish(
            tmp.path(),
            &request(&format!("k{i}")),
            &stamp(&format!("s{i}")),
        )
        .unwrap();
    }

    assert!(
        !entries.join(&full_name).exists(),
        "oldest full-key entry must be pruned"
    );
    assert!(
        !entries.join(&legacy_name).exists(),
        "prune must also remove the legacy truncated entry path \
         (full-key-only unlink would leave it)"
    );
}
