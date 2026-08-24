use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AtomId(pub u32);

pub const ATOM_TEST: AtomId = AtomId(0);

#[derive(Clone, Debug, Default)]
pub struct AtomInterner {
    by_key: HashMap<String, AtomId>,
    keys: Vec<String>,
}

impl AtomInterner {
    #[must_use]
    pub fn new() -> Self {
        let mut by_key = HashMap::new();
        by_key.insert("test".to_string(), ATOM_TEST);
        Self {
            by_key,
            keys: vec!["test".to_string()],
        }
    }

    pub fn intern(&mut self, key: &str) -> AtomId {
        if let Some(&id) = self.by_key.get(key) {
            return id;
        }
        let id = AtomId(u32::try_from(self.keys.len()).unwrap_or(u32::MAX));
        self.by_key.insert(key.to_string(), id);
        self.keys.push(key.to_string());
        id
    }

    #[must_use]
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.keys.len()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CfgPred {
    True,
    False,
    Atom(AtomId),
    Not(Box<CfgPred>),
    All(Vec<CfgPred>),
    Any(Vec<CfgPred>),
}

impl CfgPred {
    #[must_use]
    pub fn not(inner: Self) -> Self {
        match inner {
            Self::True => Self::False,
            Self::False => Self::True,
            Self::Not(inner) => *inner,
            other => Self::Not(Box::new(other)),
        }
    }

    #[must_use]
    pub fn all(parts: Vec<Self>) -> Self {
        let mut flat = Vec::new();
        for part in parts {
            match part {
                Self::True => {}
                Self::False => return Self::False,
                Self::All(inner) => flat.extend(inner),
                other => flat.push(other),
            }
        }
        match flat.len() {
            0 => Self::True,
            1 => flat.pop().unwrap_or(Self::True),
            _ => Self::All(flat),
        }
    }

    #[must_use]
    pub fn any(parts: Vec<Self>) -> Self {
        let mut flat = Vec::new();
        for part in parts {
            match part {
                Self::False => {}
                Self::True => return Self::True,
                Self::Any(inner) => flat.extend(inner),
                other => flat.push(other),
            }
        }
        match flat.len() {
            0 => Self::False,
            1 => flat.pop().unwrap_or(Self::False),
            _ => Self::Any(flat),
        }
    }

    #[must_use]
    pub fn and(self, other: Self) -> Self {
        Self::all(vec![self, other])
    }

    #[must_use]
    pub fn or(self, other: Self) -> Self {
        Self::any(vec![self, other])
    }
}

#[cfg(test)]
mod pred_test {
    use super::*;

    #[test]
    fn all_empty_is_true_any_empty_is_false() {
        assert_eq!(CfgPred::all(Vec::new()), CfgPred::True);
        assert_eq!(CfgPred::any(Vec::new()), CfgPred::False);
        assert_eq!(
            CfgPred::all(vec![CfgPred::True, CfgPred::Atom(ATOM_TEST)]),
            CfgPred::Atom(ATOM_TEST)
        );
    }

    #[test]
    fn intern_reuses_test_atom() {
        let mut atoms = AtomInterner::new();
        assert_eq!(atoms.intern("test"), ATOM_TEST);
        let a = atoms.intern("feature=\"x\"");
        let b = atoms.intern("feature=\"x\"");
        assert_eq!(a, b);
        assert_ne!(a, ATOM_TEST);
        assert!(atoms.len() >= 2);
    }
}
