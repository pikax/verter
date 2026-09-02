//! Compile-fail fixture: the composite payload's fields are PRIVATE, so an
//! untrusted producer cannot assemble a `CompositeList` (or its kind-erased
//! `CompositeMembers` core) by struct literal — the zero-mint forge. Kept
//! separate from `composite_mint_unforgeable.rs` because rustc suppresses
//! this E0451 when it coexists with that fixture's E0624/E0603 legs. If
//! the fields were widened to `pub`, this fixture would COMPILE and
//! trybuild would turn red.

use std::sync::Arc;
use verter_session::semantic_query::composite::{CompositeList, CompositeMembers, UnionKind};
use verter_session::semantic_query::SemanticNodeId;

fn forge_core(members: Arc<[SemanticNodeId]>) -> CompositeMembers {
    CompositeMembers { members }
}

fn forge_list(core: CompositeMembers) -> CompositeList<UnionKind> {
    CompositeList { core }
}

fn main() {}
