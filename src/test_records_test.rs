use super::*;

fn sample(test_id: &str) -> TestRecord {
    TestRecord {
        schema: RECORD_SCHEMA.to_string(),
        language: "python".to_string(),
        test_id: test_id.to_string(),
        identity: "id-1".to_string(),
        deps: BTreeMap::from([("app.py".to_string(), "d1".to_string())]),
        status: TestStatus::Passed,
        exit_code: Some(0),
        duration: Duration::from_millis(3),
    }
}

fn now<'a>(deps: Option<&'a BTreeMap<String, String>>) -> Selection<'a> {
    Selection {
        identity: "id-1",
        current_deps: deps,
        retry_bad: false,
        needs_duration: false,
    }
}

#[test]
fn must_run_without_record() {
    assert_eq!(must_run(None, &now(None)), Some(RunReason::NoRecord));
}

#[test]
fn record_answers_when_identity_and_deps_match() {
    let record = sample("t.py::test_a");
    assert_eq!(
        must_run(Some(record.view()), &now(Some(&record.deps))),
        None
    );
}

#[test]
fn identity_change_forces_run() {
    let record = sample("t.py::test_a");
    let mut selection = now(Some(&record.deps));
    selection.identity = "id-2";
    assert_eq!(
        must_run(Some(record.view()), &selection),
        Some(RunReason::IdentityChanged)
    );
}

#[test]
fn changed_or_missing_deps_force_run() {
    let record = sample("t.py::test_a");
    let changed = BTreeMap::from([("app.py".to_string(), "d2".to_string())]);
    assert_eq!(
        must_run(Some(record.view()), &now(Some(&changed))),
        Some(RunReason::DepsChanged)
    );
    assert_eq!(
        must_run(Some(record.view()), &now(None)),
        Some(RunReason::DepsChanged)
    );
}

#[test]
fn retry_bad_reruns_only_non_passing_records() {
    let mut record = sample("t.py::test_a");
    let mut selection = now(Some(&record.deps));
    selection.retry_bad = true;
    assert_eq!(must_run(Some(record.view()), &selection), None);
    record.status = TestStatus::TimedOut;
    let deps = record.deps.clone();
    selection.current_deps = Some(&deps);
    assert_eq!(
        must_run(Some(record.view()), &selection),
        Some(RunReason::RetryBad)
    );
}

#[test]
fn time_gate_reruns_record_without_duration() {
    let mut record = sample("t.py::test_a");
    record.duration = Duration::ZERO;
    let mut selection = now(Some(&record.deps));
    selection.needs_duration = true;
    assert_eq!(
        must_run(Some(record.view()), &selection),
        Some(RunReason::NeedsDuration)
    );
    selection.needs_duration = false;
    assert_eq!(must_run(Some(record.view()), &selection), None);
}

#[test]
fn store_then_load_round_trips_and_overwrites() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = records_dir(tmp.path(), "python");
    assert!(dir.ends_with(".kiss/test/records/python"));
    let mut record = sample("t.py::test_a");
    store_record(&dir, &record).unwrap();
    assert_eq!(load_record(&dir, "t.py::test_a"), Some(record.clone()));
    record.status = TestStatus::Failed;
    store_record(&dir, &record).unwrap();
    store_record(&dir, &sample("t.py::test_b")).unwrap();
    let all = load_records(&dir);
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].status, TestStatus::Failed);
    assert_eq!(all[1].test_id, "t.py::test_b");
    assert_eq!(load_record(&dir, "t.py::test_missing"), None);
}

#[test]
fn other_schema_is_discarded() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("records");
    let mut record = sample("t.py::test_a");
    record.schema = "kiss-test-record-v0".to_string();
    store_record(&dir, &record).unwrap();
    assert_eq!(load_record(&dir, "t.py::test_a"), None);
    assert!(load_records(&dir).is_empty());
    fs::write(dir.join("junk.json"), b"{").unwrap();
    assert!(load_records(&dir).is_empty());
}
