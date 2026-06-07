//! Discriminating guards for the oracle-query-spec registry (§Q4 / §4),
//! src-side half. The `tests/` integration binary additionally proves the
//! registry `include!`-compiles WITHOUT the unit-test `support` module
//! (`oracle_query_specs_is_pure_data`); this src-side half proves the structural
//! well-formedness validation is genuinely discriminating, with SYNTHETIC specs
//! (the real table is empty until the first row lifts).

use super::oracle::query_specs::{
    registry_well_formed, HostProjectSpec, HostSetupKindSpec, OracleValueKindSpec,
    ProjectionModeSpec, QueryHelperSpec, QuerySpec, RegistryError, SourceLocatorSpec, SymbolSpace,
    ORACLE_QUERY_SPECS,
};

/// A synthetic well-formed spec with a tweakable `oracle_family` + `query_ordinal`.
fn spec(row_function: &'static str, query_ordinal: u16, oracle_family: &'static str) -> QuerySpec {
    QuerySpec {
        row_file: "synthetic.rs",
        row_function,
        query_ordinal,
        oracle_family,
        workspace_files: &[],
        primary_canonical: "/fixtures/synthetic.ts",
        host_project: HostProjectSpec {
            project_root: "/",
            workspace_root: "/",
            tsconfig_path: "/oracle.tsconfig.json",
            host_setup_kind: HostSetupKindSpec::Standalone,
        },
        query_helper: QueryHelperSpec::ResolveExpr {
            symbol: "Foo",
            type_args: &[],
            projection_mode: ProjectionModeSpec::Shallow,
        },
        source_locator: SourceLocatorSpec {
            reference_canonical: "/fixtures/synthetic.ts",
            reference_name: "Foo",
            symbol_space: SymbolSpace::Type,
        },
        oracle_value_kind: OracleValueKindSpec::StructuredTypeExpr,
    }
}

#[test]
fn oracle_query_specs_registry_is_empty_and_well_formed() {
    // This harness-foundation block lifts ZERO rows.
    assert!(ORACLE_QUERY_SPECS.is_empty());
    assert_eq!(registry_well_formed(ORACLE_QUERY_SPECS), Ok(()));
}

#[test]
fn registry_in_src_carries_oracle_family() {
    // Every real entry carries a non-empty oracle_family (enforced structurally
    // now; vacuously true on the empty table — the discrimination is below).
    for s in ORACLE_QUERY_SPECS {
        assert!(!s.oracle_family.is_empty());
    }

    // Discriminating: a synthetic entry with an EMPTY oracle_family is rejected —
    // it could not name an `oracle_snapshots/<family>/` sub-directory.
    let bad = [spec("a_row", 0, "")];
    assert!(matches!(
        registry_well_formed(&bad),
        Err(RegistryError::EmptyOracleFamily { .. })
    ));

    // A non-empty family is accepted.
    let good = [spec("a_row", 0, "utility_composition")];
    assert_eq!(registry_well_formed(&good), Ok(()));
}

#[test]
fn registry_ordinals_must_be_unique_and_contiguous() {
    // Contiguous 0..n-1 is well-formed.
    let ok = [
        spec("multi_row", 0, "conditional_infer"),
        spec("multi_row", 1, "conditional_infer"),
    ];
    assert_eq!(registry_well_formed(&ok), Ok(()));

    // A GAP ({0, 2}) fails.
    let gap = [
        spec("multi_row", 0, "conditional_infer"),
        spec("multi_row", 2, "conditional_infer"),
    ];
    assert!(matches!(
        registry_well_formed(&gap),
        Err(RegistryError::NonContiguousOrdinals { .. })
    ));

    // A DUPLICATE ({0, 0}) fails.
    let dup = [
        spec("multi_row", 0, "conditional_infer"),
        spec("multi_row", 0, "conditional_infer"),
    ];
    assert!(matches!(
        registry_well_formed(&dup),
        Err(RegistryError::NonContiguousOrdinals { .. })
    ));

    // An OFF-BY-ONE start ({1}) fails.
    let off = [spec("multi_row", 1, "conditional_infer")];
    assert!(matches!(
        registry_well_formed(&off),
        Err(RegistryError::NonContiguousOrdinals { .. })
    ));

    // Two DISTINCT rows each contiguous from 0 are well-formed (per-row grouping).
    let two_rows = [
        spec("row_a", 0, "indexed_access"),
        spec("row_b", 0, "tuple_projection"),
        spec("row_b", 1, "tuple_projection"),
    ];
    assert_eq!(registry_well_formed(&two_rows), Ok(()));
}
