//! Compile-fail: identity types are non-interchangeable (`E0308`).
//! An unrelated fixture failure (missing import, typo) would pass
//! vacuously — each fixture must fail on the type mismatch.

#[test]
fn identity_types_reject_cross_type_misuse() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile-fail/session_handle_is_not_stable_entity_id.rs");
    t.compile_fail("tests/compile-fail/stable_entity_id_is_not_session_handle.rs");
    t.compile_fail("tests/compile-fail/query_identity_is_not_semantic_flight_key.rs");
    t.compile_fail("tests/compile-fail/input_basis_id_is_not_query_identity.rs");
}
