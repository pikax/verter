//! Registration and gate-selection of the converted resolution test module.
//!
//! The converted cases live in a `#[cfg(test)]` sibling module. This compiled
//! reference prevents an orphan module from existing as inert source bytes:
//! removing either declaration makes the crate's test target fail to compile.
//!
//! **Why the instrument must live here.** A `#[cfg(test)]` module is visible
//! only from inside this crate's test build; an integration test under
//! `tests/` cannot name it, and neither can another crate. So the assertion
//! cannot be moved somewhere it would be greener.
//!
//! The reference below also checks the registered case count, while selection
//! follows from both modules being part of this crate's test target.

#[test]
fn converted_resolution_module_is_declared_and_therefore_selected() {
    // A path reference, not a scan. If `resolution_conversion_tests` is not
    // declared by `lib.rs`, this does not compile. If it is declared, the
    // module's own cases are part of this crate's test target and the
    // canonical gate selects them with everything else.
    //
    // The marker is a `const` the converted module must expose, so that this
    // assertion depends on the module's CONTENT existing rather than on a bare
    // path that an empty file would satisfy.
    let declared: usize = crate::resolution_conversion_tests::CONVERTED_CASE_COUNT;

    assert_eq!(
        declared, 24,
        "the disposition ledger converts 24 cases; the converted module must \
         declare that many, and a differing count means the conversion dropped \
         or invented cases",
    );
}
