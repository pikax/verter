//! Compile-fail fixture: a resolution-to-surface producer cannot spell
//! "empty and fine" through the outcome's surface, and no success arm can be
//! constructed outside the producer boundary at all:
//!
//! 1. the outcome has no `Default` — there is no reasonless value to hand
//!    back when a resolution fails;
//! 2. the `unwrap_or_default` spelling — the exact form the migrated drop
//!    sites used to convert a failed resolution into an empty success —
//!    does not exist on the outcome;
//! 3. the reason-taking constructor is producer-sealed (`pub(crate)`), so an
//!    out-of-crate caller cannot mint incomplete claims;
//! 4. `Resolved` carries its surface inside the proof-bearing `Witnessed`
//!    wrapper, so the raw empty-success `Resolved(TypeInfoSurface::empty())`
//!    is a TYPE error from any crate — the shape that used to compile;
//! 5. `NoSurface` demands a `SurfaceProof` minted only by the module's
//!    private finalizer, so the bare complete-negative claim cannot be
//!    constructed from outside either (the named `no_surface` mint is
//!    producer-sealed).
//!
//! The companion fixture
//! `surface_resolution_incomplete_claim_not_forgeable.rs` proves the
//! struct-literal forges separately (privacy errors are only reported when
//! type-checking succeeds, so the forge needs its own fixture). If any of
//! these compiled, a producer could once again report success while handing
//! back nothing; trybuild would turn red on this fixture.

use verter_session::typeinfo::surface::TypeInfoSurface;
use verter_session::typeinfo::surface_resolution::SurfaceResolution;

fn discharge(outcome: SurfaceResolution<Vec<u8>>) -> Vec<u8> {
    outcome.unwrap_or_default()
}

fn main() {
    let _defaulted: SurfaceResolution<Vec<u8>> = Default::default();
    let _minted: SurfaceResolution<Vec<u8>> = SurfaceResolution::incomplete(todo!());
    let _raw_empty_success: SurfaceResolution<TypeInfoSurface> =
        SurfaceResolution::Resolved(TypeInfoSurface::empty());
    let _sealed_resolved: SurfaceResolution<TypeInfoSurface> =
        SurfaceResolution::resolved(TypeInfoSurface::empty());
    let _sealed_no_surface: SurfaceResolution<TypeInfoSurface> = SurfaceResolution::no_surface();
}
