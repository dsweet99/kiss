use crate::shared_helpers::host_cpu_count;

pub fn check_rust_shard_count(file_count: usize) -> usize {
    let cpus = host_cpu_count(4);
    cpus.min(file_count.max(1)).max(1)
}

pub const PYTHON_COLLECT_SHARD_PATH_THRESHOLD: usize = 64;

pub fn python_collect_shard_count(path_count: usize, cap: usize) -> usize {
    if path_count < PYTHON_COLLECT_SHARD_PATH_THRESHOLD {
        return 1;
    }
    let cap = cap.max(1);
    let cpus = host_cpu_count(4).clamp(1, cap);
    cpus.min(path_count / 16).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn check_rust_shard_count_follows_host() {
        assert_eq!(check_rust_shard_count(0), 1);
        let host = host_cpu_count(4);
        assert_eq!(check_rust_shard_count(10_000), host);
    }

    #[test]
    fn python_collect_shard_count_respects_threshold_and_cap() {
        assert_eq!(
            python_collect_shard_count(PYTHON_COLLECT_SHARD_PATH_THRESHOLD - 1, 3),
            1
        );
        assert!(python_collect_shard_count(1000, 3) <= 3);
        assert_eq!(python_collect_shard_count(1000, 1), 1);
    }

    // PWS1: the only place that may call available_parallelism() is host_cpu_count.
    #[test]
    fn available_parallelism_only_inside_host_cpu_count() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        scan_rs(&root, &mut offenders);
        assert!(
            offenders.is_empty(),
            "direct available_parallelism outside host_cpu_count:\n{}",
            offenders.join("\n")
        );
    }

    fn scan_rs(dir: &Path, offenders: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                scan_rs(&path, offenders);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            if path.ends_with("shared_helpers.rs") || path.ends_with("host_parallelism.rs") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            if text.contains("available_parallelism()") {
                offenders.push(path.display().to_string());
            }
        }
    }
}
