use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use crate::rust_llvm_cov_runner::RustLlvmCovError;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_events::{
    BatchCompilerArtifact, BatchEventStream, BatchTestTerminal,
};
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_shim::BatchShimMetadata;
use crate::rust_llvm_cov_runner::plan::batch_nextest_id::prefer_deps_executable;

pub(crate) fn check_aggregate_pool_profile_path_for_run(
    build_target: &Path,
    run_root: &Path,
) -> PathBuf {
    let run_token = run_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("run");
    build_target.join(format!("{run_token}-pool-%32m.profraw"))
}

pub(crate) fn synthesize_check_aggregate_shim_metadata(
    stream: &BatchEventStream,
    profile_path: &Path,
    cwd: &Path,
) -> Result<Vec<BatchShimMetadata>, RustLlvmCovError> {
    let executables = executable_candidates_by_libtest_prefix(&stream.compiler_artifacts)?;
    let mut metadata = Vec::with_capacity(stream.terminal_tests.len());
    let mut mod_names = HashMap::new();
    for test in &stream.terminal_tests {
        metadata.push(synthesize_one(
            test,
            &executables,
            profile_path,
            cwd,
            &mut mod_names,
        )?);
    }
    metadata.sort_by(|left, right| left.full_name.cmp(&right.full_name));
    Ok(metadata)
}

#[derive(Clone, Debug)]
struct HarnessCandidate {
    executable: String,
    src_path: Option<String>,
}

fn executable_candidates_by_libtest_prefix(
    artifacts: &[BatchCompilerArtifact],
) -> Result<BTreeMap<String, Vec<HarnessCandidate>>, RustLlvmCovError> {
    let mut map = BTreeMap::<String, Vec<HarnessCandidate>>::new();
    for artifact in artifacts {
        if !artifact.is_test_harness {
            continue;
        }
        let Some(prefix) = artifact.libtest_binary_prefix.as_ref() else {
            continue;
        };
        let Some(executable) = artifact.executable.as_ref() else {
            continue;
        };
        let candidate = HarnessCandidate {
            executable: executable.clone(),
            src_path: artifact.src_path.clone(),
        };
        insert_candidate(
            map.entry(prefix.clone()).or_default(),
            candidate_clone(&candidate),
        );
        if let Some(nextest_id) = artifact.nextest_binary_id.as_ref()
            && nextest_id != prefix
        {
            insert_candidate(map.entry(nextest_id.clone()).or_default(), candidate);
        }
    }
    if map.is_empty() {
        let total = artifacts.len();
        let with_exe = artifacts
            .iter()
            .filter(|artifact| artifact.executable.is_some())
            .count();
        let with_prefix = artifacts
            .iter()
            .filter(|artifact| artifact.libtest_binary_prefix.is_some())
            .count();
        let harnesses = artifacts
            .iter()
            .filter(|artifact| artifact.is_test_harness)
            .count();
        return Err(RustLlvmCovError::InvalidRequest(format!(
            "no compiler-artifact executables available to synthesize CheckAggregate shim metadata \
             (artifacts={total}, with_executable={with_exe}, with_libtest_prefix={with_prefix}, \
             test_harnesses={harnesses})"
        )));
    }
    Ok(map)
}

fn candidate_clone(candidate: &HarnessCandidate) -> HarnessCandidate {
    HarnessCandidate {
        executable: candidate.executable.clone(),
        src_path: candidate.src_path.clone(),
    }
}

fn insert_candidate(slot: &mut Vec<HarnessCandidate>, candidate: HarnessCandidate) {
    if let Some(existing) = slot
        .iter_mut()
        .find(|item| item.executable == candidate.executable)
    {
        if existing.src_path.is_none() {
            existing.src_path = candidate.src_path;
        }
        return;
    }
    if let Some(index) = slot
        .iter()
        .position(|item| prefer_deps_executable(&item.executable, &candidate.executable))
    {
        slot[index] = candidate;
        return;
    }
    if slot
        .iter()
        .any(|item| prefer_deps_executable(&candidate.executable, &item.executable))
    {
        return;
    }
    slot.push(candidate);
}

fn synthesize_one(
    test: &BatchTestTerminal,
    executables: &BTreeMap<String, Vec<HarnessCandidate>>,
    profile_path: &Path,
    cwd: &Path,
    mod_names: &mut HashMap<PathBuf, BTreeSet<String>>,
) -> Result<BatchShimMetadata, RustLlvmCovError> {
    let (prefix, test_name) = test.full_name.rsplit_once('$').ok_or_else(|| {
        RustLlvmCovError::InvalidRequest(format!(
            "libtest name `{}` is missing `$` test suffix",
            test.full_name
        ))
    })?;
    let candidates = executables.get(prefix).ok_or_else(|| {
        RustLlvmCovError::InvalidRequest(format!(
            "no compiler-artifact executable for libtest binary id `{prefix}` (test `{}`)",
            test.full_name
        ))
    })?;
    let executable = select_executable_for_test(prefix, test_name, candidates, mod_names)?;
    Ok(BatchShimMetadata {
        schema_version: "kiss-rust-llvm-cov-shim-v2".to_string(),
        id: test.full_name.replace(['/', '\\'], "_"),
        full_name: test.full_name.clone(),
        profile_path: profile_path.to_path_buf(),
        cwd: cwd.to_path_buf(),
        argv: vec![executable],
        exit_code: Some(if test.passed { 0 } else { 1 }),
        spawn_error: None,
        shim_identity: None,
        delegated_identity: None,
        stdout: test.stdout.as_ref().map(|value| value.as_bytes().to_vec()),
        stderr: test.reason.as_ref().map(|value| value.as_bytes().to_vec()),
        output_frame_count: None,
    })
}

fn select_executable_for_test(
    prefix: &str,
    test_name: &str,
    candidates: &[HarnessCandidate],
    mod_names: &mut HashMap<PathBuf, BTreeSet<String>>,
) -> Result<String, RustLlvmCovError> {
    assert!(
        !candidates.is_empty(),
        "select_executable_for_test requires at least one candidate"
    );
    if candidates.len() == 1 {
        return Ok(candidates[0].executable.clone());
    }
    let module = test_name
        .split("::")
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or(test_name);
    let mut matches = Vec::new();
    for candidate in candidates {
        let Some(src_path) = candidate.src_path.as_deref() else {
            continue;
        };
        let src_path = Path::new(src_path);
        let names = mod_names
            .entry(src_path.to_path_buf())
            .or_insert_with(|| top_level_mod_names(src_path));
        if names.contains(module) {
            matches.push(candidate);
        }
    }
    if matches.len() == 1 {
        return Ok(matches[0].executable.clone());
    }
    Err(RustLlvmCovError::InvalidRequest(format!(
        "ambiguous test harnesses for libtest id `{prefix}` test `{test_name}` \
         (candidates={}, module_matches={})",
        candidates.len(),
        matches.len()
    )))
}

pub(crate) fn top_level_mod_names(src_path: &Path) -> BTreeSet<String> {
    let Ok(text) = fs::read_to_string(src_path) else {
        return BTreeSet::new();
    };
    let mut mods = BTreeSet::new();
    for raw in text.lines() {
        let line = strip_line_comment(raw).trim();
        let Some(name) = parse_top_level_mod_name(line) else {
            continue;
        };
        mods.insert(name);
    }
    mods
}

fn parse_top_level_mod_name(line: &str) -> Option<String> {
    let mut rest = line;
    if let Some(after_pub) = rest.strip_prefix("pub") {
        rest = after_pub.trim_start();
        if rest.starts_with('(') {
            let close = rest.find(')')?;
            rest = rest[close + 1..].trim_start();
        }
    }
    let after_mod = rest.strip_prefix("mod ")?.trim_start();
    let name = after_mod
        .split(|ch: char| ch == ';' || ch == '{' || ch.is_whitespace())
        .next()
        .unwrap_or("");
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return None;
    }
    Some(name.to_string())
}

fn strip_line_comment(line: &str) -> &str {
    match line.find("//") {
        Some(index) => &line[..index],
        None => line,
    }
}

#[cfg(test)]
#[path = "batch_shim_synthesize_test.rs"]
mod tests;
