//! Shared exactness classification for component-meta surfaces.
//!
//! Both the slot-binding synthesis path and the `defineProps`
//! expansion path classify resolved values into
//! [`ExpansionExactness::ExactConcrete`] or
//! [`ExpansionExactness::ExactSymbolic`]. This module centralises the
//! predicate so both surfaces share identical semantics:
//!
//! - **Primitive / literal** values are `ExactConcrete`.
//! - **Function** shells are `ExactConcrete`.
//! - **Object** values are `ExactConcrete` only when every member's
//!   value is itself concrete (no open conditional, indexed-access,
//!   or type-parameter shells under the surface).
//! - **Anything else** (open conditionals, indexed-access shells,
//!   alias chains pointing at unresolved roots, type parameters,
//!   refs, key-of, mapped types, …) is `ExactSymbolic`.
//!
//! Two entry points expose the same semantics over the two
//! representations callers carry:
//!
//! - [`classify_node`] — graph-native, operates on a
//!   [`SemanticNodeId`]. Unwraps a single `Alias` hop before
//!   classifying so `type MyStr = string` resolves to
//!   `Primitive(String)` and publishes as concrete.
//! - [`classify_type_expr`] — `TypeExpr`-native, used by the props
//!   fast path after
//!   [`ComponentMetaQueryEngine::try_fast_expand_shallow_alias_body`]
//!   has already inlined a same-file alias body. The expanded
//!   [`TypeExpr`] is the authoritative shape; this helper classifies
//!   it without re-entering the dispatch.

use verter_semantic::analysis::type_expand::ExpansionExactness;
use verter_type_expr::{ObjectMember, TypeExpr};

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{SemanticNodeData, SemanticNodeId};

/// Compute the exactness flag for a synthesised binding's value node.
///
/// See module docs for the predicate. Aliases are unwrapped by one
/// hop before classification so `type MyStr = string` resolves to
/// `Primitive(String)` and publishes as concrete.
pub(crate) fn classify_node(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> ExpansionExactness {
    let unwrapped =
        match crate::project_semantic_dispatch::node_data_for(dispatch.ctx, node).as_deref() {
            Some(SemanticNodeData::Alias(target)) => *target,
            _ => node,
        };
    match crate::project_semantic_dispatch::node_data_for(dispatch.ctx, unwrapped).as_deref() {
        Some(SemanticNodeData::Primitive(_)) | Some(SemanticNodeData::Literal(_)) => {
            ExpansionExactness::ExactConcrete
        }
        Some(SemanticNodeData::Object(_)) if object_is_closed_node(dispatch, unwrapped) => {
            ExpansionExactness::ExactConcrete
        }
        Some(SemanticNodeData::Function { .. }) => ExpansionExactness::ExactConcrete,
        _ => ExpansionExactness::ExactSymbolic,
    }
}

/// Compute the exactness flag for a [`TypeExpr`] shape.
///
/// Mirrors [`classify_node`]'s predicate but operates on the
/// syntactic representation that the props fast path carries. The
/// caller is responsible for any alias-unwrap step before calling
/// this helper — typically [`ComponentMetaQueryEngine::try_fast_expand_shallow_alias_body`]
/// has already inlined the same-file alias body.
pub(crate) fn classify_type_expr(expr: &TypeExpr) -> ExpansionExactness {
    match strip_parens(expr) {
        TypeExpr::Primitive(_) | TypeExpr::Literal(_) => ExpansionExactness::ExactConcrete,
        // A bare constructor type (`new (...) => R`) is as concrete as its
        // sibling function type — both are fully-resolved shapes with no open
        // generic variables.
        TypeExpr::Function(_) | TypeExpr::ConstructorType(_) => ExpansionExactness::ExactConcrete,
        TypeExpr::Object(obj) if object_is_closed_expr(obj.properties.as_slice()) => {
            ExpansionExactness::ExactConcrete
        }
        _ => ExpansionExactness::ExactSymbolic,
    }
}

/// Returns `true` when `node` resolves to an `Object` whose members'
/// values are all concrete — i.e. none of them are
/// `InstantiationRef`, `IndexedAccess`, `Conditional`, or
/// `TypeParam`. Used by [`classify_node`] to distinguish closed
/// objects from open ones.
fn object_is_closed_node(dispatch: &ProjectSemanticDispatch<'_>, node: SemanticNodeId) -> bool {
    let Some(data) = crate::project_semantic_dispatch::node_data_for(dispatch.ctx, node) else {
        return false;
    };
    let view = match data.as_ref() {
        SemanticNodeData::Object(view) => view,
        _ => return false,
    };
    for member in view.members.iter() {
        let Some(member_data) =
            crate::project_semantic_dispatch::node_data_for(dispatch.ctx, member.value)
        else {
            return false;
        };
        match member_data.as_ref() {
            SemanticNodeData::InstantiationRef { .. }
            | SemanticNodeData::IndexedAccess { .. }
            | SemanticNodeData::Conditional { .. }
            | SemanticNodeData::TypeParam { .. } => return false,
            _ => continue,
        }
    }
    true
}

/// `TypeExpr`-native counterpart of [`object_is_closed_node`]. An
/// object is "closed" when every property's value is itself classified
/// as concrete — no `Ref`, `IndexedAccess`, `Conditional`, `KeyOf`,
/// `Mapped`, `TypeParameter`, or `Infer` shells. Index, call,
/// construct, and method signatures keep the object closed (their
/// shapes carry no open variables).
fn object_is_closed_expr(members: &[ObjectMember]) -> bool {
    members.iter().all(|member| match member {
        ObjectMember::Property(prop) => member_value_is_concrete(&prop.ty),
        // Index / call / construct / method signatures do not produce
        // an "open" generic shape on the surface; treat them as
        // concrete contributions to the closedness check. They are
        // rare on the props fast path (Vue prop types are typically
        // plain properties) but are kept for completeness.
        ObjectMember::IndexSignature(_)
        | ObjectMember::CallSignature(_)
        | ObjectMember::ConstructSignature(_)
        | ObjectMember::Method(_) => true,
    })
}

/// A property's value is "concrete" when it is itself classified as
/// `ExactConcrete`. Recursing through [`classify_type_expr`] keeps the
/// predicate composable: nested objects must themselves be closed to
/// keep the parent closed.
fn member_value_is_concrete(expr: &TypeExpr) -> bool {
    matches!(classify_type_expr(expr), ExpansionExactness::ExactConcrete,)
}

/// Strip outer `Parenthesized` wrappers. Mirrors the `strip_parens_expr`
/// helper used elsewhere in the resolver core; centralised here so the
/// shared predicate does not depend on a sibling crate's private helper.
fn strip_parens(expr: &TypeExpr) -> &TypeExpr {
    let mut e = expr;
    while let TypeExpr::Parenthesized(inner) = e {
        e = inner.as_ref();
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use verter_type_expr::{
        FunctionExpr, LiteralValue, ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName,
    };

    /// `new () => string` — a bare constructor type carrying a primitive
    /// return. Helper shared by the constructor-type exactness tests.
    fn constructor_type_returning_string() -> TypeExpr {
        TypeExpr::ConstructorType(Arc::new(FunctionExpr::synthetic(
            Vec::new(),
            Some(Arc::new(TypeExpr::Primitive(PrimitiveName::String))),
            Vec::new(),
        )))
    }

    #[test]
    fn function_is_concrete() {
        // Characterises the existing `Function` arm: a function type is a
        // fully-resolved shape with no open generic variables → concrete.
        let function = TypeExpr::Function(Arc::new(FunctionExpr::synthetic(
            Vec::new(),
            Some(Arc::new(TypeExpr::Primitive(PrimitiveName::String))),
            Vec::new(),
        )));
        assert_eq!(
            classify_type_expr(&function),
            ExpansionExactness::ExactConcrete,
        );
    }

    #[test]
    fn constructor_type_is_concrete_like_function() {
        // A bare constructor type (`new () => R`) is as concrete as its
        // sibling `Function`: it carries no open generic variables. It must
        // NOT fall through to the wildcard `ExactSymbolic` arm — that would
        // mis-classify a fully-resolved constructor-type prop as symbolic and
        // diverge from the `Function` classification.
        assert_eq!(
            classify_type_expr(&constructor_type_returning_string()),
            ExpansionExactness::ExactConcrete,
        );
    }

    #[test]
    fn parenthesized_constructor_type_is_concrete() {
        // Parenthesised constructor type strips to the same concrete shape.
        let wrapped = TypeExpr::Parenthesized(Arc::new(constructor_type_returning_string()));
        assert_eq!(
            classify_type_expr(&wrapped),
            ExpansionExactness::ExactConcrete,
        );
    }

    #[test]
    fn closed_object_with_constructor_type_member_is_concrete() {
        // `{ ctor: new () => string }` — a closed object whose only member is
        // a constructor type stays concrete (the member value is concrete).
        let expr = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty::synthetic(
                "ctor".to_string(),
                constructor_type_returning_string(),
                false,
                false,
            ))],
        }));
        assert_eq!(classify_type_expr(&expr), ExpansionExactness::ExactConcrete,);
    }

    #[test]
    fn primitive_is_concrete() {
        assert_eq!(
            classify_type_expr(&TypeExpr::Primitive(PrimitiveName::String)),
            ExpansionExactness::ExactConcrete,
        );
    }

    #[test]
    fn literal_is_concrete() {
        assert_eq!(
            classify_type_expr(&TypeExpr::Literal(LiteralValue::String(
                "hello".to_string(),
            ))),
            ExpansionExactness::ExactConcrete,
        );
    }

    #[test]
    fn parenthesized_unwraps_before_classifying() {
        let inner = TypeExpr::Primitive(PrimitiveName::Number);
        let wrapped = TypeExpr::Parenthesized(Arc::new(inner));
        assert_eq!(
            classify_type_expr(&wrapped),
            ExpansionExactness::ExactConcrete,
        );
    }

    #[test]
    fn closed_object_with_primitive_members_is_concrete() {
        let expr = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty::synthetic(
                    "msg".to_string(),
                    TypeExpr::Primitive(PrimitiveName::String),
                    false,
                    false,
                )),
                ObjectMember::Property(ObjectProperty::synthetic(
                    "count".to_string(),
                    TypeExpr::Primitive(PrimitiveName::Number),
                    false,
                    false,
                )),
            ],
        }));
        assert_eq!(classify_type_expr(&expr), ExpansionExactness::ExactConcrete,);
    }

    #[test]
    fn open_object_with_ref_member_is_symbolic() {
        let expr = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty::synthetic(
                "msg".to_string(),
                TypeExpr::Ref {
                    name: Arc::from("Unresolved"),
                    type_arguments: Arc::from(Vec::<TypeExpr>::new().into_boxed_slice()),
                },
                false,
                false,
            ))],
        }));
        assert_eq!(classify_type_expr(&expr), ExpansionExactness::ExactSymbolic,);
    }

    #[test]
    fn ref_at_root_is_symbolic() {
        let expr = TypeExpr::Ref {
            name: Arc::from("Unresolved"),
            type_arguments: Arc::from(Vec::<TypeExpr>::new().into_boxed_slice()),
        };
        assert_eq!(classify_type_expr(&expr), ExpansionExactness::ExactSymbolic,);
    }

    #[test]
    fn nested_open_object_propagates_symbolic() {
        // { msg: { inner: T } } — outer object has a non-concrete
        // value; classification must surface symbolic.
        let inner = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty::synthetic(
                "inner".to_string(),
                TypeExpr::TypeParameter(verter_type_expr::TypeParam {
                    name: "T".to_string(),
                    constraint: None,
                    default: None,
                }),
                false,
                false,
            ))],
        }));
        let outer = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty::synthetic(
                "msg".to_string(),
                inner,
                false,
                false,
            ))],
        }));
        assert_eq!(
            classify_type_expr(&outer),
            ExpansionExactness::ExactSymbolic,
        );
    }
}
