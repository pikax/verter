//! Discriminating test: the `is_cacheable()` predicate on
//! `ReadSetSignature` distinguishes empty from overflow.
//!
//! Pre-multi-state-carrier `ReadSetSignature` had no `is_cacheable()`
//! method — emptiness and overflow were structurally
//! indistinguishable at the consumer surface, so the warm-hit oracle
//! could not refuse an overflowed carrier without inspecting an
//! internal field. Post-fix `is_cacheable()` returns
//! `!self.overflowed`, making the two states distinguishable through
//! the public API.
//!
//! Discrimination: this test fails to compile against the carrier
//! shape that pre-dates `is_cacheable()` (the method does not
//! exist); against the post-change tree it builds and asserts both
//! the cacheability bit and that emptiness alone is NOT a
//! non-cacheable condition.

use verter_session::ReadSetSignature;

/// Discriminator: `ReadSetSignature::empty()` is cacheable while
/// `ReadSetSignature::overflow()` is not. The two states must be
/// structurally distinguishable at the carrier type — that is the
/// architectural bit the pre-fix `Arc<[FactVersionRef]>` carrier
/// lacked.
#[test]
fn empty_and_overflow_are_distinguishable_at_carrier_type() {
    let empty = ReadSetSignature::empty();
    let overflow = ReadSetSignature::overflow();

    // Cacheability: emptiness alone is NOT non-cacheable. Only the
    // overflowed bit is.
    assert!(
        empty.is_cacheable(),
        "ReadSetSignature::empty().is_cacheable() MUST be true — an empty fact \
         rail validates vacuously on warm hits and is a perfectly valid admitted \
         carrier (no observed cross-file facts). The pre-fix carrier had no \
         is_cacheable() at all because emptiness and overflow were not \
         distinguishable at the type."
    );
    assert!(
        !overflow.is_cacheable(),
        "ReadSetSignature::overflow().is_cacheable() MUST be false — an \
         overflowed tracer means the path-precise signature is too large to \
         admit safely. The pre-fix carrier could not represent this distinction."
    );

    // Overflow bit: read it the other way too.
    assert!(
        !empty.is_overflow(),
        "an empty signature is not an overflowed signature; the two states are \
         distinct architecturally."
    );
    assert!(
        overflow.is_overflow(),
        "an overflowed signature MUST report is_overflow() == true."
    );

    // Fact rail: both carry an empty rail (overflow carries no
    // partial facts), but only `overflow` flips the bit.
    assert!(empty.facts.is_empty(), "empty carrier has no facts");
    assert!(overflow.facts.is_empty(), "overflow carrier has no facts");
}
