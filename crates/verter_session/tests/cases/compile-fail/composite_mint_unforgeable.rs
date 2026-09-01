//! Compile-fail fixture: the opaque union/intersection composite payload
//! is unforgeable from OUTSIDE `verter_session` — the disclosed language
//! limit stated exactly. Rust visibility makes the mints unforgeable
//! cross-crate (this fixture proves it); IN-crate the forcing function is
//! the exhaustive carrier-category match, not the type system, so
//! in-crate unforgeability is NOT claimed.
//!
//! An untrusted producer cannot:
//! 1. assemble a `CompositeList` by struct literal (private field);
//! 2. call the category-funnel mint (`minted` is `pub(crate)`);
//! 3. mint a DERIVED composite — `CanonicalMint` lives in the
//!    `pub(crate)` dispatch module (unreachable from here), and its
//!    constructor is private to the canonical-algebra module besides;
//! 4. mint a BYPASS — every bypass category helper is `pub(crate)` and
//!    the `CompositeCarrierCategory` registry itself is `pub(crate)`,
//!    so no category value is even nameable here.
//!
//! Reading stays open (`Deref` to `[SemanticNodeId]`); only CONSTRUCTION
//! is confined. If any mint surface were widened to `pub`, the matching
//! arm below would COMPILE and trybuild would turn red.

use std::sync::Arc;
use verter_session::semantic_query::composite::CompositeList;
use verter_session::semantic_query::SemanticNodeId;

fn main() {
    let members: Arc<[SemanticNodeId]> = Arc::from([]);
    // (1) The struct-literal leg lives in its own fixture
    // (`composite_struct_literal_forge.rs`): rustc suppresses the E0451
    // when it coexists with the E0624/E0603 legs below, so a combined
    // fixture would leave that leg non-discriminating.
    // (2) The category-funnel mint is `pub(crate)`.
    let _ = CompositeList::minted(Arc::clone(&members), todo!());
    // (3) The canonical-mint witness module is not even reachable —
    // `project_semantic_dispatch` (home of `CanonicalMint`) is
    // `pub(crate)`, so a derived composite cannot be minted from outside.
    let _ = verter_session::project_semantic_dispatch::canonical_algebra::CanonicalMint {};
    // (4) Neither can any bypass category be minted.
    let _ = CompositeList::authored_shell(Arc::clone(&members));
    let _ = CompositeList::ordered_carrier(Arc::clone(&members));
    let _ = CompositeList::preserving_rebuild(Arc::clone(&members));
    let _ = CompositeList::query_subject(Arc::clone(&members));
    // (5) The category registry itself is not nameable.
    let _ = verter_session::semantic_query::composite::CompositeCarrierCategory::Canonical(todo!());
}
