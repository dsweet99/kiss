use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

fn cache_dir_under(home: &std::path::Path) -> std::path::PathBuf {
    home.join(".cache").join("kiss")
}

fn list_full_check_cache_files(home: &std::path::Path) -> Vec<std::path::PathBuf> {
    let dir = cache_dir_under(home);
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<_> = rd
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|s| {
                    s.starts_with("check_full_")
                        && std::path::Path::new(s)
                            .extension()
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("bin"))
                })
        })
        .collect();
    out.sort();
    out
}

fn chmod(path: &std::path::Path, mode: u32) {
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(mode);
    fs::set_permissions(path, perms).unwrap();
}

#[test]
fn check_all_cache_hit_replays_without_reading_sources() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    let src = repo.path().join("simple.py");
    fs::write(&src, "def foo():\n    return 1\n").unwrap();

    // Cold run populates cache.
    let out1 = kiss_binary()
        .arg("--defaults")
        .arg("check")
        .arg("--lang")
        .arg("python")
        .arg("--all")
        .arg(repo.path())
        .env("HOME", home.path())
        .output()
        .unwrap();
    let stdout1 = String::from_utf8_lossy(&out1.stdout).to_string();
    assert!(
        stdout1.contains("Analyzed:"),
        "expected summary line. stdout:\n{stdout1}"
    );
    let cache_files = list_full_check_cache_files(home.path());
    assert!(
        !cache_files.is_empty(),
        "expected full-check cache file under HOME. stdout:\n{stdout1}"
    );

    // Make sources unreadable. If we hit cache, we should still be able to replay.
    chmod(&src, 0o000);

    let out2 = kiss_binary()
        .arg("--defaults")
        .arg("check")
        .arg("--lang")
        .arg("python")
        .arg("--all")
        .arg(repo.path())
        .env("HOME", home.path())
        .output()
        .unwrap();
    let stdout2 = String::from_utf8_lossy(&out2.stdout).to_string();
    assert_eq!(
        out2.status.code(),
        out1.status.code(),
        "exit status should match on cache hit.\n--stderr1--\n{}\n--stderr2--\n{}",
        String::from_utf8_lossy(&out1.stderr),
        String::from_utf8_lossy(&out2.stderr)
    );
    assert_eq!(
        stdout2, stdout1,
        "cache-hit output should match exactly.\n--stdout1--\n{stdout1}\n--stdout2--\n{stdout2}"
    );
}

#[test]
fn check_all_cache_invalidates_on_mtime_or_size_change() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    let src = repo.path().join("simple.py");
    fs::write(&src, "def foo():\n    return 1\n").unwrap();

    let out1 = kiss_binary()
        .arg("--defaults")
        .arg("check")
        .arg("--lang")
        .arg("python")
        .arg("--all")
        .arg(repo.path())
        .env("HOME", home.path())
        .output()
        .unwrap();
    let stdout1 = String::from_utf8_lossy(&out1.stdout).to_string();
    assert!(!list_full_check_cache_files(home.path()).is_empty());

    // Change the file (updates mtime/size), then make it unreadable.
    chmod(&src, 0o200); // write-only
    fs::write(&src, "def foo():\n    return 2\n").unwrap();
    chmod(&src, 0o000); // unreadable, so a cache miss will drop parsing and change output

    let out2 = kiss_binary()
        .arg("--defaults")
        .arg("check")
        .arg("--lang")
        .arg("python")
        .arg("--all")
        .arg(repo.path())
        .env("HOME", home.path())
        .output()
        .unwrap();

    // We don't require a failure (the analyzer may skip unreadable files), but we do require
    // that it did NOT incorrectly replay the stale cached output.
    let stdout2 = String::from_utf8_lossy(&out2.stdout).to_string();
    assert_ne!(
        stdout2, stdout1,
        "after source change, cached output must not be replayed.\n--stdout1--\n{stdout1}\n--stdout2--\n{stdout2}"
    );
}

