//! Compile-FAIL fixture: the reducing lowering entry
//! (`ProjectSemanticDispatch::shallow_lower_type_expr_with_context`) lives on
//! a crate-private dispatch module, so an external compilation unit cannot
//! even NAME it to hand it a `LocatorShapeCtx` (or anything else). Together
//! with the no-PRC-conversion fixture this pins the sealed-context split:
//! the locator path cannot reach the reducing lowerer.

use verter_session::project_semantic_dispatch::ProjectSemanticDispatch;

fn main() {}
