use std::collections::{BTreeMap, BTreeSet};

use crate::analyze_cache::fnv1a64;

pub(super) fn combined_identity(
    parts: &[(String, String)],
    covered_lines: &BTreeMap<String, BTreeSet<u32>>,
) -> String {
    let mut h = fnv1a64(0xcbf2_9ce4_8422_2325, b"kiss-runtime-line-coverage-v1");
    for (key, value) in parts {
        h = fnv1a64(h, key.as_bytes());
        h = fnv1a64(h, &[0]);
        h = fnv1a64(h, value.as_bytes());
        h = fnv1a64(h, &[0]);
    }
    for (file, lines) in covered_lines {
        h = fnv1a64(h, file.as_bytes());
        h = fnv1a64(h, &[0]);
        for line in lines {
            h = fnv1a64(h, line.to_le_bytes().as_slice());
        }
        h = fnv1a64(h, &[0]);
    }
    format!("{h:016x}")
}
