//! Compile-fail fixture: a resolution-to-surface producer cannot spell
//! "empty and fine" through the outcome's surface:
//!
//! 1. the outcome has no `Default` — there is no reasonless value to hand
//!    back when a resolution fails;
//! 2. the `unwrap_or_default` spelling — the exact form the migrated drop
//!    sites used to convert a failed resolution into an empty success —
//!    does not exist on the outcome;
//! 3. even the reason-taking constructor is producer-sealed (`pub(crate)`),
//!    so an out-of-crate caller cannot mint incomplete claims at all.
//!
//! The companion fixture
//! `surface_resolution_incomplete_claim_not_forgeable.rs` proves the
//! struct-literal forge separately (privacy errors are only reported when
//! type-checking succeeds, so the forge needs its own fixture). If any of
//! these compiled, a producer could once again report success while handing
//! back nothing; trybuild would turn red on this fixture.

use verter_session::typeinfo::surface_resolution::SurfaceResolution;

fn main() {
    let _defaulted: SurfaceResolution<Vec<u8>> = Default::default();
    let _escaped: Vec<u8> = SurfaceResolution::<Vec<u8>>::NoSurface.unwrap_or_default();
    let _minted: SurfaceResolution<Vec<u8>> = SurfaceResolution::incomplete(todo!());
}
