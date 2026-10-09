#![cfg_attr(not(test), allow(dead_code))]
use serde::{Deserialize, Serialize};

use super::digest::digest_bytes;
use super::projection::SliceProjection;

pub(crate) const TARGET_SLICE_SCHEMA: &str = "target-slice-v1";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TargetSliceStamp {
    pub digest: String,
    pub complete: bool,
    pub index_schema: String,
}

#[derive(Serialize)]
struct SlicePayload<'a> {
    index_schema: &'a str,
    complete: bool,
    projection: &'a SliceProjection,
}

pub(crate) fn stamp_from_projection(
    projection: &SliceProjection,
    complete: bool,
) -> TargetSliceStamp {
    let payload = SlicePayload {
        index_schema: TARGET_SLICE_SCHEMA,
        complete,
        projection,
    };
    let bytes = serde_json::to_vec(&payload).expect("target slice payload");
    TargetSliceStamp {
        digest: digest_bytes(&bytes),
        complete,
        index_schema: TARGET_SLICE_SCHEMA.to_string(),
    }
}
