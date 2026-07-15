//! Compile-FAIL fixture: the sealed `Instantiate` cache-family key cannot be
//! forged from outside `verter_session`. Every seal asserted here is
//! UNCONDITIONAL — none is gated behind an opt-in feature — so it holds in
//! every build profile (the deleted ctors, the `pub(crate)` axis enum, the
//! private payload fields, and the tuple variant are all unconditional).
//!
//! `SemanticQueryKey::Instantiate` carries the opaque `InstantiateKey`
//! (private fields) whose `InstantiateContext` embeds the sealed, `pub(crate)`
//! `InstantiateBodySource` source-kind axis. An external crate therefore
//! cannot:
//!
//! 1. call the DELETED raw test constructors
//!    `InstantiateContext::{non_file,file_backed}_for_tests` (E0599);
//! 2. name the `pub(crate)` `InstantiateBodySource::NonFile` variant (E0603);
//! 3. struct-literal `InstantiateContext` / `InstantiateKey` (private fields,
//!    E0451);
//! 4. use struct-variant syntax on the `Instantiate` TUPLE variant (E0559 /
//!    E0769).
//!
//! If any of these seals regressed (a ctor widened to `pub`, the enum widened,
//! the fields exposed, or the variant reverted to a struct variant), the
//! corresponding line would COMPILE and trybuild would fail this fixture.

use std::sync::Arc;
use verter_session::semantic_query::{
    InstantiateContext, InstantiateKey, ProjectionMode, ProjectionReductionContext,
    ResolvedDeclSlotIdentity, SemanticNodeId, SemanticQueryKey,
};

fn main() {
    let prc = ProjectionReductionContext::published(ProjectionMode::Expanded);
    let base = ResolvedDeclSlotIdentity::type_slot_unscoped(Arc::from("/x.ts"), Arc::from("Foo"));
    let args: Arc<[SemanticNodeId]> = Arc::from(Vec::new().into_boxed_slice());

    // 1. The raw test constructors are DELETED — no such associated function.
    let _deleted_non_file = InstantiateContext::non_file_for_tests(prc, Default::default());
    let _deleted_file_backed =
        InstantiateContext::file_backed_for_tests(prc, Default::default(), Default::default());

    // 2. The source-kind axis is a `pub(crate)` enum — its variants are not
    //    nameable outside the crate.
    let _forged_source = verter_session::semantic_query::InstantiateBodySource::NonFile;

    // 3. `InstantiateContext` has private fields — not struct-literal-able.
    let _forged_context = InstantiateContext {
        projection_reduction: prc,
        resolve_env_hash: Default::default(),
    };

    // 4. `InstantiateKey` has private fields — not struct-literal-able. The
    //    provided fields carry their CORRECT types (so no incidental type
    //    mismatch), and the un-constructible `context` field is omitted — the
    //    SOLE compile error is the private-field seal.
    let _forged_key = InstantiateKey {
        base: base.clone(),
        args: args.clone(),
    };

    // 5. `SemanticQueryKey::Instantiate` is a TUPLE variant carrying the opaque
    //    key — the old struct-variant syntax no longer type-checks.
    let _forged_variant = SemanticQueryKey::Instantiate {
        base,
        args,
        context: prc,
    };
}
