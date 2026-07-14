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

/// The registry-symbol "stay symbolic" root predicate, node-domain.
/// Returns `true` when a registry-symbol body NODE has a root kind that
/// the registry must publish symbolically rather than materialise
/// eagerly — a deferred `Mapped` / `Conditional` / `IndexedAccess`
/// shell or a `TypeOf` carrier.
///
/// The registry walker (`host_manage::component_meta_methods`'
/// `imported_registry_alias_should_stay_symbolic`) classifies the
/// resolved declaration's lowered body root through this predicate; the
/// `TypeExpr`-shape sibling below answers identically for a
/// parser-produced shape (the handle-capable equivalence fixtures pin
/// the agreement). It is a ROOT-KIND classifier — it reads only the
/// node's own root variant via
/// [`crate::project_semantic_dispatch::node_data_for`], unwrapping a
/// single `Alias` hop, and makes NO resolution / reduction / descent.
pub(crate) fn node_root_should_stay_symbolic(
    ctx: &dyn crate::resolver_core::ResolverContext,
    node: SemanticNodeId,
) -> bool {
    let unwrapped = match crate::project_semantic_dispatch::node_data_for(ctx, node).as_deref() {
        Some(SemanticNodeData::Alias(target)) => *target,
        _ => node,
    };
    matches!(
        crate::project_semantic_dispatch::node_data_for(ctx, unwrapped).as_deref(),
        Some(
            SemanticNodeData::Mapped { .. }
                | SemanticNodeData::Conditional { .. }
                | SemanticNodeData::IndexedAccess { .. }
                | SemanticNodeData::TypeOf(_)
        )
    )
}

/// `TypeExpr`-shape sibling of [`node_root_should_stay_symbolic`]: the
/// registry-symbol "stay symbolic" root predicate over a parser-produced
/// `TypeExpr`. A `Mapped` / `Conditional` / `IndexedAccess` / `TypeOf`
/// root (after stripping a `Parenthesized` wrapper) stays symbolic.
///
/// This is the SINGLE definition of the `TypeExpr`-arm predicate; the
/// handle-capable equivalence fixture asserts it agrees with the
/// graph-native [`node_root_should_stay_symbolic`] (the arm the
/// registry walker classifies through) for every root kind. Keeping
/// both arms reading from one source is what makes the two predicates
/// provably equivalent (deleting a root kind from EITHER arm breaks
/// the equivalence test).
pub(crate) fn expr_root_should_stay_symbolic(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Parenthesized(inner) => expr_root_should_stay_symbolic(inner),
        TypeExpr::Mapped { .. }
        | TypeExpr::Conditional { .. }
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::TypeOf(_) => true,
        _ => false,
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
/// `InstantiationRef`, `IndexedAccess`, `Conditional`, `TypeParam`, or a
/// `BareRef` / `TypeOf` / `ImportType` carrier (a carrier applies type arguments
/// at its reference site and is symbolic until resolved). Used by
/// [`classify_node`] to distinguish closed objects from open ones.
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
            | SemanticNodeData::TypeParam { .. }
            // A `BareRef` / `TypeOf` / `ImportType` carrier-valued member is
            // symbolic — the carrier applies type arguments at its reference
            // site and has not been resolved — so the enclosing object is OPEN
            // (not closed/concrete).
            | SemanticNodeData::BareRef(_)
            | SemanticNodeData::TypeOf(_)
            | SemanticNodeData::ImportType(_) => return false,
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
            properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
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
                ObjectMember::Property(ObjectProperty::synthetic_public(
                    "msg".to_string(),
                    TypeExpr::Primitive(PrimitiveName::String),
                    false,
                    false,
                )),
                ObjectMember::Property(ObjectProperty::synthetic_public(
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
            properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
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
            properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
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
            properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
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

#[cfg(test)]
mod carrier_exactness_tests {
    //! Carrier-valued object members are NOT closed/concrete.
    //!
    //! `classify_node` / `object_is_closed_node` decide whether a synthesised
    //! binding's value is `ExactConcrete` (a fully-resolved shape) or
    //! `ExactSymbolic` (still carries open/unresolved variables). An object
    //! member whose VALUE is a `TypeOf` / `BareRef` / `ImportType` carrier is
    //! symbolic — the carrier applies type arguments at its reference site and
    //! has not been resolved — so the enclosing object must NOT be classified
    //! closed/concrete. A bare carrier ROOT is already `ExactSymbolic` (the
    //! wildcard arm); this fixture pins the OBJECT-MEMBER case, which routes
    //! through `object_is_closed_node`'s open-member set.

    use std::sync::Arc;

    use verter_type_expr::MemberVisibility;

    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{
        NodeScopeId, PrimitiveKind, ScopeId, SemanticNodeData, SemanticNodeId, SurfaceMember,
        SurfaceView, ValueRootKey,
    };
    use crate::types::HostConfig;
    use crate::VerterHost;
    use verter_semantic::analysis::type_expand::ExpansionExactness;

    fn object_with_member_value(
        graph: &crate::semantic_query_memo::SemanticGraphStore,
        member_value: SemanticNodeId,
    ) -> SemanticNodeId {
        let view = SurfaceView {
            members: Arc::from(
                vec![SurfaceMember {
                    name: Arc::from("m"),
                    value: member_value,
                    optional: false,
                    readonly: false,
                    is_method: false,
                    visibility: MemberVisibility::Public,
                    spans: Default::default(),
                    declaration_origin: None,
                    declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
                    merge_role: Default::default(),
                }]
                .into_boxed_slice(),
            ),
            call_signatures: Arc::from(Vec::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        };
        graph.intern_node(SemanticNodeData::Object(view))
    }

    fn carriers(graph: &crate::semantic_query_memo::SemanticGraphStore) -> Vec<SemanticNodeId> {
        let empty: Arc<[SemanticNodeId]> = Arc::from(Vec::new().into_boxed_slice());
        vec![
            graph.intern_node(SemanticNodeData::new_bare_ref(
                Arc::from("Foo"),
                NodeScopeId::Global,
                Arc::clone(&empty),
            )),
            graph.intern_node(SemanticNodeData::new_typeof(
                ValueRootKey {
                    scope: ScopeId {
                        canonical_id: Arc::from("/v.ts"),
                        local_scope: None,
                    },
                    name: Arc::from("factory"),
                },
                Arc::from(Vec::new().into_boxed_slice()),
                Arc::clone(&empty),
            )),
            graph.intern_node(SemanticNodeData::new_import_type(
                Arc::from("./m"),
                Arc::from(vec![Arc::<str>::from("G")].into_boxed_slice()),
                Arc::clone(&empty),
                false,
            )),
        ]
    }

    // ── E2 — an object with a carrier-valued member is NOT concrete ─────────
    //
    // NEGATIVE: with the unchanged `object_is_closed_node` open-member set
    // (which lists `InstantiationRef` / `IndexedAccess` / `Conditional` /
    // `TypeParam` but NOT the carriers), an object whose member value is a
    // carrier is wrongly treated as closed → `ExactConcrete`.
    #[test]
    fn object_with_carrier_valued_member_is_symbolic() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = Arc::clone(host.project_type_store().semantic_graph());

        for carrier in carriers(&graph) {
            let obj = object_with_member_value(&graph, carrier);
            assert_eq!(
                super::classify_node(&dispatch, obj),
                ExpansionExactness::ExactSymbolic,
                "an object whose member value is a carrier must be ExactSymbolic (open), not \
                 ExactConcrete; member carrier {:?}",
                graph.node_data(carrier).as_deref()
            );
        }
    }

    // Positive control: an object whose member value is a concrete primitive IS
    // ExactConcrete — proving the open-member-set widening does not over-broaden
    // and mis-classify a genuinely closed object.
    #[test]
    fn object_with_primitive_member_stays_concrete() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = Arc::clone(host.project_type_store().semantic_graph());

        let prim = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let obj = object_with_member_value(&graph, prim);
        assert_eq!(
            super::classify_node(&dispatch, obj),
            ExpansionExactness::ExactConcrete,
            "an object whose member value is a concrete primitive must stay ExactConcrete"
        );
    }
}
