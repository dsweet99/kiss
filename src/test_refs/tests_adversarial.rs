use super::*;
use crate::units::CodeUnitKind;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// principles.md: Quality bar §1 — no hard-coded benchmark filenames without structural predicate
/// smell_registry: filename_inventory
/// invariant: module-import witness credit must depend on structural role, not basename alone
/// counterexample: paired protocol stubs under acme/base/oi/protocols/ with identical import witness topology
#[test]
fn protocol_stub_credit_must_not_key_on_interfaces_basename() {
    use super::coverage::is_py_oi_module_import_witnessed;

    let import_bindings: HashMap<String, HashSet<String>> =
        HashMap::from([("acme.base.oi.protocols".into(), HashSet::new())]);

    let interfaces_file = PathBuf::from("acme/base/oi/protocols/interfaces.py");
    let contract_file = PathBuf::from("acme/base/oi/protocols/protocol_contract.py");

    let interfaces_def = CodeDefinition {
        name: "IWidget".into(),
        kind: CodeUnitKind::Class,
        file: interfaces_file.clone(),
        line: 1,
        end_line: 5,
        containing_class: None,
    };
    let contract_def = CodeDefinition {
        name: "IWidget".into(),
        kind: CodeUnitKind::Class,
        file: contract_file.clone(),
        line: 1,
        end_line: 5,
        containing_class: None,
    };

    let module_suffixes: HashMap<PathBuf, String> = HashMap::from([
        (
            interfaces_file,
            "acme.base.oi.protocols.interfaces".into(),
        ),
        (
            contract_file,
            "acme.base.oi.protocols.protocol_contract".into(),
        ),
    ]);

    let interfaces_credited = is_py_oi_module_import_witnessed(
        &interfaces_def,
        &import_bindings,
        &module_suffixes,
    );
    let contract_credited = is_py_oi_module_import_witnessed(
        &contract_def,
        &import_bindings,
        &module_suffixes,
    );

    assert_eq!(
        interfaces_credited, contract_credited,
        "isomorphic protocol stubs in base/oi must receive identical module-import witness credit under basename rename"
    );
    assert!(
        contract_credited,
        "neutral protocol stub (protocol_contract.py) must receive module-import witness credit without benchmark basename interfaces.py"
    );
}
