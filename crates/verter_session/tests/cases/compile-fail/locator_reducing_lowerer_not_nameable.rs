//! Compile-FAIL fixture: the reducing lowering entry
//! (`ProjectSemanticDispatch::shallow_lower_type_expr_with_context`) is
//! crate-private to the engine, so an external compilation unit cannot call
//! it to hand it a `LocatorShapeCtx` (or anything else). Together with the
//! no-PRC-conversion fixture this pins the sealed-context split: the locator
//! path cannot reach the reducing lowerer.

use verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch;
use verter_type_engine::resolver_core::ResolverCapabilities;

fn reach<C: ResolverCapabilities>(dispatch: &ProjectSemanticDispatch<'_, C>) {
    let _ = dispatch.shallow_lower_type_expr_with_context(
        todo!(),
        todo!(),
        todo!(),
        todo!(),
        todo!(),
        todo!(),
        todo!(),
        todo!(),
    );
}

fn main() {}
