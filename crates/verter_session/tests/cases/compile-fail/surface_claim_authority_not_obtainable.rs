//! Compile-fail fixture: a crate that depends on the engine and the host
//! cannot obtain the surface-claim authority of a live host, nor build one
//! out of nothing.
//!
//! 1. the host keeps its engine's authority private: neither the project store
//!    nor the request attachment hands it out across the crate boundary;
//! 2. the engine's one mint is crate-private (its only caller is
//!    `EngineStores::create`, which builds a NEW engine);
//! 3. the authority has no `Default` and no `Clone`, so a borrow never yields
//!    an owned duplicate;
//! 4. its field is private, so it cannot be constructed by tuple literal.
//!
//! If any of these compiled, any crate could state a complete
//! surface-resolution claim — including an empty success — that no producer
//! ever resolved; trybuild would turn red on this fixture.

use verter_type_engine::semantic_query::surface_resolution::SurfaceClaimAuthority;

fn from_live_host(host: &verter_session::VerterHost) {
    let _ = host.project_type_store().surface_claims();
}

fn main() {
    let _ = from_live_host;
    let _minted = SurfaceClaimAuthority::mint();
    let _defaulted: SurfaceClaimAuthority = Default::default();
    let _duplicated = |authority: &SurfaceClaimAuthority| -> SurfaceClaimAuthority {
        <SurfaceClaimAuthority as Clone>::clone(authority)
    };
    let _forged = SurfaceClaimAuthority(());
}
