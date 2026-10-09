//! Compile-fail fixture: a live engine's output authority cannot be reminted
//! from handles recovered from that engine.
//!
//! The only mint, `EngineStores::create`, builds a NEW set of engine stores;
//! it accepts no graph or store handle. Passing a live host's recovered graph
//! (or its retention account in place of a store account) does not compile,
//! so recovering a store never yields authority over the engine that owns it.

use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use verter_type_engine::project_semantic_dispatch::engine_resources::EngineStores;

fn remint(host: &verter_session::VerterHost) {
    let store = host.project_type_store();
    let live_graph = Arc::clone(store.semantic_graph());
    let live_account = Arc::clone(store.retention_account());
    let counter = Arc::new(AtomicU64::new(0));
    let _ = EngineStores::create(live_graph, live_account, &counter, Default::default());
}

fn main() {
    let _ = remint;
}
