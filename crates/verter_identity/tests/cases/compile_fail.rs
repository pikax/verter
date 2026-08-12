//! Compile-fail proof that the identity types this crate lands are
//! non-interchangeable, per `identity-encoding.md` §5 ("stable ID versus
//! session handle misuse compile tests") and architecture.md §3.1
//! ("Identity types are non-interchangeable").
//!
//! `trybuild` spawns a full `cargo build` per fixture; each fixture must
//! fail to compile with a type-mismatch error (`E0308`), never for an
//! unrelated reason (an unrelated failure — a missing import, a typo —
//! would make this a vacuously-passing test, exactly the stub pattern
//! `CLAUDE.md`'s Stub Prevention section forbids).

#[test]
fn identity_types_reject_cross_type_misuse() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile-fail/session_handle_is_not_stable_entity_id.rs");
    t.compile_fail("tests/compile-fail/stable_entity_id_is_not_session_handle.rs");
    t.compile_fail("tests/compile-fail/query_identity_is_not_semantic_flight_key.rs");
    t.compile_fail("tests/compile-fail/input_basis_id_is_not_query_identity.rs");
}
