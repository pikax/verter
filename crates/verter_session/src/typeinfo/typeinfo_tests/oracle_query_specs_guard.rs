//! Discriminating guards for the oracle-query-spec registry (§Q4 / §4),
//! src-side half. The `tests/` integration binary additionally proves the
//! registry `include!`-compiles WITHOUT the unit-test `support` module
//! (`oracle_query_specs_is_pure_data`); this src-side half proves the structural
//! well-formedness validation is genuinely discriminating, with SYNTHETIC specs.
//! The real table seats the 8 lifted rows (two index-signature publication
//! queries + two built-in modifier-utility queries + three U2
//! IndexedAccess-reduction carve-out queries + the mapped-modifier `-?`
//! carve-out query at U2.MAPPED_TEMPLATE).

use super::oracle::query_specs::{
    registry_well_formed, HostProjectSpec, HostSetupKindSpec, OracleValueKindSpec,
    ProjectionModeSpec, QueryHelperSpec, QuerySpec, RegistryError, SourceLocatorSpec, SymbolSpace,
    DEEP_PATH_SOURCE, INDEX_SIGNATURES_SOURCE, MAPPED_MODIFIERS_SOURCE, ORACLE_QUERY_SPECS,
    TYPESCRIPT_RULES_SOURCE, UTILITY_EDGE_SOURCE, WIDE_DEEP_SOURCE,
};

/// The registry inlines each fixture's source bytes (`INDEX_SIGNATURES_SOURCE` /
/// `UTILITY_EDGE_SOURCE`) as `&'static str` rather than `include_str!`-ing the
/// `fixtures/*.ts` copy, because the registry file is ALSO `include!`'d into the
/// `tests/` integration binary where a relative `include_str!` path would not
/// resolve. The sibling `#[ignore]`d typeinfo tests, by contrast, `include_str!`
/// the on-disk fixture. This guard PINS the two representations byte-for-byte, so
/// an edit to one without the other (a silent drift between what the oracle row
/// upserts and what the ignored sibling test reads) FAILS. The `include_str!`
/// here resolves correctly because THIS guard is a normal module beside
/// `fixtures/`, not the `include!`'d registry file.
#[test]
fn inlined_registry_source_is_byte_identical_to_fixture_files() {
    assert_eq!(
        INDEX_SIGNATURES_SOURCE,
        include_str!("fixtures/index_signatures.ts"),
        "INDEX_SIGNATURES_SOURCE (inlined in the registry) drifted from \
         fixtures/index_signatures.ts (read by the sibling #[ignore]d tests)",
    );
    assert_eq!(
        UTILITY_EDGE_SOURCE,
        include_str!("fixtures/utility_edge.ts"),
        "UTILITY_EDGE_SOURCE (inlined in the registry) drifted from \
         fixtures/utility_edge.ts (read by the sibling #[ignore]d tests)",
    );
    assert_eq!(
        TYPESCRIPT_RULES_SOURCE,
        include_str!("fixtures/typescript_rules.ts"),
        "TYPESCRIPT_RULES_SOURCE (inlined in the registry) drifted from \
         fixtures/typescript_rules.ts (read by the sibling #[ignore]d tests)",
    );
    assert_eq!(
        DEEP_PATH_SOURCE,
        include_str!("fixtures/deep_path.ts"),
        "DEEP_PATH_SOURCE (inlined in the registry) drifted from \
         fixtures/deep_path.ts (read by the sibling #[ignore]d tests)",
    );
    assert_eq!(
        WIDE_DEEP_SOURCE,
        include_str!("fixtures/wide_deep.ts"),
        "WIDE_DEEP_SOURCE (inlined in the registry) drifted from \
         fixtures/wide_deep.ts (read by the sibling #[ignore]d tests)",
    );
    assert_eq!(
        MAPPED_MODIFIERS_SOURCE,
        include_str!("fixtures/mapped_modifiers.ts"),
        "MAPPED_MODIFIERS_SOURCE (inlined in the registry) drifted from \
         fixtures/mapped_modifiers.ts (read by the sibling #[ignore]d tests)",
    );
}

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
fn oracle_query_specs_registry_holds_the_lifted_rows_and_is_well_formed() {
    // The lifts seat the two index-signature publication queries, the two
    // built-in modifier-utility queries, the three U2 IndexedAccess-reduction
    // carve-out queries, and the U2.MAPPED_TEMPLATE `-?` optional-remover query;
    // the table is well-formed (non-empty `oracle_family`, contiguous ordinals).
    assert_eq!(registry_well_formed(ORACLE_QUERY_SPECS), Ok(()));

    // The seated set is EXACTLY the two index-signature publication rows + the
    // two built-in modifier-utility rows + the three IndexedAccess-reduction
    // carve-out rows + the `-?` optional-remover row, one query each. A stray
    // addition / removal FAILS here (discriminating).
    let seated: Vec<(&str, &str, u16)> = ORACLE_QUERY_SPECS
        .iter()
        .map(|s| (s.row_file, s.row_function, s.query_ordinal))
        .collect();
    assert_eq!(
        seated,
        vec![
            (
                "index_signatures.rs",
                "index_signatures_numeric_index_publishes_signature",
                0
            ),
            (
                "index_signatures.rs",
                "index_signatures_symbol_index_publishes_signature",
                0
            ),
            (
                "utility_edge.rs",
                "utility_edge_required_strips_optional_markers",
                0
            ),
            (
                "utility_edge.rs",
                "utility_edge_readonly_required_composes_modifiers",
                0
            ),
            (
                "typescript_rules.rs",
                "typescript_rules_indexed_access_reduces_terminal_property",
                0
            ),
            (
                "deep_path.rs",
                "deep_path_projection_resolves_terminal_without_losing_shape",
                0
            ),
            (
                "wide_deep.rs",
                "wide_deep_projected_token_resolves_literal_union",
                0
            ),
            (
                "mapped_modifiers.rs",
                "mapped_modifier_minus_optional_strips_optional_and_undefined",
                0
            ),
        ],
    );
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
