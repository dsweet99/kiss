#[path = "reference_inference_assignments.rs"]
mod reference_inference_assignments;
use super::definition::{find_impl_blocks, find_python_class_block};
use reference_inference_assignments::{
    tuple_assignment_receiver_type, type_from_assignment_rhs, type_from_assignment_target,
};
#[cfg(test)]
#[path = "reference_inference_coverage.rs"]
mod reference_inference_coverage;

include!("reference_inference_body.txt");
