//! Compile-fail fixture: the sealed `OutputProjector` capability is not
//! nameable — and so not implementable — from outside the `verter_session`
//! crate.
//!
//! `OutputProjector` (and its owning `project_semantic_dispatch` /
//! `output_materialization` modules) is `pub(crate)`, so an external crate
//! cannot even NAME the trait to write an `impl OutputProjector for X`. The
//! sealed-supertrait seal (`sealed::Sealed`, a PRIVATE `mod sealed` inside
//! `mod projector` — nameable only from within `projector`) is the SECOND
//! barrier, but the `pub(crate)` visibility alone already makes the
//! out-of-crate impl unwriteable — that visibility error IS the compile-fail.
//! (The in-crate sibling case — a `carrier`-side `impl
//! projector::sealed::Sealed for HotCap` — is independently `E0603` because
//! `mod sealed` is private; that compiler enforcement is documented at the
//! seal definition in `output_materialization.rs`.)
//!
//! This fixture lives outside the `verter_session` crate (compiled as a
//! trybuild integration test, seeing only the PUBLIC API) and attempts to name
//! the trait. The compile MUST FAIL.

struct ForeignSink;

// The path `verter_session::project_semantic_dispatch::output_materialization::OutputProjector`
// is unresolvable from an external crate (every segment is `pub(crate)`), so
// this `impl` cannot name the trait. trybuild captures the resolution/privacy
// error.
impl verter_session::project_semantic_dispatch::output_materialization::OutputProjector
    for ForeignSink
{
}

fn main() {}
