//! Compile-fail: identity types are non-interchangeable (`E0308`), and
//! `SourceUnitId` has no descriptor constructor (`E0599`). An unrelated
//! fixture failure (missing import, typo) would pass vacuously — each
//! fixture must fail on the named diagnostic.

#[test]
fn identity_types_reject_cross_type_misuse() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile-fail/session_handle_is_not_stable_entity_id.rs");
    t.compile_fail("tests/compile-fail/stable_entity_id_is_not_session_handle.rs");
    t.compile_fail("tests/compile-fail/query_identity_is_not_semantic_flight_key.rs");
    t.compile_fail("tests/compile-fail/input_basis_id_is_not_query_identity.rs");
    t.compile_fail("tests/compile-fail/direct_batch_id_is_not_direct_invocation_id.rs");
    t.compile_fail("tests/compile-fail/raw_u64_is_not_parse_instance_generation.rs");
    t.compile_fail("tests/compile-fail/source_unit_id_has_no_from_canonical.rs");
}
