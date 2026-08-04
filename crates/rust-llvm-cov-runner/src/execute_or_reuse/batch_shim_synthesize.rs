use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::execute_or_reuse::batch_events::{BatchCompilerArtifact, BatchEventStream, BatchTestTerminal};
use crate::plan::batch_nextest_id::prefer_deps_executable;
use crate::execute_or_reuse::batch_shim::BatchShimMetadata;
use crate::RustLlvmCovError;

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

/// Build shim metadata for CheckAggregate when there is no target runner.
///
/// Maps each terminal libtest name to a test-harness executable. Libtest-json-plus
/// uses `{package}::{target}$…` for both lib and bin unit tests when they share a
/// target name, so colliding prefixes are disambiguated via each target's
/// `src_path` top-level `mod` declarations.
pub(crate) fn synthesize_check_aggregate_shim_metadata(
    stream: &BatchEventStream,
    profile_path: &Path,
    cwd: &Path,
) -> Result<Vec<BatchShimMetadata>, RustLlvmCovError> {
    let executables = executable_candidates_by_libtest_prefix(&stream.compiler_artifacts)?;
    let mut metadata = Vec::with_capacity(stream.terminal_tests.len());
    for test in &stream.terminal_tests {
        metadata.push(synthesize_one(test, &executables, profile_path, cwd)?);
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
        insert_candidate(map.entry(prefix.clone()).or_default(), candidate_clone(&candidate));
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
        let harnesses = artifacts.iter().filter(|artifact| artifact.is_test_harness).count();
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
    if let Some(index) = slot.iter().position(|item| {
        prefer_deps_executable(&item.executable, &candidate.executable)
    }) {
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
    let executable = select_executable_for_test(prefix, test_name, candidates)?;
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
        if top_level_mod_names(Path::new(src_path)).contains(module) {
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
mod tests {
    use super::{
        check_aggregate_pool_profile_path_for_run, synthesize_check_aggregate_shim_metadata,
        top_level_mod_names,
    };
    use crate::execute_or_reuse::batch_events::{BatchCompilerArtifact, BatchEventStream, BatchTestTerminal};
    use std::path::PathBuf;

    fn artifact(
        prefix: &str,
        executable: &str,
        src_path: Option<&str>,
        is_test_harness: bool,
    ) -> BatchCompilerArtifact {
        BatchCompilerArtifact {
            executable: Some(executable.to_string()),
            filenames: vec![format!("{executable}.rmeta")],
            nextest_binary_id: Some(prefix.to_string()),
            libtest_binary_prefix: Some(prefix.to_string()),
            src_path: src_path.map(str::to_string),
            is_test_harness,
        }
    }

    fn terminal(full_name: &str, passed: bool) -> BatchTestTerminal {
        BatchTestTerminal {
            full_name: full_name.to_string(),
            test_name: full_name
                .rsplit_once('$')
                .map(|(_, name)| name.to_string())
                .unwrap(),
            passed,
            exec_time_secs: 0.01,
            stdout: None,
            reason: None,
        }
    }

    #[test]
    fn synthesizes_shared_pool_metadata_with_deps_preference() {
        let stream = BatchEventStream {
            compiler_artifacts: vec![
                artifact(
                    "kiss-ai::bin/kiss",
                    "/repo/target/debug/kiss",
                    None,
                    true,
                ),
                artifact(
                    "kiss-ai::bin/kiss",
                    "/repo/target/debug/deps/kiss-abc",
                    None,
                    true,
                ),
                artifact("kiss-ai::kiss", "/repo/target/debug/deps/kiss_lib-def", None, true),
            ],
            terminal_tests: vec![
                terminal("kiss-ai::bin/kiss$cli::smoke", true),
                terminal("kiss-ai::kiss$config::tests::defaults", true),
            ],
            ..BatchEventStream::default()
        };
        let profile = PathBuf::from("/tmp/instances/pool-%32m.profraw");
        let meta = synthesize_check_aggregate_shim_metadata(
            &stream,
            &profile,
            PathBuf::from("/repo").as_path(),
        )
        .unwrap();
        assert_eq!(meta.len(), 2);
        assert_eq!(meta[0].argv, vec!["/repo/target/debug/deps/kiss-abc"]);
        assert_eq!(meta[1].argv, vec!["/repo/target/debug/deps/kiss_lib-def"]);
        assert_eq!(meta[0].profile_path, profile);
        assert_eq!(meta[1].profile_path, profile);
    }

    #[test]
    fn colliding_lib_and_bin_libtest_prefix_disambiguates_via_src_mods() {
        let tmp = tempfile::tempdir().unwrap();
        let lib_src = tmp.path().join("lib.rs");
        let bin_src = tmp.path().join("main.rs");
        std::fs::write(&lib_src, "pub mod check_cache;\npub mod config;\n").unwrap();
        std::fs::write(&bin_src, "mod analyze;\nmod test_runner;\nmod bin_cli;\n").unwrap();

        let stream = BatchEventStream {
            compiler_artifacts: vec![
                BatchCompilerArtifact {
                    executable: Some("/repo/target/debug/deps/kiss-lib".into()),
                    filenames: vec![],
                    nextest_binary_id: Some("kiss-ai::kiss".into()),
                    libtest_binary_prefix: Some("kiss-ai::kiss".into()),
                    src_path: Some(lib_src.to_string_lossy().into_owned()),
                    is_test_harness: true,
                },
                BatchCompilerArtifact {
                    executable: Some("/repo/target/debug/deps/kiss-bin".into()),
                    filenames: vec![],
                    nextest_binary_id: Some("kiss-ai::bin/kiss".into()),
                    libtest_binary_prefix: Some("kiss-ai::kiss".into()),
                    src_path: Some(bin_src.to_string_lossy().into_owned()),
                    is_test_harness: true,
                },
                // Non-test bin must be ignored even if it shares the prefix.
                BatchCompilerArtifact {
                    executable: Some("/repo/target/debug/kiss".into()),
                    filenames: vec![],
                    nextest_binary_id: Some("kiss-ai::bin/kiss".into()),
                    libtest_binary_prefix: Some("kiss-ai::kiss".into()),
                    src_path: Some(bin_src.to_string_lossy().into_owned()),
                    is_test_harness: false,
                },
            ],
            terminal_tests: vec![
                terminal("kiss-ai::kiss$check_cache::tests::smoke", true),
                terminal(
                    "kiss-ai::kiss$analyze::cov_records_cache::tests::round_trip",
                    true,
                ),
            ],
            ..BatchEventStream::default()
        };
        let meta = synthesize_check_aggregate_shim_metadata(
            &stream,
            PathBuf::from("/tmp/pool-%32m.profraw").as_path(),
            PathBuf::from("/repo").as_path(),
        )
        .unwrap();
        assert_eq!(meta.len(), 2);
        assert_eq!(meta[0].argv, vec!["/repo/target/debug/deps/kiss-bin"]);
        assert_eq!(meta[1].argv, vec!["/repo/target/debug/deps/kiss-lib"]);
    }

    #[test]
    fn pool_profile_path_uses_online_merge_pattern() {
        let run_path = check_aggregate_pool_profile_path_for_run(
            PathBuf::from("/tmp/target").as_path(),
            PathBuf::from("/tmp/cache/runs/run-abc").as_path(),
        );
        assert_eq!(
            run_path,
            PathBuf::from("/tmp/target/run-abc-pool-%32m.profraw")
        );
    }

    #[test]
    fn missing_executable_for_libtest_prefix_errors() {
        let stream = BatchEventStream {
            compiler_artifacts: vec![artifact("other::other", "/tmp/other", None, true)],
            terminal_tests: vec![terminal("kiss-ai::kiss$missing", true)],
            ..BatchEventStream::default()
        };
        let err = synthesize_check_aggregate_shim_metadata(
            &stream,
            PathBuf::from("/tmp/pool-%32m.profraw").as_path(),
            PathBuf::from("/repo").as_path(),
        )
        .unwrap_err();
        assert!(format!("{err:?}").contains("kiss-ai::kiss"));
    }

    #[test]
    fn top_level_mod_names_reads_pub_and_private_mods() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("main.rs");
        std::fs::write(
            &path,
            "// mod ignored_comment;\npub mod analyze;\nmod test_runner;\npub(crate) mod symbol_mv_support;\nfn main() {}\n",
        )
        .unwrap();
        let mods = top_level_mod_names(&path);
        assert!(mods.contains("analyze"));
        assert!(mods.contains("test_runner"));
        assert!(mods.contains("symbol_mv_support"));
        assert!(!mods.contains("ignored_comment"));
    }
}
