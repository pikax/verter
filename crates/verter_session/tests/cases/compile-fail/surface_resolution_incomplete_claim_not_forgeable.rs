//! Compile-fail fixture: neither the reason-bearing incomplete claim nor the
//! proof-bearing success payload of a resolution-to-surface outcome can be
//! forged by struct literal:
//!
//! 1. `IncompleteSurface`'s fields are module-private, so an `Incomplete` arm
//!    can never exist without a producer-recorded reason, and the recorded
//!    reason can never be destructured away;
//! 2. `Witnessed` — the proof-bearing surface wrapper of `Resolved` /
//!    `OpenPresence` — has private fields, so a success payload cannot be
//!    assembled around the finalizer.
//!
//! Privacy errors are only reported when type-checking succeeds, so this
//! fixture holds ONLY the two struct-literal forges; the sibling fixtures
//! `surface_resolution_empty_success_unconstructible.rs` and
//! `surface_resolution_reason_mint_is_sealed.rs` pin the constructor seals.
//! If either literal compiled, a caller could assemble a reason-shaped claim
//! (or forge success evidence) outside the producer boundary; trybuild would
//! turn red on this fixture.

use verter_session::typeinfo::surface_resolution::{IncompleteSurface, Witnessed};

fn main() {
    let _forged: IncompleteSurface<Vec<u8>> = IncompleteSurface {
        reason: todo!(),
        partial: None,
    };
    let _forged_witness: Witnessed<Vec<u8>> = Witnessed {
        value: Vec::new(),
        _proof: todo!(),
    };
}
