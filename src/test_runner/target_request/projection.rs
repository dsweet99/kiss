use std::path::Path;

use kiss::Language;
use serde::Serialize;

use super::history::reverse_records;
use super::plan_store;
use super::resolved::{OperandClass, ResolvedTarget, ReverseRecord, SourceRegion};
use super::slice::{TargetSliceStamp, stamp_from_projection};
use super::stamp::GitDepStamp;
use super::types::{TargetFocus, TargetRequest};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) enum SliceProjection {
    Workspace {
        selectors: Vec<String>,
        sources: Vec<String>,
    },
    TestDescriptors {
        descriptors: Vec<String>,
        selectors: Vec<String>,
    },
    SourceRegions {
        regions: Vec<SourceRegion>,
        reverse: Vec<ReverseRecord>,
    },
    Vcs {
        git: GitDepStamp,
        historical_reverse: Vec<ReverseRecord>,
        regions: Vec<SourceRegion>,
        selectors: Vec<String>,
    },
    Mixed {
        parts: Vec<SliceProjection>,
    },
}

pub(crate) fn remember_target_plan(
    repo_root: &Path,
    request: &TargetRequest,
    resolved: &ResolvedTarget,
) {
    let (projection, complete) = build_slice_projection(repo_root, request, resolved);
    let stamp = stamp_from_projection(&projection, complete);
    if plan_store::publish(repo_root, request, &stamp).is_ok() {
        let _ = plan_store::load_current(repo_root);
        let _ = plan_store::load_plan_for_identity(repo_root, request, &stamp);
    }
    let _ = super::slice::target_slice_stamp(resolved, complete);
}

pub(crate) fn build_slice_projection(
    repo_root: &Path,
    request: &TargetRequest,
    resolved: &ResolvedTarget,
) -> (SliceProjection, bool) {
    let complete = projection_complete(repo_root, request, resolved);
    let projection = match &request.focus {
        TargetFocus::Workspace => workspace_projection(repo_root, request),
        TargetFocus::Git(_) => vcs_projection(repo_root, resolved),
        TargetFocus::Operands(_) => operand_projection(repo_root, resolved),
    };
    (projection, complete)
}

fn workspace_projection(repo_root: &Path, request: &TargetRequest) -> SliceProjection {
    let lang = request.lang.map(|filter| filter.to_language());
    let ignore = request.ignore.as_slice();
    let mut selectors = Vec::new();
    if let Some((python, rust, _)) =
        crate::test_runner::workspace_selector_cache::load_cached_workspace_selectors_for_lang(
            repo_root,
            ignore,
            &[],
            lang,
        )
    {
        selectors.extend(python);
        selectors.extend(rust);
    }
    selectors.sort();
    selectors.dedup();
    SliceProjection::Workspace {
        selectors,
        sources: current_sources(repo_root, ignore, lang),
    }
}

fn vcs_projection(repo_root: &Path, resolved: &ResolvedTarget) -> SliceProjection {
    let Some(git) = resolved.git_stamp.clone() else {
        return SliceProjection::Vcs {
            git: empty_git_placeholder(),
            historical_reverse: reverse_records(repo_root, &resolved.historical_paths),
            regions: resolved.regions.clone(),
            selectors: resolved.direct_selectors.clone(),
        };
    };
    SliceProjection::Vcs {
        git,
        historical_reverse: reverse_records(repo_root, &resolved.historical_paths),
        regions: resolved.regions.clone(),
        selectors: resolved.direct_selectors.clone(),
    }
}

fn operand_projection(repo_root: &Path, resolved: &ResolvedTarget) -> SliceProjection {
    let test_only = resolved.operand_classes.iter().all(|class| {
        matches!(
            class,
            OperandClass::TestFile | OperandClass::TestSymbol | OperandClass::PythonNodeid
        )
    });
    if test_only && !resolved.operand_classes.is_empty() {
        return SliceProjection::TestDescriptors {
            descriptors: resolved.direct_selectors.clone(),
            selectors: resolved.direct_selectors.clone(),
        };
    }
    let source_only = resolved.operand_classes.iter().all(|class| {
        matches!(
            class,
            OperandClass::SourceFile | OperandClass::SourceSymbol | OperandClass::Directory
        )
    });
    if source_only {
        return SliceProjection::SourceRegions {
            regions: resolved.regions.clone(),
            reverse: reverse_for_regions(repo_root, &resolved.regions),
        };
    }
    SliceProjection::Mixed {
        parts: vec![
            SliceProjection::TestDescriptors {
                descriptors: resolved.direct_selectors.clone(),
                selectors: resolved.direct_selectors.clone(),
            },
            SliceProjection::SourceRegions {
                regions: resolved.regions.clone(),
                reverse: reverse_for_regions(repo_root, &resolved.regions),
            },
        ],
    }
}

fn reverse_for_regions(repo_root: &Path, regions: &[SourceRegion]) -> Vec<ReverseRecord> {
    let paths: Vec<String> = regions
        .iter()
        .filter_map(|region| match region {
            SourceRegion::FileAll { path } | SourceRegion::FileLines { path, .. } => {
                Some(path.clone())
            }
            SourceRegion::WorkspaceAll => None,
        })
        .collect();
    reverse_records(repo_root, &paths)
}

fn current_sources(repo_root: &Path, ignore: &[String], lang: Option<Language>) -> Vec<String> {
    let root = repo_root.to_string_lossy().into_owned();
    let (python, rust) = kiss::gather_files_by_lang(std::slice::from_ref(&root), None, ignore);
    let files = match lang {
        Some(Language::Python) => python,
        Some(Language::Rust) => rust,
        None => python.into_iter().chain(rust).collect(),
    };
    let mut sources: Vec<String> = files
        .into_iter()
        .map(|path| rel_source(repo_root, &path))
        .collect();
    sources.sort();
    sources.dedup();
    sources
}

fn rel_source(repo_root: &Path, path: &Path) -> String {
    path.strip_prefix(repo_root)
        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"))
}

impl SliceProjection {
    pub(crate) fn selectors(&self) -> Vec<String> {
        match self {
            Self::Workspace { selectors, .. } | Self::TestDescriptors { selectors, .. } => {
                selectors.clone()
            }
            Self::SourceRegions { reverse, .. } => reverse
                .iter()
                .flat_map(|record| record.selectors.iter().cloned())
                .collect(),
            Self::Vcs {
                selectors,
                historical_reverse,
                ..
            } => {
                let mut union = selectors.clone();
                union.extend(
                    historical_reverse
                        .iter()
                        .flat_map(|record| record.selectors.iter().cloned()),
                );
                union.sort();
                union.dedup();
                union
            }
            Self::Mixed { parts } => {
                let mut union = Vec::new();
                for part in parts {
                    union.extend(part.selectors());
                }
                union.sort();
                union.dedup();
                union
            }
        }
    }

    pub(crate) fn coverage_regions(&self) -> Vec<SourceRegion> {
        match self {
            Self::Workspace { .. } => vec![SourceRegion::WorkspaceAll],
            Self::TestDescriptors { .. } => Vec::new(),
            Self::SourceRegions { regions, .. } | Self::Vcs { regions, .. } => regions.clone(),
            Self::Mixed { parts } => {
                let mut regions = Vec::new();
                for part in parts {
                    for region in part.coverage_regions() {
                        if !regions.contains(&region) {
                            regions.push(region);
                        }
                    }
                }
                regions
            }
        }
    }
}

fn projection_complete(
    repo_root: &Path,
    request: &TargetRequest,
    resolved: &ResolvedTarget,
) -> bool {
    if !super::manifest::manifests_complete(repo_root, request) {
        return false;
    }
    match &request.focus {
        TargetFocus::Workspace => workspace_complete(repo_root, request),
        TargetFocus::Git(_) => git_projection_complete(repo_root, resolved),
        TargetFocus::Operands(_) => operand_projection_complete(repo_root, resolved),
    }
}

fn operand_projection_complete(repo_root: &Path, resolved: &ResolvedTarget) -> bool {
    let test_only = resolved.operand_classes.iter().all(|class| {
        matches!(
            class,
            OperandClass::TestFile | OperandClass::TestSymbol | OperandClass::PythonNodeid
        )
    });
    if test_only && !resolved.operand_classes.is_empty() {
        return !resolved.direct_selectors.is_empty();
    }
    let paths: Vec<String> = resolved
        .regions
        .iter()
        .filter_map(|region| match region {
            SourceRegion::FileAll { path } | SourceRegion::FileLines { path, .. } => {
                Some(path.clone())
            }
            SourceRegion::WorkspaceAll => None,
        })
        .collect();
    if resolved
        .regions
        .iter()
        .any(|region| matches!(region, SourceRegion::WorkspaceAll))
    {
        return true;
    }
    if paths.is_empty() && resolved.direct_selectors.is_empty() {
        return false;
    }
    let existing = paths
        .iter()
        .filter(|path| repo_root.join(path).is_file())
        .count();
    if existing == paths.len() && !paths.is_empty() {
        return true;
    }
    reverse_for_regions(repo_root, &resolved.regions).len() == paths.len()
}

fn git_projection_complete(repo_root: &Path, resolved: &ResolvedTarget) -> bool {
    let records = reverse_records(repo_root, &resolved.historical_paths);
    records.len() == resolved.historical_paths.len()
}

fn workspace_complete(repo_root: &Path, request: &TargetRequest) -> bool {
    let lang = request.lang.map(|filter| filter.to_language());
    let Some((python, rust, _)) =
        crate::test_runner::workspace_selector_cache::load_cached_workspace_selectors_for_lang(
            repo_root,
            &request.ignore,
            &[],
            lang,
        )
    else {
        return false;
    };
    super::manifest::cache_matches_inventory(repo_root, request, &python, &rust)
}

fn empty_git_placeholder() -> GitDepStamp {
    GitDepStamp {
        kind: super::stamp::GitStampKind::Commit,
        tracked: super::stamp::TrackedTreeStamp {
            digest: String::new(),
        },
        head_oid: None,
        untracked: None,
        merge_base_sha: None,
        explicit_ref: None,
        candidates: Vec::new(),
    }
}

pub(crate) fn slice_for(
    repo_root: &Path,
    request: &TargetRequest,
    resolved: &ResolvedTarget,
) -> TargetSliceStamp {
    let (projection, complete) = build_slice_projection(repo_root, request, resolved);
    stamp_from_projection(&projection, complete)
}

#[cfg(test)]
mod coverage_region_tests {
    use super::*;

    #[test]
    fn workspace_projection_covers_workspace_all() {
        let projection = SliceProjection::Workspace {
            selectors: vec!["tests/a.py::test_a".into()],
            sources: vec!["app.py".into()],
        };
        assert_eq!(
            projection.coverage_regions(),
            vec![SourceRegion::WorkspaceAll]
        );
    }

    #[test]
    fn test_descriptors_have_empty_coverage_obligation() {
        let projection = SliceProjection::TestDescriptors {
            descriptors: vec!["test_lib.py::test_fast".into()],
            selectors: vec!["test_lib.py::test_fast".into()],
        };
        assert!(
            projection.coverage_regions().is_empty(),
            "{:?}",
            projection.coverage_regions()
        );
    }

    #[test]
    fn mixed_union_keeps_only_source_regions() {
        let projection = SliceProjection::Mixed {
            parts: vec![
                SliceProjection::TestDescriptors {
                    descriptors: vec!["tests/test_app.py::test_value".into()],
                    selectors: vec!["tests/test_app.py::test_value".into()],
                },
                SliceProjection::SourceRegions {
                    regions: vec![SourceRegion::FileAll {
                        path: "pkg/app.py".into(),
                    }],
                    reverse: Vec::new(),
                },
            ],
        };
        assert_eq!(
            projection.coverage_regions(),
            vec![SourceRegion::FileAll {
                path: "pkg/app.py".into(),
            }]
        );
    }
}
