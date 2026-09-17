use super::runtime::EnsureRequest;

pub(crate) trait SourceDeltaMisses {
    fn extra_source_delta_misses(
        &self,
        request: &EnsureRequest,
        planned: &[String],
    ) -> Result<Vec<String>, String> {
        let _ = (request, planned);
        Ok(Vec::new())
    }
}
