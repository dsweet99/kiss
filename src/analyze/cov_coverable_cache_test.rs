use super::{CovCoverableKey, store_coverable_denoms, try_load_coverable_denoms};
use crate::analyze::line_coverage::CoverableDenom;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

fn touch_source(path: &std::path::Path, body: &str) -> PathBuf {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
    path.to_path_buf()
}

#[test]
fn coverable_cache_hits_then_misses_on_source_change() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    let py = touch_source(&repo.join("pkg/a.py"), "x = 1\n");
    let rs = touch_source(&repo.join("src/lib.rs"), "fn f() {}\n");
    let denoms = vec![CoverableDenom {
        file: py.clone(),
        lines: vec![1],
        mixed: false,
    }];
    let key = CovCoverableKey {
        repo_root: repo,
        py_files: std::slice::from_ref(&py),
        rs_files: std::slice::from_ref(&rs),
        ignore: &[],
        lang_filter: None,
    };
    assert!(try_load_coverable_denoms(&key).is_none());
    store_coverable_denoms(&key, &denoms);
    let loaded = try_load_coverable_denoms(&key).expect("facts hit");
    assert_eq!(loaded, denoms);

    std::thread::sleep(Duration::from_millis(5));
    fs::write(&py, "x = 2\n").unwrap();
    let _ = fs::File::options()
        .write(true)
        .open(&py)
        .unwrap()
        .set_modified(SystemTime::now())
        .ok();
    assert!(try_load_coverable_denoms(&key).is_none());
}
