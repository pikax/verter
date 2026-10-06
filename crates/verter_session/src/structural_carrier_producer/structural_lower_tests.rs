//! Unit tests of the query-free structural lowerer's own scaffolding: the
//! binder stack and the typed lowering error.

use std::sync::Arc;

use super::{BinderScope, StructuralLowerContext, StructuralLowerError};
use crate::semantic_query::SemanticNodeId;

#[test]
fn binder_scope_resolves_innermost_shadowing_name() {
    // An inner frame binding the same name shadows the outer one; an
    // unbound name misses (the caller then emits a `BareRef`).
    let mut outer = BinderScope::default();
    outer.bind(Arc::from("T"), SemanticNodeId(1));
    outer.bind(Arc::from("U"), SemanticNodeId(2));
    let mut inner = BinderScope::default();
    inner.bind(Arc::from("T"), SemanticNodeId(9));
    let stack = [outer, inner];
    let ctx = StructuralLowerContext::new(&stack);

    // Innermost `T` wins over the outer `T`.
    assert_eq!(ctx.lookup_binder("T"), Some(SemanticNodeId(9)));
    // `U` is only in the outer frame, still visible.
    assert_eq!(ctx.lookup_binder("U"), Some(SemanticNodeId(2)));
    // An unbound name misses.
    assert_eq!(ctx.lookup_binder("Missing"), None);
}

#[test]
fn structural_lower_error_is_a_typed_variant() {
    // The error is a real typed variant carrying a diagnostic shape name,
    // never an `Unknown`-as-control-flow signal.
    let err = StructuralLowerError::UnsupportedWithoutResolution {
        shape: "RecursiveRef",
    };
    assert_eq!(
        err,
        StructuralLowerError::UnsupportedWithoutResolution {
            shape: "RecursiveRef"
        }
    );
}
