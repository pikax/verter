//! Discriminating guards for the oracle-query-spec registry (§Q4 / §4),
//! src-side half. The `tests/` integration binary additionally proves the
//! registry `include!`-compiles WITHOUT the unit-test `support` module
//! (`oracle_query_specs_is_pure_data`); this src-side half proves the structural
//! well-formedness validation is genuinely discriminating, with SYNTHETIC specs.
//! The real table seats the 46 lifted rows — the authoritative enumeration
//! lives on `ORACLE_QUERY_SPECS`' doc comment and is pinned exactly by
//! `oracle_query_specs_registry_holds_the_lifted_rows_and_is_well_formed`.

use super::oracle::query_specs::{
    registry_well_formed, HostProjectSpec, HostSetupKindSpec, OracleValueKindSpec, ProbeRhsSpec,
    ProjectionModeSpec, QueryHelperSpec, QuerySpec, RegistryError, SourceLocatorSpec, SymbolSpace,
    BRANDED_TYPES_SOURCE, CLASS_FEATURES_SOURCE, DECORATORS_SOURCE, DEEP_PATH_SOURCE,
    FUNCTION_ADVANCED_SOURCE, INDEX_SIGNATURES_SOURCE, JSX_SOURCE, MAPPED_MODIFIERS_SOURCE,
    MAPPED_TEMPLATE_SOURCE, MODERN_TS_FEATURES_SOURCE, MODE_BOUNDARY_REEXPORT_BARREL_SOURCE,
    MODE_BOUNDARY_REEXPORT_LEAF_SOURCE, MODE_BOUNDARY_REEXPORT_LINK_1_SOURCE,
    MODE_BOUNDARY_REEXPORT_LINK_2_SOURCE, MODE_BOUNDARY_REEXPORT_LINK_3_SOURCE,
    MODE_BOUNDARY_REEXPORT_LINK_4_SOURCE, MODE_BOUNDARY_REEXPORT_LINK_5_SOURCE,
    MODE_BOUNDARY_REEXPORT_LINK_6_SOURCE, MODE_BOUNDARY_REEXPORT_PRINCIPAL_SOURCE,
    MODULE_FEATURES_BASE_SOURCE, MODULE_FEATURES_CJS_SOURCE, MODULE_FEATURES_CONSUMER_SOURCE,
    MODULE_FEATURES_LEAF_SOURCE, MODULE_FEATURES_PATCH_SOURCE, MODULE_FEATURES_SOURCE,
    ORACLE_QUERY_SPECS, SUBSTITUTION_TYPES_SOURCE, TEMPLATE_LITERAL_INFERENCE_SOURCE,
    TYPESCRIPT_RULES_SOURCE, UNION_KEY_ACCESS_SOURCE, UTILITY_COMPOSITION_SOURCE,
    UTILITY_EDGE_SOURCE, UTILITY_TOP_BOTTOM_SOURCE, VARIADIC_TUPLES_SOURCE, WIDE_DEEP_SOURCE,
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
        CLASS_FEATURES_SOURCE,
        include_str!("fixtures/class_features.ts"),
        "CLASS_FEATURES_SOURCE (inlined in the registry) drifted from \
         fixtures/class_features.ts"
    );
    assert_eq!(
        FUNCTION_ADVANCED_SOURCE,
        include_str!("fixtures/function_advanced.ts"),
        "FUNCTION_ADVANCED_SOURCE (inlined in the registry) drifted from \
         fixtures/function_advanced.ts"
    );
    assert_eq!(
        BRANDED_TYPES_SOURCE,
        include_str!("fixtures/branded_types.ts"),
        "BRANDED_TYPES_SOURCE (inlined in the registry) drifted from \
         fixtures/branded_types.ts"
    );
    assert_eq!(
        DECORATORS_SOURCE,
        include_str!("fixtures/decorators.ts"),
        "DECORATORS_SOURCE (inlined in the registry) drifted from \
         fixtures/decorators.ts"
    );
    assert_eq!(
        SUBSTITUTION_TYPES_SOURCE,
        include_str!("fixtures/substitution_types.ts"),
        "SUBSTITUTION_TYPES_SOURCE (inlined in the registry) drifted from \
         fixtures/substitution_types.ts"
    );
    assert_eq!(
        INDEX_SIGNATURES_SOURCE,
        include_str!("fixtures/index_signatures.ts"),
        "INDEX_SIGNATURES_SOURCE (inlined in the registry) drifted from \
         fixtures/index_signatures.ts (read by the sibling #[ignore]d tests)",
    );
    assert_eq!(
        JSX_SOURCE,
        include_str!("fixtures/jsx.ts"),
        "JSX_SOURCE (inlined in the registry) drifted from \
         fixtures/jsx.ts (read by the sibling #[ignore]d tests)",
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
    assert_eq!(
        UNION_KEY_ACCESS_SOURCE,
        include_str!("fixtures/union_key_access.ts"),
        "UNION_KEY_ACCESS_SOURCE (inlined in the registry) drifted from \
         fixtures/union_key_access.ts (read by the sibling #[ignore]d tests)",
    );
    assert_eq!(
        MODE_BOUNDARY_REEXPORT_PRINCIPAL_SOURCE,
        include_str!("fixtures/mode_boundary_reexport_principal.ts"),
        "MODE_BOUNDARY_REEXPORT_PRINCIPAL_SOURCE (inlined in the registry) drifted from \
         fixtures/mode_boundary_reexport_principal.ts (read by the sibling #[ignore]d tests)",
    );
    assert_eq!(
        MODE_BOUNDARY_REEXPORT_LINK_1_SOURCE,
        include_str!("fixtures/mode_boundary_reexport_link_1.ts"),
        "MODE_BOUNDARY_REEXPORT_LINK_1_SOURCE (inlined in the registry) drifted from \
         fixtures/mode_boundary_reexport_link_1.ts (read by the sibling #[ignore]d tests)",
    );
    assert_eq!(
        MODE_BOUNDARY_REEXPORT_LINK_2_SOURCE,
        include_str!("fixtures/mode_boundary_reexport_link_2.ts"),
        "MODE_BOUNDARY_REEXPORT_LINK_2_SOURCE (inlined in the registry) drifted from \
         fixtures/mode_boundary_reexport_link_2.ts (read by the sibling #[ignore]d tests)",
    );
    assert_eq!(
        MODE_BOUNDARY_REEXPORT_LINK_3_SOURCE,
        include_str!("fixtures/mode_boundary_reexport_link_3.ts"),
        "MODE_BOUNDARY_REEXPORT_LINK_3_SOURCE (inlined in the registry) drifted from \
         fixtures/mode_boundary_reexport_link_3.ts (read by the sibling #[ignore]d tests)",
    );
    assert_eq!(
        MODE_BOUNDARY_REEXPORT_LINK_4_SOURCE,
        include_str!("fixtures/mode_boundary_reexport_link_4.ts"),
        "MODE_BOUNDARY_REEXPORT_LINK_4_SOURCE (inlined in the registry) drifted from \
         fixtures/mode_boundary_reexport_link_4.ts (read by the sibling #[ignore]d tests)",
    );
    assert_eq!(
        MODE_BOUNDARY_REEXPORT_LINK_5_SOURCE,
        include_str!("fixtures/mode_boundary_reexport_link_5.ts"),
        "MODE_BOUNDARY_REEXPORT_LINK_5_SOURCE (inlined in the registry) drifted from \
         fixtures/mode_boundary_reexport_link_5.ts (read by the sibling #[ignore]d tests)",
    );
    assert_eq!(
        MODE_BOUNDARY_REEXPORT_LINK_6_SOURCE,
        include_str!("fixtures/mode_boundary_reexport_link_6.ts"),
        "MODE_BOUNDARY_REEXPORT_LINK_6_SOURCE (inlined in the registry) drifted from \
         fixtures/mode_boundary_reexport_link_6.ts (read by the sibling #[ignore]d tests)",
    );
    assert_eq!(
        MODE_BOUNDARY_REEXPORT_BARREL_SOURCE,
        include_str!("fixtures/mode_boundary_reexport_barrel.ts"),
        "MODE_BOUNDARY_REEXPORT_BARREL_SOURCE (inlined in the registry) drifted from \
         fixtures/mode_boundary_reexport_barrel.ts (read by the sibling #[ignore]d tests)",
    );
    assert_eq!(
        MODE_BOUNDARY_REEXPORT_LEAF_SOURCE,
        include_str!("fixtures/mode_boundary_reexport_leaf.ts"),
        "MODE_BOUNDARY_REEXPORT_LEAF_SOURCE (inlined in the registry) drifted from \
         fixtures/mode_boundary_reexport_leaf.ts (read by the sibling #[ignore]d tests)",
    );
    assert_eq!(
        UTILITY_TOP_BOTTOM_SOURCE,
        include_str!("fixtures/utility_top_bottom.ts"),
        "UTILITY_TOP_BOTTOM_SOURCE (inlined in the registry) drifted from \
         fixtures/utility_top_bottom.ts (read by the sibling #[ignore]d tests)",
    );
    assert_eq!(
        VARIADIC_TUPLES_SOURCE,
        include_str!("fixtures/variadic_tuples.ts"),
        "VARIADIC_TUPLES_SOURCE (inlined in the registry) drifted from \
         fixtures/variadic_tuples.ts (read by the sibling #[ignore]d tests)",
    );
    assert_eq!(
        UTILITY_COMPOSITION_SOURCE,
        include_str!("fixtures/utility_composition.ts"),
        "UTILITY_COMPOSITION_SOURCE (inlined in the registry) drifted from \
         fixtures/utility_composition.ts (read by the sibling #[ignore]d tests)",
    );
    assert_eq!(
        MODERN_TS_FEATURES_SOURCE,
        include_str!("fixtures/modern_ts_features.ts"),
        "MODERN_TS_FEATURES_SOURCE (inlined in the registry) drifted from \
         fixtures/modern_ts_features.ts (read by the sibling #[ignore]d tests)",
    );
    assert_eq!(
        MODULE_FEATURES_SOURCE,
        include_str!("fixtures/module_features.ts"),
        "MODULE_FEATURES_SOURCE (inlined in the registry) drifted from \
         fixtures/module_features.ts (read by the sibling #[ignore]d tests)",
    );
    assert_eq!(
        MODULE_FEATURES_LEAF_SOURCE,
        include_str!("fixtures/module_features_leaf.ts"),
        "MODULE_FEATURES_LEAF_SOURCE (inlined in the registry) drifted from \
         fixtures/module_features_leaf.ts (read by the sibling #[ignore]d tests)",
    );
    assert_eq!(
        MODULE_FEATURES_BASE_SOURCE,
        include_str!("fixtures/module_features_base.ts"),
        "MODULE_FEATURES_BASE_SOURCE (inlined in the registry) drifted from \
         fixtures/module_features_base.ts (read by the sibling #[ignore]d tests)",
    );
    assert_eq!(
        MODULE_FEATURES_PATCH_SOURCE,
        include_str!("fixtures/module_features_patch.ts"),
        "MODULE_FEATURES_PATCH_SOURCE (inlined in the registry) drifted from \
         fixtures/module_features_patch.ts (read by the sibling #[ignore]d tests)",
    );
    assert_eq!(
        MODULE_FEATURES_CJS_SOURCE,
        include_str!("fixtures/module_features_cjs.d.ts"),
        "MODULE_FEATURES_CJS_SOURCE (inlined in the registry) drifted from \
         fixtures/module_features_cjs.d.ts (read by the sibling #[ignore]d tests)",
    );
    assert_eq!(
        MODULE_FEATURES_CONSUMER_SOURCE,
        include_str!("fixtures/module_features_consumer.ts"),
        "MODULE_FEATURES_CONSUMER_SOURCE (inlined in the registry) drifted from \
         fixtures/module_features_consumer.ts (read by the sibling #[ignore]d tests)",
    );
    assert_eq!(
        MAPPED_TEMPLATE_SOURCE,
        include_str!("fixtures/mapped_template.ts"),
        "MAPPED_TEMPLATE_SOURCE (inlined in the registry) drifted from \
         fixtures/mapped_template.ts (read by the sibling #[ignore]d tests)",
    );
    assert_eq!(
        TEMPLATE_LITERAL_INFERENCE_SOURCE,
        include_str!("fixtures/template_literal_inference.ts"),
        "TEMPLATE_LITERAL_INFERENCE_SOURCE (inlined in the registry) drifted from \
         fixtures/template_literal_inference.ts (read by the sibling #[ignore]d tests)",
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
            probe_rhs: ProbeRhsSpec::Bare,
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
    // The lifts seat 46 queries: the two index-signature publication
    // queries, the two built-in modifier-utility queries, the three U2
    // IndexedAccess-reduction carve-out queries, the U2.MAPPED_TEMPLATE
    // `-?` optional-remover query, the three keyof-expansion carve-out
    // queries, the eight U2.UTILITIES lifts (the five Awaited rows,
    // the two NonNullable rows, and the variadic concat row), the
    // nineteen U2.CLASS_SURFACES-era lifts (two brand-tag index chains,
    // three class-features static rows, nine function-advanced
    // signature-bucket/prototype/overload rows, the sb15 bare-generic
    // ReturnType row, two typescript-rules construct-signature rows, and
    // two decoration-invariance rows), the four U2.MODULE_AUGMENTATION-era
    // lifts (the `as const` typeof indexed member + the two `typeof import(...)`
    // value-member projections [named-value + default-shape] + the namespace
    // alias-chain projection), the two U2.INDEXED_ACCESS JSX parametric
    // intrinsic-lookup lifts (`IntrinsicPropsFor<"div">` /
    // `IntrinsicPropsFor<"span">`), and the two U2.MAPPED_TEMPLATE-era lifts
    // (the `RecordTemplateRootSlot` string-literal index-chain query + the
    // `CounterHandlers` key-remap mapped-type query); the table is well-formed
    // (non-empty `oracle_family`, contiguous ordinals).
    assert_eq!(registry_well_formed(ORACLE_QUERY_SPECS), Ok(()));

    // The seated set is EXACTLY those 46 rows, one query each. A stray
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
            (
                "typescript_rules.rs",
                "typescript_rules_keyof_materializes_literal_key_union",
                0
            ),
            (
                "mode_boundary_invariants.rs",
                "mode_boundary_keyof_across_reexport_chain_resolves_all_keys",
                0
            ),
            (
                "union_key_access.rs",
                "union_key_access_keyof_self_projects_full_value_union",
                0
            ),
            (
                "utility_top_bottom.rs",
                "utility_top_bottom_utb17_awaited_null_is_null",
                0
            ),
            (
                "utility_top_bottom.rs",
                "utility_top_bottom_utb18_awaited_undefined_is_undefined",
                0
            ),
            (
                "utility_top_bottom.rs",
                "utility_top_bottom_utb19_awaited_nested_promise_is_inner_primitive",
                0
            ),
            (
                "typescript_rules.rs",
                "typescript_rules_awaited_recursively_unwraps_promises",
                0
            ),
            (
                "utility_edge.rs",
                "utility_edge_non_nullable_strips_null_and_undefined",
                0
            ),
            (
                "variadic_tuples.rs",
                "variadic_tuple_concat_alias_produces_joined_literal_tuple",
                0
            ),
            (
                "utility_top_bottom.rs",
                "utility_top_bottom_utb21_non_nullable_unknown_is_empty_object",
                0
            ),
            (
                "utility_top_bottom.rs",
                "utility_top_bottom_utb15_awaited_unknown_is_unknown",
                0
            ),
            (
                "branded_types.rs",
                "branded_key_access_projects_literal_brand_tag",
                0
            ),
            (
                "branded_types.rs",
                "branded_key_access_projects_boolean_literal_brand_tag",
                0
            ),
            (
                "class_features.rs",
                "class_features_static_inheritance_resolves_inherited_field_type",
                0
            ),
            (
                "class_features.rs",
                "class_features_static_inheritance_resolves_inherited_method_return",
                0
            ),
            (
                "class_features.rs",
                "class_features_static_generic_method_instantiation_projects_return_with_substitution",
                0
            ),
            (
                "function_advanced.rs",
                "function_advanced_constructor_parameters_publishes_constructor_arg_tuple",
                0
            ),
            (
                "function_advanced.rs",
                "function_advanced_instance_type_publishes_constructor_return_shape",
                0
            ),
            (
                "function_advanced.rs",
                "function_advanced_call_construct_hybrid_parameters_uses_call_signature",
                0
            ),
            (
                "function_advanced.rs",
                "function_advanced_call_construct_hybrid_return_type_uses_call_signature",
                0
            ),
            (
                "function_advanced.rs",
                "function_advanced_call_construct_hybrid_constructor_parameters_uses_construct_signature",
                0
            ),
            (
                "function_advanced.rs",
                "function_advanced_call_construct_hybrid_instance_type_uses_construct_signature",
                0
            ),
            (
                "function_advanced.rs",
                "function_advanced_class_method_prototype_extraction_projects_return",
                0
            ),
            (
                "function_advanced.rs",
                "function_advanced_class_method_prototype_extraction_projects_parameters",
                0
            ),
            (
                "function_advanced.rs",
                "function_advanced_return_type_of_overloaded_function_uses_last_overload",
                0
            ),
            (
                "substitution_types.rs",
                "substitution_types_sb15_recursive_generic_substitution",
                0
            ),
            (
                "typescript_rules.rs",
                "typescript_rules_constructor_parameters_resolve_tuple",
                0
            ),
            (
                "typescript_rules.rs",
                "typescript_rules_instance_type_resolves_constructed_object",
                0
            ),
            (
                "decorators.rs",
                "decorators_identity_method_decorator_preserves_return_inference",
                0
            ),
            (
                "decorators.rs",
                "decorators_metadata_reader_describe_return_is_literal_union",
                0
            ),
            (
                "modern_ts_features.rs",
                "import_attribute_simulated_string_literal_indexed_member",
                0
            ),
            (
                "module_features.rs",
                "module_features_namespace_geometry_vector_aliases_point",
                0
            ),
            (
                "module_features.rs",
                "module_features_typeof_import_named_value_resolves_to_literal",
                0
            ),
            (
                "module_features.rs",
                "module_features_typeof_import_default_resolves_value_shape",
                0
            ),
            (
                "jsx.rs",
                "jsx_intrinsic_via_generic_lookup_div_resolves_to_div_shape",
                0
            ),
            (
                "jsx.rs",
                "jsx_intrinsic_via_generic_lookup_span_resolves_to_span_shape",
                0
            ),
            (
                "mapped_template.rs",
                "record_with_template_literal_key_union_projects_root_slot",
                0
            ),
            (
                "template_literal_inference.rs",
                "template_literal_key_remap_capitalises_each_event_key",
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

// ---------------------------------------------------------------------------
// The v4 `relation_verdict` registry (addendum §4/§5)
// ---------------------------------------------------------------------------

use super::oracle::identity;
use super::oracle::query_specs::{
    relation_registry_well_formed, EngineObservationPin, RelationEngineVerdict,
    RelationRegistryError, RELATION_QUERY_SPECS,
};
use super::oracle::relation_probe;

/// The 28 `relation_semantics.rs` projection-contract row names — the
/// authoritative list the 26 relation identities map onto (the two collapse
/// pairs are named explicitly below).
const EXPECTED_CONTRACT_ROWS: &[&str] = &[
    "relation_any_extends_string_distributes_both_branches",
    "relation_unknown_extends_string_selects_false_branch",
    "relation_never_extends_string_directly_selects_true_branch",
    "relation_never_via_generic_helper_collapses_to_never",
    "relation_string_extends_any_selects_true_branch",
    "relation_string_extends_unknown_selects_true_branch",
    "relation_string_extends_never_selects_false_branch",
    "relation_required_property_assignable_to_optional",
    "relation_optional_property_not_assignable_to_required",
    "relation_empty_object_assignable_to_all_optional",
    "relation_mutable_property_assignable_to_readonly",
    "relation_readonly_property_assignable_to_mutable",
    "relation_function_with_wider_param_assignable_to_narrower_target",
    "relation_function_with_narrower_param_not_assignable_to_wider_target",
    "relation_fixed_tuple_assignable_to_first_plus_rest",
    "relation_rest_tuple_not_assignable_to_fixed_tuple",
    "relation_one_tuple_assignable_to_one_with_optional_second_slot",
    "relation_empty_tuple_assignable_to_readonly_array",
    "relation_distributive_conditional_over_union_emits_branch_union",
    "relation_tuple_wrapped_conditional_over_union_does_not_distribute",
    "relation_intersection_assignable_to_one_base_arm",
    "relation_one_arm_not_assignable_to_intersection",
    "relation_infer_value_of_object_property",
    "relation_infer_head_of_tuple_pattern",
    "relation_infer_tail_of_tuple_pattern",
    "relation_infer_return_of_function",
    "relation_infer_params_of_function_preserves_optional_undefined",
    "relation_infer_single_param_of_function",
];

#[test]
fn relation_registry_holds_the_26_identities_and_maps_the_28_contracts() {
    assert_eq!(
        relation_registry_well_formed(RELATION_QUERY_SPECS),
        Ok(()),
        "the relation registry is well-formed"
    );
    // Exactly 26 specs — NO strict-axis multiplication (no ON/OFF pairs).
    assert_eq!(
        RELATION_QUERY_SPECS.len(),
        26,
        "the registry seats exactly the 26 relation identities"
    );

    // The 28 projection contracts map onto exactly the 26 specs: the contract
    // rows' union IS the 28-name set, each named exactly once.
    let mut mapped: Vec<&str> = RELATION_QUERY_SPECS
        .iter()
        .flat_map(|s| s.contract_rows.iter().copied())
        .collect();
    mapped.sort_unstable();
    let mut expected: Vec<&str> = EXPECTED_CONTRACT_ROWS.to_vec();
    expected.sort_unstable();
    assert_eq!(
        mapped, expected,
        "the contract-row mapping covers exactly the 28 projection contracts"
    );

    // The two collapses are EXPLICIT: exactly two specs cover two contract
    // rows each — the `never`→`string` pair (direct + via-generic) and the
    // `string|number`→`string` pair (distributive + tuple-wrapped).
    let two_row_specs: Vec<&super::oracle::query_specs::RelationQuerySpec> = RELATION_QUERY_SPECS
        .iter()
        .filter(|s| s.contract_rows.len() == 2)
        .collect();
    assert_eq!(two_row_specs.len(), 2, "exactly two collapse specs");
    let never = RELATION_QUERY_SPECS
        .iter()
        .find(|s| s.row_function == "relation_never_extends_string")
        .expect("the never collapse spec");
    assert_eq!(never.source_text, "never");
    assert_eq!(never.target_text, "string");
    assert_eq!(
        never.contract_rows,
        &[
            "relation_never_extends_string_directly_selects_true_branch",
            "relation_never_via_generic_helper_collapses_to_never",
        ]
    );
    let union = RELATION_QUERY_SPECS
        .iter()
        .find(|s| s.row_function == "relation_whole_union_not_assignable")
        .expect("the whole-union collapse spec");
    assert_eq!(union.source_text, "string | number");
    assert_eq!(union.target_text, "string");
    assert_eq!(
        union.contract_rows,
        &[
            "relation_distributive_conditional_over_union_emits_branch_union",
            "relation_tuple_wrapped_conditional_over_union_does_not_distribute",
        ]
    );

    // The known-mismatch ledger is exactly 9 rows: 6 `UnsupportedKey` pins
    // (exactly the binder-carrying infer rows — a direct inference target is
    // outside the engine's supported key) + 3 named `MismatchedVerdict` pins
    // with their source-proven engine answers.
    let pins: Vec<(&str, EngineObservationPin)> = RELATION_QUERY_SPECS
        .iter()
        .filter_map(|s| s.engine_pin.map(|p| (s.row_function, p)))
        .collect();
    assert_eq!(pins.len(), 9, "the ledger seats exactly 9 rows");
    let unsupported: Vec<&str> = RELATION_QUERY_SPECS
        .iter()
        .filter(|s| s.engine_pin == Some(EngineObservationPin::UnsupportedKey))
        .map(|s| s.row_function)
        .collect();
    let binder_rows: Vec<&str> = RELATION_QUERY_SPECS
        .iter()
        .filter(|s| !s.binder_layout.is_empty())
        .map(|s| s.row_function)
        .collect();
    assert_eq!(unsupported.len(), 6);
    assert_eq!(
        unsupported, binder_rows,
        "the UnsupportedKey pins are exactly the 6 infer rows"
    );
    for (row, pinned) in [
        (
            "relation_optional_to_required",
            RelationEngineVerdict::Assignable,
        ),
        (
            "relation_readonly_to_mutable",
            RelationEngineVerdict::NotAssignable,
        ),
        (
            "relation_fixed_to_first_rest",
            RelationEngineVerdict::NotAssignable,
        ),
    ] {
        let spec = RELATION_QUERY_SPECS
            .iter()
            .find(|s| s.row_function == row)
            .unwrap_or_else(|| panic!("registry seats {row}"));
        assert_eq!(
            spec.engine_pin,
            Some(EngineObservationPin::MismatchedVerdict(pinned)),
            "{row}: the pinned engine answer is the source-proven mismatch"
        );
    }
}

#[test]
fn relation_registry_derives_26_distinct_snapshot_ids() {
    // Every spec derives its v4 identity (the shared tsgo-free derivation —
    // proving every operand canonicalizes and every binder layout set-matches
    // its target operand's reserved refs), and the 26 derived snapshot_ids are
    // ALL DISTINCT (the two collapse pairs genuinely collapsed: no two specs
    // share a raw relation identity).
    let env = super::oracle::driver::pinned_env();
    let mut ids: Vec<String> = Vec::new();
    for spec in RELATION_QUERY_SPECS {
        let id = relation_probe::relation_identity_from_spec(spec).unwrap_or_else(|e| {
            panic!(
                "{}: the registry spec must derive its identity: {e:?}",
                spec.row_function
            )
        });
        ids.push(identity::derive_relation_snapshot_id(&id, &env));
    }
    let mut sorted = ids.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        ids.len(),
        "the 26 specs derive 26 DISTINCT snapshot_ids"
    );
}

#[test]
fn relation_registry_well_formed_is_discriminating() {
    use super::oracle::query_specs::{HostSetupKindSpec, RelationBinderSpec, RelationQuerySpec};

    fn rel_spec(
        row_function: &'static str,
        binder_layout: &'static [RelationBinderSpec],
        contract_rows: &'static [&'static str],
        engine_pin: Option<EngineObservationPin>,
    ) -> RelationQuerySpec {
        RelationQuerySpec {
            row_file: "relation_verdict_oracle.rs",
            row_function,
            query_ordinal: 0,
            oracle_family: "relation_verdict",
            host_project: super::oracle::query_specs::HostProjectSpec {
                project_root: "/",
                workspace_root: "/",
                tsconfig_path: "/oracle.tsconfig.json",
                host_setup_kind: HostSetupKindSpec::Standalone,
            },
            source_text: "string",
            target_text: "number",
            binder_layout,
            contract_rows,
            engine_pin,
        }
    }

    // A duplicate (row_file, row_function, query_ordinal) key rejects.
    let dup = [
        rel_spec("row_a", &[], &["c1"], None),
        rel_spec("row_a", &[], &["c2"], None),
    ];
    assert!(matches!(
        relation_registry_well_formed(&dup),
        Err(RelationRegistryError::DuplicateKey { .. })
    ));

    // A binder layout with a broken ordinal sequence rejects.
    let bad_ord = [rel_spec(
        "row_b",
        &[RelationBinderSpec {
            ordinal: 7,
            name: "A",
            constraint: None,
        }],
        &["c1"],
        Some(EngineObservationPin::UnsupportedKey),
    )];
    assert!(matches!(
        relation_registry_well_formed(&bad_ord),
        Err(RelationRegistryError::BinderOrdinalSequence { .. })
    ));

    // A duplicate binder name rejects.
    let dup_name = [rel_spec(
        "row_c",
        &[
            RelationBinderSpec {
                ordinal: 0,
                name: "A",
                constraint: None,
            },
            RelationBinderSpec {
                ordinal: 1,
                name: "A",
                constraint: None,
            },
        ],
        &["c1"],
        Some(EngineObservationPin::UnsupportedKey),
    )];
    assert!(matches!(
        relation_registry_well_formed(&dup_name),
        Err(RelationRegistryError::DuplicateBinderName { .. })
    ));

    // An UnsupportedKey pin on a binder-free spec rejects (only an inference
    // context is an unsupported key).
    let bad_pin = [rel_spec(
        "row_d",
        &[],
        &["c1"],
        Some(EngineObservationPin::UnsupportedKey),
    )];
    assert!(matches!(
        relation_registry_well_formed(&bad_pin),
        Err(RelationRegistryError::PinShape { .. })
    ));

    // A MismatchedVerdict pin on a binder-carrying spec rejects (an inference
    // key has no engine verdict to pin).
    let bad_pin2 = [rel_spec(
        "row_e",
        &[RelationBinderSpec {
            ordinal: 0,
            name: "A",
            constraint: None,
        }],
        &["c1"],
        Some(EngineObservationPin::MismatchedVerdict(
            RelationEngineVerdict::Assignable,
        )),
    )];
    assert!(matches!(
        relation_registry_well_formed(&bad_pin2),
        Err(RelationRegistryError::PinShape { .. })
    ));

    // A contract row named by two specs rejects.
    let dup_contract = [
        rel_spec("row_f", &[], &["shared_contract"], None),
        rel_spec("row_g", &[], &["shared_contract"], None),
    ];
    assert!(matches!(
        relation_registry_well_formed(&dup_contract),
        Err(RelationRegistryError::DuplicateContractRow { .. })
    ));

    // An empty contract-row mapping rejects.
    let empty_contract = [rel_spec("row_h", &[], &[], None)];
    assert!(matches!(
        relation_registry_well_formed(&empty_contract),
        Err(RelationRegistryError::EmptyField { .. })
    ));
}
