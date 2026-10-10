//! Compile-fail fixture: a root scope guard is bound to the thread whose
//! interns it roots. Moving the guard to another thread would pop another
//! thread's scope stack; work fanned out to another thread enters the scope
//! there through its `RootScopeHandle` instead.

use verter_type_engine::semantic_query_memo::{SemanticGraphStore, SemanticRootScope};

fn require_send<T: Send>(_: T) {}

fn main() {
    let store = SemanticGraphStore::default();
    let scope: SemanticRootScope = store.enter_root_scope();
    require_send(scope);
}
