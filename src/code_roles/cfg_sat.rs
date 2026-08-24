use std::collections::BTreeSet;

use super::cfg_pred::{ATOM_TEST, AtomId, CfgPred};
use super::types::CodeContextSet;

#[must_use]
pub fn sat_with_test(pred: &CfgPred, test: bool) -> bool {
    let mut atoms = BTreeSet::new();
    collect_atoms(pred, &mut atoms);
    atoms.remove(&ATOM_TEST);
    sat_assign(pred, test, &atoms.into_iter().collect::<Vec<_>>(), 0, 0)
}

#[must_use]
pub fn contexts_for_pred(pred: &CfgPred, allow_production: bool) -> CodeContextSet {
    let test = sat_with_test(pred, true);
    let production = allow_production && sat_with_test(pred, false);
    CodeContextSet { production, test }
}

fn collect_atoms(pred: &CfgPred, out: &mut BTreeSet<AtomId>) {
    match pred {
        CfgPred::True | CfgPred::False => {}
        CfgPred::Atom(id) => {
            out.insert(*id);
        }
        CfgPred::Not(inner) => collect_atoms(inner, out),
        CfgPred::All(parts) | CfgPred::Any(parts) => {
            for part in parts {
                collect_atoms(part, out);
            }
        }
    }
}

fn sat_assign(pred: &CfgPred, test: bool, atoms: &[AtomId], index: usize, mask: u64) -> bool {
    if index == atoms.len() {
        return eval(pred, test, atoms, mask);
    }
    let bit = 1_u64 << index;
    sat_assign(pred, test, atoms, index + 1, mask)
        || sat_assign(pred, test, atoms, index + 1, mask | bit)
}

fn eval(pred: &CfgPred, test: bool, atoms: &[AtomId], mask: u64) -> bool {
    match pred {
        CfgPred::True => true,
        CfgPred::False => false,
        CfgPred::Atom(id) if *id == ATOM_TEST => test,
        CfgPred::Atom(id) => atom_value(*id, atoms, mask),
        CfgPred::Not(inner) => !eval(inner, test, atoms, mask),
        CfgPred::All(parts) => parts.iter().all(|p| eval(p, test, atoms, mask)),
        CfgPred::Any(parts) => parts.iter().any(|p| eval(p, test, atoms, mask)),
    }
}

fn atom_value(id: AtomId, atoms: &[AtomId], mask: u64) -> bool {
    atoms
        .iter()
        .enumerate()
        .find_map(|(i, atom)| (*atom == id).then_some((mask & (1_u64 << i)) != 0))
        .unwrap_or(false)
}

#[cfg(test)]
mod sat_test {
    use super::*;
    use crate::code_roles::cfg_parse::parse_cfg_tokens;
    use crate::code_roles::cfg_pred::AtomInterner;
    use proc_macro2::TokenStream;
    use std::path::Path;
    use std::str::FromStr;

    fn pred(src: &str) -> CfgPred {
        let mut atoms = AtomInterner::new();
        parse_cfg_tokens(
            TokenStream::from_str(src).unwrap(),
            &mut atoms,
            Path::new("x.rs"),
        )
        .unwrap()
    }

    #[test]
    fn test_and_unix_is_test_only() {
        let p = pred("all(test, unix)");
        assert!(!sat_with_test(&p, false));
        assert!(sat_with_test(&p, true));
        assert!(contexts_for_pred(&p, true).is_test_only());
    }

    #[test]
    fn any_test_or_feature_is_production() {
        let p = pred("any(test, feature = \"x\")");
        assert!(sat_with_test(&p, false));
        assert!(sat_with_test(&p, true));
        assert!(contexts_for_pred(&p, true).production);
    }

    #[test]
    fn not_test_is_production() {
        let p = pred("not(test)");
        assert!(sat_with_test(&p, false));
        assert!(!sat_with_test(&p, true));
    }

    #[test]
    fn empty_all_and_any() {
        let p_all = pred("all()");
        assert!(sat_with_test(&p_all, false));
        let p_any = pred("any()");
        assert!(!sat_with_test(&p_any, false));
    }

    #[test]
    fn repeated_atom_contradiction_is_unsat() {
        let p = pred("all(feature = \"x\", not(feature = \"x\"))");
        assert!(!sat_with_test(&p, false));
        assert!(!sat_with_test(&p, true));
    }
}
