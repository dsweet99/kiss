use super::TestSelector;

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ChangedTestSelector {
    pub(crate) selector: TestSelector,
}

#[cfg(test)]
impl ChangedTestSelector {
    pub(crate) fn new(selector: TestSelector) -> Self {
        Self { selector }
    }
}
