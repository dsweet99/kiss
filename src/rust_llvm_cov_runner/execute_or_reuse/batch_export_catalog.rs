use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::rust_llvm_cov_runner::execute_or_reuse::batch_events::BatchCompilerArtifact;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_export::InstanceExportRequest;

pub fn build_object_catalog(
    artifacts: &[BatchCompilerArtifact],
    build_target: &Path,
    export_requests: &[InstanceExportRequest],
    env: &BTreeMap<String, String>,
) -> Vec<PathBuf> {
    let mut objects = object_paths_from_artifacts(artifacts);
    for request in export_requests {
        objects.extend(request.objects.iter().cloned());
    }
    collect_instrumented_objects_from_build_target(build_target, &mut objects);
    for value in env.values() {
        let path = PathBuf::from(value);
        if path.is_file() {
            objects.push(path);
        }
    }
    objects.sort();
    objects.dedup();
    objects
}

pub fn object_paths_from_artifacts(artifacts: &[BatchCompilerArtifact]) -> Vec<PathBuf> {
    let mut objects = Vec::new();
    for artifact in artifacts {
        objects.extend(object_paths_for_artifact(artifact));
    }
    objects.sort();
    objects.dedup();
    objects
}

pub(crate) fn object_paths_for_artifact(artifact: &BatchCompilerArtifact) -> Vec<PathBuf> {
    let mut objects = Vec::new();
    for filename in &artifact.filenames {
        if is_object_file(filename) {
            objects.push(PathBuf::from(filename));
        }
    }
    if let Some(executable) = artifact.executable.as_ref() {
        let path = PathBuf::from(executable);
        if path.is_file() {
            objects.push(path);
        }
    }
    objects
}

fn is_object_file(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext, "o" | "rlib"))
}

fn collect_instrumented_objects_from_build_target(build_target: &Path, out: &mut Vec<PathBuf>) {
    let mut scanned = BTreeSet::new();
    for dir in [
        build_target.join("debug").join("deps"),
        build_target.join("debug"),
        build_target.join("deps"),
        build_target.to_path_buf(),
    ] {
        if !dir.is_dir() || !scanned.insert(dir.clone()) {
            continue;
        }
        collect_instrumented_objects_in_flat_dir(&dir, out);
    }
}

fn collect_instrumented_objects_in_flat_dir(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            continue;
        }
        if is_instrumented_catalog_object(&path) || is_instrumented_catalog_executable(&path) {
            out.push(path);
        }
    }
}

fn is_instrumented_catalog_executable(path: &Path) -> bool {
    if !path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| !name.ends_with(".d") && !name.contains('.'))
    {
        return false;
    }
    path.parent()
        .and_then(|parent| parent.file_name())
        .is_some_and(|name| name == "deps" || name == "debug" || name == "release")
}

fn is_instrumented_catalog_object(path: &Path) -> bool {
    if let Some(ext) = path.extension().and_then(|value| value.to_str()) {
        return matches!(ext, "o" | "rlib");
    }
    path.parent()
        .and_then(|parent| parent.file_name())
        .is_some_and(|name| name == "deps")
        && !path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".d"))
}

#[cfg(test)]
#[path = "batch_export_catalog_test.rs"]
mod tests;
