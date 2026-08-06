use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;

use crate::cache::load_rslip_cache_entry;
use crate::{RslipError, RslipOutcome, rslip_outcome_from_cache};

use super::{RslipCacheCandidate, RslipCacheCandidateGroup};

pub(super) fn coalesce_rslip_miss_candidates(
    misses: Vec<RslipCacheCandidate>,
) -> Vec<RslipCacheCandidateGroup> {
    let mut groups: BTreeMap<(PathBuf, String), Vec<RslipCacheCandidate>> = BTreeMap::new();
    for miss in misses {
        groups
            .entry((miss.canonical_cache_root.clone(), miss.fingerprint.clone()))
            .or_default()
            .push(miss);
    }
    let mut runner_groups = Vec::new();
    for ((_canonical_root, fingerprint), candidates) in groups {
        let indices: Vec<usize> = candidates.iter().map(|candidate| candidate.index).collect();
        let mut iter = candidates.into_iter();
        let mut representative = iter
            .next()
            .expect("rslip miss group contains a representative");
        for other in iter {
            representative.req.force_rerun |= other.req.force_rerun;
        }
        runner_groups.push(RslipCacheCandidateGroup {
            indices,
            representative,
            fingerprint,
        });
    }
    runner_groups.sort_by_key(|group| group.indices.first().copied().unwrap_or(usize::MAX));
    runner_groups
}

/// Briefly lock each miss group, recheck the cache, then release the lock.
/// Retains only groups that are still missing so FDs are not held across the run.
pub(super) fn brief_lock_filter_rslip_miss_groups(
    groups: Vec<RslipCacheCandidateGroup>,
    out: &mut [Option<Result<RslipOutcome, RslipError>>],
) -> Vec<RslipCacheCandidateGroup> {
    let mut runner_groups = Vec::new();
    for group in groups {
        match crate::lock_rslip_cache_entry(
            &group.representative.req.cache_root,
            &group.fingerprint,
        ) {
            Ok(_guard) => {
                if !group.representative.req.force_rerun
                    && let Some(entry) = load_rslip_cache_entry(
                        &group.representative.req.cache_root,
                        &group.fingerprint,
                    )
                {
                    for index in group.indices {
                        out[index] = Some(Ok(rslip_outcome_from_cache(entry.clone())));
                    }
                } else {
                    runner_groups.push(group);
                }
            }
            Err(err) => {
                for index in group.indices {
                    out[index] = Some(Err(RslipError::Io(io::Error::new(
                        err.kind(),
                        err.to_string(),
                    ))));
                }
            }
        }
    }
    runner_groups
}
