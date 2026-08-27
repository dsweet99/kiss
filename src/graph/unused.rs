#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GraphIsolation {
    IsolatedModule,
    UnreferencedModule,
}

impl GraphIsolation {
    pub(crate) fn module_is_isolated(
        self,
        fan_in: usize,
        fan_out: usize,
        has_test_importer: bool,
    ) -> bool {
        match self {
            Self::IsolatedModule => fan_in == 0 && fan_out == 0,
            Self::UnreferencedModule => fan_in == 0 && !has_test_importer,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::GraphIsolation;

    #[test]
    fn isolated_module_requires_zero_fan_out() {
        assert!(GraphIsolation::IsolatedModule.module_is_isolated(0, 0, false));
        assert!(!GraphIsolation::IsolatedModule.module_is_isolated(0, 1, false));
        assert!(GraphIsolation::UnreferencedModule.module_is_isolated(0, 1, false));
        assert!(!GraphIsolation::UnreferencedModule.module_is_isolated(0, 1, true));
    }
}
