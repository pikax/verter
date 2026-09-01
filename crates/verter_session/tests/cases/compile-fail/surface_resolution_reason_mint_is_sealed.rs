//! Compile-fail fixture: the type-level non-empty reason and the success-arm
//! proof cannot be minted outside the producer boundary:
//!
//! 1. `NonEmptyReasons`' checked constructor is producer-sealed
//!    (`pub(crate)`), so an out-of-crate caller cannot bridge a
//!    possibly-empty set into the claim type;
//! 2. `NonEmptyReasons` has no `Default` — a reason-free incomplete claim
//!    has no spelling at all;
//! 3. `SurfaceProof` — the opaque evidence every success arm carries — has a
//!    private field, so the module's private finalizer cannot be
//!    impersonated.
//!
//! If any of these compiled, the non-emptiness of `Incomplete`'s reason (or
//! the finalizer-only mint of the success arms) would be convention rather
//! than type; trybuild would turn red on this fixture.

use verter_session::semantic_query::PartialReasonSet;
use verter_session::typeinfo::surface_resolution::{NonEmptyReasons, SurfaceProof};

fn main() {
    let _empty_reason: Option<NonEmptyReasons> = NonEmptyReasons::new(PartialReasonSet::empty());
    let _default_reason: NonEmptyReasons = Default::default();
    let _forged_proof: SurfaceProof = SurfaceProof(());
}
