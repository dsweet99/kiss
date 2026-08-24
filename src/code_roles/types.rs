#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CodeContextSet {
    pub production: bool,
    pub test: bool,
}

impl CodeContextSet {
    #[must_use]
    pub const fn none() -> Self {
        Self {
            production: false,
            test: false,
        }
    }

    #[must_use]
    pub const fn production_only() -> Self {
        Self {
            production: true,
            test: false,
        }
    }

    #[must_use]
    pub const fn test_only() -> Self {
        Self {
            production: false,
            test: true,
        }
    }

    #[must_use]
    pub const fn both() -> Self {
        Self {
            production: true,
            test: true,
        }
    }

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self {
            production: self.production || other.production,
            test: self.test || other.test,
        }
    }

    #[must_use]
    pub const fn is_test_only(self) -> bool {
        self.test && !self.production
    }

    #[must_use]
    pub const fn role(self) -> CodeRole {
        if self.is_test_only() {
            CodeRole::TestOnly
        } else {
            CodeRole::Production
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CodeRole {
    Production,
    TestOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileComposition {
    ProductionOnly,
    TestOnly,
    Mixed,
}

#[cfg(test)]
mod types_test {
    use super::*;

    #[test]
    fn context_union_and_role() {
        let shared = CodeContextSet::production_only().union(CodeContextSet::test_only());
        assert_eq!(shared, CodeContextSet::both());
        assert_eq!(shared.role(), CodeRole::Production);
        assert_eq!(CodeContextSet::test_only().role(), CodeRole::TestOnly);
        assert_eq!(CodeContextSet::none().role(), CodeRole::Production);
    }
}
