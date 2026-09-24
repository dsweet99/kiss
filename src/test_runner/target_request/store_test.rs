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
