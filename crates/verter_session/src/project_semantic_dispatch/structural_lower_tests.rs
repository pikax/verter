//! Tests for the query-free structural lowerer ([`super`]).
//!
//! Two kinds of test live here:
//! - unit tests of the lowerer's own scaffolding (the binder stack), and
//! - lowering fixtures that assert the EMITTED graph shape for each
//!   `TypeExpr` variant — the structural-equivalence set (no-resolution
//!   shapes lowered via the eager and structural paths must agree) and the
//!   discriminating carrier set (shapes that intentionally diverge from the
//!   eager path, asserted directly against the new graph).

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_type_expr::{
    FunctionExpr, FunctionParam, LiteralValue, MappedModifier, ObjectExpr, ObjectMember,
    ObjectProperty, PrimitiveName, SyntheticCarrierKey, SyntheticCarrierSurfaceKind, TypeExpr,
    ValueRef,
};

use super::{
    lower_type_expr_structural, BinderScope, StructuralLowerContext, StructuralLowerError,
};
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::scope_shadowing::ScopeShadowing;
use crate::semantic_query::{
    DeclIdentity, IndexKey, MemberMergeRole, NodeScopeId, OptionalityMod, PrimitiveKind,
    ProjectionReductionContext, ReadonlyMod, SemanticNodeData, SemanticNodeId, SyntheticBindingId,
};
use crate::semantic_query_memo::SemanticGraphStore;
use crate::VerterHost;

/// A real declaration-bound file scope for emission fixtures — deliberately
/// NOT `Global`/empty, so a fixture discriminates against a lowerer that
/// hardcodes or drops the scope (the `BareRef.scope` capture-root
/// invariant: the scope is the owner-supplied lexical root).
fn fixture_scope() -> NodeScopeId {
    NodeScopeId::File {
        canonical_id: Arc::from("/fixture.ts"),
        whole_hash: [7u8; 16],
        local_scope: None,
    }
}

/// Lower `expr` with no binders in scope; return the graph + interned root
/// node id so a fixture can walk the emitted carrier and its children.
fn lower_root(host: &VerterHost, expr: &TypeExpr) -> (Arc<SemanticGraphStore>, SemanticNodeId) {
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let binders: [BinderScope; 0] = [];
    let ctx = StructuralLowerContext::new(&binders);
    let handle = lower_type_expr_structural(&graph, expr, fixture_scope(), &ctx)
        .expect("structural lowering should succeed for a resolvable shape");
    (graph, handle.node())
}

/// Read an interned node's payload.
fn node(graph: &SemanticGraphStore, id: SemanticNodeId) -> Arc<SemanticNodeData> {
    graph.node_data(id).expect("interned node must exist")
}

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

// --- Discriminating carrier fixtures (set B) ----------------------------
// These assert the EMITTED graph directly: the structural lowerer
// intentionally diverges from the eager path (which resolves these shapes),
// so there is nothing to compare against — only the carrier shape matters.

#[test]
fn lowers_bare_ref_with_empty_type_args() {
    // `Foo` (no args, not a binder) → `BareRef { name, scope, type_args: [] }`,
    // NEVER a resolved `DeclRef`.
    let host = VerterHost::new_standalone(Default::default());
    let expr = TypeExpr::Ref {
        name: Arc::from("Foo"),
        type_arguments: verter_type_expr::empty_type_args(),
    };
    let (graph, root) = lower_root(&host, &expr);
    match &*node(&graph, root) {
        SemanticNodeData::BareRef {
            name,
            scope,
            type_args,
        } => {
            assert_eq!(name.as_ref(), "Foo");
            assert_eq!(
                *scope,
                fixture_scope(),
                "BareRef.scope must be the owner-supplied capture root"
            );
            assert!(type_args.is_empty(), "bare `Foo` carries no type args");
        }
        other => panic!("expected BareRef, got {other:?}"),
    }
}

#[test]
fn lowers_generic_ref_with_structurally_lowered_args() {
    // `Foo<string>` → `BareRef { name, type_args: [Primitive(String)] }`,
    // NEVER a resolved `InstantiationRef`; the arg is structurally lowered.
    let host = VerterHost::new_standalone(Default::default());
    let expr = TypeExpr::Ref {
        name: Arc::from("Foo"),
        type_arguments: Arc::from(
            vec![TypeExpr::Primitive(PrimitiveName::String)].into_boxed_slice(),
        ),
    };
    let (graph, root) = lower_root(&host, &expr);
    let arg = match &*node(&graph, root) {
        SemanticNodeData::BareRef {
            name, type_args, ..
        } => {
            assert_eq!(name.as_ref(), "Foo");
            assert_eq!(type_args.len(), 1, "Foo<string> carries exactly one arg");
            type_args[0]
        }
        other => panic!("expected BareRef, got {other:?}"),
    };
    assert!(
        matches!(
            &*node(&graph, arg),
            SemanticNodeData::Primitive(PrimitiveKind::String)
        ),
        "the type argument must be STRUCTURALLY lowered to Primitive(String), not resolved"
    );
}

#[test]
fn bound_type_param_ref_returns_binder_node_not_bare_ref() {
    // Inside a generic decl, a `Ref` to a bound type-param returns the
    // binder node verbatim — NOT a fresh `BareRef`. (The binder node's kind
    // is irrelevant to the arm; it returns whatever the owner bound, so the
    // fixture binds an arbitrary pre-interned node and asserts identity.)
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let binder_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let mut frame = BinderScope::default();
    frame.bind(Arc::from("T"), binder_id);
    let stack = [frame];
    let ctx = StructuralLowerContext::new(&stack);
    let expr = TypeExpr::Ref {
        name: Arc::from("T"),
        type_arguments: verter_type_expr::empty_type_args(),
    };
    let handle = lower_type_expr_structural(&graph, &expr, fixture_scope(), &ctx)
        .expect("structural lowering should succeed");
    assert_eq!(
        handle.node(),
        binder_id,
        "a bound `T` resolves to its binder node"
    );
    assert!(
        !matches!(
            &*node(&graph, handle.node()),
            SemanticNodeData::BareRef { .. }
        ),
        "a bound type-param must NOT emit a BareRef carrier"
    );
}

#[test]
fn applied_binder_ref_is_a_typed_error_not_a_bare_ref() {
    // Inside a generic decl binding `T`, an APPLIED binder `T<X>` cannot be
    // represented by the query-free lowerer (there is no structural "apply args
    // to a binder" carrier), so it is a typed `UnsupportedWithoutResolution`
    // error — NEVER a `BareRef { name: "T" }` that would leak past binder
    // shadowing. (Binder lookup must run BEFORE the type-arg gate.)
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let binder_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let mut frame = BinderScope::default();
    frame.bind(Arc::from("T"), binder_id);
    let stack = [frame];
    let ctx = StructuralLowerContext::new(&stack);
    let expr = TypeExpr::Ref {
        name: Arc::from("T"),
        type_arguments: Arc::from(
            vec![TypeExpr::Primitive(PrimitiveName::String)].into_boxed_slice(),
        ),
    };
    let result = lower_type_expr_structural(&graph, &expr, fixture_scope(), &ctx);
    assert_eq!(
        result.err(),
        Some(StructuralLowerError::UnsupportedWithoutResolution {
            shape: "AppliedBinder"
        }),
        "an applied binder `T<X>` must be a typed error, not a leaked BareRef"
    );
}

#[test]
fn lowers_unknown_to_raw_fallback_verbatim() {
    // Unsupported raw syntax → `RawFallback { raw }` with the exact text
    // preserved. RawFallback is display/compat only, never a miss signal.
    let host = VerterHost::new_standalone(Default::default());
    let expr = TypeExpr::Unknown {
        raw: "Weird<& Type>".to_string(),
    };
    let (graph, root) = lower_root(&host, &expr);
    match &*node(&graph, root) {
        SemanticNodeData::RawFallback { raw } => assert_eq!(raw.as_ref(), "Weird<& Type>"),
        other => panic!("expected RawFallback, got {other:?}"),
    }
}

#[test]
fn lowers_synthetic_slot_binding_with_content_free_id() {
    // `SyntheticSlotBinding` → `SyntheticBinding` whose `id` EXCLUDES the
    // value-side `value_node` ordinal (content-free identity) while the
    // `value_node` is retained separately as provenance.
    let host = VerterHost::new_standalone(Default::default());
    let key = SyntheticCarrierKey {
        scope_canonical_id: Arc::from("/Comp.vue"),
        surface_kind: SyntheticCarrierSurfaceKind::SlotBinding,
        slot_name: Some(Arc::from("default")),
        binding_name: Arc::from("row"),
        value_node: 42,
    };
    let expr = TypeExpr::SyntheticSlotBinding(Arc::new(key.clone()));
    let (graph, root) = lower_root(&host, &expr);
    match &*node(&graph, root) {
        SemanticNodeData::SyntheticBinding { id, value_node } => {
            assert_eq!(
                *id,
                SyntheticBindingId::from_carrier_key(&key),
                "id must be the content-free identity"
            );
            assert_eq!(id.binding_name.as_ref(), "row");
            assert_eq!(id.slot_name.as_deref(), Some("default"));
            assert_eq!(
                *value_node, 42,
                "value_node provenance is retained separately from identity"
            );
        }
        other => panic!("expected SyntheticBinding, got {other:?}"),
    }
}

#[test]
fn lowers_typeof_preserving_root_path_and_type_args() {
    // `typeof factory.make<string>` → `TypeOf` whose value root is built
    // from the OWNER file scope + the FIRST path segment (never a host
    // lookup of where the value resolves), with the remaining segments as
    // the projected path and the instantiation args structurally lowered.
    let host = VerterHost::new_standalone(Default::default());
    let expr = TypeExpr::TypeOf(ValueRef {
        path: vec!["factory".to_string(), "make".to_string()],
        type_args: vec![TypeExpr::Primitive(PrimitiveName::String)],
    });
    let (graph, root) = lower_root(&host, &expr);
    let arg = match &*node(&graph, root) {
        SemanticNodeData::TypeOf {
            value_root,
            path,
            type_args,
        } => {
            assert_eq!(
                value_root.name.as_ref(),
                "factory",
                "root is the first segment"
            );
            assert_eq!(
                value_root.scope.canonical_id.as_ref(),
                "/fixture.ts",
                "value-root scope is the owner file scope, not a resolved location"
            );
            assert_eq!(path.len(), 1, "remaining path segments preserved");
            assert_eq!(path[0].as_ref(), "make");
            assert_eq!(type_args.len(), 1, "instantiation arg carried");
            type_args[0]
        }
        other => panic!("expected TypeOf, got {other:?}"),
    };
    assert!(
        matches!(
            &*node(&graph, arg),
            SemanticNodeData::Primitive(PrimitiveKind::String)
        ),
        "the instantiation-expression arg must be structurally lowered"
    );
}

#[test]
fn lowers_import_type_with_qualifier_and_args() {
    // `import("./m").Box<string>` → `ImportType` (type position) with exact
    // specifier/qualifier/type_args and the import resolver NEVER called.
    let host = VerterHost::new_standalone(Default::default());
    let expr = TypeExpr::ImportType {
        specifier: Arc::from("./m"),
        qualifier: Arc::from(vec![Arc::<str>::from("Box")].into_boxed_slice()),
        typeof_query: false,
        type_arguments: Arc::from(
            vec![TypeExpr::Primitive(PrimitiveName::String)].into_boxed_slice(),
        ),
    };
    let (graph, root) = lower_root(&host, &expr);
    let arg = match &*node(&graph, root) {
        SemanticNodeData::ImportType {
            specifier,
            qualifier,
            type_args,
            typeof_query,
        } => {
            assert_eq!(specifier.as_ref(), "./m");
            assert_eq!(qualifier.len(), 1);
            assert_eq!(qualifier[0].as_ref(), "Box");
            assert!(!*typeof_query, "import(...).Box is type position");
            assert_eq!(type_args.len(), 1);
            type_args[0]
        }
        other => panic!("expected ImportType, got {other:?}"),
    };
    assert!(matches!(
        &*node(&graph, arg),
        SemanticNodeData::Primitive(PrimitiveKind::String)
    ));
}

#[test]
fn lowers_typeof_import_type_sets_typeof_query() {
    // `typeof import("./m").make<string>` → `ImportType` with typeof_query.
    let host = VerterHost::new_standalone(Default::default());
    let expr = TypeExpr::ImportType {
        specifier: Arc::from("./m"),
        qualifier: Arc::from(vec![Arc::<str>::from("make")].into_boxed_slice()),
        typeof_query: true,
        type_arguments: Arc::from(
            vec![TypeExpr::Primitive(PrimitiveName::String)].into_boxed_slice(),
        ),
    };
    let (graph, root) = lower_root(&host, &expr);
    match &*node(&graph, root) {
        SemanticNodeData::ImportType {
            qualifier,
            typeof_query,
            type_args,
            ..
        } => {
            assert!(*typeof_query, "typeof import(...) sets typeof_query");
            assert_eq!(qualifier[0].as_ref(), "make");
            assert_eq!(type_args.len(), 1);
        }
        other => panic!("expected ImportType, got {other:?}"),
    }
}

#[test]
fn lowers_tuple_preserving_rest_element() {
    // `[head: string, ...tail: number[]]` → `Tuple` preserving the
    // per-element label / rest metadata, with the rest value structurally
    // lowered. NEVER a standalone `Rest` node and NO spread normalization.
    let host = VerterHost::new_standalone(Default::default());
    let expr = TypeExpr::Tuple {
        elements: Arc::from(
            vec![
                verter_type_expr::TupleElement {
                    label: Some("head".to_string()),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    rest: false,
                },
                verter_type_expr::TupleElement {
                    label: Some("tail".to_string()),
                    ty: TypeExpr::Array {
                        element: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
                        readonly: false,
                    },
                    optional: false,
                    rest: true,
                },
            ]
            .into_boxed_slice(),
        ),
        readonly: false,
    };
    let (graph, root) = lower_root(&host, &expr);
    match &*node(&graph, root) {
        SemanticNodeData::Tuple { elements, readonly } => {
            assert!(!readonly);
            assert_eq!(
                elements.len(),
                2,
                "both elements preserved (no spread collapse)"
            );
            assert_eq!(elements[0].label.as_deref(), Some("head"));
            assert!(!elements[0].rest, "head is not a rest element");
            assert_eq!(elements[1].label.as_deref(), Some("tail"));
            assert!(elements[1].rest, "the rest-element flag must be preserved");
            assert!(
                matches!(
                    &*node(&graph, elements[1].value),
                    SemanticNodeData::Array { .. }
                ),
                "the rest element value is the structurally lowered number[]"
            );
        }
        other => panic!("expected Tuple, got {other:?}"),
    }
}

#[test]
fn lowers_constructor_type_wrapping_a_function_signature() {
    // `new (x: number) => Foo` → `ConstructorType(Function(...))`, keeping
    // constructor-ness distinct from a plain function (the eager path
    // flattens both to `Function`).
    let host = VerterHost::new_standalone(Default::default());
    let func = FunctionExpr::synthetic(
        vec![FunctionParam::synthetic(
            Some("x".to_string()),
            TypeExpr::Primitive(PrimitiveName::Number),
            false,
            false,
        )],
        Some(Arc::new(TypeExpr::Ref {
            name: Arc::from("Foo"),
            type_arguments: verter_type_expr::empty_type_args(),
        })),
        vec![],
    );
    let expr = TypeExpr::ConstructorType(Arc::new(func));
    let (graph, root) = lower_root(&host, &expr);
    let signature = match &*node(&graph, root) {
        SemanticNodeData::ConstructorType { signature } => *signature,
        other => panic!("expected ConstructorType, got {other:?}"),
    };
    assert!(
        !matches!(&*node(&graph, root), SemanticNodeData::Function { .. }),
        "constructor-ness must NOT collapse to a plain Function"
    );
    match &*node(&graph, signature) {
        SemanticNodeData::Function {
            params,
            return_type,
            ..
        } => {
            assert_eq!(params.len(), 1);
            assert_eq!(params[0].name.as_deref(), Some("x"));
            assert!(matches!(
                &*node(&graph, params[0].ty),
                SemanticNodeData::Primitive(PrimitiveKind::Number)
            ));
            // The return is the structurally lowered `Foo` carrier, NOT a
            // resolved decl.
            assert!(matches!(
                &*node(&graph, *return_type),
                SemanticNodeData::BareRef { .. }
            ));
        }
        other => panic!("expected Function signature, got {other:?}"),
    }
}

#[test]
fn lowers_type_literal_construct_signature_as_object_not_constructor_type() {
    // `{ new(): T }` is an Object carrying a construct signature — NOT a
    // `ConstructorType` (that is the bare `new () => T` form). The two must stay
    // distinct (Vue treats a ctor-type as Function, a type-literal with a
    // construct signature as Object). Discriminates against a lowerer that
    // flattens the type literal to ConstructorType.
    let host = VerterHost::new_standalone(Default::default());
    let func = FunctionExpr::synthetic(
        vec![],
        Some(Arc::new(TypeExpr::Ref {
            name: Arc::from("T"),
            type_arguments: verter_type_expr::empty_type_args(),
        })),
        vec![],
    );
    let expr = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![ObjectMember::ConstructSignature(func)],
    }));
    let (graph, root) = lower_root(&host, &expr);
    match &*node(&graph, root) {
        SemanticNodeData::Object(view) => {
            assert!(view.members.is_empty(), "no plain members");
            assert_eq!(
                view.construct_signatures.len(),
                1,
                "the construct signature is preserved on the Object surface"
            );
            assert!(
                matches!(
                    &*node(&graph, view.construct_signatures[0]),
                    SemanticNodeData::Function { .. }
                ),
                "the construct signature lowers to a Function node"
            );
        }
        other => panic!("expected Object with a construct signature, got {other:?}"),
    }
    assert!(
        !matches!(
            &*node(&graph, root),
            SemanticNodeData::ConstructorType { .. }
        ),
        "a type-literal `{{ new(): T }}` must NOT flatten to ConstructorType"
    );
}

#[test]
fn generic_function_binds_own_type_param_in_params_and_return() {
    // `<T>(x: T) => T` — the own `<T>` binds: both the param `x: T` and the
    // return `T` resolve to the SAME `TypeParam` binder node, never a
    // `BareRef` and never two distinct nodes.
    let host = VerterHost::new_standalone(Default::default());
    let func = FunctionExpr::synthetic(
        vec![FunctionParam::synthetic(
            Some("x".to_string()),
            TypeExpr::Ref {
                name: Arc::from("T"),
                type_arguments: verter_type_expr::empty_type_args(),
            },
            false,
            false,
        )],
        Some(Arc::new(TypeExpr::Ref {
            name: Arc::from("T"),
            type_arguments: verter_type_expr::empty_type_args(),
        })),
        vec![verter_type_expr::TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
    );
    let expr = TypeExpr::Function(Arc::new(func));
    let (graph, root) = lower_root(&host, &expr);
    match &*node(&graph, root) {
        SemanticNodeData::Function {
            params,
            return_type,
            type_parameters,
            ..
        } => {
            assert_eq!(type_parameters.len(), 1);
            assert_eq!(type_parameters[0].name.as_ref(), "T");
            let param_ty = params[0].ty;
            assert_eq!(
                param_ty, *return_type,
                "the own `T` binds to one binder node in both positions"
            );
            assert!(
                matches!(&*node(&graph, param_ty), SemanticNodeData::TypeParam { .. }),
                "an own type-param lowers to a TypeParam binder, not a BareRef"
            );
        }
        other => panic!("expected Function, got {other:?}"),
    }
}

#[test]
fn generic_function_constraint_sees_prior_type_param_binder() {
    // `<T, U extends T>(x: T) => …` — the generic head lowers INCREMENTALLY
    // (TS scoping): `U`'s `extends T` constraint sees the PRIOR `T` binder, so
    // it resolves to the FIRST own type-param's `TypeParam` binder node — the
    // same node the param `x: T` resolves to — NEVER a `BareRef(T)` and never
    // an outer binding.
    let host = VerterHost::new_standalone(Default::default());
    let func = FunctionExpr::synthetic(
        vec![FunctionParam::synthetic(
            Some("x".to_string()),
            TypeExpr::Ref {
                name: Arc::from("T"),
                type_arguments: verter_type_expr::empty_type_args(),
            },
            false,
            false,
        )],
        None,
        vec![
            verter_type_expr::TypeParam {
                name: "T".to_string(),
                constraint: None,
                default: None,
            },
            verter_type_expr::TypeParam {
                name: "U".to_string(),
                constraint: Some(Arc::new(TypeExpr::Ref {
                    name: Arc::from("T"),
                    type_arguments: verter_type_expr::empty_type_args(),
                })),
                default: None,
            },
        ],
    );
    let expr = TypeExpr::Function(Arc::new(func));
    let (graph, root) = lower_root(&host, &expr);
    match &*node(&graph, root) {
        SemanticNodeData::Function {
            params,
            type_parameters,
            ..
        } => {
            assert_eq!(type_parameters.len(), 2);
            assert_eq!(type_parameters[1].name.as_ref(), "U");
            // `params[0].ty` is the first own type-param's `T` binder (the
            // full own-generic frame is in scope for the params).
            let t_binder = params[0].ty;
            let u_constraint = type_parameters[1]
                .constraint
                .expect("U carries an `extends T` constraint");
            assert_eq!(
                u_constraint, t_binder,
                "`U extends T` binds T to the first own type-param binder, not a BareRef/outer ref"
            );
            assert!(
                matches!(
                    &*node(&graph, u_constraint),
                    SemanticNodeData::TypeParam { .. }
                ),
                "U's constraint resolves to the T TypeParam binder"
            );
            assert!(
                !matches!(&*node(&graph, u_constraint), SemanticNodeData::BareRef { .. }),
                "pre-fix the constraint lowered under the OUTER context → BareRef(T); must not regress"
            );
        }
        other => panic!("expected Function, got {other:?}"),
    }
}

#[test]
fn generic_function_default_sees_prior_type_param_binder() {
    // `<T, U = T>(x: T) => …` — incremental head lowering means `U`'s default
    // `= T` sees the PRIOR `T` binder and resolves to the FIRST own type-param
    // binder node (same as the param `x: T`), NEVER a `BareRef(T)`.
    let host = VerterHost::new_standalone(Default::default());
    let func = FunctionExpr::synthetic(
        vec![FunctionParam::synthetic(
            Some("x".to_string()),
            TypeExpr::Ref {
                name: Arc::from("T"),
                type_arguments: verter_type_expr::empty_type_args(),
            },
            false,
            false,
        )],
        None,
        vec![
            verter_type_expr::TypeParam {
                name: "T".to_string(),
                constraint: None,
                default: None,
            },
            verter_type_expr::TypeParam {
                name: "U".to_string(),
                constraint: None,
                default: Some(Arc::new(TypeExpr::Ref {
                    name: Arc::from("T"),
                    type_arguments: verter_type_expr::empty_type_args(),
                })),
            },
        ],
    );
    let expr = TypeExpr::Function(Arc::new(func));
    let (graph, root) = lower_root(&host, &expr);
    match &*node(&graph, root) {
        SemanticNodeData::Function {
            params,
            type_parameters,
            ..
        } => {
            assert_eq!(type_parameters.len(), 2);
            assert_eq!(type_parameters[1].name.as_ref(), "U");
            let t_binder = params[0].ty;
            let u_default = type_parameters[1]
                .default
                .expect("U carries a `= T` default");
            assert_eq!(
                u_default, t_binder,
                "`U = T` binds T to the first own type-param binder, not a BareRef/outer ref"
            );
            assert!(
                matches!(
                    &*node(&graph, u_default),
                    SemanticNodeData::TypeParam { .. }
                ),
                "U's default resolves to the T TypeParam binder"
            );
            assert!(
                !matches!(&*node(&graph, u_default), SemanticNodeData::BareRef { .. }),
                "pre-fix the default lowered under the OUTER context → BareRef(T); must not regress"
            );
        }
        other => panic!("expected Function, got {other:?}"),
    }
}

#[test]
fn lowers_indexed_access_as_deferred_shell_with_string_key() {
    // `Foo["bar"]` → deferred `IndexedAccess` shell; the object is the
    // structurally lowered `Foo` carrier and the literal key folds to
    // `IndexKey::String`. NEVER projected/executed.
    let host = VerterHost::new_standalone(Default::default());
    let expr = TypeExpr::IndexedAccess {
        object: Arc::new(TypeExpr::Ref {
            name: Arc::from("Foo"),
            type_arguments: verter_type_expr::empty_type_args(),
        }),
        index: Arc::new(TypeExpr::Literal(LiteralValue::String("bar".to_string()))),
    };
    let (graph, root) = lower_root(&host, &expr);
    match &*node(&graph, root) {
        SemanticNodeData::IndexedAccess { object, index } => {
            assert!(
                matches!(&*node(&graph, *object), SemanticNodeData::BareRef { .. }),
                "the object operand is structurally lowered, not resolved"
            );
            match index {
                IndexKey::String(s) => assert_eq!(s.as_ref(), "bar"),
                other => panic!("expected String index key, got {other:?}"),
            }
        }
        other => panic!("expected IndexedAccess, got {other:?}"),
    }
}

#[test]
fn lowers_conditional_as_deferred_shell() {
    // `T extends string ? number : boolean` → deferred `Conditional` shell
    // with all operands lowered and NO branch decision; a naked type-param
    // check is distributive.
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let t_binder = graph.intern_node_with_scope(
        SemanticNodeData::TypeParam {
            decl: DeclIdentity::from_scope(&fixture_scope(), Arc::from("T")),
            param_index: 0,
            constraint: None,
            default: None,
            display_name: Arc::from("T"),
        },
        fixture_scope(),
    );
    let mut frame = BinderScope::default();
    frame.bind(Arc::from("T"), t_binder);
    let stack = [frame];
    let ctx = StructuralLowerContext::new(&stack);
    let expr = TypeExpr::Conditional {
        check: Arc::new(TypeExpr::Ref {
            name: Arc::from("T"),
            type_arguments: verter_type_expr::empty_type_args(),
        }),
        extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
        true_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
        false_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Boolean)),
    };
    let handle = lower_type_expr_structural(&graph, &expr, fixture_scope(), &ctx)
        .expect("structural lowering should succeed");
    match &*node(&graph, handle.node()) {
        SemanticNodeData::Conditional {
            check,
            extends,
            true_branch_ref,
            false_branch_ref,
            distributive,
        } => {
            assert_eq!(*check, t_binder, "check is the bound T binder");
            assert!(*distributive, "a naked type-param check is distributive");
            assert!(matches!(
                &*node(&graph, *extends),
                SemanticNodeData::Primitive(PrimitiveKind::String)
            ));
            assert!(matches!(
                &*node(&graph, *true_branch_ref),
                SemanticNodeData::Primitive(PrimitiveKind::Number)
            ));
            assert!(matches!(
                &*node(&graph, *false_branch_ref),
                SemanticNodeData::Primitive(PrimitiveKind::Boolean)
            ));
        }
        other => panic!("expected Conditional, got {other:?}"),
    }
}

#[test]
fn conditional_binds_bare_infer_in_true_branch() {
    // `T extends infer P ? P : never` — the `infer P` in the `extends` clause
    // binds `P` for the TRUE branch (TS scoping). The true-branch `P` must
    // resolve to the SAME `Infer` carrier the `extends` arm interned, NOT leak
    // out as an unbound `BareRef { name: "P" }`.
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let t_binder = graph.intern_node_with_scope(
        SemanticNodeData::TypeParam {
            decl: DeclIdentity::from_scope(&fixture_scope(), Arc::from("T")),
            param_index: 0,
            constraint: None,
            default: None,
            display_name: Arc::from("T"),
        },
        fixture_scope(),
    );
    let mut frame = BinderScope::default();
    frame.bind(Arc::from("T"), t_binder);
    let stack = [frame];
    let ctx = StructuralLowerContext::new(&stack);
    let expr = TypeExpr::Conditional {
        check: Arc::new(TypeExpr::Ref {
            name: Arc::from("T"),
            type_arguments: verter_type_expr::empty_type_args(),
        }),
        extends: Arc::new(TypeExpr::Infer {
            name: "P".to_string(),
        }),
        true_type: Arc::new(TypeExpr::Ref {
            name: Arc::from("P"),
            type_arguments: verter_type_expr::empty_type_args(),
        }),
        false_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Never)),
    };
    let handle = lower_type_expr_structural(&graph, &expr, fixture_scope(), &ctx)
        .expect("structural lowering should succeed");
    match &*node(&graph, handle.node()) {
        SemanticNodeData::Conditional {
            extends,
            true_branch_ref,
            ..
        } => {
            assert!(
                matches!(&*node(&graph, *extends), SemanticNodeData::Infer { name } if name.as_ref() == "P"),
                "extends lowers `infer P` to an Infer carrier"
            );
            assert_eq!(
                *true_branch_ref, *extends,
                "the true-branch `P` binds to the `infer P` carrier introduced by `extends`"
            );
            assert!(
                !matches!(
                    &*node(&graph, *true_branch_ref),
                    SemanticNodeData::BareRef { .. }
                ),
                "a bound infer name must NOT leak out as a BareRef"
            );
        }
        other => panic!("expected Conditional, got {other:?}"),
    }
}

#[test]
fn conditional_binds_nested_infer_in_true_branch() {
    // `T extends Array<infer E> ? E : never` — the `infer E` NESTED inside the
    // `extends` clause still binds `E` for the TRUE branch. The true-branch `E`
    // resolves to the SAME `Infer` carrier nested in the lowered `extends`.
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let binders: [BinderScope; 0] = [];
    let ctx = StructuralLowerContext::new(&binders);
    let expr = TypeExpr::Conditional {
        check: Arc::new(TypeExpr::Ref {
            name: Arc::from("T"),
            type_arguments: verter_type_expr::empty_type_args(),
        }),
        extends: Arc::new(TypeExpr::Array {
            element: Arc::new(TypeExpr::Infer {
                name: "E".to_string(),
            }),
            readonly: false,
        }),
        true_type: Arc::new(TypeExpr::Ref {
            name: Arc::from("E"),
            type_arguments: verter_type_expr::empty_type_args(),
        }),
        false_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Never)),
    };
    let handle = lower_type_expr_structural(&graph, &expr, fixture_scope(), &ctx)
        .expect("structural lowering should succeed");
    match &*node(&graph, handle.node()) {
        SemanticNodeData::Conditional {
            extends,
            true_branch_ref,
            ..
        } => {
            let nested_infer = match &*node(&graph, *extends) {
                SemanticNodeData::Array { element, .. } => *element,
                other => panic!("expected Array extends, got {other:?}"),
            };
            assert!(
                matches!(&*node(&graph, nested_infer), SemanticNodeData::Infer { name } if name.as_ref() == "E"),
                "the Array element is the `infer E` carrier"
            );
            assert_eq!(
                *true_branch_ref, nested_infer,
                "the true-branch `E` binds to the nested `infer E` carrier"
            );
            assert!(
                !matches!(
                    &*node(&graph, *true_branch_ref),
                    SemanticNodeData::BareRef { .. }
                ),
                "a bound nested infer name must NOT leak out as a BareRef"
            );
        }
        other => panic!("expected Conditional, got {other:?}"),
    }
}

#[test]
fn conditional_binds_object_member_infer_in_true_branch() {
    // `T extends { a: infer P } ? P : never` — the `infer P` nested in an
    // object-type member of the `extends` clause binds `P` for the TRUE
    // branch (TS scoping). The true-branch `P` resolves to the SAME `Infer`
    // carrier the object member lowered to, NOT an unbound
    // `BareRef { name: "P" }`. (This is exactly the composite-extends shape
    // the eager binder `collect_infer_bindings_into_env` covers via its
    // `Object` arm — the structural collector must reach the same coverage.)
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let binders: [BinderScope; 0] = [];
    let ctx = StructuralLowerContext::new(&binders);
    let expr = TypeExpr::Conditional {
        check: Arc::new(TypeExpr::Ref {
            name: Arc::from("T"),
            type_arguments: verter_type_expr::empty_type_args(),
        }),
        extends: Arc::new(TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
                "a".to_string(),
                TypeExpr::Infer {
                    name: "P".to_string(),
                },
                false,
                false,
            ))],
        }))),
        true_type: Arc::new(TypeExpr::Ref {
            name: Arc::from("P"),
            type_arguments: verter_type_expr::empty_type_args(),
        }),
        false_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Never)),
    };
    let handle = lower_type_expr_structural(&graph, &expr, fixture_scope(), &ctx)
        .expect("structural lowering should succeed");
    match &*node(&graph, handle.node()) {
        SemanticNodeData::Conditional {
            extends,
            true_branch_ref,
            ..
        } => {
            let member_infer = match &*node(&graph, *extends) {
                SemanticNodeData::Object(view) => {
                    assert_eq!(view.members.len(), 1);
                    view.members[0].value
                }
                other => panic!("expected Object extends, got {other:?}"),
            };
            assert!(
                matches!(&*node(&graph, member_infer), SemanticNodeData::Infer { name } if name.as_ref() == "P"),
                "the object member `a` is the `infer P` carrier"
            );
            assert_eq!(
                *true_branch_ref, member_infer,
                "the true-branch `P` binds to the object-member `infer P` carrier"
            );
            assert!(
                !matches!(
                    &*node(&graph, *true_branch_ref),
                    SemanticNodeData::BareRef { .. }
                ),
                "a bound object-member infer name must NOT leak out as a BareRef"
            );
        }
        other => panic!("expected Conditional, got {other:?}"),
    }
}

#[test]
fn conditional_binds_function_param_infer_in_true_branch() {
    // `T extends (x: infer P) => any ? P : never` — the `infer P` in a
    // function-parameter type of the `extends` clause binds `P` for the TRUE
    // branch (TS scoping). The true-branch `P` resolves to the SAME `Infer`
    // carrier the function parameter lowered to, NOT an unbound
    // `BareRef { name: "P" }`. (The eager binder covers this via its
    // `Function` arm; the structural collector must match it.)
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let binders: [BinderScope; 0] = [];
    let ctx = StructuralLowerContext::new(&binders);
    let func = FunctionExpr::synthetic(
        vec![FunctionParam::synthetic(
            Some("x".to_string()),
            TypeExpr::Infer {
                name: "P".to_string(),
            },
            false,
            false,
        )],
        Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Any))),
        vec![],
    );
    let expr = TypeExpr::Conditional {
        check: Arc::new(TypeExpr::Ref {
            name: Arc::from("T"),
            type_arguments: verter_type_expr::empty_type_args(),
        }),
        extends: Arc::new(TypeExpr::Function(Arc::new(func))),
        true_type: Arc::new(TypeExpr::Ref {
            name: Arc::from("P"),
            type_arguments: verter_type_expr::empty_type_args(),
        }),
        false_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Never)),
    };
    let handle = lower_type_expr_structural(&graph, &expr, fixture_scope(), &ctx)
        .expect("structural lowering should succeed");
    match &*node(&graph, handle.node()) {
        SemanticNodeData::Conditional {
            extends,
            true_branch_ref,
            ..
        } => {
            let param_infer = match &*node(&graph, *extends) {
                SemanticNodeData::Function { params, .. } => {
                    assert_eq!(params.len(), 1);
                    params[0].ty
                }
                other => panic!("expected Function extends, got {other:?}"),
            };
            assert!(
                matches!(&*node(&graph, param_infer), SemanticNodeData::Infer { name } if name.as_ref() == "P"),
                "the function parameter `x` is the `infer P` carrier"
            );
            assert_eq!(
                *true_branch_ref, param_infer,
                "the true-branch `P` binds to the function-param `infer P` carrier"
            );
            assert!(
                !matches!(
                    &*node(&graph, *true_branch_ref),
                    SemanticNodeData::BareRef { .. }
                ),
                "a bound function-param infer name must NOT leak out as a BareRef"
            );
        }
        other => panic!("expected Conditional, got {other:?}"),
    }
}

#[test]
fn conditional_binds_mapped_as_remap_infer_in_true_branch() {
    // `T extends { [K in S as infer R]: number } ? R : never` — the `infer R`
    // in the mapped `as` remap (the `name_type`) of the `extends` clause binds
    // `R` for the TRUE branch (TS scoping). The collector must descend the
    // mapped source + value AND the `as`-remap `name_type`, so the true-branch
    // `R` resolves to the SAME `Infer` carrier the remap lowered to, NOT an
    // unbound `BareRef { name: "R" }`. The mapped name-remap is the structural
    // SUPERSET the eager binder does not cover — the correct structural fidelity
    // for the carrier graph per the carrier-resolution ruling. FAILS without the
    // fix: the collector descended source/value only, so `R` was never collected
    // and leaked as a `BareRef`.
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let binders: [BinderScope; 0] = [];
    let ctx = StructuralLowerContext::new(&binders);
    let expr = TypeExpr::Conditional {
        check: Arc::new(TypeExpr::Ref {
            name: Arc::from("T"),
            type_arguments: verter_type_expr::empty_type_args(),
        }),
        extends: Arc::new(TypeExpr::Mapped {
            parameter: "K".to_string(),
            source: Arc::new(TypeExpr::Ref {
                name: Arc::from("S"),
                type_arguments: verter_type_expr::empty_type_args(),
            }),
            value: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
            optional: MappedModifier::None,
            readonly: MappedModifier::None,
            name_type: Some(Arc::new(TypeExpr::Infer {
                name: "R".to_string(),
            })),
        }),
        true_type: Arc::new(TypeExpr::Ref {
            name: Arc::from("R"),
            type_arguments: verter_type_expr::empty_type_args(),
        }),
        false_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Never)),
    };
    let handle = lower_type_expr_structural(&graph, &expr, fixture_scope(), &ctx)
        .expect("structural lowering should succeed");
    match &*node(&graph, handle.node()) {
        SemanticNodeData::Conditional {
            true_branch_ref, ..
        } => {
            assert!(
                matches!(&*node(&graph, *true_branch_ref), SemanticNodeData::Infer { name } if name.as_ref() == "R"),
                "the true-branch `R` binds to the mapped `as infer R` carrier, got {:?}",
                &*node(&graph, *true_branch_ref)
            );
            assert!(
                !matches!(
                    &*node(&graph, *true_branch_ref),
                    SemanticNodeData::BareRef { .. }
                ),
                "a bound mapped-remap infer name must NOT leak out as a BareRef"
            );
        }
        other => panic!("expected Conditional, got {other:?}"),
    }
}

#[test]
fn lowers_interface_heritage_preserving_ref_args_and_member_provenance() {
    // `interface Props extends Base<string> { own: number }` is the type
    // `Base<string> & { own: number }`: the heritage reference keeps its
    // args as a `BareRef`, and the own-body member carries its declaration
    // provenance + merge role.
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let base_ref = TypeExpr::Ref {
        name: Arc::from("Base"),
        type_arguments: Arc::from(
            vec![TypeExpr::Primitive(PrimitiveName::String)].into_boxed_slice(),
        ),
    };
    let own_body = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
            "own".to_string(),
            TypeExpr::Primitive(PrimitiveName::Number),
            false,
            false,
        ))],
    }));
    let expr = TypeExpr::Intersection(Arc::from(vec![base_ref, own_body].into_boxed_slice()));
    let binders: [BinderScope; 0] = [];
    // The consuming declaration's own-body role.
    let ctx = StructuralLowerContext::new(&binders).with_merge_role(MemberMergeRole::OwnBody);
    let handle = lower_type_expr_structural(&graph, &expr, fixture_scope(), &ctx)
        .expect("structural lowering should succeed");
    let arms: Arc<[SemanticNodeId]> = match &*node(&graph, handle.node()) {
        SemanticNodeData::Intersection(arms) => Arc::clone(arms),
        other => panic!("expected Intersection, got {other:?}"),
    };
    assert_eq!(arms.len(), 2, "heritage ref arm + own-body object arm");
    match &*node(&graph, arms[0]) {
        SemanticNodeData::BareRef {
            name, type_args, ..
        } => {
            assert_eq!(name.as_ref(), "Base");
            assert_eq!(type_args.len(), 1, "heritage `Base<string>` keeps its arg");
            assert!(matches!(
                &*node(&graph, type_args[0]),
                SemanticNodeData::Primitive(PrimitiveKind::String)
            ));
        }
        other => panic!("expected heritage BareRef, got {other:?}"),
    }
    match &*node(&graph, arms[1]) {
        SemanticNodeData::Object(view) => {
            assert_eq!(view.members.len(), 1);
            let m = &view.members[0];
            assert_eq!(m.name.as_ref(), "own");
            assert_eq!(
                m.merge_role,
                MemberMergeRole::OwnBody,
                "the own member carries the own-body merge role"
            );
            assert_eq!(
                m.declaration_origin.as_deref(),
                Some("/fixture.ts"),
                "the member is declared in the object's lowering file"
            );
            assert!(!m.declared_in_macro_type_arg);
            assert!(matches!(
                &*node(&graph, m.value),
                SemanticNodeData::Primitive(PrimitiveKind::Number)
            ));
        }
        other => panic!("expected own-body Object, got {other:?}"),
    }
}

#[test]
fn lowers_mapped_type_as_deferred_shell() {
    // `{ readonly [K in keyof Foo]: number }` → deferred `Mapped` shell: the
    // `keyof Foo` source unwraps to source = Foo with a `keyof` key space, the
    // binder `K` is a mapper `TypeParam`, and the `readonly +` modifier maps
    // through. NEVER materialized into per-key members.
    let host = VerterHost::new_standalone(Default::default());
    let expr = TypeExpr::Mapped {
        parameter: "K".to_string(),
        source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::Ref {
            name: Arc::from("Foo"),
            type_arguments: verter_type_expr::empty_type_args(),
        }))),
        value: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
        optional: MappedModifier::None,
        readonly: MappedModifier::Add,
        name_type: None,
    };
    let (graph, root) = lower_root(&host, &expr);
    match &*node(&graph, root) {
        SemanticNodeData::Mapped { source, mapper } => {
            assert!(
                matches!(&*node(&graph, *source), SemanticNodeData::BareRef { .. }),
                "the keyof source unwraps to the underlying T"
            );
            match &*node(&graph, mapper.key_space) {
                SemanticNodeData::KeyOf { base } => {
                    assert_eq!(*base, *source, "key space is `keyof` over the same source")
                }
                other => panic!("expected KeyOf key space, got {other:?}"),
            }
            assert!(
                matches!(
                    &*node(&graph, mapper.parameter_node),
                    SemanticNodeData::TypeParam { .. }
                ),
                "the mapper parameter is interned as a TypeParam binder"
            );
            assert!(matches!(
                &*node(&graph, mapper.value_expr),
                SemanticNodeData::Primitive(PrimitiveKind::Number)
            ));
            assert_eq!(mapper.readonly, ReadonlyMod::Add);
            assert_eq!(mapper.optionality, OptionalityMod::Keep);
            assert!(mapper.name_remap.is_none());
        }
        other => panic!("expected Mapped, got {other:?}"),
    }
}

#[test]
fn structural_root_is_an_unmaterialized_carrier() {
    // Runtime half of `unresolved_carriers_not_materialized_during_emission`:
    // the structural lowerer EMITS carriers and never raises / materializes
    // them back during emission. Lowering `Foo<Bar>` and an import type leaves
    // the root node a `BareRef` / `ImportType` carrier — NOT a reparsed /
    // materialized fallback.
    let host = VerterHost::new_standalone(Default::default());
    let generic = TypeExpr::Ref {
        name: Arc::from("Foo"),
        type_arguments: Arc::from(
            vec![TypeExpr::Ref {
                name: Arc::from("Bar"),
                type_arguments: verter_type_expr::empty_type_args(),
            }]
            .into_boxed_slice(),
        ),
    };
    let (graph, root) = lower_root(&host, &generic);
    assert!(
        matches!(&*node(&graph, root), SemanticNodeData::BareRef { .. }),
        "the `Foo<Bar>` root stays a BareRef carrier, not a materialized fallback"
    );
    let import = TypeExpr::ImportType {
        specifier: Arc::from("./m"),
        qualifier: Arc::from(vec![Arc::<str>::from("Box")].into_boxed_slice()),
        typeof_query: false,
        type_arguments: verter_type_expr::empty_type_args(),
    };
    let (graph, root) = lower_root(&host, &import);
    assert!(
        matches!(&*node(&graph, root), SemanticNodeData::ImportType { .. }),
        "the import-type root stays an ImportType carrier, not a materialized fallback"
    );
}

// --- Structural-equivalence set (set A) ---------------------------------
// For a shape that needs NO resolution — primitives, literals, objects (with
// member spans / origin / flags), unions, intersections, arrays, tuples
// (incl. `rest`), functions (params / return), template literals, transparent
// parenthesized types, and composites of those — the eager path and the
// query-free structural path must build the SAME graph. "No resolution
// needed" means the eager path produces no `DeclRef` / `InstantiationRef` /
// import result / host reduction: every operand is a structural terminal or
// composite, never a bare name.
//
// A function's generic HEAD (its type-param constraint/default lowering) is
// deliberately NOT part of this equivalence set: it INTENTIONALLY diverges.
// The eager path lowers a binder-node's constraint/default under the OUTER
// env, whereas the structural lowerer lowers them INCREMENTALLY (each head
// binder sees the prior binders, per TS scoping — arguably more TS-correct).
// So a no-resolution generic head like `<T, U extends T>(y: U) => void`
// interns to a DIFFERENT id across the two paths and is pinned by the carrier
// fixtures `generic_function_constraint_sees_prior_type_param_binder` /
// `generic_function_default_sees_prior_type_param_binder`, NOT by this
// structural-equivalence set.
//
// Both paths lower into the SAME content-addressed semantic-graph store
// under the SAME scope, so the node arena's `(data, scope)` hash-consing
// makes structural equivalence directly observable as interned-id equality:
// equivalent trees collapse onto the one interned id, while ANY divergence in
// variant, child, surface flag, or scope yields a different dedup key and a
// different id. The id-equality assertion is therefore the normalized
// structural-snapshot comparison the equivalence set calls for — and it is
// maximally discriminating: it would fail if the structural lowerer emitted
// even a subtly different shape (a guarantee `structural_equivalence_\
// assertion_is_discriminating` proves by construction, so the agreement
// fixtures below are not vacuous).

/// Lower `expr` through the OLD eager path into the host's shared graph.
///
/// Empty type-param env / name-resolution / shadowing and a non-reducing
/// structural reduction context, so a no-resolution shape lowers to its pure
/// structural graph with nothing to resolve, substitute, or reduce.
fn lower_eager(host: &VerterHost, expr: &TypeExpr) -> SemanticNodeId {
    let dispatch = ProjectSemanticDispatch::new(host);
    let env = FxHashMap::default();
    let name_resolution = FxHashMap::default();
    let shadowing = ScopeShadowing::empty();
    let mut substitutions = Vec::new();
    dispatch.shallow_lower_type_expr_with_context(
        expr,
        &env,
        &fixture_scope(),
        &name_resolution,
        None,
        &shadowing,
        &mut substitutions,
        ProjectionReductionContext::structural_transit(),
    )
}

/// Lower `expr` via BOTH paths into the host's single shared store and assert
/// they intern to the SAME node (structural equivalence). Returns the shared
/// graph + root id so the caller can additionally assert the root variant.
#[track_caller]
fn assert_paths_agree(
    host: &VerterHost,
    expr: &TypeExpr,
) -> (Arc<SemanticGraphStore>, SemanticNodeId) {
    let (graph, structural) = lower_root(host, expr);
    let eager = lower_eager(host, expr);
    assert_eq!(
        eager, structural,
        "the eager and query-free structural paths must build the SAME \
         interned graph for a no-resolution shape (hash-consed by content)"
    );
    (graph, structural)
}

#[test]
fn structural_equivalence_for_primitive() {
    let host = VerterHost::new_standalone(Default::default());
    let expr = TypeExpr::Primitive(PrimitiveName::String);
    let (graph, root) = assert_paths_agree(&host, &expr);
    assert!(matches!(
        &*node(&graph, root),
        SemanticNodeData::Primitive(PrimitiveKind::String)
    ));
}

#[test]
fn structural_equivalence_for_literal() {
    let host = VerterHost::new_standalone(Default::default());
    let expr = TypeExpr::Literal(LiteralValue::String("lit".to_string()));
    let (graph, root) = assert_paths_agree(&host, &expr);
    assert!(matches!(
        &*node(&graph, root),
        SemanticNodeData::Literal(LiteralValue::String(s)) if s == "lit"
    ));
}

#[test]
fn structural_equivalence_for_union() {
    // `string | number` — a union of structural terminals, nothing to resolve.
    let host = VerterHost::new_standalone(Default::default());
    let expr = TypeExpr::Union(Arc::from(
        vec![
            TypeExpr::Primitive(PrimitiveName::String),
            TypeExpr::Primitive(PrimitiveName::Number),
        ]
        .into_boxed_slice(),
    ));
    let (graph, root) = assert_paths_agree(&host, &expr);
    assert!(
        matches!(&*node(&graph, root), SemanticNodeData::Union(arms) if arms.len() == 2),
        "the agreed root is the structural two-arm union shell"
    );
}

#[test]
fn structural_equivalence_for_intersection_of_objects() {
    // `{ a: string } & { b: number }` — an intersection of object literals,
    // every operand structural (no bare-name heritage to resolve).
    let host = VerterHost::new_standalone(Default::default());
    let obj = |name: &str, prim: PrimitiveName| {
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
                name.to_string(),
                TypeExpr::Primitive(prim),
                false,
                false,
            ))],
        }))
    };
    let expr = TypeExpr::Intersection(Arc::from(
        vec![
            obj("a", PrimitiveName::String),
            obj("b", PrimitiveName::Number),
        ]
        .into_boxed_slice(),
    ));
    let (graph, root) = assert_paths_agree(&host, &expr);
    assert!(
        matches!(&*node(&graph, root), SemanticNodeData::Intersection(arms) if arms.len() == 2),
        "the agreed root is the structural two-arm intersection shell"
    );
}

#[test]
fn structural_equivalence_for_array() {
    let host = VerterHost::new_standalone(Default::default());
    let expr = TypeExpr::Array {
        element: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
        readonly: false,
    };
    let (graph, root) = assert_paths_agree(&host, &expr);
    assert!(matches!(
        &*node(&graph, root),
        SemanticNodeData::Array { .. }
    ));
}

#[test]
fn structural_equivalence_for_tuple_with_rest() {
    // `[head: string, ...tail: number[]]` — labels, the rest flag, and the
    // nested `number[]` must agree element-for-element across both paths.
    let host = VerterHost::new_standalone(Default::default());
    let expr = TypeExpr::Tuple {
        elements: Arc::from(
            vec![
                verter_type_expr::TupleElement {
                    label: Some("head".to_string()),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    rest: false,
                },
                verter_type_expr::TupleElement {
                    label: Some("tail".to_string()),
                    ty: TypeExpr::Array {
                        element: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
                        readonly: false,
                    },
                    optional: false,
                    rest: true,
                },
            ]
            .into_boxed_slice(),
        ),
        readonly: false,
    };
    let (graph, root) = assert_paths_agree(&host, &expr);
    assert!(
        matches!(&*node(&graph, root), SemanticNodeData::Tuple { elements, .. } if elements.len() == 2 && elements[1].rest),
        "the agreed tuple preserves both elements and the rest flag"
    );
}

#[test]
fn structural_equivalence_for_function() {
    // `(x: number) => string` — params and return are structural terminals.
    let host = VerterHost::new_standalone(Default::default());
    let func = FunctionExpr::synthetic(
        vec![FunctionParam::synthetic(
            Some("x".to_string()),
            TypeExpr::Primitive(PrimitiveName::Number),
            false,
            false,
        )],
        Some(Arc::new(TypeExpr::Primitive(PrimitiveName::String))),
        vec![],
    );
    let expr = TypeExpr::Function(Arc::new(func));
    let (graph, root) = assert_paths_agree(&host, &expr);
    assert!(matches!(
        &*node(&graph, root),
        SemanticNodeData::Function { params, .. } if params.len() == 1
    ));
}

#[test]
fn structural_equivalence_for_object_with_members() {
    // `{ a: string; b: number }` — both paths must agree on the object's
    // members down to their spans / declaration-origin / flags (all part of
    // the interned `SurfaceMember`, so id-equality compares them exactly).
    let host = VerterHost::new_standalone(Default::default());
    let expr = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![
            ObjectMember::Property(ObjectProperty::synthetic_public(
                "a".to_string(),
                TypeExpr::Primitive(PrimitiveName::String),
                false,
                false,
            )),
            ObjectMember::Property(ObjectProperty::synthetic_public(
                "b".to_string(),
                TypeExpr::Primitive(PrimitiveName::Number),
                false,
                false,
            )),
        ],
    }));
    let (graph, root) = assert_paths_agree(&host, &expr);
    assert!(
        matches!(&*node(&graph, root), SemanticNodeData::Object(view) if view.members.len() == 2),
        "the agreed root is the structural object with both members"
    );
}

#[test]
fn structural_equivalence_for_template_literal() {
    // `` `a${string}b` `` — quasis plus a structural interpolation.
    let host = VerterHost::new_standalone(Default::default());
    let expr = TypeExpr::TemplateLiteral {
        quasis: vec!["a".to_string(), "b".to_string()],
        expressions: Arc::from(vec![TypeExpr::Primitive(PrimitiveName::String)].into_boxed_slice()),
    };
    let (graph, root) = assert_paths_agree(&host, &expr);
    assert!(matches!(
        &*node(&graph, root),
        SemanticNodeData::TemplateLiteral { .. }
    ));
}

#[test]
fn structural_equivalence_for_parenthesized_is_transparent() {
    // A parenthesized type is structurally transparent: `(string | number)`
    // lowers identically to the bare `string | number` on BOTH paths, so the
    // parenthesized and unparenthesized roots are the one interned union.
    let host = VerterHost::new_standalone(Default::default());
    let inner = TypeExpr::Union(Arc::from(
        vec![
            TypeExpr::Primitive(PrimitiveName::String),
            TypeExpr::Primitive(PrimitiveName::Number),
        ]
        .into_boxed_slice(),
    ));
    let parenthesized = TypeExpr::Parenthesized(Arc::new(inner.clone()));
    let (_g, paren_root) = assert_paths_agree(&host, &parenthesized);
    let (_g2, bare_root) = lower_root(&host, &inner);
    assert_eq!(
        paren_root, bare_root,
        "parentheses are transparent: `(A | B)` and `A | B` intern identically"
    );
}

#[test]
fn structural_equivalence_for_nested_composite() {
    // `{ a: string }[] | [string]` — a composite of composites exercises the
    // recursion: the union, the array, the nested object, and the tuple must
    // all agree across both paths.
    let host = VerterHost::new_standalone(Default::default());
    let object_array = TypeExpr::Array {
        element: Arc::new(TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
                "a".to_string(),
                TypeExpr::Primitive(PrimitiveName::String),
                false,
                false,
            ))],
        }))),
        readonly: false,
    };
    let string_tuple = TypeExpr::Tuple {
        elements: Arc::from(
            vec![verter_type_expr::TupleElement {
                label: None,
                ty: TypeExpr::Primitive(PrimitiveName::String),
                optional: false,
                rest: false,
            }]
            .into_boxed_slice(),
        ),
        readonly: false,
    };
    let expr = TypeExpr::Union(Arc::from(
        vec![object_array, string_tuple].into_boxed_slice(),
    ));
    let (graph, root) = assert_paths_agree(&host, &expr);
    assert!(matches!(
        &*node(&graph, root),
        SemanticNodeData::Union(arms) if arms.len() == 2
    ));
}

#[test]
fn structural_equivalence_assertion_is_discriminating() {
    // Teeth for the whole set: id-equality is NOT vacuous. Two structurally
    // DIFFERENT no-resolution shapes do NOT share an interned id, so
    // `assert_paths_agree` would FAIL if the structural lowerer diverged from
    // the eager path on any of the agreement fixtures above.
    let host = VerterHost::new_standalone(Default::default());
    let string_or_number = TypeExpr::Union(Arc::from(
        vec![
            TypeExpr::Primitive(PrimitiveName::String),
            TypeExpr::Primitive(PrimitiveName::Number),
        ]
        .into_boxed_slice(),
    ));
    let string_or_boolean = TypeExpr::Union(Arc::from(
        vec![
            TypeExpr::Primitive(PrimitiveName::String),
            TypeExpr::Primitive(PrimitiveName::Boolean),
        ]
        .into_boxed_slice(),
    ));
    // Eager `string | number` vs structural `string | boolean`: a single
    // differing arm must move the interned id.
    let eager = lower_eager(&host, &string_or_number);
    let (_g, structural_other) = lower_root(&host, &string_or_boolean);
    assert_ne!(
        eager, structural_other,
        "a one-arm structural difference must intern to a DIFFERENT id — \
         proving the id-equality methodology discriminates structure"
    );
    // And the matching structural `string | number` DOES agree, so the
    // inequality above is about structure, not about the two paths being
    // unable to ever agree.
    let structural_same = lower_root(&host, &string_or_number).1;
    assert_eq!(
        eager, structural_same,
        "the SAME shape still agrees across paths"
    );
}
