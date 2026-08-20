//! `assemble_sequence` cannot be called with a raw `{code, source_map}`
//! pair — the signature itself (`&[&ValidatedFragment]`) makes it a type
//! error, not merely a documented convention. See the fixture for the
//! full rationale.

#[test]
fn assemble_sequence_rejects_a_raw_code_map_pair_at_compile_time() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/cases/compile-fail/assemble_sequence_requires_validated_fragment.rs");
}
