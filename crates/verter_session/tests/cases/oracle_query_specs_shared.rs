//! `oracle_query_specs_is_pure_data` + `registry_in_src_carries_oracle_family`
//! (tests-side half), `docs/arch/u0-oracle-harness-design.md` §Q4 / §4.
//!
//! The oracle-query-spec registry lives in `src/typeinfo/typeinfo_tests/` so the
//! lifted UNIT tests reach it; this `tests/` integration guard reaches the SAME
//! table via an `include!` of that exact file — proving there is ONE table, not
//! two that can drift. The `include!` COMPILING here is itself the discriminating
//! proof of purity: this integration binary has NO access to the unit-test
//! `support` module, so if the registry ever referenced `super::support`, a
//! private unit-test type, or a helper call, this file would FAIL to compile.
//!
//! The registry is pasted into a private module so its items do not leak into
//! the integration crate root.

mod registry {
    include!("../../src/typeinfo/typeinfo_tests/oracle_query_specs.rs");
}

use registry::{registry_well_formed, ORACLE_QUERY_SPECS};

/// The shared table compiles + validates here WITHOUT the unit-test `support`
/// module — the structural proof of the pure-data contract.
#[test]
fn oracle_query_specs_is_pure_data() {
    // Reached as ONE table from the integration binary; well-formed by §Q4.
    assert_eq!(registry_well_formed(ORACLE_QUERY_SPECS), Ok(()));
}

/// Every registry entry carries a non-empty `oracle_family` (the
/// directory/presentation key the driver builds the snapshot path from). Vacuous
/// on the empty initial table, but the consuming code is real and the src-side
/// guard proves the validation discriminates an empty family.
#[test]
fn registry_in_src_carries_oracle_family() {
    for spec in ORACLE_QUERY_SPECS {
        assert!(
            !spec.oracle_family.is_empty(),
            "registry entry {}::{} ordinal {} has an empty oracle_family",
            spec.row_file,
            spec.row_function,
            spec.query_ordinal
        );
    }
}
