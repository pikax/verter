//! Compile-fail proof for `PendingNavRequest`'s Vapor-private visibility.
//!
//! `PendingNavRequest` is `pub(in crate::template::code_gen::vapor)` —
//! visible only within the Vapor backend (`vapor/**`), invisible to every
//! other module in the crate (`ide`, `svelte`, `script`, `compile`, the
//! shared `template::code_gen::types` itself, ...) and to every external
//! consumer. `VaporElementState::pending_nav_requests` holds the opaque
//! `PendingNavQueue` wrapper instead, so no sibling of `vapor` can name
//! `PendingNavRequest` even through that field.
//!
//! Same structural limitation as `segmented_overwrite_compile_fail.rs`
//! (see its own doc comment): `trybuild` spawns a genuinely separate crate
//! build, so this only proves external unreachability. It cannot, by
//! construction, re-prove the narrower intra-crate claim ("a sibling of
//! `vapor` inside this same crate is excluded too") — any external harness
//! is itself an external consumer, indistinguishable here from a coarser
//! `pub(crate)`. That narrower claim is held by Rust's own privacy checker
//! (`E0603`-class), which runs unconditionally on every `cargo build`/
//! `cargo check` of this crate — independently confirmed live during this
//! change by a scratch reference from `template::code_gen::types` (a
//! `vapor` sibling), which failed with exactly `E0603: enum import
//! `PendingNavRequest` is private`.
//!
//! **Which specific wall the pinned `.stderr` proves depends on how this
//! test is invoked.** `trybuild` determines its OWN nested probe crate's
//! features by walking to a `.fingerprint/` directory NEXT TO the
//! currently-running test binary's own path
//! (`trybuild::features::find` — it reads the `features` field Cargo
//! recorded for that exact binary's build). Under a live `cargo test -p
//! verter_compiler --features bench --test main
//! pending_nav_request_compile_fail`, that walk succeeds: `template` ends
//! up `pub` at the module level, the fixture reaches `PendingNavRequest`
//! itself, and the pinned error would be its own `pub(in
//! crate::template::code_gen::vapor)` restriction. Under the CANONICAL GATE
//! (`cargo nextest archive` → extract → run as a bare executable), the
//! running binary has no `.fingerprint/` sibling at all — nextest archives
//! bundle binaries and their runtime dependencies, not Cargo's
//! incremental-build bookkeeping — so the walk fails, trybuild falls back
//! to the probe crate's DEFAULT (non-`bench`) features, `template` stays
//! `pub(crate)`, and the fixture hits that coarser module-privacy wall
//! FIRST. Both walls are real, both hold `PendingNavRequest` unreachable
//! from outside the crate; the pinned `.stderr` here matches the canonical
//! gate's own (archived) execution, since that is the mandatory
//! verification path.
#[test]
#[cfg_attr(
    not(feature = "bench"),
    ignore = "run with --features bench — this crate's own resolved features gate whether the \
              probe runs at all; the pinned .stderr matches the canonical (archived) gate \
              regardless (see this module's own doc comment)"
)]
fn pending_nav_request_is_unreachable_outside_vapor() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/cases/compile-fail/pending_nav_request_unreachable.rs");
}
