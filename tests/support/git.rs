use std::path::{Path, PathBuf};
use std::process::Command;

pub fn git_command(repo: &Path) -> Command {
    kiss::scrubbed_git_command(repo)
}

pub fn init_git_repo(dir: &Path) {
    assert!(
        git_command(dir)
            .args(["init", "-b", "main"])
            .status()
            .unwrap()
            .success()
    );
    for kv in [("user.email", "t@t.t"), ("user.name", "t")] {
        git_command(dir)
            .args(["config", kv.0, kv.1])
            .status()
            .unwrap();
    }
}

pub fn commit_all(dir: &Path, message: &str) {
    assert!(
        git_command(dir)
            .args(["add", "-A"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_command(dir)
            .args(["commit", "-m", message])
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn bare_git_commands_stay_in_approved_helpers() {
    let roots = [Path::new("src"), Path::new("tests")];
    let mut offenders = Vec::new();
    for root in roots {
        collect_bare_git_commands(root, &mut offenders);
    }
    assert!(
        offenders.is_empty(),
        "use a scrubbed git helper instead of bare Command::new(\"git\"): {offenders:?}"
    );
}

fn collect_bare_git_commands(path: &Path, offenders: &mut Vec<PathBuf>) {
    if let Ok(meta) = path.metadata() {
        if meta.is_dir() {
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    collect_bare_git_commands(&entry.path(), offenders);
                }
            }
        } else if should_scan_rust_file(path)
            && let Ok(contents) = std::fs::read_to_string(path)
        {
            let needle = ["Command::new(", "\"git\"", ")"].concat();
            if contents.contains(&needle) {
                offenders.push(path.to_path_buf());
            }
        }
    }
}

fn should_scan_rust_file(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("rs") && !bare_git_command_allowed(path)
}

fn bare_git_command_allowed(path: &Path) -> bool {
    path == Path::new("src/shared_helpers.rs")
}
