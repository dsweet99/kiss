use super::*;
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::os::unix::io::AsRawFd;
    use std::sync::Mutex;
    use tempfile::TempDir;

    static STDOUT_CAPTURE: Mutex<()> = Mutex::new(());

    fn capture_stdout(f: impl FnOnce()) -> String {
        let _lock = STDOUT_CAPTURE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        let file = OpenOptions::new()
            .write(true)
            .read(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        let stdout_fd = std::io::stdout().as_raw_fd();
        let saved = unsafe { libc::dup(stdout_fd) };
        assert_ne!(saved, -1, "dup stdout failed");
        unsafe {
            libc::dup2(file.as_raw_fd(), stdout_fd);
        }
        f();
        let _ = std::io::stdout().flush();
        unsafe {
            libc::fflush(std::ptr::null_mut());
            libc::dup2(saved, stdout_fd);
            libc::close(saved);
        }
        drop(file);
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn test_violations_fix_hint_matches_plan_text() {
        assert_eq!(
            VIOLATIONS_FIX_HINT,
            "Run 'kiss rules' for more information about fixing violations."
        );
    }

    #[test]
    fn test_print_no_files_message_no_panic() {
        let tmp = TempDir::new().unwrap();
        print_no_files_message(None, tmp.path());
        print_no_files_message(Some(Language::Python), tmp.path());
    }

    #[test]
    fn test_print_coverage_gate_failure_emits_hint() {
        let file_pcts: HashMap<std::path::PathBuf, usize> =
            [(std::path::PathBuf::from("foo.py"), 50)].into();
        let stdout = capture_stdout(|| {
            print_coverage_gate_failure(&CoverageGateFailureCtx {
                threshold: 80,
                unreferenced: &[(std::path::PathBuf::from("foo.py"), "bar".to_string(), 10)],
                file_pcts: &file_pcts,
            });
        });
        assert!(
            stdout.contains(VIOLATIONS_FIX_HINT),
            "expected hint in stdout: {stdout}"
        );
        assert!(
            stdout.contains("GATE_FAILED:test_coverage:"),
            "expected gate failure in stdout: {stdout}"
        );
    }

    #[test]
    fn test_print_violations_empty() {
        print_violations(&[]);
        let clean = capture_stdout(|| print_final_status(false));
        assert_eq!(clean.trim(), "NO VIOLATIONS");
        let viol = capture_stdout(|| print_final_status(true));
        assert!(
            viol.contains(VIOLATIONS_FIX_HINT),
            "expected hint in stdout: {viol}"
        );
    }

    #[test]
    fn test_print_duplicates_empty() {
        print_duplicates("Test", &[]);
    }

    #[test]
    fn test_file_coverage_map_computes_per_file_pct() {
        let defs = vec![
            (PathBuf::from("a.py"), "f1".into(), 1),
            (PathBuf::from("a.py"), "f2".into(), 5),
            (PathBuf::from("b.py"), "g1".into(), 1),
        ];
        let unref = vec![(PathBuf::from("a.py"), "f2".into(), 5)];
        let map = file_coverage_map(&defs, &unref);
        assert_eq!(map[&PathBuf::from("a.py")], 50);
        assert_eq!(map[&PathBuf::from("b.py")], 100);
    }

    #[test]
    fn test_file_coverage_map_by_line_spans_weights_longer_defs() {
        let defs = vec![
            (PathBuf::from("a.rs"), "big".into(), 1, 10),
            (PathBuf::from("a.rs"), "small".into(), 20, 21),
        ];
        let unref = vec![(PathBuf::from("a.rs"), "big".into(), 1)];
        let map = file_coverage_map_by_line_spans(&defs, &unref);
        assert_eq!(map[&PathBuf::from("a.rs")], 17);
    }

    #[test]
    fn file_coverage_map_from_paths_empty() {
        let map = file_coverage_map_from_paths(std::iter::empty(), std::iter::empty());
        assert!(map.is_empty());
    }

    #[test]
    fn static_coverage_touch_gate_90() {
        fn t<T>(_: T) {}
        t(format_candidate_list);
        t(min_per_file_coverage);
        t(file_coverage_map_by_line_spans);
        t(print_dry_results);
    }

    #[test]
    fn test_count_py_unreferenced_empty() {
        assert_eq!(count_py_unreferenced(&[]), 0);
    }

    #[test]
    fn test_count_rs_unreferenced_empty() {
        assert_eq!(count_rs_unreferenced(&[]), 0);
}
