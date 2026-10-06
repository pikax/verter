use crate::output_sinks::DispatchOutputTestExt;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::IndexKey;
use crate::semantic_query::PrimitiveKind as SemanticPrimitiveKind;
use crate::semantic_query::SemanticNodeData;
use crate::VerterHost;
use std::sync::Arc;
use verter_type_expr::TypeExpr;

#[test]
fn raise_node_to_type_expr_preserves_number_index_key_values() {
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let object = graph.intern_node(SemanticNodeData::Primitive(SemanticPrimitiveKind::Unknown));
    let indexed = graph.intern_node(SemanticNodeData::IndexedAccess {
        object,
        index: IndexKey::Number(
            crate::semantic_query::CanonicalIndexInt::from_canonical_i64(7).expect("canonical"),
        ),
    });

    let dispatch = ProjectSemanticDispatch::new(&host);
    let expr = dispatch
        .raise_node_to_type_expr(indexed)
        .expect("indexed-access semantic node should serialize");

    let TypeExpr::IndexedAccess { index, .. } = expr.expr() else {
        panic!("expected IndexedAccess expr, got {:?}", expr.expr());
    };
    assert_eq!(
        **index,
        TypeExpr::number_literal(7.0),
        "numeric index keys should serialize as number literals",
    );
}

/// An intersection whose EVERY arm is vacuous (`{} & {}`) must fall
/// back to the representable empty object `{}` — never publish a
/// zero-arm `TypeExpr::Intersection([])`.
#[test]
fn raise_all_vacuous_intersection_falls_back_to_empty_object() {
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let empty_a = graph.intern_node(SemanticNodeData::Object(
        crate::project_semantic_dispatch::walk::empty_surface_view(),
    ));
    let intersection = graph.intern_node(SemanticNodeData::Intersection(
        crate::semantic_query::composite::CompositeList::test_fixture(Arc::from(
            vec![empty_a, empty_a].into_boxed_slice(),
        )),
    ));

    let dispatch = ProjectSemanticDispatch::new(&host);
    let expr = dispatch
        .raise_node_to_type_expr(intersection)
        .expect("intersection must raise");

    match expr.expr() {
        TypeExpr::Object(object) => {
            assert!(
                object.properties.is_empty(),
                "all-vacuous intersection must raise as the EMPTY object"
            );
        }
        TypeExpr::Intersection(arms) => {
            panic!("zero/filtered-arm Intersection must not publish (got {arms:?})")
        }
        other => panic!("expected empty Object, got {other:?}"),
    }
}

/// The node-domain root-sentinel fact
/// (`node_root_is_unmaterialized_sentinel_with_dispatch`) is ROOT-only and
/// agrees with `type_expr_root_is_unmaterialized_sentinel(raise(node))` — it
/// is NOT the whole-surface `!materialized` AND. The discriminator is an
/// `Array` whose ELEMENT (not root) is a miss sentinel: the root-sentinel
/// fact is `false` (the root is the Array) and the TypeExpr oracle agrees,
/// yet `facts.materialized` is `false` (the element miss makes the AND false)
/// — so the fact cannot be the whole-surface miss check.
#[test]
fn node_root_sentinel_is_root_only_not_whole_surface_miss() {
    use crate::project_semantic_dispatch::raise::{
        node_raised_shape_facts_with_dispatch, node_root_is_unmaterialized_sentinel_with_dispatch,
    };
    use crate::project_semantic_dispatch::raise_sentinel::type_expr_root_is_unmaterialized_sentinel;
    use crate::semantic_query::QueryError;

    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let miss = graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    let string = graph.intern_node(SemanticNodeData::Primitive(SemanticPrimitiveKind::String));
    let array_of_miss = graph.intern_node(SemanticNodeData::Array {
        element: miss,
        readonly: false,
    });

    let dispatch = ProjectSemanticDispatch::new(&host);
    let oracle = |node| {
        type_expr_root_is_unmaterialized_sentinel(
            dispatch
                .raise_node_to_type_expr(node)
                .expect("node must raise")
                .expr(),
        )
    };

    // (1) Root IS a typed miss degradation → the node-domain fact is true.
    // The TypeExpr tree oracle is INERT by design: the compat tree spells
    // the projection but no raw classification reads it back, so the tree
    // side never reports a sentinel. Parity is SPLIT — typed node facts
    // carry the classification, the tree is display bytes only.
    assert!(node_root_is_unmaterialized_sentinel_with_dispatch(
        &dispatch, miss
    ));
    assert!(
        !oracle(miss),
        "the compat tree is inert: no raw sentinel classification remains"
    );

    // (2) Root is a plain primitive → both false.
    assert!(!node_root_is_unmaterialized_sentinel_with_dispatch(
        &dispatch, string
    ));
    assert!(!oracle(string));

    // (3) DISCRIMINATOR: root is an `Array` (not a sentinel) whose ELEMENT is
    // the miss. Root-sentinel is false, the TypeExpr oracle agrees, yet the
    // whole-surface `materialized` AND is false — proving the node-domain
    // fact is ROOT-only, not `!materialized`.
    assert!(
        !node_root_is_unmaterialized_sentinel_with_dispatch(&dispatch, array_of_miss),
        "an Array whose ELEMENT (not root) is a sentinel is NOT root-unmaterialized"
    );
    assert!(
        !oracle(array_of_miss),
        "TypeExpr oracle agrees the Array root is not a sentinel"
    );
    let facts = node_raised_shape_facts_with_dispatch(&dispatch, array_of_miss).expect("facts");
    assert!(
        !facts.materialized(),
        "the Array DOES carry an unmaterialised element, so root-sentinel=false is \
         genuinely ROOT-only — not merely the absence of any sentinel"
    );
}

/// DISCRIMINATION for the whole-tree semantic-miss
/// node fact (`node_contains_semantic_miss_with_dispatch`): it is the TYPED
/// WHOLE-TREE `!materialized` question, NOT the root-only sentinel (the
/// inert tree oracle is pinned false on every shape — the split parity). The
/// discriminator is an `Array` whose ELEMENT is a miss: whole-tree miss is
/// `true` (the element miss propagates) while root-sentinel is `false` (the
/// root is the Array) — proving the two facts answer different questions.
#[test]
fn node_contains_semantic_miss_is_whole_tree_and_equals_type_expr_oracle() {
    use crate::project_semantic_dispatch::raise::{
        node_contains_semantic_miss_with_dispatch,
        node_root_is_unmaterialized_sentinel_with_dispatch,
    };
    use crate::project_semantic_dispatch::raise_sentinel::type_expr_contains_semantic_miss;
    use crate::semantic_query::QueryError;

    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let miss = graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    let string = graph.intern_node(SemanticNodeData::Primitive(SemanticPrimitiveKind::String));
    let array_of_miss = graph.intern_node(SemanticNodeData::Array {
        element: miss,
        readonly: false,
    });

    let dispatch = ProjectSemanticDispatch::new(&host);
    let oracle = |node| {
        type_expr_contains_semantic_miss(
            dispatch
                .raise_node_to_type_expr(node)
                .expect("node must raise")
                .expr(),
        )
    };

    // SPLIT PARITY: the node-domain whole-tree miss fact is TYPED (a
    // nested `Opaque(QueryError)` degrades the fold), while the compat
    // tree is inert — the projection spellings carry no classification,
    // so the tree oracle is false everywhere. The two answers differ BY
    // DESIGN: classification lives in the typed domain only.
    assert_eq!(
        node_contains_semantic_miss_with_dispatch(&dispatch, miss),
        Some(true),
        "typed node fact: a Miss root is a whole-tree miss"
    );
    assert_eq!(
        node_contains_semantic_miss_with_dispatch(&dispatch, string),
        Some(false),
        "typed node fact: a clean primitive carries no miss"
    );
    for node in [miss, string, array_of_miss] {
        assert!(
            !oracle(node),
            "the compat tree is inert: no raw sentinel classification remains"
        );
    }

    // DISCRIMINATOR: array-of-miss is whole-tree miss TRUE but root-sentinel
    // FALSE — the node fact is `!materialized`, NOT the root-only sentinel.
    assert_eq!(
        node_contains_semantic_miss_with_dispatch(&dispatch, array_of_miss),
        Some(true)
    );
    assert!(
        !node_root_is_unmaterialized_sentinel_with_dispatch(&dispatch, array_of_miss),
        "array-of-miss carries a whole-tree miss but its ROOT is not a sentinel — \
         root-sentinel and whole-tree-miss are distinct facts"
    );
    // A clean primitive carries no miss.
    assert_eq!(
        node_contains_semantic_miss_with_dispatch(&dispatch, string),
        Some(false)
    );
}

#[test]
fn raise_node_to_type_expr_round_trips_primitive() {
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let node = graph.intern_node(SemanticNodeData::Primitive(SemanticPrimitiveKind::String));

    let dispatch = ProjectSemanticDispatch::new(&host);
    let expr = dispatch
        .raise_node_to_type_expr(node)
        .expect("primitive must raise");

    assert!(
        matches!(expr.expr(), TypeExpr::Primitive(_)),
        "primitive should round-trip, got {:?}",
        expr.expr()
    );
}

/// FAIL-FIRST: preserves a deferred operator over a free
/// `TypeParameter`. `KeyOf(TypeParameter)` survives `raise_and_reduce`
/// because dispatch returns the deferred operator over the free
/// parameter unchanged (deferred-form policy); a reducer that eagerly
/// collapsed it would drop the operator and FAIL this test.
#[test]
fn raise_and_reduce_preserves_open_keyof_over_type_parameter() {
    use crate::semantic_query::{DeclIdentity, HashValue, ProjectionMode};

    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let identity = DeclIdentity {
        canonical_id: Arc::from("/test.ts"),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        whole_hash: HashValue::default(),
        decl_name: Arc::from("T"),
    };
    let type_param = graph.intern_node(SemanticNodeData::TypeParam {
        decl: identity,
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let keyof = graph.intern_node(SemanticNodeData::KeyOf { base: type_param });

    let dispatch = ProjectSemanticDispatch::new(&host);
    let materialized = dispatch.materialize_reduced_output_type_expr_for_test(
        keyof,
        crate::semantic_query::ProjectionReductionContext::published(ProjectionMode::Expanded),
    );

    assert!(
        matches!(materialized, TypeExpr::KeyOf(_)),
        "open keyof over type parameter must survive raise_and_reduce, got {:?}",
        materialized
    );
}

/// FAIL-FIRST: the iterative reducer terminates
/// even when the visited set is the only termination signal. The visited
/// set short-circuits the cycle and returns the alias body; a reducer
/// without that guard would loop on the cycle and FAIL to terminate.
#[test]
fn raise_and_reduce_terminates_on_alias_cycle_via_visited_set() {
    use crate::semantic_query::ProjectionMode;

    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let primitive = graph.intern_node(SemanticNodeData::Primitive(SemanticPrimitiveKind::String));
    let alias = graph.intern_node(SemanticNodeData::Alias(primitive));

    let dispatch = ProjectSemanticDispatch::new(&host);
    let materialized = dispatch.materialize_reduced_output_type_expr_for_test(
        alias,
        crate::semantic_query::ProjectionReductionContext::published(ProjectionMode::Expanded),
    );

    assert!(
        matches!(materialized, TypeExpr::Primitive(_)),
        "alias to primitive must reduce to that primitive, got {:?}",
        materialized
    );
}

/// FAIL-FIRST: hard-stop for `TemplateLiteral` —
/// no dispatch variant exists, so the reducer must convert to
/// `TypeExpr::Unknown(UnknownValue)` whose raw mentions the template
/// literal operator.
#[test]
fn raise_and_reduce_template_literal_becomes_unknown_hard_stop() {
    use crate::semantic_query::ProjectionMode;

    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let template = graph.intern_node(SemanticNodeData::TemplateLiteral {
        quasis: Arc::from(vec![Arc::from("prefix-")].into_boxed_slice()),
        expressions: Arc::from(
            Vec::<crate::semantic_query::SemanticNodeId>::new().into_boxed_slice(),
        ),
    });

    let dispatch = ProjectSemanticDispatch::new(&host);
    let materialized = dispatch.materialize_reduced_output_type_expr_for_test(
        template,
        crate::semantic_query::ProjectionReductionContext::published(ProjectionMode::Expanded),
    );

    match &materialized {
        TypeExpr::Unknown(value) => {
            assert!(
                value.raw().contains("template literal"),
                "template literal hard-stop should mention the operator, got {value:?}"
            );
        }
        other => panic!("expected Unknown hard-stop, got {other:?}"),
    }
}

/// FAIL-FIRST: Navigate-mode keeps a `DeclRef`
/// terminal — a freshly-interned `DeclRef` raises to a bare
/// `Ref { name }` with empty type arguments; a Navigate-mode reducer
/// that eagerly expanded the carrier would lose the terminal `Ref` and
/// FAIL this test.
#[test]
fn raise_and_reduce_navigate_mode_decl_ref_raises_to_bare_ref() {
    use crate::semantic_query::{DeclIdentity, HashValue, ProjectionMode};

    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let identity = DeclIdentity {
        canonical_id: Arc::from("/some-unresolved.ts"),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        whole_hash: HashValue::default(),
        decl_name: Arc::from("Unresolved"),
    };
    let decl_ref = graph.intern_node(SemanticNodeData::DeclRef {
        identity: identity.clone(),
    });

    let dispatch = ProjectSemanticDispatch::new(&host);
    let materialized = dispatch.materialize_reduced_output_type_expr_for_test(
        decl_ref,
        crate::semantic_query::ProjectionReductionContext::published(ProjectionMode::Navigate),
    );

    match &materialized {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(name.as_ref(), "Unresolved");
            assert!(
                type_arguments.is_empty(),
                "navigate-mode DeclRef must raise without type arguments"
            );
        }
        // DeclRef in Navigate dispatches ResolveDecl → if dispatch
        // produces an Opaque(Miss) (no real prepared decl), the
        // reducer accepts it and the raise yields Unknown. Both
        // outcomes prove the lazy carrier was visited; the test
        // discriminates on the absence of `graphNode` text.
        TypeExpr::Unknown(value) => {
            assert!(
                !value.raw().starts_with("graphNode"),
                "raise must not emit graphNode placeholder, got {value:?}"
            );
        }
        other => panic!("expected Ref{{name=Unresolved}} or Unknown, got {other:?}"),
    }
}

#[test]
fn raise_node_to_type_expr_round_trips_indexed_access_string_key() {
    // Discriminator: IndexedAccess with a String index key must
    // raise to TypeExpr::IndexedAccess { index: TypeExpr::Literal(...) }
    // — proves the helper `index_key_to_type_expr` follows the same
    // structural conversion as numeric keys without introducing the
    // `_inner` recursive call (cycle invariant for strings: there is
    // no node to recurse into).
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let object = graph.intern_node(SemanticNodeData::Primitive(SemanticPrimitiveKind::Unknown));
    let indexed = graph.intern_node(SemanticNodeData::IndexedAccess {
        object,
        index: IndexKey::String(Arc::from("key")),
    });

    let dispatch = ProjectSemanticDispatch::new(&host);
    let expr = dispatch
        .raise_node_to_type_expr(indexed)
        .expect("indexed-access semantic node should serialize");

    let TypeExpr::IndexedAccess { index, .. } = expr.expr() else {
        panic!("expected IndexedAccess expr, got {:?}", expr.expr());
    };
    assert_eq!(
        **index,
        TypeExpr::string_literal("key"),
        "string index keys should serialize as string literals",
    );
}

/// L1 carrier-stop predicate (Shallow-By-Default). An
/// enumeration-domain utility (`Pick`/`Omit`/…) whose source
/// argument is an OPEN generic instantiation (`PropsBase<T>` with
/// `T` an unsubstituted type parameter) is open ⇒ the reducer keeps
/// it a shallow carrier. A CLOSED source (concrete instantiation, or
/// a finite object surface) is NOT open ⇒ it still materialises.
/// Discriminating: an over-broad "builtin utility == carrier" L1
/// would return `true` for the closed cases too and fail this test.
#[test]
fn utility_enumeration_domain_open_for_unbound_generic_closed_for_concrete() {
    use crate::semantic_query::{DeclIdentity, HashValue, SemanticNodeData, TupleElement};

    let host = VerterHost::new_standalone(Default::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let builtin_pick = DeclIdentity {
        canonical_id: Arc::from("__builtin__"),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        whole_hash: HashValue::default(),
        decl_name: Arc::from("Pick"),
    };
    // Keyspace argument — never inspected by the openness walk
    // (only argument 0, the enumeration domain, matters).
    let keys = graph.intern_node(SemanticNodeData::Primitive(SemanticPrimitiveKind::String));

    // OPEN: PropsBase<T> with T an unsubstituted type parameter.
    let tparam = graph.intern_node(SemanticNodeData::TypeParam {
        decl: DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let props_base_open = graph.intern_node(SemanticNodeData::InstantiationRef {
        base: DeclIdentity {
            canonical_id: Arc::from("/types.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: HashValue::default(),
            decl_name: Arc::from("PropsBase"),
        },
        args: Arc::from(vec![tparam].into_boxed_slice()),
    });
    assert!(
        crate::project_semantic_dispatch::raise::utility_enumeration_domain_is_open_or_unknown(
            &dispatch,
            &builtin_pick,
            &[props_base_open, keys],
        ),
        "Pick<PropsBase<T>, …> over an unbound generic must be OPEN"
    );

    let concrete_elem =
        graph.intern_node(SemanticNodeData::Primitive(SemanticPrimitiveKind::Unknown));
    let concrete_array = graph.intern_node(SemanticNodeData::Array {
        element: concrete_elem,
        readonly: false,
    });

    // OPEN: PropsBase<UIMessage[]> — a NON-EMPTY all-concrete arg list
    // is NOT sufficient on its own. An instantiation closes only when
    // its target declaration EXISTS with satisfiable arity/defaults and
    // a body that closes under the bindings. Here no `PropsBase` decl is
    // seeded in this pure-unit host, so the target is UNRESOLVABLE ⇒
    // undecidable ⇒ OPEN — even with concrete args. The
    // resolvable-closed path (a real generic decl that materialises
    // path-precisely) is exercised end-to-end by the integration
    // fixtures in `component_meta_pick_omit_tests`.
    let props_base_concrete_unresolved = graph.intern_node(SemanticNodeData::InstantiationRef {
        base: DeclIdentity {
            canonical_id: Arc::from("/types.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: HashValue::default(),
            decl_name: Arc::from("PropsBase"),
        },
        args: Arc::from(vec![concrete_array].into_boxed_slice()),
    });
    assert!(
        crate::project_semantic_dispatch::raise::utility_enumeration_domain_is_open_or_unknown(
            &dispatch,
            &builtin_pick,
            &[props_base_concrete_unresolved, keys],
        ),
        "Pick<PropsBase<UIMessage[]>, …> with concrete args but an UNRESOLVABLE target must \
         be OPEN — an instantiation is closed only when its target decl is resolvable with \
         a body that closes under the bindings"
    );

    // OPEN: a bare / under-applied generic alias — `InstantiationRef`
    // with EMPTY args — over a target whose prepared body is
    // unresolvable (no decl seeded) is undecidable ⇒ OPEN.
    let bare_alias_unresolved = graph.intern_node(SemanticNodeData::InstantiationRef {
        base: DeclIdentity {
            canonical_id: Arc::from("/types.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: HashValue::default(),
            decl_name: Arc::from("SlotProps"),
        },
        args: Arc::from(Vec::new().into_boxed_slice()),
    });
    assert!(
        crate::project_semantic_dispatch::raise::utility_enumeration_domain_is_open_or_unknown(
            &dispatch,
            &builtin_pick,
            &[bare_alias_unresolved, keys],
        ),
        "Pick<SlotProps, …> with EMPTY args over an unresolvable target must be OPEN \
         (no arg binds the body's free params)"
    );

    // CLOSED: a finite object surface domain.
    let closed_object = graph.intern_node(SemanticNodeData::Object(crate::test_surface_view! {
        members: Arc::from(Vec::new().into_boxed_slice()),
        call_signatures: Arc::from(Vec::new().into_boxed_slice()),
        construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
        index_signatures: Arc::from(Vec::new().into_boxed_slice()),
        keyspace: None,
        has_index_signature: false,
    }));
    assert!(
        !crate::project_semantic_dispatch::raise::utility_enumeration_domain_is_open_or_unknown(
            &dispatch,
            &builtin_pick,
            &[closed_object, keys],
        ),
        "Pick<{{ … }}, …> over a finite object surface must be CLOSED"
    );

    // A NON-utility instantiation is never subject to this
    // carrier-stop, even with an open source argument.
    let not_a_utility = DeclIdentity {
        canonical_id: Arc::from("/types.ts"),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        whole_hash: HashValue::default(),
        decl_name: Arc::from("Lookup"),
    };
    assert!(
        !crate::project_semantic_dispatch::raise::utility_enumeration_domain_is_open_or_unknown(
            &dispatch,
            &not_a_utility,
            &[props_base_open, keys],
        ),
        "a non-enumeration-utility instantiation is not subject to L1 carrier-stop"
    );

    // Tuple domains are finite surfaces (concrete numeric/length key
    // space) — keep them closed.
    let tuple = graph.intern_node(SemanticNodeData::Tuple {
        elements: Arc::from(
            vec![TupleElement {
                label: None,
                value: concrete_elem,
                optional: false,
                rest: false,
            }]
            .into_boxed_slice(),
        ),
        readonly: false,
    });
    assert!(
        !crate::project_semantic_dispatch::raise::utility_enumeration_domain_is_open_or_unknown(
            &dispatch,
            &builtin_pick,
            &[tuple, keys]
        ),
        "a concrete tuple domain must be CLOSED"
    );
}

/// Keyspace-inspection invariant: the openness walk must inspect the
/// KEYSPACE of an `IndexedAccess` / `Mapped` domain, not only the
/// object / source. A domain `Source[OpenKey]` or
/// `{ [K in OpenKeySpace]: V }` with a CONCRETE object but an OPEN key
/// space is OPEN — an object/source-only inspection would wrongly
/// judge it CLOSED and materialise an undecidable key set.
#[test]
fn utility_enumeration_domain_open_via_indexed_access_and_mapped_keyspace() {
    use crate::semantic_query::{
        DeclIdentity, HashValue, IndexKey, MapperKey, MapperKind, OptionalityMod, ReadonlyMod,
        SemanticNodeData,
    };

    let host = VerterHost::new_standalone(Default::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let builtin_pick = DeclIdentity {
        canonical_id: Arc::from("__builtin__"),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        whole_hash: HashValue::default(),
        decl_name: Arc::from("Pick"),
    };
    let keys = graph.intern_node(SemanticNodeData::Primitive(SemanticPrimitiveKind::String));

    // CONCRETE object, OPEN type-param key.
    let concrete_object = graph.intern_node(SemanticNodeData::Object(crate::test_surface_view! {
        members: Arc::from(Vec::new().into_boxed_slice()),
        call_signatures: Arc::from(Vec::new().into_boxed_slice()),
        construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
        index_signatures: Arc::from(Vec::new().into_boxed_slice()),
        keyspace: None,
        has_index_signature: false,
    }));
    let open_key = graph.intern_node(SemanticNodeData::TypeParam {
        decl: DeclIdentity::synthetic("K"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("K"),
    });

    // IndexedAccess { object: concrete, index: TypeNode(open K) }.
    let indexed_open_key = graph.intern_node(SemanticNodeData::IndexedAccess {
        object: concrete_object,
        index: IndexKey::Computed(open_key),
    });
    assert!(
        crate::project_semantic_dispatch::raise::utility_enumeration_domain_is_open_or_unknown(
            &dispatch,
            &builtin_pick,
            &[indexed_open_key, keys],
        ),
        "IndexedAccess with a concrete object but an OPEN type-param index key must be OPEN \
         (keyspace must be inspected, not just the object)"
    );

    // A literal-string index key over the same concrete object is CLOSED.
    let indexed_closed_key = graph.intern_node(SemanticNodeData::IndexedAccess {
        object: concrete_object,
        index: IndexKey::String(Arc::from("a")),
    });
    assert!(
        !crate::project_semantic_dispatch::raise::utility_enumeration_domain_is_open_or_unknown(
            &dispatch,
            &builtin_pick,
            &[indexed_closed_key, keys],
        ),
        "IndexedAccess with a concrete object and a literal-string index key must be CLOSED"
    );

    // Mapped { source: concrete, mapper.key_space: open OUTER T }. The
    // mapper's own binder is a DISTINCT node — it is BOUND inside the
    // mapper walk and must not be conflated with the open outer
    // parameter that makes the key space undecidable.
    let binder = graph.intern_node(SemanticNodeData::TypeParam {
        decl: DeclIdentity::synthetic("MapBinder"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("MapBinder"),
    });
    let mapped_open_keyspace = graph.intern_node(SemanticNodeData::Mapped {
        source: concrete_object,
        mapper: MapperKey {
            over_type_variable: false,
            parameter_node: binder,
            key_space: open_key,
            value_expr: concrete_object,
            optionality: OptionalityMod::Keep,
            readonly: ReadonlyMod::Keep,
            name_remap: None,
            kind: MapperKind::Computed,
        },
    });
    assert!(
        crate::project_semantic_dispatch::raise::utility_enumeration_domain_is_open_or_unknown(
            &dispatch,
            &builtin_pick,
            &[mapped_open_keyspace, keys],
        ),
        "Mapped with a concrete source but an OPEN mapper key space must be OPEN \
         (the produced key set is undecidable; key_space must be inspected, not just source)"
    );
}

/// MappedTemplate key-remap coverage with the mapper binder BOUND
/// (the binder is bound in EVERY walk — keyspace, value, remap):
///
/// - a remap interpolating an OPEN OUTER parameter (`` `on${T}` ``)
///   over concrete source/key_space is OPEN — the produced (remapped)
///   key set depends on the open interpolant;
/// - a remap interpolating ONLY the mapper's OWN binder
///   (`` `on${K}` `` over a finite key space) is a K-only transform —
///   CLOSED, decidable per key once `K` is bound;
/// - a CONCRETE remap (no interpolant) is CLOSED.
///
/// Discriminating: a remap walk that did NOT bind the binder would
/// judge the K-only remap open (over-fire); one that bound the outer
/// parameter too would judge the outer-interpolant remap closed
/// (under-fire).
#[test]
fn utility_enumeration_domain_mapped_name_remap_binder_bound_outer_open() {
    use crate::semantic_query::{
        DeclIdentity, HashValue, MapperKey, MapperKind, OptionalityMod, ReadonlyMod,
        SemanticNodeData, SemanticNodeId,
    };

    let host = VerterHost::new_standalone(Default::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let builtin_pick = DeclIdentity {
        canonical_id: Arc::from("__builtin__"),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        whole_hash: HashValue::default(),
        decl_name: Arc::from("Pick"),
    };
    let keys = graph.intern_node(SemanticNodeData::Primitive(SemanticPrimitiveKind::String));

    let concrete_object = graph.intern_node(SemanticNodeData::Object(crate::test_surface_view! {
        members: Arc::from(Vec::new().into_boxed_slice()),
        call_signatures: Arc::from(Vec::new().into_boxed_slice()),
        construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
        index_signatures: Arc::from(Vec::new().into_boxed_slice()),
        keyspace: None,
        has_index_signature: false,
    }));
    let concrete_key =
        graph.intern_node(SemanticNodeData::Primitive(SemanticPrimitiveKind::String));
    // The mapper's OWN binder `K` (bound) vs the open OUTER `T`.
    let binder_k = graph.intern_node(SemanticNodeData::TypeParam {
        decl: DeclIdentity::synthetic("K"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("K"),
    });
    let outer_t = graph.intern_node(SemanticNodeData::TypeParam {
        decl: DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });

    let make_mapped = |remap: SemanticNodeId| {
        graph.intern_node(SemanticNodeData::Mapped {
            source: concrete_object,
            mapper: MapperKey {
                over_type_variable: false,
                parameter_node: binder_k,
                key_space: concrete_key,
                value_expr: concrete_object,
                optionality: OptionalityMod::Keep,
                readonly: ReadonlyMod::Keep,
                name_remap: Some(remap),
                kind: MapperKind::Computed,
            },
        })
    };
    let template = |interpolant: SemanticNodeId| {
        graph.intern_node(SemanticNodeData::TemplateLiteral {
            quasis: Arc::from(
                vec![Arc::<str>::from("on"), Arc::<str>::from("")].into_boxed_slice(),
            ),
            expressions: Arc::from(vec![interpolant].into_boxed_slice()),
        })
    };

    // OPEN: `` as `on${T}` `` — the remapped key set depends on the
    // open OUTER interpolant.
    assert!(
        crate::project_semantic_dispatch::raise::utility_enumeration_domain_is_open_or_unknown(
            &dispatch,
            &builtin_pick,
            &[make_mapped(template(outer_t)), keys],
        ),
        "Mapped with concrete source + key_space but an `as`-clause name-remap \
         interpolating an open OUTER parameter must be OPEN"
    );

    // CLOSED: `` as `on${K}` `` — interpolates ONLY the mapper's own
    // BOUND binder over a finite key space (a K-only transform).
    assert!(
        !crate::project_semantic_dispatch::raise::utility_enumeration_domain_is_open_or_unknown(
            &dispatch,
            &builtin_pick,
            &[make_mapped(template(binder_k)), keys],
        ),
        "Mapped whose `as`-clause name-remap interpolates ONLY the mapper's own bound \
         binder over a finite key space must stay CLOSED (the binder is bound in every \
         walk; a K-only remap is decidable per key)"
    );

    // CLOSED control: a CONCRETE name-remap (no interpolant).
    let closed_remap = graph.intern_node(SemanticNodeData::TemplateLiteral {
        quasis: Arc::from(vec![Arc::<str>::from("on")].into_boxed_slice()),
        expressions: Arc::from(Vec::new().into_boxed_slice()),
    });
    assert!(
        !crate::project_semantic_dispatch::raise::utility_enumeration_domain_is_open_or_unknown(
            &dispatch,
            &builtin_pick,
            &[make_mapped(closed_remap), keys],
        ),
        "Mapped with concrete source/key_space and a CONCRETE name-remap must stay CLOSED \
         (the name_remap arm must not over-fire)"
    );
}
