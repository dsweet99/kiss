use crate::rust_llvm_cov_runner::RustLineCoverage;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_aggregate::InstanceResult;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_export::ExportCounters;
use std::collections::BTreeMap;

pub(crate) struct FreshCheckAggregateExport {
    pub(crate) exact: bool,
    pub(crate) instances: Vec<InstanceResult>,
    pub(crate) exported: BTreeMap<String, RustLineCoverage>,
    pub(crate) counters: ExportCounters,
}

impl FreshCheckAggregateExport {
    pub(crate) fn new(
        exact: bool,
        instances: Vec<InstanceResult>,
        exported: BTreeMap<String, RustLineCoverage>,
        counters: ExportCounters,
    ) -> Self {
        Self {
            exact,
            instances,
            exported,
            counters,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_check_aggregate_export_constructor_preserves_fields() {
        let export =
            FreshCheckAggregateExport::new(true, Vec::new(), BTreeMap::new(), Default::default());

        assert!(export.exact);
        assert!(export.instances.is_empty());
        assert!(export.exported.is_empty());
        assert_eq!(export.counters.export_jobs, 0);
    }
}
