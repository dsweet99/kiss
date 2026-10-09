use std::path::Path;

use kiss::Language;
use serde::Serialize;

use super::resolved::{OperandClass, ResolvedTarget, SourceRegion};
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
        selectors: Vec<String>,
    },
    Vcs {
        git: GitDepStamp,
        regions: Vec<SourceRegion>,
        selectors: Vec<String>,
    },
    Mixed {
        parts: Vec<SliceProjection>,
    },
}

pub(crate) fn build_slice_projection(
    repo_root: &Path,
    request: &TargetRequest,
    resolved: &ResolvedTarget,
) -> (SliceProjection, bool) {
    let complete = projection_complete(repo_root, request, resolved);
    let projection = match &request.focus {
        TargetFocus::Workspace => workspace_projection(repo_root, request),
        TargetFocus::Git(_) => vcs_projection(repo_root, request, resolved),
        TargetFocus::Operands(_) => operand_projection(repo_root, request, resolved),
    };
    (projection, complete)
}

fn workspace_projection(repo_root: &Path, request: &TargetRequest) -> SliceProjection {
    let lang = request.lang;
    let ignore = request.ignore.as_slice();
    let mut selectors = Vec::new();
    let want = crate::test_runner::language_keyed::LanguageKeyed::from_fn(|language| {
        language.allowed_by(lang)
    });
    if want.python {
        selectors.extend(python_workspace_selectors(repo_root, request));
    }
    if want.rust {
        selectors.extend(rust_workspace_selectors(repo_root, ignore));
    }
    selectors.sort();
    selectors.dedup();
    SliceProjection::Workspace {
        selectors,
        sources: current_sources(repo_root, ignore, lang),
    }
}

fn python_workspace_selectors(repo_root: &Path, request: &TargetRequest) -> Vec<String> {
    let ignore = request.ignore.as_slice();
    if let Some(cached) =
        crate::test_runner::workspace_selector_cache::load_cached_python_workspace_selectors(
            repo_root,
            ignore,
            &[],
        )
    {
        return cached;
    }
    if !super::manifest::has_python_test_files(repo_root, request) {
        return Vec::new();
    }
    let Ok(found) =
        crate::test_runner::runners::enumerate_workspace_python_selectors(repo_root, ignore, &[])
    else {
        return Vec::new();
    };
    let _ = crate::test_runner::workspace_selector_cache::store_python_workspace_selectors(
        repo_root,
        ignore,
        &found,
        &[],
    );
    found
}

fn rust_workspace_selectors(repo_root: &Path, ignore: &[String]) -> Vec<String> {
    if let Some(cached) =
        crate::test_runner::workspace_selector_cache::load_cached_rust_workspace_selectors(
            repo_root, ignore,
        )
    {
        return cached;
    }
    let Ok(found) =
        crate::test_runner::runners::enumerate_workspace_rust_selectors(repo_root, ignore)
    else {
        return Vec::new();
    };
    let _ = crate::test_runner::workspace_selector_cache::store_rust_workspace_selectors(
        repo_root, ignore, &found,
    );
    found
}

fn vcs_projection(
    repo_root: &Path,
    request: &TargetRequest,
    resolved: &ResolvedTarget,
) -> SliceProjection {
    SliceProjection::Vcs {
        git: resolved
            .git_stamp
            .clone()
            .unwrap_or_else(empty_git_placeholder),
        regions: resolved.regions.clone(),
        selectors: vcs_selectors(repo_root, request, resolved),
    }
}

fn vcs_selectors(
    repo_root: &Path,
    request: &TargetRequest,
    resolved: &ResolvedTarget,
) -> Vec<String> {
    let paths = region_paths(&resolved.regions);
    let mut selectors = resolved.direct_selectors.clone();
    selectors.extend(population_selectors_for_paths(repo_root, &paths, request));
    if paths.iter().any(|path| path.ends_with(".py")) {
        selectors.extend(
            python_workspace_selectors(repo_root, request)
                .into_iter()
                .filter(|selector| {
                    let file = selector
                        .split_once("::")
                        .map_or(selector.as_str(), |(file, _)| file);
                    paths.iter().any(|path| path == file)
                }),
        );
    }
    selectors.sort();
    selectors.dedup();
    selectors
}

fn operand_projection(
    repo_root: &Path,
    request: &TargetRequest,
    resolved: &ResolvedTarget,
) -> SliceProjection {
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
            selectors: selectors_for_regions(repo_root, request, &resolved.regions),
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
                selectors: selectors_for_regions(repo_root, request, &resolved.regions),
            },
        ],
    }
}

fn region_paths(regions: &[SourceRegion]) -> Vec<String> {
    regions
        .iter()
        .filter_map(|region| match region {
            SourceRegion::FileAll { path } | SourceRegion::FileLines { path, .. } => {
                Some(path.clone())
            }
            SourceRegion::WorkspaceAll => None,
        })
        .collect()
}

fn selectors_for_regions(
    repo_root: &Path,
    request: &TargetRequest,
    regions: &[SourceRegion],
) -> Vec<String> {
    population_selectors_for_paths(repo_root, &region_paths(regions), request)
}

pub(crate) fn population_selectors_for_paths(
    repo_root: &Path,
    paths: &[String],
    request: &TargetRequest,
) -> Vec<String> {
    let mut selectors = Vec::new();
    for language in Language::ALL {
        let has_source = paths.iter().any(|path| {
            let path = Path::new(path);
            path.extension()
                .and_then(|ext| ext.to_str())
                .and_then(crate::test_runner::lang_registry::language_for_extension)
                == Some(language)
                && !crate::test_runner::lang_registry::rules_for(language).is_test_source(path)
        });
        if !has_source {
            continue;
        }
        match language {
            Language::Python => selectors.extend(python_workspace_selectors(repo_root, request)),
            Language::Rust => selectors.extend(
                crate::test_runner::workspace_selector_cache::load_cached_rust_workspace_selectors(
                    repo_root,
                    &request.ignore,
                )
                .unwrap_or_default(),
            ),
        }
    }
    selectors.sort();
    selectors.dedup();
    selectors
}

fn current_sources(repo_root: &Path, ignore: &[String], lang: Option<Language>) -> Vec<String> {
    let root = repo_root.to_string_lossy().into_owned();
    let (python, rust) = kiss::gather_files_by_lang(std::slice::from_ref(&root), lang, ignore);
    let mut by_language = crate::test_runner::language_keyed::LanguageKeyed { python, rust };
    let mut sources: Vec<String> = Language::ALL
        .into_iter()
        .filter(|language| language.allowed_by(lang))
        .flat_map(|language| std::mem::take(by_language.get_mut(language)))
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
            Self::SourceRegions { selectors, .. } | Self::Vcs { selectors, .. } => {
                selectors.clone()
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

    pub(crate) fn report_regions(&self) -> Vec<SourceRegion> {
        match self {
            Self::Workspace { .. } => vec![SourceRegion::WorkspaceAll],
            Self::TestDescriptors { .. } => Vec::new(),
            Self::SourceRegions { regions, .. } | Self::Vcs { regions, .. } => regions.clone(),
            Self::Mixed { parts } => {
                let mut regions = Vec::new();
                for part in parts {
                    for region in part.report_regions() {
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
        TargetFocus::Git(_) => true,
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
    let paths = region_paths(&resolved.regions);
    if resolved
        .regions
        .iter()
        .any(|region| matches!(region, SourceRegion::WorkspaceAll))
    {
        return true;
    }
    if paths.is_empty() {
        return !resolved.direct_selectors.is_empty();
    }
    paths.iter().all(|path| repo_root.join(path).is_file())
}

fn workspace_complete(repo_root: &Path, request: &TargetRequest) -> bool {
    let lang = request.lang;
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
mod report_region_tests {
    use super::*;

    #[test]
    fn rust_only_repo_lists_no_python_selectors_without_collecting() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"t\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(tmp.path().join("src/lib.rs"), "pub fn f() {}\n").unwrap();
        let request = TargetRequest {
            focus: TargetFocus::Workspace,
            lang: None,
            ignore: Vec::new(),
        };
        crate::test_runner::lang_python::collect::reset_full_suite_subprocess_collects_for_tests();
        assert!(python_workspace_selectors(tmp.path(), &request).is_empty());
        assert_eq!(
            crate::test_runner::lang_python::collect::full_suite_subprocess_collects_for_tests(),
            0
        );
    }

    #[test]
    fn workspace_projection_reports_workspace_all() {
        let projection = SliceProjection::Workspace {
            selectors: vec!["tests/a.py::test_a".into()],
            sources: vec!["app.py".into()],
        };
        assert_eq!(
            projection.report_regions(),
            vec![SourceRegion::WorkspaceAll]
        );
    }

    #[test]
    fn test_descriptors_have_no_report_regions() {
        let projection = SliceProjection::TestDescriptors {
            descriptors: vec!["test_lib.py::test_fast".into()],
            selectors: vec!["test_lib.py::test_fast".into()],
        };
        assert!(
            projection.report_regions().is_empty(),
            "{:?}",
            projection.report_regions()
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
                    selectors: Vec::new(),
                },
            ],
        };
        assert_eq!(
            projection.report_regions(),
            vec![SourceRegion::FileAll {
                path: "pkg/app.py".into(),
            }]
        );
    }
}
