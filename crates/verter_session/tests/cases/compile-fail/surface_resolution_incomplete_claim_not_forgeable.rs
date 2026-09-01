//! Compile-fail fixture: the reason-bearing incomplete claim of a
//! resolution-to-surface outcome cannot be forged by struct literal — its
//! fields are module-private, so an `Incomplete` arm can never exist
//! without a producer-recorded reason, and the recorded reason can never be
//! destructured away. If this compiled, a caller could assemble a
//! reason-shaped claim (or strip one) outside the producer boundary;
//! trybuild would turn red on this fixture.

use verter_session::semantic_query::PartialReasonSet;
use verter_session::typeinfo::surface_resolution::IncompleteSurface;

fn main() {
    let _forged: IncompleteSurface<Vec<u8>> = IncompleteSurface {
        reason: PartialReasonSet::empty(),
        partial: None,
    };
}
