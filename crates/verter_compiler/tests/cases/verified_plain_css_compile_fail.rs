//! Structural negatives for the plain-CSS witness.
//!
//! One test per fixture, named for the SUBJECT each one asserts, because the
//! test name is what a failing summary shows: a name covering two subjects
//! cannot say which of them regressed.

/// The arbitrary-event IR sink is not reachable from the syntax crate's root,
/// so a caller cannot mint a natively-tagged IR without routing bytes through
/// the grammar. Subject: crate-root reachability (`E0432`) — NOT the sink's own
/// item visibility, which is a separate closure.
#[test]
fn style_syntax_ir_sink_is_unreachable_from_the_crate_root() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/cases/compile-fail/verified_plain_css_sink_unreachable.rs");
}
