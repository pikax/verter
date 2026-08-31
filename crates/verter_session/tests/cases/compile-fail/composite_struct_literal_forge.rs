//! Compile-fail fixture: the composite payload's member list is a PRIVATE
//! field, so an untrusted producer cannot assemble a `CompositeList` by
//! struct literal — the zero-mint forge. Kept separate from
//! `composite_mint_unforgeable.rs` because rustc suppresses this E0451
//! when it coexists with that fixture's E0624/E0603 legs. If the field
//! were widened to `pub`, this fixture would COMPILE and trybuild would
//! turn red.

use std::sync::Arc;
use verter_session::semantic_query::composite::CompositeList;
use verter_session::semantic_query::SemanticNodeId;

fn main() {
    let members: Arc<[SemanticNodeId]> = Arc::from([]);
    let _ = CompositeList { members };
}
