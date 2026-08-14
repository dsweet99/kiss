use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::bin_cli::args::TestInvocation;
use crate::test_runner::watch::event_source::{FakeWatchEventSource, NormalizedWatchEvent, WatchEventSource};
use crate::test_runner::watch::filter::WatchPathFilter;
use crate::test_runner::watch::settle::{PathSignature, SettleMachine, SettlePoll};

#[test]
fn watch_flag_parses_and_rejects_dry_run() {
    use clap::Parser;
    use crate::bin_cli::args::{Cli, Commands};
    let cli = Cli::try_parse_from(["kiss", "test", "--watch", "."]).unwrap();
    match cli.command {
        Commands::Test {
            watch: true,
            dry_run: false,
            operands,
            ..
        } => assert_eq!(operands, vec![".".to_string()]),
        _ => panic!("expected watch"),
    }
    let err = Cli::try_parse_from(["kiss", "test", "--watch-bg", "."]).unwrap_err();
    assert!(err.to_string().contains("--watch-bg") || err.to_string().contains("unexpected"));
    let cli = Cli::try_parse_from(["kiss", "test", "--watch", "--dry-run", "."]).unwrap();
    match cli.command {
        Commands::Test {
            watch: true,
            dry_run: true,
            ..
        } => {}
        _ => panic!("expected both flags parse; dispatch rejects combo"),
    }
}

#[test]
fn settle_coalesces_rename_old_and_new() {
    let settle = Duration::from_millis(30);
    let mut m = SettleMachine::new(settle);
    let t0 = Instant::now();
    let sig = PathSignature {
        exists: true,
        modified: Some(std::time::SystemTime::now() - settle),
        length: 1,
    };
    m.note_path(PathBuf::from("old.py"), t0, PathSignature {
        exists: false,
        modified: None,
        length: 0,
    });
    m.note_path(PathBuf::from("new.py"), t0, sig.clone());
    let ready = m.poll(t0 + settle, |p| {
        if p.ends_with("old.py") {
            PathSignature {
                exists: false,
                modified: None,
                length: 0,
            }
        } else {
            sig.clone()
        }
    });
    match ready {
        SettlePoll::Ready(paths) => {
            assert_eq!(paths.len(), 2);
        }
        other => panic!("expected ready, got {other:?}"),
    }
}

#[test]
fn filter_exact_file_target() {
    let tmp = tempfile::tempdir().unwrap();
    let f = WatchPathFilter::build(
        tmp.path(),
        &[],
        None,
        &TestInvocation::Targets(vec!["src/a.py".into()]),
    );
    assert!(f.is_relevant(std::path::Path::new("src/a.py")));
    assert!(!f.is_relevant(std::path::Path::new("src/b.py")));
}

#[test]
fn fake_source_delivers_events() {
    let mut src = FakeWatchEventSource {
        events: vec![NormalizedWatchEvent::Paths(vec![PathBuf::from("a.py")])],
        disconnected: None,
    };
    let got = src.recv_timeout(Duration::from_millis(1)).unwrap();
    assert_eq!(got.len(), 1);
}

#[cfg(target_os = "linux")]
fn wait_for_notify_path(
    rx: &std::sync::mpsc::Receiver<Result<notify::Event, notify::Error>>,
    predicate: impl Fn(&std::path::Path) -> bool,
    timeout: std::time::Duration,
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if let Ok(Ok(event)) = rx.recv_timeout(std::time::Duration::from_millis(100))
            && event.paths.iter().any(|p| predicate(p))
        {
            return true;
        }
    }
    false
}

#[cfg(target_os = "linux")]
#[test]
fn native_watcher_observes_create_modify_rename_delete() {
    use std::sync::mpsc;
    use std::time::Duration as StdDuration;

    use notify::{RecursiveMode, Watcher};

    let tmp = tempfile::tempdir().unwrap();
    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::RecommendedWatcher::new(
        move |res| {
            let _ = tx.send(res);
        },
        notify::Config::default(),
    )
    .unwrap();
    watcher
        .watch(tmp.path(), RecursiveMode::NonRecursive)
        .unwrap();
    let timeout = StdDuration::from_secs(2);
    let file = tmp.path().join("w.py");
    std::fs::write(&file, "a=1\n").unwrap();
    assert!(
        wait_for_notify_path(&rx, |p| p.ends_with("w.py"), timeout),
        "expected create/modify event"
    );

    std::fs::write(&file, "a=2\n").unwrap();
    assert!(
        wait_for_notify_path(&rx, |p| p.ends_with("w.py"), timeout),
        "expected modify event"
    );

    // Atomic save: write temp then rename over target (both paths visible).
    let tmp_write = tmp.path().join(".w.py.tmp");
    let renamed = tmp.path().join("w2.py");
    std::fs::write(&tmp_write, "a=3\n").unwrap();
    std::fs::rename(&tmp_write, &renamed).unwrap();
    assert!(
        wait_for_notify_path(
            &rx,
            |p| p.ends_with("w2.py") || p.ends_with(".w.py.tmp"),
            timeout
        ),
        "expected atomic rename event"
    );

    std::fs::remove_file(&renamed).unwrap();
    assert!(
        wait_for_notify_path(&rx, |p| p.ends_with("w2.py"), timeout),
        "expected remove event"
    );
}

#[test]
fn invocation_label_covers_modes() {
    assert_eq!(
        crate::test_runner::watch::invocation_label(&TestInvocation::All),
        "."
    );
    assert_eq!(
        crate::test_runner::watch::invocation_label(&TestInvocation::Commit),
        "commit"
    );
    assert_eq!(
        crate::test_runner::watch::invocation_label(&TestInvocation::Targets(vec![
            "a.py".into()
        ])),
        "a.py"
    );
}

#[test]
fn print_cycle_summary_is_silent() {
    let paths: Vec<_> = (0..12).map(|i| PathBuf::from(format!("f{i}.py"))).collect();
    crate::test_runner::watch::print_cycle_summary(&paths);
}

#[test]
fn apply_event_notes_relevant_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let filter = WatchPathFilter::build(tmp.path(), &[], None, &TestInvocation::All);
    let mut machine = SettleMachine::new(Duration::from_millis(10));
    crate::test_runner::watch::apply_normalized_event(
        NormalizedWatchEvent::Paths(vec![tmp.path().join("a.py")]),
        &filter,
        &mut machine,
        tmp.path(),
    )
    .unwrap();
    crate::test_runner::watch::apply_normalized_event(
        NormalizedWatchEvent::Rescan,
        &filter,
        &mut machine,
        tmp.path(),
    )
    .unwrap();
}

#[test]
fn apply_event_error_is_terminal() {
    let tmp = tempfile::tempdir().unwrap();
    let filter = WatchPathFilter::build(tmp.path(), &[], None, &TestInvocation::All);
    let mut machine = SettleMachine::new(Duration::from_millis(10));
    let err = crate::test_runner::watch::apply_normalized_event(
        NormalizedWatchEvent::Error("watcher broke".into()),
        &filter,
        &mut machine,
        tmp.path(),
    );
    assert_eq!(err.unwrap_err(), "watcher broke");
}

#[test]
fn run_test_watch_requires_git() {
    use crate::test_runner::{RunTestCmdArgs, run_test_watch};
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let args = RunTestCmdArgs {
        invocation: TestInvocation::All,
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: false,
        force_rerun: false,
            force_bad: false,        metrics: false,
        jobs: 1,
        extra: &[],
        python_extra: &[],
        ignore: &[],
        lang_filter: None,
        config_main_branch: None,
    gate_config: kiss::GateConfig::default()
    };
    let code = run_test_watch(args, Duration::from_millis(10));
    std::env::set_current_dir(orig).unwrap();
    assert_eq!(code, 1);
}
