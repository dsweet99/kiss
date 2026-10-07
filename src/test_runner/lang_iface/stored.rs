use serde::{Deserialize, Serialize};

/// Id of a language's current witness generation; a report pins it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GenerationIds {
    pub(crate) witness: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_empty() {
        let ids = GenerationIds::default();
        assert_eq!(ids.witness, None);
    }
}
