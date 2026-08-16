//! Compile-fail proof for `SegmentedOverwriteAuthority`'s call-site guard.
//!
//! `SegmentedOverwriteAuthority::new()` is `pub(in crate::template::code_gen)`
//! — visible to the Vue VDOM/Vapor/SSR emitters and their shared plumbing,
//! invisible to every other module in the crate (`ide`, `svelte`, `script`,
//! `compile`, ...) and to every external consumer. `CodeGenOutput::
//! overwrite_segmented` now REQUIRES a `SegmentedOverwriteAuthority` value
//! as a parameter, so reaching it needs a token, and getting a token needs
//! this exact restricted path.
//!
//! `trybuild` spawns a full `cargo build` of a genuinely separate fixture
//! crate, so this only proves the item is unreachable from OUTSIDE
//! `verter_compiler` — a strictly external vantage point. It cannot, by
//! construction, directly re-prove the narrower intra-crate claim ("`ide`/
//! `svelte`, which sit inside this same crate, are excluded too"): any
//! external harness (`trybuild`, a `compile_fail` doctest) is itself an
//! external consumer, so `pub(in crate::template::code_gen)` and the
//! coarser `pub(crate)` are indistinguishable from that vantage point —
//! weakening the constructor to `pub(crate)` (which WOULD let `ide`/
//! `svelte` reach it, defeating the guard) would not change this test's
//! verdict. That narrower claim is held by Rust's own privacy checker
//! (`E0603`/`E0624`), which runs unconditionally on every `cargo build`/
//! `cargo check` of this crate — not opt-in, not skippable, and unable to
//! silently rot the way a test someone forgets to run could.
//!
//! What this test adds beyond that standing guarantee: an INDEPENDENT,
//! durable, executable proof — reusable via the canonical gate — that
//! `SegmentedOverwriteAuthority` is unreachable from outside the crate, run
//! against the actual built artifact rather than trusted from a doc
//! comment.
//!
//! **Which specific wall the pinned `.stderr` proves depends on how this
//! test is invoked**, because `trybuild` determines its OWN nested probe
//! crate's features by walking to a `.fingerprint/` directory NEXT TO the
//! currently-running test binary's own path (`trybuild::features::find` —
//! it reads the `features` field Cargo recorded for that exact binary's
//! build). Under a live `cargo test -p verter_compiler --features bench
//! --test main segmented_overwrite_compile_fail`, that walk succeeds:
//! `template` (and `code_gen`/`types`) end up `pub` at the module level, the
//! fixture reaches `SegmentedOverwriteAuthority::new()` itself, and the
//! pinned error is the item's own `pub(in crate::template::code_gen)`
//! restriction (`E0624`). Under the CANONICAL GATE (`cargo nextest
//! archive` → extract → run as a bare executable), the running binary has
//! no `.fingerprint/` sibling at all — nextest archives bundle binaries and
//! their runtime dependencies, not Cargo's incremental-build bookkeeping —
//! so the walk fails, trybuild falls back to the probe crate's DEFAULT
//! (non-`bench`) features, `template` stays `pub(crate)`, and the fixture
//! hits that coarser module-privacy wall FIRST (`E0603`). Both walls are
//! real, both hold `SegmentedOverwriteAuthority` unreachable from outside
//! the crate; the pinned `.stderr` here matches the canonical gate's own
//! (archived) execution, since that is the mandatory verification path.
#[test]
#[cfg_attr(
    not(feature = "bench"),
    ignore = "run with --features bench — this crate's own resolved features gate whether the \
              probe runs at all; the pinned .stderr matches the canonical (archived) gate \
              regardless (see this module's own doc comment)"
)]
fn segmented_overwrite_authority_is_unreachable_outside_the_crate() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/cases/compile-fail/segmented_overwrite_authority_unreachable.rs");
}
