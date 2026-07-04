//! Compile-FAIL fixture: even a consumer that has OBTAINED a sealed
//! `Instantiate` key — e.g. through the `test-support` production-shaped
//! `instantiate_key_for_tests` helper — cannot extract or transplant the raw
//! `InstantiateContext` out of the opaque `SemanticQueryKey::Instantiate`
//! payload.
//!
//! The key's public accessors expose only SAFE facts; the `InstantiateContext`
//! itself, the `args` slice, and the `InstantiateBodySource` axis are
//! `pub(crate)` reveal accessors (E0624 from outside the crate), and the
//! `InstantiateKey` fields are private (E0451). So there is no external path
//! from the opaque key to a raw `InstantiateContext` that could be transplanted
//! onto a foreign base.

use std::sync::Arc;
use verter_session::semantic_query::{
    InstantiateContext, InstantiateKey, ProjectionMode, ProjectionReductionContext,
    ResolvedDeclSlotIdentity, SemanticNodeId, SemanticQueryKey,
};
use verter_session::{HostConfig, VerterHost};

fn main() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let base = ResolvedDeclSlotIdentity::type_slot_unscoped(Arc::from("/x.ts"), Arc::from("Foo"));
    let args: Arc<[SemanticNodeId]> = Arc::from(Vec::new().into_boxed_slice());
    let prc = ProjectionReductionContext::published(ProjectionMode::Expanded);

    // The `test-support` helper is the ONLY external entry — and it hands back
    // an OPAQUE `SemanticQueryKey`, never a raw context.
    let key = verter_session::for_tests::instantiate_key_for_tests(&host, base, args, prc);

    // ...but the raw `InstantiateContext` cannot be extracted from it:
    if let SemanticQueryKey::Instantiate(k) = key {
        // `pub(crate)` reveal accessors — not callable outside the crate.
        let _ctx: InstantiateContext = k.context();
        let _source = k.body_source();
        let _args = k.args();
        // Private fields — not destructurable.
        let InstantiateKey { context, .. } = k;
        let _ = context;
    }
}
