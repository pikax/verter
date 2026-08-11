//! The disabled-arm proofs. Compiled ONLY when the `attribution`
//! feature is OFF — that is, in the configuration every production
//! build and the canonical gate use.
//!
//! Two things are proven here, and they are the two that a
//! measurement-only substrate has to get right:
//!
//! 1. the macros do not evaluate their amount/digest argument, so
//!    instrumenting a site with an expensive quantity is free when the
//!    feature is off; and
//! 2. the recording macros still accept every declared site name, so
//!    the schema and the call sites cannot drift apart behind a feature
//!    nobody enables.
//!
//! The absence of a READER is not proven here — it cannot be, because a
//! test that could observe the absence would be the reader. It is
//! proven by the compile-fail fixture driven from
//! `tests/cases/attribution_compile_fail.rs`, which asserts that
//! naming `attribution::snapshot()` fails to compile in this
//! configuration.

use std::cell::Cell;

use super::schema::{WorkDomain, WorkSite, WorkUnit};

thread_local! {
    static SIDE_EFFECTS: Cell<u32> = const { Cell::new(0) };
}

/// Returns a number AND records that it was called. If a disabled macro
/// evaluates its argument, this counter moves.
fn tattling_amount() -> usize {
    SIDE_EFFECTS.with(|cell| cell.set(cell.get() + 1));
    99
}

#[test]
fn disabled_macros_never_evaluate_their_argument() {
    SIDE_EFFECTS.with(|cell| cell.set(0));

    crate::attribute_n!(ContentHash, tattling_amount());
    crate::attribute_max!(QueueDepth, tattling_amount());
    crate::attribute_digest!(ComponentMetaDigest, tattling_amount());

    assert_eq!(
        SIDE_EFFECTS.with(|cell| cell.get()),
        0,
        "a disabled attribution macro evaluated its amount expression — \
         the OFF arm must expand to a site-name check and nothing else"
    );

    // Control: the tattler DOES move the counter when it is genuinely
    // called, so a zero above means "not evaluated", not "broken probe".
    let _ = tattling_amount();
    assert_eq!(SIDE_EFFECTS.with(|cell| cell.get()), 1);
}

#[test]
fn disabled_macros_still_accept_every_declared_site() {
    // A representative macro invocation per macro shape. The value of
    // this test is that it FAILS TO COMPILE if a site named here is
    // renamed or removed — which is the whole point of naming the site
    // in the disabled arm.
    crate::attribute!(TaskExecute);
    crate::attribute_n!(CarrierParse, 0usize);
    crate::attribute_max!(StoreRetainedBytes, 0u64);
    crate::attribute_digest!(CompiledOutputDigest, 0u64);
    {
        crate::attribute_scope!(IndexedReadyBuild);
    }
}

#[test]
fn schema_is_visible_without_the_feature() {
    // The schema compiles unconditionally so the disabled arm can name
    // sites; it carries no storage and no reader.
    assert_eq!(WorkSite::ContentHash.domain(), WorkDomain::Hashing);
    assert_eq!(WorkSite::ContentHash.unit(), WorkUnit::Bytes);
    assert_eq!(WorkSite::ALL.len(), WorkSite::COUNT);
}
