#![cfg_attr(not(test), allow(dead_code))]

pub(crate) trait KernelHooks: Sync {
    fn is_test_source(&self, path: &std::path::Path) -> bool {
        let _ = path;
        false
    }

    fn validate_extra_args(&self, extras: &[String]) -> Result<(), String> {
        let _ = extras;
        Ok(())
    }

    fn validate_explicit_targets(
        &self,
        repo_root: &std::path::Path,
        files: &[std::path::PathBuf],
        direct: &std::collections::BTreeSet<String>,
    ) -> Result<(), String> {
        let _ = (repo_root, files, direct);
        Ok(())
    }

    fn extras_block_cold_population(&self, extras: &[String]) -> bool {
        let _ = extras;
        false
    }
}
