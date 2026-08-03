#[test]
fn registered_authority_capabilities_are_not_mintable_outside_their_authorities() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile-fail/registered_snapshot_id_bytes_api.rs");
    t.compile_fail("tests/compile-fail/registered_snapshot_outside_mint.rs");
    t.compile_fail("tests/compile-fail/authority_namespace_outside_mint.rs");
    t.compile_fail("tests/compile-fail/canonical_grammar_outside_mint.rs");
    t.compile_fail("tests/compile-fail/accepted_source_outside_mint.rs");
}

#[test]
fn deserialization_cannot_mint_live_authority_capabilities() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile-fail/deserialize_live_capabilities.rs");
}
