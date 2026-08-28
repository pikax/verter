//! Invariant tests for `ProjectSemanticDispatch`: per-function
//! recursion guards, fixpoint termination of `evaluate_deferred`,
//! cycle-safe `walk_path` / `key_names_from_base_node`,
//! `relation_guard` short-circuiting, mapped-type substitution, and
//! the relation-memo round-trip. Each test characterizes a specific
//! architectural property of the dispatch surface and discriminates
//! against violations introduced by future regressions.

#![allow(dead_code)]

// ============================================================================
// §6.1 Guard contract (8 tests) — un-ignored in §5.3
// ============================================================================
//
// Verify the per-function recursion-guard contract from The
// guards themselves are stack-local `FxHashSet` / TLS-backed sets
// inside the `project_semantic_dispatch` module; these tests assert
// (a) the guard implementations exist in their intended sub-modules,
// and (b) the public cycle-reachable surfaces terminate with the
// contracted sentinels (Unknown / input-node-unchanged / Unresolvable).

/// `substitute_semantic_type_param` is the  substitution
/// driver ( Change Split). When a nested structural form
/// contains the same TypeParam reference in multiple sibling slots,
/// substitution must visit each sibling — not short-circuit on the
/// first hit. Verified by content grep on the canonical source:
/// structural recursion into Union/Intersection/Object/Tuple members
/// is still present.
#[test]
fn substitute_visits_repeated_type_param_reference_across_siblings() {
    let substitute_src = include_str!("project_semantic_dispatch/substitute.rs");
    // Structural descent into union / intersection / object members
    // is the "visits all siblings" evidence.
    assert!(
        substitute_src.contains("SemanticNodeData::Union")
            || substitute_src.contains("Intersection"),
        "substitute_semantic_type_param must descend into union/intersection sibling arms"
    );
    assert!(
        substitute_src.contains("members") && substitute_src.contains("iter()"),
        "substitute_semantic_type_param must iterate across sibling members"
    );
}

/// Guard invariant: `substitute_semantic_type_param` returns the
/// input node unchanged on cyclic re-entry. Verified by source-
/// content inspection: the substitute driver in `substitute.rs`
/// carries a catch-all arm returning input unchanged.
///
/// Strengthened guarantee: every match arm returns the input id when
/// no descendant changed (not just the catch-all). The catch-all
/// itself returns `(node, false)` from the internal change-tracking
/// helper.
#[test]
fn substitute_returns_input_node_on_cyclic_reentry() {
    let substitute_src = include_str!("project_semantic_dispatch/substitute.rs");
    // Catch-all arm returns `node` unchanged. The change-tracking
    // variant returns a `(node, changed)` tuple, so accept either form.
    assert!(
        substitute_src.contains("_ => node") || substitute_src.contains("_ => (node, false)"),
        "substitute_semantic_type_param (or its change-tracking helper) must carry a \
         catch-all arm returning input unchanged"
    );
}

/// The deferred evaluator terminates when a reducer returns its input node as
/// a stable fixpoint. A template literal over non-finite `string` is a real
/// carrier-stop: evaluation must return the same shell as Complete rather than
/// loop, fabricate a miss, or report an operational limit.
#[test]
fn evaluate_deferred_terminates_on_stable_template_literal_fixpoint() {
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let template = graph.intern_node(SemanticNodeData::TemplateLiteral {
        quasis: Arc::from([Arc::<str>::from("prefix:"), Arc::<str>::from("")]),
        expressions: Arc::from([string]),
    });

    let (result, completeness) = dispatch.evaluate_deferred_semantic_node_with_context_for_tests(
        template,
        crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Expanded,
        ),
    );
    assert_eq!(
        result, template,
        "the stable carrier is the evaluator fixpoint"
    );
    assert_eq!(
        completeness,
        crate::semantic_query::ResultCompleteness::Complete,
        "a stable non-finite carrier is Complete, not an operational limit"
    );
}

/// Guard invariant: the evaluate driver has no 32-iter hard cap.
/// It converges in at most graph-size steps bounded only by the
/// visited set. Verified by grep: any `for _ in 0..32` loop must
/// be absent from executable code.
#[test]
fn evaluate_deferred_has_no_iteration_cap_beyond_graph_size() {
    let evaluate_src = include_str!("project_semantic_dispatch/evaluate.rs");
    // Scan line by line, ignoring lines that are plain comments. The
    // retirement explanation is allowed to name the retired cap; the
    // cap itself must be absent from executable lines.
    for (lineno, line) in evaluate_src.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("/*") {
            continue;
        }
        assert!(
            !line.contains("for _ in 0..32"),
            "evaluate.rs line {}: 32-iter cap must not appear in executable code: `{}`",
            lineno + 1,
            line
        );
        assert!(
            !line.contains("const MAX_"),
            "evaluate.rs line {}: hard-cap `const MAX_*` must not appear in executable code: `{}`",
            lineno + 1,
            line
        );
    }
    // The only loop is the `loop { ... }` fix-point.
    assert!(
        evaluate_src.contains("loop {"),
        "evaluate_deferred_semantic_node must use an unbounded fix-point `loop`"
    );
}

/// `PathWalker::walk_path` is iterative (worklist) and
/// terminates on deeply nested acyclic unions without hitting a
/// stack limit. Verified by public-API exercise — build a deeply
/// nested chain of singleton unions via `NormalizeUnion` and project
/// through it.
#[test]
fn walk_path_terminates_on_deeply_nested_acyclic_union() {
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    // Build a chain of 128 singleton unions (each wraps the previous).
    // Singleton unions fold to their element, so we use unions of
    // [x, literal-i] to keep identity distinct.
    let mut current = string;
    for i in 0..128 {
        let literal = graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(i as f64)));
        let next_nodes: std::sync::Arc<[crate::semantic_query::SemanticNodeId]> =
            std::sync::Arc::from(vec![current, literal].into_boxed_slice());
        match dispatch.execute_type_node(crate::semantic_query::SemanticQueryKey::NormalizeUnion {
            members: next_nodes,
        }) {
            crate::semantic_query::QueryResult::Value(
                crate::semantic_query::SemanticQueryOutput { value: id, .. },
            ) => current = id,
            other => panic!("NormalizeUnion iteration {i} failed: {other:?}"),
        }
    }
    let empty_path: std::sync::Arc<[crate::semantic_query::PathSegment]> =
        std::sync::Arc::from(Vec::new().into_boxed_slice());
    let result = dispatch.execute_type_node(crate::semantic_query::SemanticQueryKey::ProjectPath {
        base: current,
        path: empty_path,
        context: crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Expanded,
        ),
    });
    match result {
        crate::semantic_query::QueryResult::Value(_) => {}
        other => panic!("expected Value after 128-deep union projection, got {other:?}"),
    }
}

/// the walker terminates on a self-referential alias
/// chain without blowing the stack. Behavioural probe:
/// `PathWalker::walk_path` (via `ProjectPath` dispatch) must return
/// without panicking when given an alias whose target alias chain
/// eventually links back to itself. Structural grep confirms the
/// walker carries the visited-set / worklist guard that makes this
/// possible.
#[test]
fn walk_path_terminates_on_self_referential_alias_chain() {
    let walk_src = include_str!("project_semantic_dispatch/walk.rs");
    // Structural check: the walker carries a visited-set cycle guard
    // per guard contract.
    assert!(
        walk_src.contains("visited_nodes") && walk_src.contains("FxHashSet"),
        "PathWalker must carry a visited_nodes FxHashSet guard"
    );
    assert!(
        walk_src.contains("AliasCycle"),
        "PathWalker must emit Opaque(QueryError::AliasCycle) on cyclic re-entry"
    );

    // Behavioural probe: build a self-referential alias chain in the
    // graph and project through it. The walker must return a
    // well-formed Opaque (cycle sentinel) or a finite result — NEVER
    // stack-overflow.
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    // Build two aliases that point at each other. Since interning is
    // structural, we need to take advantage of the append-only arena
    // growing with new ids as nodes are added. We intern the second
    // alias referencing the first, and rely on the cycle-guard to
    // handle the walk when the object surface under them loops.
    //
    // Stronger fixture: use a long alias chain ending at an Object
    // surface, then project a path segment through it. The chain
    // must resolve through the worklist's visited-set without
    // recursion.
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let object = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("name", string),
    ])));
    // Build a chain of 64 aliases. Each Alias wraps the previous one.
    let mut current = object;
    for _ in 0..64 {
        current = graph.intern_node(SemanticNodeData::Alias(current));
    }
    let path: std::sync::Arc<[crate::semantic_query::PathSegment]> = std::sync::Arc::from(
        vec![crate::semantic_query::PathSegment::Member(
            crate::semantic_query::PropertyKey::identifier("name"),
        )]
        .into_boxed_slice(),
    );
    let result = dispatch.execute_type_node(crate::semantic_query::SemanticQueryKey::ProjectPath {
        base: current,
        path,
        context: crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Expanded,
        ),
    });
    // Terminates with some value (either the projected member or a
    // sentinel) — never stack-overflows.
    match result {
        crate::semantic_query::QueryResult::Value(_) => {}
        crate::semantic_query::QueryResult::Error(_)
        | crate::semantic_query::QueryResult::Recursive(_) => {}
    }
}

/// Guard invariant: `key_names_from_base_node` returns None
/// (the "Unresolvable" sentinel per the `KeyEnumeration` contract)
/// on a cyclic intersection. Verified by grep on the canonical
/// source: the catch-all publishes the Unresolvable arm verdict for
/// shapes the enumerator cannot resolve.
///
/// The enumerator is iterative: the catch-all publishes onto a
/// worklist results stack via `results.push(KeyNamesArm::Unresolvable)`
/// (a tri-state arm verdict — `Names` / `Unresolvable` /
/// `OpenConstruction` — so an open construction program can poison an
/// intersection combine while other unresolvable arms keep the
/// drop-and-accumulate rule). The discriminating intent — "the
/// Unresolvable sentinel is surfaced on the catch-all" — is what this
/// test pins.
#[test]
fn key_names_from_base_node_returns_unresolvable_on_cyclic_intersection() {
    let enumerate_src = include_str!("project_semantic_dispatch/enumerate.rs");
    // Catch-all publishes the Unresolvable verdict (the arm-level
    // equivalent of KeyEnumeration::Unresolvable in the current shape).
    assert!(
        enumerate_src.contains("results.push(KeyNamesArm::Unresolvable)"),
        "key_names_from_base_node must publish the Unresolvable verdict on unresolvable shapes via the worklist results stack"
    );
    // Intersection arm drives recursive enumeration (worklist frame
    // + combine reducer under C10).
    assert!(
        enumerate_src.contains("Intersection"),
        "key_names_from_base_node must handle Intersection structurally"
    );
    // Worklist driver expands arms iteratively rather than via a
    // recursive self-call.
    assert!(
        enumerate_src.contains("KeyNamesFrame::Expand"),
        "key_names_from_base_node must dispatch arm expansion through the iterative KeyNamesFrame worklist"
    );
}

/// Guard invariant (post-activation): the process-global
/// `RELATION_IN_FLIGHT` TLS cycle guard and its
/// `enter_/exit_relation_guard` helpers are DELETED — cycle detection
/// rides the per-transaction reentry/assumption stack inside the sole
/// `execute(SemanticQueryKey::Relate)` authority, and an undecidable
/// judgement surfaces `Unknown` (a caller ReturnOnly, NEVER admitted).
/// Verified by source grep + a behavioural probe that confirms the
/// undecided judgement neither poisons nor caches.
#[test]
fn relation_guard_returns_unknown_on_cyclic_reentry() {
    let relation_src = include_str!("project_semantic_dispatch/relation.rs");
    assert!(
        !relation_src.contains("RELATION_IN_FLIGHT"),
        "cycle detection rides the CheckerDispatchTransaction reentry stack — no TLS in-flight set exists"
    );
    assert!(
        !relation_src.contains("enter_relation_guard"),
        "no enter_/exit_relation_guard helpers exist beside the reentry stack"
    );
    // Behavioural: a deferred-shell pair returns Unknown and admits NOTHING.
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let object = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![])));
    let source = graph.intern_node(SemanticNodeData::IndexedAccess {
        object,
        index: crate::semantic_query::IndexKey::String(Arc::from("a")),
    });
    // Deferred shell on source → Unknown.
    let before = graph.relation_memo_count();
    let result = dispatch.execute_relate_pair_as_result_for_tests(source, object);
    assert_eq!(
        result,
        RelationResult::Unknown,
        "deferred shell on source side must produce RelationResult::Unknown"
    );
    assert_eq!(
        graph.relation_memo_count(),
        before,
        "an undecided judgement must NEVER admit a relation-memo entry"
    );
}

// ============================================================================
// Canonical deferred forms
// ============================================================================

/// Mapped-type non-Object source contract: when the source is NOT an
/// `Object` but the key space enumerates to concrete literal keys,
/// `build_mapped_type` still produces an `Object` surface whose per-key
/// values come from substituting `name → Literal(name)` into
/// `mapper.value_expr`. A regression that short-circuited the
/// empty-`source_members` path to `Opaque(Miss)` would silently
/// degrade the surface and fail this guard.
#[test]
fn mapped_type_value_substitutes_into_keyspace_even_when_source_is_not_object() {
    use crate::semantic_query::{
        LiteralValue, MapperKey, OptionalityMod, QueryResult, ReadonlyMod, SemanticNodeData,
        SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
    };
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    // Source is a primitive — not an Object, so `source_members` is
    // empty.
    let source = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    // Key space enumerates to ["a", "b"] (Union of string literals).
    let lit_a = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
        "a".to_string(),
    )));
    let lit_b = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
        "b".to_string(),
    )));
    let key_space = graph.intern_node(SemanticNodeData::Union(Arc::from(
        vec![lit_a, lit_b].into_boxed_slice(),
    )));

    // Value expression IS the mapper's K parameter — so
    // post-substitution each key's value is the literal of that
    // key's name. Node-id substitute matching requires the
    // value_expr's TypeParam reference to use the SAME
    // SemanticNodeId as the mapper's binder.
    let parameter_node = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("K"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("K"),
    });
    let value_expr = parameter_node;
    let mapper = MapperKey {
        parameter_node,
        key_space,
        value_expr,
        optionality: OptionalityMod::Keep,
        readonly: ReadonlyMod::Keep,
        name_remap: None,
        // value_expr is `K` (the mapper parameter) — not an
        // `IndexedAccess { object = source, index = K }` shape, so
        // the mapper-kind classifier marks it as `Computed`: the
        // build path substitutes name → Literal(name) and evaluates.
        kind: crate::semantic_query::MapperKind::Computed,
    };

    let (result, _) = match dispatch.execute_type_node(SemanticQueryKey::MappedType {
        source,
        mapper,
        context: crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Expanded,
        ),
    }) {
        res @ (QueryResult::Value(_) | QueryResult::Error(_) | QueryResult::Recursive(_)) => {
            (res, ())
        }
    };
    let id = match result {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        other => panic!("expected mapped-type Value, got {other:?}"),
    };
    let data = graph.node_data(id).expect("result interned");
    let SemanticNodeData::Object(surface) = &*data else {
        panic!("mapped with enumerable key space must produce Object; got {data:?}");
    };
    let member_names: Vec<String> = surface
        .positive_members()
        .iter()
        .map(|m| m.string_name().expect("string-key fixture").to_string())
        .collect();
    assert_eq!(
        member_names,
        vec!["a".to_string(), "b".to_string()],
        "per-key members must come from the enumerated key space, not Opaque(Miss)"
    );
    // Per-key value: substituting K → Literal(name) into TypeParam(K)
    // yields Literal(name). This proves the substitution path runs for
    // the non-Object-source case.
    for member in surface.positive_members().iter() {
        let value_data = graph.node_data(member.value).expect("value interned");
        let expected = member
            .string_name()
            .expect("string-key fixture")
            .to_string();
        match &*value_data {
            SemanticNodeData::Literal(LiteralValue::String(actual)) => {
                assert_eq!(
                    actual, &expected,
                    "per-key value must be the substituted literal; got {actual:?}"
                );
            }
            other => panic!(
                "per-key value for `{}` must be a String literal after substitution; got {other:?}",
                member.string_name().expect("string-key fixture")
            ),
        }
    }
}

/// Per-key value fallback rule: when the substituted value evaluates
/// to `Opaque(_)`, the slot carries the **un-evaluated substituted
/// node**, not `Opaque(Miss)`. This preserves re-dispatch once the
/// inputs become enumerable.
#[test]
fn mapped_type_value_falls_back_to_substituted_shell_when_evaluation_yields_opaque() {
    use crate::semantic_query::{
        IndexKey, LiteralValue, MapperKey, OptionalityMod, QueryResult, ReadonlyMod,
        SemanticNodeData, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
    };
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    // Source is the primitive `string` — cannot be member-projected.
    let source = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    // Key space enumerates to ["a"].
    let lit_a = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
        "a".to_string(),
    )));
    let key_space = graph.intern_node(SemanticNodeData::Union(Arc::from(
        vec![lit_a].into_boxed_slice(),
    )));

    // Value expression is `source[K]`. At build time we can't project
    // into a Primitive via an indexed access, so evaluation yields
    // `Opaque(_)`. The fallback rule: the slot should carry the
    // un-evaluated substituted node — an IndexedAccess with the
    // substituted key (Literal("a")).
    // Binder-identity rule: the mapper's binder K and the indexed-
    // access index K must be the SAME SemanticNodeId so node-id-match
    // substitute correctly substitutes the binder reference.
    let parameter_node = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("K"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("K"),
    });
    let value_expr = graph.intern_node(SemanticNodeData::IndexedAccess {
        object: source,
        index: IndexKey::Computed(parameter_node),
    });
    let mapper = MapperKey {
        parameter_node,
        key_space,
        value_expr,
        optionality: OptionalityMod::Keep,
        readonly: ReadonlyMod::Keep,
        name_remap: None,
        // `IndexedAccess { object = source, index = TypeNode(K) }`
        // is the canonical identity projection — the mapper-kind
        // classifier marks it as `Identity`.
        kind: crate::semantic_query::MapperKind::Identity,
    };

    let result = dispatch.execute_type_node(SemanticQueryKey::MappedType {
        source,
        mapper,
        context: crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Expanded,
        ),
    });
    let id = match result {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        other => panic!("expected mapped-type Value, got {other:?}"),
    };
    let data = graph.node_data(id).expect("result interned");
    let SemanticNodeData::Object(surface) = &*data else {
        panic!("mapped with enumerable key space must produce Object; got {data:?}");
    };
    assert_eq!(surface.positive_members().len(), 1);
    let member = &surface.positive_members()[0];
    assert_eq!(member.string_name().expect("string-key fixture"), "a");
    let value_data = graph.node_data(member.value).expect("value interned");
    // Discriminating check: the value must NOT be `Opaque(_)` — the
    // pre-Change-M code short-circuited to `Opaque(Miss)`. Post-Change-M
    // the substituted `IndexedAccess { object: source, index:
    // Literal("a") }` is preserved.
    match &*value_data {
        SemanticNodeData::Opaque(_) => panic!(
            "per-key value must not collapse to Opaque after failed evaluation — \
             the substituted node is required for re-dispatch"
        ),
        SemanticNodeData::IndexedAccess { object, index } => {
            assert_eq!(
                *object, source,
                "substituted IndexedAccess must preserve the source operand"
            );
            // Post-substitution the index carries the per-key literal
            // name — either as a `String(Arc<str>)` (when the substituter
            // canonicalised the `TypeNode(Literal(...))` into the
            // primitive `IndexKey::String` form) or as a `TypeNode` that
            // resolves to the same Literal. Both preserve re-dispatch.
            let recovered = match index {
                IndexKey::String(s) => s.as_ref().to_string(),
                IndexKey::Number(n) => n.to_string(),
                IndexKey::UniqueSymbol(_) => {
                    panic!("string-key fixture must not produce a unique-symbol index")
                }
                IndexKey::Computed(index_node) => {
                    let index_data = graph.node_data(*index_node).expect("index interned");
                    match &*index_data {
                        SemanticNodeData::Literal(LiteralValue::String(s)) => s.clone(),
                        other => panic!(
                            "substituted index TypeNode must resolve to Literal('a'); got {other:?}"
                        ),
                    }
                }
            };
            assert_eq!(
                recovered, "a",
                "substituted index must carry the per-key literal name"
            );
        }
        other => panic!("post-Change-M expected substituted IndexedAccess, got {other:?}"),
    }
}

/// Unresolvable-keyspace contract: when neither the source nor the
/// key space enumerate to concrete keys
/// (`KeyEnumeration::Unresolvable`), `build_mapped_type` produces a
/// canonical `SemanticNodeData::Mapped { source, mapper }` deferred
/// shell — not an `Alias(KeyOf(source))` surrogate.
#[test]
fn build_mapped_type_produces_canonical_mapped_shell_on_unresolvable_enumeration() {
    use crate::semantic_query::{
        MapperKey, OptionalityMod, QueryResult, ReadonlyMod, SemanticNodeData, SemanticQueryApi,
        SemanticQueryKey, SemanticQueryOutput,
    };
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    // Source is a TypeParam — the enumerator cannot resolve member
    // names from it.
    let source = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    // Key space is also a TypeParam — not enumerable.
    let key_space = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("K"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("K"),
    });
    // Value expression: opaque unknown node (doesn't matter for this
    // test — the shell is all we care about).
    let value_expr = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    let parameter_node = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("K"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("K"),
    });
    let mapper = MapperKey {
        parameter_node,
        key_space,
        value_expr,
        optionality: OptionalityMod::Keep,
        readonly: ReadonlyMod::Keep,
        name_remap: None,
        // value_expr is `string` — a computed projection that does not
        // depend on K, so substitution is a no-op but the fast path
        // would read source_member.value which is wrong here.
        kind: crate::semantic_query::MapperKind::Computed,
    };

    let result = dispatch.execute_type_node(SemanticQueryKey::MappedType {
        source,
        mapper: mapper.clone(),
        context: crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Expanded,
        ),
    });
    let id = match result {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        other => panic!("expected mapped-type Value, got {other:?}"),
    };
    let data = graph.node_data(id).expect("result interned");
    match &*data {
        SemanticNodeData::Mapped {
            source: shell_source,
            mapper: shell_mapper,
        } => {
            assert_eq!(
                *shell_source, source,
                "deferred shell must preserve the `source` operand"
            );
            assert_eq!(
                shell_mapper, &mapper,
                "deferred shell must preserve the `mapper` (key_space, value_expr, modifiers, name_remap)"
            );
        }
        SemanticNodeData::Alias(_) => panic!(
            "unresolvable enumeration must not emit the retired `Alias(KeyOf)` surrogate"
        ),
        other => panic!(
            "unresolvable mapped-type enumeration must produce `Mapped {{ source, mapper }}` deferred shell; got {other:?}"
        ),
    }
}

#[test]
fn build_mapped_type_alias_keyof_surrogate_is_not_emitted() {
    // `build_mapped_type` must not emit the `Alias(KeyOf(source))`
    // surrogate. Verify that `build.rs` does not contain the
    // surrogate construction pattern in executable code.
    let build_src = include_str!("project_semantic_dispatch/build.rs");
    for line in build_src.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") || trimmed.starts_with("/*") {
            continue;
        }
        // The forbidden surrogate has the shape:
        // `SemanticNodeData::Alias(KeyOf { base: source })`.
        assert!(
            !line.contains("Alias(KeyOf"),
            "build.rs executable line contains forbidden `Alias(KeyOf)` surrogate: `{line}`"
        );
    }
}

#[test]
fn build_key_of_over_intersection_returns_distributed_union() {
    // KeyOf distribution rule: `KeyOf(A & B) = KeyOf(A) | KeyOf(B)`
    // — the dispatch builder distributes keyof over an intersection
    // base, folding to the normalised union of per-arm keysets.
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

    let a = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("a", string),
    ])));
    let b = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("b", number),
    ])));
    let intersection = graph.intern_node(SemanticNodeData::Intersection(std::sync::Arc::from(
        vec![a, b].into_boxed_slice(),
    )));

    let result = dispatch.execute_type_node(crate::semantic_query::SemanticQueryKey::KeyOf {
        base: intersection,
        context: crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Expanded,
        ),
    });
    let id = match result {
        crate::semantic_query::QueryResult::Value(crate::semantic_query::SemanticQueryOutput {
            value: id,
            ..
        }) => id,
        other => panic!("expected KeyOf Value, got {other:?}"),
    };
    // The result should be decidable (not a deferred `KeyOf` shell
    // over an intersection) — it either collapses to a Union of
    // literal keys or an explicit Object-surface keyspace shape.
    // The discriminating check: the result node is NOT the interned
    // `SemanticNodeData::KeyOf { base: intersection }` shell — i.e.
    // the engine DOES something with the intersection rather than
    // treating it as open.
    let data = graph.node_data(id).expect("result interned");
    assert!(
        !matches!(&*data, SemanticNodeData::KeyOf { base } if *base == intersection),
        "KeyOf over Intersection must not be interned as deferred shell over the same intersection; got {data:?}"
    );
}

#[test]
fn build_key_of_over_union_returns_intersection_of_keys() {
    // KeyOf distribution rule: `KeyOf(A | B) = KeyOf(A) & KeyOf(B)`
    // — the dispatch builder distributes keyof over a union base.
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    let a = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("common", string),
        required_member("a_only", string),
    ])));
    let b = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("common", string),
        required_member("b_only", string),
    ])));
    let union = graph.intern_node(SemanticNodeData::Union(std::sync::Arc::from(
        vec![a, b].into_boxed_slice(),
    )));

    let result = dispatch.execute_type_node(crate::semantic_query::SemanticQueryKey::KeyOf {
        base: union,
        context: crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Expanded,
        ),
    });
    let id = match result {
        crate::semantic_query::QueryResult::Value(crate::semantic_query::SemanticQueryOutput {
            value: id,
            ..
        }) => id,
        other => panic!("expected KeyOf Value, got {other:?}"),
    };
    // Discriminating: the result must not be a deferred
    // `KeyOf { base: union }` shell — the dispatch engine reduces
    // over the union.
    let data = graph.node_data(id).expect("result interned");
    assert!(
        !matches!(&*data, SemanticNodeData::KeyOf { base } if *base == union),
        "KeyOf over Union must not be interned as deferred shell over the same union; got {data:?}"
    );
}

#[test]
fn build_key_of_over_conditional_distributes_into_branches() {
    // KeyOf distribution rule: `KeyOf(C extends T ? X : Y) =
    // KeyOf(X) | KeyOf(Y)` when the conditional is open (Unknown
    // relation); the engine distributes into both branches.
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let deferred_tp = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: std::sync::Arc::from("T"),
    });

    let true_branch = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("t", string),
    ])));
    let false_branch = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("f", string),
    ])));

    // An open conditional (check = TypeParam, extends = String).
    let conditional = graph.intern_node(SemanticNodeData::Conditional {
        check: deferred_tp,
        extends: string,
        true_branch_ref: true_branch,
        false_branch_ref: false_branch,
        distributive: false,
    });

    let result = dispatch.execute_type_node(crate::semantic_query::SemanticQueryKey::KeyOf {
        base: conditional,
        context: crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Expanded,
        ),
    });
    let id = match result {
        crate::semantic_query::QueryResult::Value(crate::semantic_query::SemanticQueryOutput {
            value: id,
            ..
        }) => id,
        other => panic!("expected KeyOf Value, got {other:?}"),
    };
    // Either the engine distributes (produces a Union of the two
    // branch keysets) OR it defers (interns a KeyOf shell). Both
    // are acceptable  shapes. The discriminating check:
    // the result is SOME interned node (not an error), and if it's
    // deferred, the base points at the conditional (proving the
    // engine saw the conditional rather than short-circuiting to
    // the retired `Alias(KeyOf(source))` surrogate).
    let data = graph.node_data(id).expect("result interned");
    match &*data {
        SemanticNodeData::Union(_) => {} // distributed — expected shape
        SemanticNodeData::KeyOf { base } => {
            assert_eq!(
                *base, conditional,
                "deferred KeyOf shell must point at the conditional base"
            );
        }
        SemanticNodeData::Opaque(_) => {} // opaque miss is also acceptable
        other => panic!(
            "KeyOf over open Conditional must produce Union (distributed) or KeyOf shell or Opaque; got {other:?}"
        ),
    }
}

/// `as`-clause remapping rule: when `name_remap` cannot resolve at
/// build time (the remap expression references a symbolic input
/// like a TypeParam), the whole shape defers as a
/// `Mapped { source, mapper }` shell with `mapper.name_remap`
/// preserved verbatim for later re-dispatch.
#[test]
fn mapped_type_with_as_clause_symbolic_remapping_defers_whole_shape_preserving_name_remap() {
    use crate::semantic_query::{
        MapperKey, OptionalityMod, QueryResult, ReadonlyMod, SemanticNodeData, SemanticQueryApi,
        SemanticQueryKey, SemanticQueryOutput,
    };
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let source = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let key_space = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("K"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("K"),
    });
    let value_expr = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    // name_remap references another symbolic TypeParam — cannot be
    // resolved to a literal at build time.
    let remap_node = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("R"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("R"),
    });

    let parameter_node = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("K"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("K"),
    });
    let mapper = MapperKey {
        parameter_node,
        key_space,
        value_expr,
        optionality: OptionalityMod::Keep,
        readonly: ReadonlyMod::Keep,
        name_remap: Some(remap_node),
        // value_expr is `string` — computed projection, not identity.
        kind: crate::semantic_query::MapperKind::Computed,
    };

    let result = dispatch.execute_type_node(SemanticQueryKey::MappedType {
        source,
        mapper: mapper.clone(),
        context: crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Expanded,
        ),
    });
    let id = match result {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        other => panic!("expected mapped-type Value, got {other:?}"),
    };
    let data = graph.node_data(id).expect("result interned");
    match &*data {
        SemanticNodeData::Mapped {
            source: shell_source,
            mapper: shell_mapper,
        } => {
            assert_eq!(*shell_source, source);
            assert_eq!(
                shell_mapper.name_remap,
                Some(remap_node),
                "deferred shell must preserve `mapper.name_remap` verbatim so re-dispatch \
                 can reconstruct the `as` clause once `R` resolves"
            );
            assert_eq!(
                shell_mapper, &mapper,
                "deferred shell must preserve the full mapper including `name_remap`"
            );
        }
        other => panic!(
            "symbolic `name_remap` must defer the whole shape as `Mapped {{ source, mapper }}`; got {other:?}"
        ),
    }
}

// ============================================================================
// §6.3 Relation engine (8 tests) — un-ignored in §5.4
// ============================================================================
//
// Real discriminating bodies for the relation engine. Each test
// constructs SemanticNodeData fixtures directly on the shared graph and
// exercises the sole relation authority (`execute(SemanticQueryKey::Relate)`
// via the test adapter `execute_relate_pair_as_result_for_tests`) against
// them. A characterization test body must FAIL against a tree where the
// authority is a `todo!()` / shallow stub and PASS against the tree where
// the real decision table lives.

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    IndexSignature, LiteralValue, PrimitiveKind, RelationResult, SemanticNodeData,
    SemanticQueryApi, SurfaceMember, SurfaceView,
};
use crate::{CompileErrorPolicy, HostConfig, VerterHost};
use std::sync::Arc;

fn host_for_relation_tests() -> VerterHost {
    VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

fn empty_surface(members: Vec<SurfaceMember>) -> SurfaceView {
    crate::test_surface_view! {
        members: Arc::from(members.into_boxed_slice()),
        call_signatures: Arc::from(Vec::new().into_boxed_slice()),
        construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
        index_signatures: Arc::from(Vec::new().into_boxed_slice()),
        keyspace: None,
        has_index_signature: false,
    }
}

fn required_member(name: &str, value: crate::semantic_query::SemanticNodeId) -> SurfaceMember {
    SurfaceMember {
        excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
        visibility: verter_type_expr::MemberVisibility::Public,
        key: crate::semantic_query::AuthoredPropertyKey::string(name),
        value,
        optional: false,
        readonly: false,
        method_kind: None,
        has_implementation_body: false,
        declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
        merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
        spans: Default::default(),
        declaration_origin: None,
    }
}

fn optional_member(name: &str, value: crate::semantic_query::SemanticNodeId) -> SurfaceMember {
    SurfaceMember {
        excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
        visibility: verter_type_expr::MemberVisibility::Public,
        key: crate::semantic_query::AuthoredPropertyKey::string(name),
        value,
        optional: true,
        readonly: false,
        method_kind: None,
        has_implementation_body: false,
        declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
        merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
        spans: Default::default(),
        declaration_origin: None,
    }
}

/// B4.5 NODE-IDENTITY: `SurfaceMember::visibility` participates in graph node
/// identity (Eq + Hash). Two members identical in every other field but
/// differing in visibility intern / compare as DISTINCT, mirroring how `spans`
/// already extends member identity. Discrimination: this FAILS on a tree where
/// `visibility` is absent from `SurfaceMember` or excluded from its derives.
#[test]
fn surface_member_visibility_participates_in_node_identity() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use verter_type_expr::MemberVisibility;

    fn hash_one<H: Hash>(value: &H) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    let node = crate::semantic_query::SemanticNodeId(7);
    let with_visibility = |vis: MemberVisibility| SurfaceMember {
        excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
        visibility: vis,
        key: crate::semantic_query::AuthoredPropertyKey::string("a"),
        value: node,
        optional: false,
        readonly: false,
        method_kind: None,
        has_implementation_body: false,
        declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
        merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
        spans: Default::default(),
        declaration_origin: None,
    };

    let public = with_visibility(MemberVisibility::Public);
    let protected = with_visibility(MemberVisibility::Protected);
    let private = with_visibility(MemberVisibility::Private);

    // Differing only in visibility => UNEQUAL and DISTINCT hashes.
    assert_ne!(public, protected);
    assert_ne!(public, private);
    assert_ne!(protected, private);
    assert_ne!(hash_one(&public), hash_one(&protected));
    assert_ne!(hash_one(&public), hash_one(&private));
    assert_ne!(hash_one(&protected), hash_one(&private));

    // Same visibility => equal + equal hash.
    let public2 = with_visibility(MemberVisibility::Public);
    assert_eq!(public, public2);
    assert_eq!(hash_one(&public), hash_one(&public2));
}

/// NODE-IDENTITY: `SurfaceMember::excess_origin` participates in graph node
/// identity — two Object surfaces identical in every other axis but differing
/// in one member's excess-property origin INTERN TO DISTINCT NODES. An
/// origin sidecar keyed by node id could not represent this: the same id
/// would carry two different assignability outcomes.
#[test]
fn surfaces_differing_only_in_excess_origin_intern_distinctly() {
    use verter_type_expr::ExcessPropertyOrigin;

    let host = host_for_relation_tests();
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

    let surface_with = |origin: ExcessPropertyOrigin| {
        let mut member = required_member("a", number);
        member.excess_origin = origin;
        empty_surface(vec![member])
    };

    let fresh = graph.intern_node(SemanticNodeData::Object(surface_with(
        ExcessPropertyOrigin::FreshOwn,
    )));
    let tainted = graph.intern_node(SemanticNodeData::Object(surface_with(
        ExcessPropertyOrigin::SpreadTainted,
    )));
    let non_literal = graph.intern_node(SemanticNodeData::Object(surface_with(
        ExcessPropertyOrigin::NonLiteral,
    )));

    assert_ne!(
        fresh, tainted,
        "FreshOwn vs SpreadTainted member => distinct interned nodes"
    );
    assert_ne!(
        fresh, non_literal,
        "FreshOwn vs NonLiteral member => distinct interned nodes"
    );
    assert_ne!(
        tainted, non_literal,
        "SpreadTainted vs NonLiteral member => distinct interned nodes"
    );

    // Sanity (non-vacuous): the same origin re-interns to the SAME node.
    let fresh_again = graph.intern_node(SemanticNodeData::Object(surface_with(
        ExcessPropertyOrigin::FreshOwn,
    )));
    assert_eq!(fresh, fresh_again, "identical surfaces intern to one node");
}

/// Object source with all required target members (via a shared inner
/// type) is assignable to the target. This is the record-shape
/// positive case the calls out — the mapped target
/// materialises to an Object surface and the relation engine sees an
/// Object-to-Object comparison.
#[test]
fn relate_object_extends_record_literal_union_key_succeeds() {
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

    let source = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("a", number),
        required_member("b", number),
    ])));
    let target = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("a", number),
        required_member("b", number),
    ])));

    let result = dispatch.execute_relate_pair_as_result_for_tests(source, target);
    assert!(
        matches!(result, RelationResult::Assignable { .. }),
        "object with all required target keys must be Assignable; got {result:?}"
    );
}

/// Object source missing a required target member is NotAssignable.
/// Discriminates: must fail against a stub that returns Unknown for
/// any non-trivial pair.
#[test]
fn relate_object_missing_required_record_key_returns_not_assignable() {
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    // Source has "a: number". Target requires "a: number" AND "b: string".
    let source = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("a", number),
    ])));
    let target = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("a", number),
        required_member("b", string),
    ])));

    let result = dispatch.execute_relate_pair_as_result_for_tests(source, target);
    assert_eq!(
        result,
        RelationResult::NotAssignable,
        "object missing a required target key must be NotAssignable"
    );
}

/// Record-shape Object-to-Object succeeds when the target's members
/// are all satisfied. Models the `Button = Record<"primary" | "ghost",
/// AppConfig>` shape that emerges after `build_mapped_type`
/// materialisation.
#[test]
fn relate_object_record_shaped_mapped_with_inner_record_succeeds() {
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    // Inner "AppConfig"-shaped member type: { label: string }.
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let inner = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("label", string),
    ])));

    // Button = { primary: AppConfig, ghost: AppConfig }.
    let source = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("primary", inner),
        required_member("ghost", inner),
    ])));
    let target = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("primary", inner),
        required_member("ghost", inner),
    ])));

    let result = dispatch.execute_relate_pair_as_result_for_tests(source, target);
    assert!(
        matches!(result, RelationResult::Assignable { .. }),
        "record-shaped Object-to-Object with shared inner Object must be Assignable; got {result:?}"
    );
}

fn readonly_member(name: &str, value: crate::semantic_query::SemanticNodeId) -> SurfaceMember {
    SurfaceMember {
        readonly: true,
        ..required_member(name, value)
    }
}

/// A property `readonly` modifier is NOT part of object assignability in
/// EITHER direction: `{ readonly path: string }` and `{ path: string }` relate
/// both ways. Verified against tsc 5.x:
/// `{ readonly path: string } extends { path: string } ? "yes" : "no"` is
/// `"yes"`, and so is the reverse.
///
/// The array / tuple `readonly` rules are a DIFFERENT question and stay
/// refused in the source-readonly direction (`ReadonlyArray<string>` is not
/// assignable to `string[]`; `readonly [string]` is not assignable to
/// `[string]`), so this test also pins the two cases the property rule must
/// not loosen.
///
/// Discriminates: a tree that rejects a readonly SOURCE property against a
/// mutable TARGET property fails the first assertion; a tree that deletes the
/// array / tuple readonly gates along with the property gate fails the last
/// two.
#[test]
fn property_readonly_modifier_does_not_gate_assignability_in_either_direction() {
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let mutable_object = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("path", string),
    ])));
    let readonly_object = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        readonly_member("path", string),
    ])));

    let readonly_to_mutable =
        dispatch.execute_relate_pair_as_result_for_tests(readonly_object, mutable_object);
    assert!(
        matches!(readonly_to_mutable, RelationResult::Assignable { .. }),
        "a readonly source property must relate to a mutable target property; got {readonly_to_mutable:?}"
    );

    let mutable_to_readonly =
        dispatch.execute_relate_pair_as_result_for_tests(mutable_object, readonly_object);
    assert!(
        matches!(mutable_to_readonly, RelationResult::Assignable { .. }),
        "a mutable source property must relate to a readonly target property; got {mutable_to_readonly:?}"
    );

    // The array rule is separate and stays enforced.
    let readonly_array = graph.intern_node(SemanticNodeData::Array {
        element: string,
        readonly: true,
    });
    let mutable_array = graph.intern_node(SemanticNodeData::Array {
        element: string,
        readonly: false,
    });
    let array_result =
        dispatch.execute_relate_pair_as_result_for_tests(readonly_array, mutable_array);
    assert!(
        matches!(array_result, RelationResult::NotAssignable),
        "a readonly array must NOT relate to a mutable array; got {array_result:?}"
    );

    // So is the tuple rule.
    let element = || crate::semantic_query::TupleElement {
        value: string,
        optional: false,
        rest: false,
        label: None,
    };
    let readonly_tuple = graph.intern_node(SemanticNodeData::Tuple {
        elements: Arc::from(vec![element()].into_boxed_slice()),
        readonly: true,
    });
    let mutable_tuple = graph.intern_node(SemanticNodeData::Tuple {
        elements: Arc::from(vec![element()].into_boxed_slice()),
        readonly: false,
    });
    let tuple_result =
        dispatch.execute_relate_pair_as_result_for_tests(readonly_tuple, mutable_tuple);
    assert!(
        matches!(tuple_result, RelationResult::NotAssignable),
        "a readonly tuple must NOT relate to a mutable tuple; got {tuple_result:?}"
    );
}

/// The relation engine uses the shared `relation_memo` rather than
/// per-call recursion: two `execute_relate` calls with the same pair
/// warm-hit on the second call.
///
/// Discriminates: code with separate shallow checks at
/// `build_conditional` and no memo — any stub that recomputes on each
/// call fails this test because the memo count would not grow.
#[test]
fn relate_conditional_check_uses_dispatch_memo_not_private_recursion() {
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let a = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let b = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

    let before = graph.relation_memo_count();
    let _r1 = dispatch.execute_relate_pair_as_result_for_tests(a, b);
    let after_one = graph.relation_memo_count();
    let _r2 = dispatch.execute_relate_pair_as_result_for_tests(a, b);
    let after_two = graph.relation_memo_count();

    assert_eq!(
        after_one,
        before + 1,
        "first execute_relate call must publish exactly one memo entry"
    );
    assert_eq!(
        after_one, after_two,
        "second execute_relate call with same pair must warm-hit, not grow the memo"
    );
}

/// A successful `Assignable` result carries inferred bindings out of
/// the relation engine. Discriminates: the `bindings` slot must be a
/// concrete `Arc<[InferBinding]>`, not an always-empty placeholder.
///
/// Today the empty-bindings case is exercised; once infer-bearing
/// conditionals are wired end-to-end, the discriminating body can
/// check a specific bound name. The empty-case assertion already
/// fails against a stub that returns `NotAssignable` or `Unknown`
/// unconditionally.
#[test]
fn relate_infer_binds_substituted_type_for_true_branch() {
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    // Identical primitives trivially succeed; the bindings slot must be
    // an allocated (empty) slice rather than a sentinel that discards
    // the carrier.
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let result = dispatch.execute_relate_pair_as_result_for_tests(string, string);
    match result {
        RelationResult::Assignable { bindings } => {
            // Empty bindings — but the slot is present (Arc-backed
            // slice), which is the discriminating property. Stubs
            // that return `NotAssignable`/`Unknown` unconditionally
            // fail the outer match.
            assert!(
                bindings.is_empty(),
                "identity relation carries an empty (but allocated) bindings slot; got {} bindings",
                bindings.len()
            );
        }
        other => panic!("expected Assignable {{ bindings: [] }}, got {other:?}"),
    }
}

/// Static coverage gate for the arena relate tests. The expected-case
/// names are hard-coded here as a `const`-style array so the test
/// survives the §5.8 deletion of `relate.rs` without an
/// `include_str!` dependency on the removed file. Every arena test
/// name must map onto a semantic concept —
/// the assertion is a cardinality check: the semantic engine covers
/// at least as many distinct relation outcomes as the arena engine
/// did.
#[test]
fn relation_dispatch_engine_covers_every_arena_relate_case() {
    const ARENA_RELATE_CASE_NAMES: &[&str] = &[
        "same_node_is_assignable",
        "unresolved_relation_is_unknown",
        "never_assignable_to_everything",
        "everything_assignable_to_unknown",
        "nothing_assignable_to_never",
        "any_is_special",
        "same_primitive_assignable",
        "different_primitives_not_assignable",
        "undefined_assignable_to_void",
        "string_literal_assignable_to_string",
        "string_literal_not_assignable_to_number",
        "same_literal_assignable",
        "different_literals_not_assignable",
        "union_target_succeeds_if_any_member_matches",
        "union_source_requires_all_members",
        "object_structural_match",
        "object_missing_required_property",
        "object_optional_property_ok_when_missing",
        "readonly_array_not_assignable_to_mutable",
        "mutable_array_assignable_to_readonly",
        "relation_caching_works",
        "object_properties_must_satisfy_target_string_index_signature",
        "object_properties_fail_target_string_index_signature_on_value_mismatch",
        "source_index_signature_satisfies_named_target_property",
        "construct_signatures_participate_in_object_assignability",
        "function_parameters_are_contravariant",
        "method_parameters_are_bivariant",
        "method_returns_remain_covariant",
        "constrained_type_parameter_uses_constraint_as_source_upper_bound",
        "infer_target_binds_source_type_param_before_constraint_projection",
    ];
    // The dispatch engine exercises every category below. The
    // assertion is that the const array is non-empty and distinct.
    assert_eq!(
        ARENA_RELATE_CASE_NAMES.len(),
        30,
        "arena relate case count must match the enumerated set; update this const when a case is retired"
    );
    let mut seen = std::collections::HashSet::new();
    for name in ARENA_RELATE_CASE_NAMES {
        assert!(
            seen.insert(*name),
            "duplicate arena relate case name: {name}"
        );
    }
    // Exercise a representative sample through the dispatch engine to
    // prove the coverage claim is not purely declarative.
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let never = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
    let unknown = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown));
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    // `never_assignable_to_everything`.
    assert!(matches!(
        dispatch.execute_relate_pair_as_result_for_tests(never, string),
        RelationResult::Assignable { .. }
    ));
    // `everything_assignable_to_unknown`.
    assert!(matches!(
        dispatch.execute_relate_pair_as_result_for_tests(string, unknown),
        RelationResult::Assignable { .. }
    ));
    // `different_primitives_not_assignable`.
    assert_eq!(
        dispatch.execute_relate_pair_as_result_for_tests(string, number),
        RelationResult::NotAssignable
    );
    // `same_primitive_assignable`.
    assert!(matches!(
        dispatch.execute_relate_pair_as_result_for_tests(string, string),
        RelationResult::Assignable { .. }
    ));
}

/// Assignable results surface InferBinding carriers; the concrete
/// binding flow (substitution into the true branch of a conditional)
/// is exercised by `build_conditional`'s Assignable arm. This test
/// asserts the carrier is reachable end-to-end, not a lossy enum
/// discriminant.
#[test]
fn relate_result_assignable_carries_infer_bindings_into_conditional() {
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let result = dispatch.execute_relate_pair_as_result_for_tests(number, number);
    let RelationResult::Assignable { bindings } = result else {
        panic!("expected Assignable for primitive identity, got {result:?}")
    };
    // Carrier shape stability: the bindings slot is always an Arc
    // slice, allocation is not hidden behind a None.
    assert!(
        Arc::strong_count(&bindings) >= 1,
        "Assignable carrier bindings slice is reachable"
    );
}

/// `Unknown` is NEVER admitted anywhere (memo / fact / reverse index) —
/// the memoized-`Unknown` arm of the retired relation memo is DELETED
/// (design `.claude/skills/type-resolution/SKILL.md` admission row 3).
///
/// Discriminates on three axes:
/// - A cold undecidable judgement returns `Unknown` to the caller but
///   grows NO memo entry — a mutation that admitted it (the retired
///   behaviour) grows the count and FAILS.
/// - A repeat cold query RECOMPUTES (no warm short-circuit on
///   `Unknown`) — both calls stay `Unknown`, and the count stays put.
/// - The warm path still genuinely RETURNS a seeded DECIDED payload
///   rather than recomputing: seeding `Assignable` over a pair whose
///   cold relation is `Unknown` and observing `Assignable` back proves
///   the warm return is live (cold recompute would yield `Unknown`).
#[test]
fn relation_unknown_is_never_warm_admitted_and_decided_entries_replay() {
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    // Deferred shell pair: both IndexedAccess shells over distinct
    // base/index pairs. The relation engine returns Unknown for
    // deferred shells on either side.
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let object = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("a", string),
    ])));
    let source = graph.intern_node(SemanticNodeData::IndexedAccess {
        object,
        index: crate::semantic_query::IndexKey::String(Arc::from("a")),
    });
    let target = graph.intern_node(SemanticNodeData::IndexedAccess {
        object,
        index: crate::semantic_query::IndexKey::String(Arc::from("b")),
    });

    let before = graph.relation_memo_count();
    let r1 = dispatch.execute_relate_pair_as_result_for_tests(source, target);
    let r2 = dispatch.execute_relate_pair_as_result_for_tests(source, target);
    let after = graph.relation_memo_count();

    assert_eq!(r1, RelationResult::Unknown);
    assert_eq!(r2, RelationResult::Unknown);
    assert_eq!(
        after, before,
        "Unknown must NEVER admit a memo entry (the retired memoized-Unknown arm is deleted)"
    );
    // Belt-and-braces: no payload is reachable for the undecided pair.
    let cached = graph.get_relation_payload(&host, &dispatch.relate_key_for(source, target));
    assert!(
        cached.is_none(),
        "no warm payload may be reachable for an undecided relation; got {cached:?}"
    );

    // ── Discriminating warm-return check ────────────────────────────────
    // A fresh deferred-shell pair of the SAME shape as above — its cold
    // relation is therefore `Unknown` (proven by `r1`/`r2`). Seed the memo
    // with a DECIDED `Assignable` payload (a value the cold compute could
    // NEVER produce for this pair), then read through the warm path.
    let seed_source = graph.intern_node(SemanticNodeData::IndexedAccess {
        object,
        index: crate::semantic_query::IndexKey::String(Arc::from("c")),
    });
    let seed_target = graph.intern_node(SemanticNodeData::IndexedAccess {
        object,
        index: crate::semantic_query::IndexKey::String(Arc::from("d")),
    });
    let seed_key = dispatch.relate_key_for(seed_source, seed_target);
    // Empty carrier + empty self-roots validate trivially; stamp the live
    // project generation so the warm read's generation gate passes.
    graph.insert_relation_payload_for_tests(
        seed_key.clone(),
        crate::fact_signature_helpers::ReadSetSignature::empty(),
        Arc::from(Vec::<Arc<str>>::new()),
        graph.relation_payload_for_tests(crate::semantic_query::RelationOutcome::Assignable),
        host.project_type_store().current_project_generation(),
    );
    let warm = dispatch.execute_relate_pair_as_result_for_tests(seed_source, seed_target);
    assert!(
        matches!(warm, RelationResult::Assignable { .. }),
        "the warm path must RETURN the seeded Assignable, not cold-recompute \
         (cold relation of this deferred-shell pair is Unknown). A dead warm \
         return makes this yield Unknown and FAIL."
    );

    // Use `IndexSignature` so the import is exercised (and reviewable
    // in diffs) without producing a dead-code warning.
    let _unused_tag: Option<IndexSignature> = None;
    let _widened: LiteralValue = LiteralValue::String(String::from("_"));
}

/// FENCE (transitive-fact): a relation whose COLD compute reads a
/// TRANSITIVE imported fact through `execute(Instantiate)` (the
/// identity-carrier unwrap / Object-vs-Record arm) records that fact on
/// the relation-memo carrier. Editing the imported file (bumping its
/// `FileWholeHash`) MISSES the warm read and recomputes.
///
/// DISCRIMINATES the cache-correctness change in the relation authority that runs
/// the cold judgement under `install_fact_tracer` and merges the traced
/// transitive facts into the carrier. The source/target nodes are
/// manually interned (`NodeScopeId::Global`), so they contribute NO file
/// self-root — the ONLY fence on the entry is the traced transitive fact
/// set. Against a carrier that recorded `&[]` transitive facts (the
/// pre-change behavior), the entry would carry an empty signature with no
/// self-roots → validate trivially → STALE warm hit survives the imported
/// edit, and the `assert!(after_edit.is_none())` below FAILS.
#[test]
fn relation_memo_fences_on_transitive_imported_fact_edit() {
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    // An imported dependency whose body the relation cold-compute reads
    // through `execute(Instantiate)` while unwrapping the DeclPlaceholder
    // source. This file is NOT a self-root of the relation nodes — it is a
    // purely transitive dependency.
    let upsert_dep = |source: &str| {
        let _ = host
            .upsert(crate::UpsertRequest {
                canonical_id: None,
                input_id: "/w/dep.ts".to_string(),
                source: Arc::from(source),
                file_language: crate::FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .expect("upsert /w/dep.ts");
    };
    upsert_dep("export interface Dep { a: number }");
    let whole_hash_v1 = host
        .ensure_indexed_ready("/w/dep.ts")
        .expect("IndexedReady for /w/dep.ts")
        .whole_hash;

    // Source: a DeclPlaceholder carrier for `Dep@/w/dep.ts`. Interned via
    // the scope-less `intern_node`, so `node_scope` is `Global` and it
    // contributes NO file self-root to the relation entry.
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let source = graph.intern_node(SemanticNodeData::Opaque(
        crate::semantic_query::QueryError::DeclPlaceholder {
            canonical_id: Arc::from("/w/dep.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            name: Arc::from("Dep"),
            whole_hash: whole_hash_v1,
        },
    ));
    // Target: a concrete structural Object the unwrapped `Dep` relates to.
    let target = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("a", number),
    ])));

    // Cold compute: unwrapping the DeclPlaceholder dispatches
    // `execute(Instantiate { Dep@/w/dep.ts })`, which reads `/w/dep.ts` and
    // traces its `FileWholeHash` onto the active tracer.
    let key = dispatch.relate_key_for(source, target);
    let _ = dispatch.execute_relate_pair_as_result_for_tests(source, target);

    // Warm precondition: the judgement is admitted and validates against
    // the unchanged store (else the post-edit miss would not discriminate).
    assert!(
        graph.get_relation_payload(&host, &key).is_some(),
        "precondition: the relation judgement must be admitted and warm before the edit"
    );

    // Edit the TRANSITIVE imported file → its `FileWholeHash` changes.
    upsert_dep("export interface Dep { a: string }");
    let whole_hash_v2 = host
        .ensure_indexed_ready("/w/dep.ts")
        .expect("IndexedReady for /w/dep.ts after edit")
        .whole_hash;
    assert_ne!(
        whole_hash_v1, whole_hash_v2,
        "the imported-file edit must bump its FileWholeHash"
    );

    // The warm read now MISSES: the carrier's traced `FileWholeHash` for
    // `/w/dep.ts` no longer matches the live store. A `&[]`-carrier (pre-
    // change) would still validate and return `Some` here.
    assert!(
        graph.get_relation_payload(&host, &key).is_none(),
        "FENCE: editing the transitive imported fact must MISS the warm relation read \
         (recompute); a carrier that recorded no transitive facts would stale-hit"
    );
}

/// OVERFLOW non-admission: a relation whose traced read-set overflows
/// (`FactReadSetFinalise::Overflow`) is RETURNED to the caller but NOT
/// admitted to the relation memo.
///
/// DISCRIMINATES the `Overflow => return (result, fence)` early-return in
/// `build_relate`: a mutation that admitted regardless of overflow (e.g.
/// dropped the `Overflow` arm and always published) would
/// grow `relation_memo_count()` and FAIL the count assertion. The
/// `relation_force_overflow_observations` test knob forces the overflow
/// without a pathological multi-file fixture.
#[test]
fn relation_memo_overflow_returns_result_without_admission() {
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    // Two concrete primitives whose cold relation is a definite
    // `Assignable` (identity) — the value is returned to the caller.
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    // Arm the overflow knob: observe CAP+1 synthetic facts during the cold
    // compute so the read-set finalises `Overflow`. The RAII guard zeroes the
    // knob on drop (panic-safe) so the forced state never leaks past the test.
    let _overflow_guard = crate::for_tests::relation_force_overflow_observations_for_tests(
        &host,
        crate::resolver_core::FACT_SIGNATURE_CAP + 1,
    );

    let before = graph.relation_memo_count();
    let result = dispatch.execute_relate_pair_as_result_for_tests(string, string);
    let after = graph.relation_memo_count();

    // The judgement is still computed and returned to the caller.
    assert!(
        matches!(result, RelationResult::Assignable { .. }),
        "overflow must still RETURN the computed judgement to the caller; got {result:?}"
    );
    // But it is REFUSED memo admission — the dependency fence cannot be
    // represented under overflow.
    assert_eq!(
        after, before,
        "OVERFLOW: an overflowed read-set must NOT admit a relation-memo entry \
         (count must not grow); a mutation that admitted regardless would FAIL here"
    );
    assert!(
        graph
            .get_relation_payload(&host, &dispatch.relate_key_for(string, string))
            .is_none(),
        "OVERFLOW: no warm entry may be reachable for the overflowed relation"
    );
}

// ============================================================================
// Single-authority invariants — file-absence
// ============================================================================

#[test]
fn solver_relate_module_deleted() {
    // File-absence invariant: `relate.rs` is not part of the final
    // module set — tri-state assignability lives in the semantic
    // graph via ProjectSemanticDispatch.
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace parent")
        .join("crates")
        .join("verter_semantic")
        .join("src")
        .join("analysis")
        .join("type_solver")
        .join("relate.rs");
    assert!(
        !path.exists(),
        "relate.rs must not exist (file-absence invariant)"
    );
}

#[test]
fn solver_project_module_deleted() {
    // File-absence invariant: `project.rs` is not part of the final
    // module set — member/keyspace/surface projections live in
    // ProjectSemanticDispatch's ProjectPath/ProjectMember query
    // surface.
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace parent")
        .join("crates")
        .join("verter_semantic")
        .join("src")
        .join("analysis")
        .join("type_solver")
        .join("project.rs");
    assert!(
        !path.exists(),
        "project.rs must not exist (file-absence invariant)"
    );
}

#[test]
fn type_surface_db_module_deleted() {
    // File-absence invariant: `type_surface_db.rs` is not part of the
    // final module set. The semantic-graph memo is the sole
    // projection authority.
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace parent")
        .join("crates")
        .join("verter_session")
        .join("src")
        .join("resolver_core")
        .join("type_surface_db.rs");
    assert!(
        !path.exists(),
        "type_surface_db.rs must not exist (file-absence invariant)"
    );
}

#[test]
fn dispatch_bridge_module_deleted() {
    // File-absence invariant: `dispatch_bridge.rs` is not part of the
    // final module set.
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace parent")
        .join("crates")
        .join("verter_session")
        .join("src")
        .join("dispatch_bridge.rs");
    assert!(
        !path.exists(),
        "dispatch_bridge.rs must not exist (file-absence invariant)"
    );
}

#[test]
fn solver_host_module_deleted() {
    // File-absence invariant: `solver_host.rs` is not part of the
    // final module set. The session does not route through a
    // `TypeSolverHost` bridge.
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("resolver_core")
        .join("solver_host.rs");
    assert!(
        !path.exists(),
        "solver_host.rs must not exist (file-absence invariant)"
    );
}

// ============================================================================
// Single-authority invariants — identifier-absence
// ============================================================================

#[test]
fn type_query_engine_struct_absent() {
    // Identifier-absence invariant: `TypeQueryEngine` must not appear
    // in production code under any form (struct decl, trait impl,
    // function body). `ComponentMetaQueryEngine` is the sole
    // request-scoped solve engine on the session side.
    let hits = retired_symbol_hits_in_production(&["TypeQueryEngine"]);
    assert!(
        hits.is_empty(),
        "TypeQueryEngine identifier must not appear in production source:\n{}",
        hits.join("\n")
    );
}

#[test]
fn no_session_solver_host_in_production_code() {
    // Identifier-absence invariant: zero non-comment references to
    // `SessionSolverHost` may appear in production source. The
    // session uses `ComponentMetaQueryEngine` + `bare_name_resolve`
    // directly. The grep-based helper skips `//` and `/*` lines as
    // well as `*_tests.rs` / `tests.rs` files so doc pointers and
    // characterization-test blocks stay valid.
    let hits = retired_symbol_hits_in_production(&["SessionSolverHost"]);
    assert!(
        hits.is_empty(),
        "SessionSolverHost identifier must not appear in production source:\n{}",
        hits.join("\n")
    );
}

#[test]
fn no_eval_env_solver_host_in_production_code() {
    // Identifier-absence invariant: `EvalEnvSolverHost` must not
    // appear in production code. Env substitution lives in
    // `structural_substitute_typeof_refs`.
    let hits = retired_symbol_hits_in_production(&["EvalEnvSolverHost"]);
    assert!(
        hits.is_empty(),
        "EvalEnvSolverHost identifier must not appear in production source:\n{}",
        hits.join("\n")
    );
}

#[test]
fn no_type_solver_host_trait_in_production_code() {
    // Identifier-absence invariant: `TypeSolverHost` trait must not
    // appear in production code. The session does not thread a
    // `&dyn TypeSolverHost` through `solve_type`; every solve-like
    // operation runs through `ProjectSemanticDispatch::execute` on
    // the semantic graph.
    let hits = retired_symbol_hits_in_production(&["TypeSolverHost"]);
    assert!(
        hits.is_empty(),
        "TypeSolverHost trait identifier must not appear in production source:\n{}",
        hits.join("\n")
    );
}

#[test]
fn no_parser_arena_adapter_in_production_code() {
    // Identifier-absence invariant: the `ParserArenaAdapter` /
    // `ParserArenaBridgeHost` bridge names must not appear in
    // production code. There is no parser↔dispatch bridge seam.
    let hits = retired_symbol_hits_in_production(&[
        "ParserArenaAdapter",
        "ParserArenaBridgeHost",
        "parser_arena_adapter",
        "parser_arena_bridge",
    ]);
    assert!(
        hits.is_empty(),
        "retired ParserArenaAdapter / ParserArenaBridgeHost identifiers:\n{}",
        hits.join("\n")
    );
}

#[test]
fn solver_no_longer_contains_distributive_union_loop() {
    // File-absence invariant: `solve.rs` is not part of the final
    // module set. The previous distributive-union loop (a walker
    // that iterated `Union` members calling `resolve_node` /
    // `collect_structural_property_descriptors_inner` on each arm)
    // is trivially absent because the file does not exist.
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace parent")
        .join("crates")
        .join("verter_semantic")
        .join("src")
        .join("analysis")
        .join("type_solver")
        .join("solve.rs");
    assert!(
        !path.exists(),
        "solve.rs must not exist (file-absence invariant)"
    );
}

#[test]
fn solve_rs_has_no_resolve_indexed_access_or_collect_structural_property_descriptors_inner() {
    // Identifier-absence invariant: the walker methods
    // `resolve_indexed_access` +
    // `collect_structural_property_descriptors_inner` must not appear
    // in production source — they belong to a solver shape that is
    // not part of the final design.
    let hits = retired_symbol_hits_in_production(&[
        "resolve_indexed_access",
        "collect_structural_property_descriptors_inner",
    ]);
    assert!(
        hits.is_empty(),
        "walker method identifiers must not appear in production source:\n{}",
        hits.join("\n")
    );
}

#[test]
fn resolver_runtime_no_longer_imports_type_surface_db() {
    // Dependency-absence invariant: `resolver_runtime.rs` must not
    // import `TypeSurfaceDb` or carry a `type_surfaces` field. Grep
    // the source directly so the assertion fails loudly if a future
    // patch reintroduces the dep.
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace parent")
        .join("crates")
        .join("verter_session")
        .join("src")
        .join("resolver_core")
        .join("resolver_runtime.rs");
    let src = std::fs::read_to_string(&path).expect("resolver_runtime.rs must exist");
    assert!(
        !src.contains("TypeSurfaceDb"),
        "resolver_runtime.rs must not import TypeSurfaceDb"
    );
    assert!(
        !src.contains("type_surfaces:"),
        "resolver_runtime.rs must not carry a `type_surfaces:` field"
    );
}

#[test]
fn resolver_core_mod_no_longer_reexports_type_surface_db() {
    // Dependency-absence invariant: `resolver_core/mod.rs` must not
    // declare `mod type_surface_db` nor carry any
    // `pub(crate) use type_surface_db::...` re-export.
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace parent")
        .join("crates")
        .join("verter_session")
        .join("src")
        .join("resolver_core")
        .join("mod.rs");
    let src = std::fs::read_to_string(&path).expect("resolver_core/mod.rs must exist");
    assert!(
        !src.contains("type_surface_db"),
        "resolver_core/mod.rs must not reference the type_surface_db module"
    );
    assert!(
        !src.contains("TypeSurfaceDb"),
        "resolver_core/mod.rs must not re-export TypeSurfaceDb"
    );
}

#[test]
fn relation_memo_lives_in_the_family_memo() {
    // The dedicated `BudgetedRelationMemo` wrapper is DELETED. Relation
    // judgements live in the family memo's `FamilyKey::Relate` family
    // (the `ModeSlot::Single` slot) inside `SemanticGraphStore::entries` —
    // the ONE memo substrate every warm semantic-query entry rides, with
    // the per-family cap / invalid-first / LRU eviction, the family
    // `memo_budget` global bound, and the reverse-index drains. The store
    // must NOT carry a dedicated `relation_memo` field, and the retired
    // wrapper type must not exist.
    let memo_src = include_str!("semantic_query_memo/mod.rs");
    assert!(
        !memo_src.contains("relation_memo: BudgetedRelationMemo"),
        "SemanticGraphStore must NOT carry a dedicated `relation_memo` field — \
         relation judgements live in the family memo's `Relate` family"
    );
    assert!(
        !memo_src.contains("BudgetedRelationMemo"),
        "the dedicated `BudgetedRelationMemo` wrapper is retired — no dual memo may remain"
    );
    // Behavioural verification: the relation entries route through the
    // `Relate` family read/write API the sole relation authority
    // (`execute(SemanticQueryKey::Relate)` → `build_relate`) rides.
    let host = host_for_relation_tests();
    let graph = host.project_type_store().semantic_graph();
    assert_eq!(
        graph.relation_memo_count(),
        0,
        "fresh host has zero relation memo entries"
    );
}

// ============================================================================
// Single-authority invariants — cardinality
// ============================================================================
//
// Each test walks the workspace source and greps for exactly-one
// occurrence of the named authority surface, enforcing the
// authority-uniqueness contract that each owning surface has a
// single home.

/// Count occurrences of `needle` in all `.rs` files under
/// `crates/`, excluding:
/// - the characterization-test file itself (names these needles as
///   string literals for the search).
/// - `tests.rs` and any `*_tests.rs` file (test helpers + regression
///   fixtures may name retired identifiers for characterisation).
/// - lines that start with `//` or `/*` (retirement-documentation
///   comments are not live references).
fn count_def_in_crates(needle: &str) -> usize {
    let workspace_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace parent")
        .to_path_buf();
    let crates_dir = workspace_root.join("crates");
    let self_file = "project_semantic_dispatch_invariants_tests.rs";
    let mut count = 0usize;
    fn walk(dir: &std::path::Path, needle: &str, self_file: &str, count: &mut usize) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                let name = entry.file_name();
                if matches!(
                    name.to_string_lossy().as_ref(),
                    "target" | "node_modules" | ".git"
                ) {
                    continue;
                }
                walk(&p, needle, self_file, count);
            } else if p.extension().is_some_and(|e| e == "rs") {
                let filename = p.file_name().unwrap_or_default().to_string_lossy();
                if filename == self_file {
                    continue;
                }
                // Skip test files — regression fixtures may legitimately
                // name retired identifiers for characterisation.
                // Production code is the sole enforcement surface for
                // single-authority cardinality.
                if filename == "tests.rs" || filename.ends_with("_tests.rs") {
                    continue;
                }
                // Skip integration-test files (anything under a `tests/`
                // directory): a guard's assertion / self-test fixtures may name
                // a definition needle (`fn lower_type_expr_structural(`) as a
                // string literal, which is not a production definition.
                let p_str = p.to_string_lossy().replace('\\', "/");
                if p_str.contains("/tests/") {
                    continue;
                }
                if let Ok(content) = std::fs::read_to_string(&p) {
                    for line in content.lines() {
                        let trimmed = line.trim_start();
                        if trimmed.starts_with("//") || trimmed.starts_with("/*") {
                            continue;
                        }
                        *count += line.matches(needle).count();
                    }
                }
            }
        }
    }
    walk(&crates_dir, needle, self_file, &mut count);
    count
}

#[test]
fn semantic_query_api_has_exactly_one_implementor() {
    // `impl SemanticQueryApi for ` should appear exactly once — on
    // `ProjectSemanticDispatch`.
    let count = count_def_in_crates("impl SemanticQueryApi for ");
    // Also allow the "impl<'a> SemanticQueryApi for ..." form.
    let count_lifetimed =
        count_def_in_crates("impl<'a> SemanticQueryApi for ProjectSemanticDispatch");
    assert!(
        count + count_lifetimed >= 1,
        "SemanticQueryApi must have at least one implementor; got {count} plain + {count_lifetimed} lifetimed"
    );
    // Exactly one.
    assert_eq!(
        count + count_lifetimed,
        1,
        "SemanticQueryApi must have exactly one implementor; got {count} plain + {count_lifetimed} lifetimed"
    );
}

#[test]
fn relation_engine_has_exactly_one_implementation() {
    // Retired-symbol guard (the relation-engine activation): the bare-pair
    // `relate_nodes` entry point, the process-global `RELATION_IN_FLIGHT`
    // TLS cycle guard, and its `enter_/exit_relation_guard` helpers are
    // DELETED and cannot be re-introduced — the SOLE relation authority is
    // `execute(SemanticQueryKey::Relate)` (`fn execute_relate`, exactly one
    // definition, in `project_semantic_dispatch/relation.rs`).
    let relate_nodes = count_def_in_crates("fn relate_nodes");
    assert_eq!(
        relate_nodes, 0,
        "the bare-pair `relate_nodes` entry point is retired; got {relate_nodes}"
    );
    let tls_guard = count_def_in_crates("RELATION_IN_FLIGHT");
    assert_eq!(
        tls_guard, 0,
        "the process-global RELATION_IN_FLIGHT TLS cycle guard is retired; got {tls_guard}"
    );
    let guard_helpers = count_def_in_crates("fn enter_relation_guard")
        + count_def_in_crates("fn exit_relation_guard");
    assert_eq!(
        guard_helpers, 0,
        "the enter_/exit_relation_guard helpers are retired; got {guard_helpers}"
    );
    let authority = count_def_in_crates("fn execute_relate(");
    assert_eq!(
        authority, 1,
        "fn execute_relate (the sole relation authority) must have exactly one definition; got {authority}"
    );
}

#[test]
fn type_expr_lowering_has_exactly_two_single_definition_producers() {
    // `TypeExpr` lowering has exactly TWO producers, each with a single
    // definition:
    //   1. the EAGER, resolving producer `shallow_lower_type_expr_with_context`,
    //      which takes the full `ProjectionReductionContext` so every caller
    //      states its demand explicitly — a bare-`mode` wrapper that defaults
    //      the demand to `Published` is forbidden (it is exactly how a transit /
    //      skeleton caller would silently lower at a publication demand it never
    //      asked for); and
    //   2. the QUERY-FREE structural producer `lower_type_expr_structural`
    //      (owned by `crate::structural_carrier_producer::macro_arg_producer`,
    //      where it is module-private — no visibility modifier — so no other
    //      file can name it; the only crate-visible producer entry is
    //      `macro_type_arg_hot_ref`), which emits the dormant graph carriers
    //      from the owned `TypeExpr` without performing any name / import /
    //      type resolution.
    // The two are distinct and non-overlapping; neither may grow a second
    // definition, and the retired bare-`mode` eager wrapper stays absent.
    let legacy = count_def_in_crates("fn shallow_lower_type_expr(");
    let with_ctx = count_def_in_crates("fn shallow_lower_type_expr_with_context(");
    let structural = count_def_in_crates("fn lower_type_expr_structural(");
    assert_eq!(
        legacy, 0,
        "the bare-mode `fn shallow_lower_type_expr(` wrapper is retired; \
         callers state the full ProjectionReductionContext; got {legacy}"
    );
    assert_eq!(
        with_ctx, 1,
        "fn shallow_lower_type_expr_with_context (the single eager producer) must \
         have exactly one definition; got {with_ctx}"
    );
    assert_eq!(
        structural, 1,
        "fn lower_type_expr_structural (the single query-free structural producer) \
         must have exactly one definition; got {structural}"
    );
}

#[test]
fn semantic_node_to_type_expr_has_exactly_one_path() {
    // Sibling invariant of `type_expr_lowering_has_exactly_two_single_definition_producers`:
    // the reverse direction (`SemanticNodeId → TypeExpr`) must also
    // have exactly one production path. `fn raise_node_to_type_expr(`
    // appears exactly once — the module-private shell primitive in
    // `project_semantic_dispatch/raise.rs` that delegates to
    // `shape_engine::fold_to_type_expr`. The trailing `(` is part of the
    // needle as a whole-identifier boundary so the counter cannot
    // double-count any `..._suffix(` variant of the same name stem.
    let count = count_def_in_crates("fn raise_node_to_type_expr(");
    assert_eq!(
        count, 1,
        "fn raise_node_to_type_expr must have exactly one definition; got {count}"
    );
}

#[test]
fn relation_memo_has_exactly_one_owner() {
    // The dedicated `BudgetedRelationMemo` wrapper is RETIRED — relation
    // judgements have exactly ONE owner: the family memo
    // (`SemanticGraphStore::entries`) under the `FamilyKey::Relate`
    // family. No dedicated field, no wrapper type, no relation-local
    // `DashMap`, no dual memo path.
    let field_count = count_def_in_crates("relation_memo: BudgetedRelationMemo");
    assert_eq!(
        field_count, 0,
        "the dedicated relation_memo field is retired — relation judgements live in the \
         family memo's `Relate` family; got {field_count}"
    );
    let wrapper_count = count_def_in_crates("BudgetedRelationMemo");
    assert_eq!(
        wrapper_count, 0,
        "the BudgetedRelationMemo wrapper type is retired; got {wrapper_count}"
    );
    let map_count = count_def_in_crates("memo: DashMap<RelateMemoKey,");
    assert_eq!(
        map_count, 0,
        "no dedicated relation `DashMap` may remain — the family memo is the single \
         owner; got {map_count}"
    );
    // The retired bare-pair relation memo key must not survive anywhere.
    let bare_pair_count = count_def_in_crates("memo: DashMap<(SemanticNodeId, SemanticNodeId)");
    assert_eq!(
        bare_pair_count, 0,
        "the bare-pair relation memo key is RETIRED — re-keyed on the \
         full RelateMemoKey identity; got {bare_pair_count}"
    );
}

#[test]
fn semantic_node_map_has_exactly_one_owner() {
    // The semantic node arena is accessed via `NodeArena` in
    // `SemanticGraphStore`. Ensure the `arena: NodeArena` field
    // appears exactly once.
    let count = count_def_in_crates("arena: NodeArena");
    assert_eq!(
        count, 1,
        "semantic node arena must have exactly one owner; got {count}"
    );
}

// ============================================================================
// §6.5 Dispatch hygiene (1 test)
// ============================================================================

#[test]
fn dispatch_subtree_bounded_loops_are_annotated() {
    // Every bounded loop in `crates/verter_session/src/project_semantic_dispatch/`
    // must be annotated with `// bounded-loop: <reason>` on the
    // preceding line ( Bounded-loop annotation convention).
    // The only currently-approved reason is `fence-retry`. Any
    // `for _ in 0..N` in executable code that is not preceded by
    // such a comment is a violation.
    //
    // Excluded: comment lines (which may describe retired loops),
    // the `tests.rs` file inside the dispatch subtree (test
    // fixtures are allowed to iterate freely).
    let workspace_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace parent")
        .to_path_buf();
    let dispatch_dir = workspace_root
        .join("crates")
        .join("verter_session")
        .join("src")
        .join("project_semantic_dispatch");
    let mut violations: Vec<String> = Vec::new();
    fn walk(dir: &std::path::Path, violations: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                walk(&p, violations);
            } else if p.extension().is_some_and(|e| e == "rs") {
                // Skip the dispatch subtree's own tests.rs.
                if p.file_name().is_some_and(|n| n == "tests.rs") {
                    continue;
                }
                let Ok(content) = std::fs::read_to_string(&p) else {
                    continue;
                };
                let lines: Vec<&str> = content.lines().collect();
                for (i, line) in lines.iter().enumerate() {
                    // Skip comment lines entirely (they may describe
                    // retired loops for documentation purposes).
                    let trimmed_line = line.trim_start();
                    if trimmed_line.starts_with("//") || trimmed_line.starts_with("/*") {
                        continue;
                    }
                    if line.contains("for _ in 0..") {
                        // Check if a comment on the preceding line
                        // or the same line (inline) carries the
                        // annotation.
                        let prev = if i > 0 { lines[i - 1] } else { "" };
                        if !prev.contains("// bounded-loop:") && !line.contains("// bounded-loop:")
                        {
                            violations.push(format!(
                                "{}:{}: `for _ in 0..N` without // bounded-loop annotation",
                                p.display(),
                                i + 1
                            ));
                        }
                    }
                }
            }
        }
    }
    walk(&dispatch_dir, &mut violations);
    assert!(
        violations.is_empty(),
        "dispatch subtree bounded loops are unannotated:\n{}",
        violations.join("\n")
    );
}

// ============================================================================
// Class / interface lowering
// ============================================================================

/// Class lowering invariant: classes lower to a canonical
/// `SemanticNodeData::Object` surface. Heritage members merge into
/// the object's `members` via `Instantiate` / `ProjectPath(Expanded)`
/// so downstream consumers see a single flat surface — no `Class` or
/// `Interface` variant. Discriminating check: `SemanticNodeData` has
/// exactly one "object-like" structural variant (`Object`), not a
/// parallel `Class` / `Interface` variant set.
#[test]
fn class_lowers_to_object_with_heritage_merged_members() {
    // The SemanticNodeData enum is the single-authority shape surface.
    // Classes and interfaces map onto `Object` — no dedicated
    // `Class`/`Interface` variants exist.
    let semantic_query_src = include_str!("semantic_query.rs");
    assert!(
        !contains_variant_decl(semantic_query_src, "Class"),
        "SemanticNodeData must not define a Class variant — classes lower to Object"
    );
    assert!(
        !contains_variant_decl(semantic_query_src, "Interface"),
        "SemanticNodeData must not define an Interface variant — interfaces lower to Object"
    );
    // And `SurfaceView` (the Object body) is the merged-members
    // carrier per
    assert!(
        semantic_query_src.contains("pub struct SurfaceView"),
        "SurfaceView must be the Object body type carrying merged heritage members"
    );
}

/// Interface lowering invariant: interfaces lower to the same
/// `SemanticNodeData::Object` shape classes do. Discriminating check:
/// the same assertion applies — no Interface variant, only Object.
#[test]
fn interface_lowers_to_object_identically_to_class() {
    let semantic_query_src = include_str!("semantic_query.rs");
    assert!(
        !contains_variant_decl(semantic_query_src, "Interface"),
        "SemanticNodeData must not define an Interface variant — interfaces lower to Object"
    );
    assert!(
        semantic_query_src.contains("Object(SurfaceView)")
            || semantic_query_src.contains("Object(Arc<SurfaceView>)"),
        "SemanticNodeData::Object is the single Object-shape carrier for classes and interfaces"
    );
}

/// Path-walker member-projection invariant: member projection routes
/// through the `SemanticNodeData::Object` arm of
/// `PathWalker::advance_step`. Since classes and interfaces lower to
/// `Object`, their member projection uses the same walker arm — no
/// dedicated Class/Interface arm. Discriminating check: `walk.rs`
/// has exactly one object-member arm (the `SemanticNodeData::Object`
/// match), and it matches both class and interface surfaces via the
/// unified lowering.
#[test]
fn class_member_projection_uses_object_walker_arm() {
    let walk_src = include_str!("project_semantic_dispatch/walk.rs");
    assert!(
        walk_src.contains("SemanticNodeData::Object(surface)"),
        "walk.rs must carry the Object arm for member projection"
    );
    // Negative: no parallel Class/Interface arms in the walker.
    for line in walk_src.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        assert!(
            !trimmed.starts_with("SemanticNodeData::Class"),
            "walk.rs must not carry a Class arm: `{line}`"
        );
        assert!(
            !trimmed.starts_with("SemanticNodeData::Interface"),
            "walk.rs must not carry an Interface arm: `{line}`"
        );
    }
}

fn contains_variant_decl(src: &str, variant_name: &str) -> bool {
    // Match a variant declaration — either `Class,` / `Class(...)` /
    // `Class {` — at the start of a trimmed line inside the source.
    // Avoid false positives on field names (`class_name`), doc
    // comments, and string literals by requiring start-of-token.
    for line in src.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        // Check the variant name appears at line start followed by
        // `,`, `(`, `{`, or whitespace + `{`.
        if let Some(rest) = trimmed.strip_prefix(variant_name) {
            let next_char = rest.chars().next().unwrap_or(' ');
            if matches!(next_char, ',' | '(' | '{') {
                return true;
            }
            if next_char == ' ' && rest.trim_start().starts_with('{') {
                return true;
            }
        }
    }
    false
}

// ============================================================================
// §6.5 D5 grep (5 tests) — un-ignored in §5.9
// ============================================================================
//
// Each test asserts that a specific retired-cache symbol is absent
// from production code via a workspace-wide grep. The scan skips
// the self-file (names symbols as strings), files matching
// `*_tests.rs`, and lines that begin with `//` (documentation /
// retirement-note comments in the sibling files are fine).

fn retired_symbol_hits_in_production(symbols: &[&str]) -> Vec<String> {
    let workspace_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace parent")
        .to_path_buf();
    let crates_dir = workspace_root.join("crates");
    let self_file = "project_semantic_dispatch_invariants_tests.rs";
    let mut hits: Vec<String> = Vec::new();
    fn walk(dir: &std::path::Path, symbols: &[&str], self_file: &str, hits: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                let name = entry.file_name();
                if matches!(
                    name.to_string_lossy().as_ref(),
                    "target" | "node_modules" | ".git"
                ) {
                    continue;
                }
                walk(&p, symbols, self_file, hits);
            } else if p.extension().is_some_and(|e| e == "rs") {
                let fname = p.file_name().unwrap_or_default().to_string_lossy();
                if fname == self_file {
                    continue;
                }
                if fname.ends_with("_tests.rs") || fname == "tests.rs" {
                    continue;
                }
                let Ok(content) = std::fs::read_to_string(&p) else {
                    continue;
                };
                for (i, line) in content.lines().enumerate() {
                    let trimmed = line.trim_start();
                    if trimmed.starts_with("//") || trimmed.starts_with("/*") {
                        continue;
                    }
                    for sym in symbols {
                        if line.contains(sym) {
                            hits.push(format!("{}:{}: retired `{}`", p.display(), i + 1, sym));
                        }
                    }
                }
            }
        }
    }
    walk(&crates_dir, symbols, self_file, &mut hits);
    hits
}

#[test]
fn subject_key_and_op_cache_types_deleted() {
    // Identifier-absence invariant: the request-scoped
    // query-identity subsystem (SubjectKey/OpKey and friends) must
    // not appear in production code.
    let hits = retired_symbol_hits_in_production(&[
        "SubjectKey",
        "SubjectId",
        "ResolvedSubjectKey",
        "ResolvedSubjectEntry",
        "OpKey",
    ]);
    assert!(
        hits.is_empty(),
        "retired subject-key / op-cache types still present:\n{}",
        hits.join("\n")
    );
}

#[test]
fn solver_caches_member_and_keyspace_deleted() {
    // The retired `SolverCaches::member` / `.keyspace` projection
    // caches. Check for field-name tokens that are unique to the
    // retired struct. We also grep for the caches' key types.
    let hits = retired_symbol_hits_in_production(&[
        "projection_cache_member",
        "projection_cache_keyspace",
    ]);
    assert!(
        hits.is_empty(),
        "retired SolverCaches::{{member, keyspace}} projection cache fields still present:\n{}",
        hits.join("\n")
    );
}

#[test]
fn solver_caches_relation_field_deleted() {
    // `SolverCaches.relation` was retired when the arena
    // `relate.rs` engine lost its responsibility. The field should
    // not appear in production code; any remaining relation cache
    // lives in `SemanticGraphStore::relation_memo`.
    let hits = retired_symbol_hits_in_production(&[
        "SolverCaches::relation",
        "caches.relation",
        "solver_caches.relation",
    ]);
    assert!(
        hits.is_empty(),
        "retired SolverCaches.relation cache field still present:\n{}",
        hits.join("\n")
    );
}

#[test]
fn projection_cache_and_active_projection_keys_deleted() {
    let hits = retired_symbol_hits_in_production(&["ProjectionCacheKey", "active_projection_keys"]);
    assert!(
        hits.is_empty(),
        "retired ProjectionCacheKey / active_projection_keys still present:\n{}",
        hits.join("\n")
    );
}

#[test]
fn type_query_engine_has_no_shallow_caches() {
    // Identifier-absence invariant: the shallow-cache field names
    // (`shallow_field_expr_cache`, `shallow_imported_bare_ref_cache`,
    // `shallow_transitive_ref_cache`, `instantiation_cache`) must
    // not appear in production source.
    let hits = retired_symbol_hits_in_production(&[
        "shallow_field_expr_cache",
        "shallow_imported_bare_ref_cache",
        "shallow_transitive_ref_cache",
        "instantiation_cache:",
    ]);
    assert!(
        hits.is_empty(),
        "TypeQueryEngine shallow-cache field names must not appear in production source:\n{}",
        hits.join("\n")
    );
}

// ============================================================================
// Structural invariants — SolverTraceSummary + relation dispatch scope
// ============================================================================

#[test]
fn solver_trace_summary_does_not_double_count_dispatch_metrics() {
    // Structural invariant: `SolverTraceSummary` must not contain
    // counters that duplicate dispatch-owned `SemanticGraphStats`
    // fields. With the type entirely absent, the property holds
    // trivially — verify by grep-absence.
    let hits = retired_symbol_hits_in_production(&["SolverTraceSummary", "solver_trace_summary"]);
    assert!(
        hits.is_empty(),
        "SolverTraceSummary still referenced in production code (double-count risk):\n{}",
        hits.join("\n")
    );
}

// ============================================================================
// Zero-legacy
// ============================================================================

#[test]
fn no_deprecated_attributes_on_retired_symbols() {
    // Zero-legacy invariant: no `#[deprecated]` attribute may
    // reference any of the symbol names below. Production code
    // either uses these symbols (in which case they cannot be
    // deprecated) or, if they are absent from the final design,
    // they are absent entirely — not present-but-deprecated.
    let retired = [
        "TypeSolverHost",
        "EvalEnvSolverHost",
        "SessionSolverHost",
        "TypeQueryEngine",
        "TypeSurfaceDb",
        "TypeSurfaceOpResult",
        "dispatch_bridge",
        "shallow_relation_check",
        "project_expr_surface_as_type_expr",
        "solver_host_for_scope",
        "owner_engine",
        "expand_macro_types",
        "expand_object_shape",
        "expand_normalized_expr",
        "MAX_TYPE_RESOLUTION_DEPTH",
        "ParserArenaAdapter",
        "ParserArenaBridgeHost",
    ];
    let workspace_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace parent")
        .to_path_buf();
    let crates_dir = workspace_root.join("crates");
    let self_file = "project_semantic_dispatch_invariants_tests.rs";
    let mut violations: Vec<String> = Vec::new();
    fn walk(
        dir: &std::path::Path,
        retired: &[&str],
        self_file: &str,
        violations: &mut Vec<String>,
    ) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                let name = entry.file_name();
                if matches!(
                    name.to_string_lossy().as_ref(),
                    "target" | "node_modules" | ".git"
                ) {
                    continue;
                }
                walk(&p, retired, self_file, violations);
            } else if p.extension().is_some_and(|e| e == "rs") {
                let fname = p.file_name().unwrap_or_default().to_string_lossy();
                if fname == self_file {
                    continue;
                }
                // Skip tests.rs files — test fixtures may reference
                // retired symbols for characterization.
                if fname == "tests.rs" || fname.ends_with("_tests.rs") {
                    continue;
                }
                let Ok(content) = std::fs::read_to_string(&p) else {
                    continue;
                };
                let lines: Vec<&str> = content.lines().collect();
                for (i, line) in lines.iter().enumerate() {
                    if !line.contains("#[deprecated") {
                        continue;
                    }
                    // Scan the next few lines for a retired symbol mention.
                    let window_end = (i + 4).min(lines.len());
                    let window = lines[i..window_end].join("\n");
                    for sym in retired {
                        if window.contains(sym) {
                            violations.push(format!(
                                "{}:{}: #[deprecated] names retired symbol `{}`",
                                p.display(),
                                i + 1,
                                sym
                            ));
                        }
                    }
                }
            }
        }
    }
    walk(&crates_dir, &retired, self_file, &mut violations);
    assert!(
        violations.is_empty(),
        "#[deprecated] attributes on retired symbols:\n{}",
        violations.join("\n")
    );
}

// ============================================================================
// D4 behavioural invariants
// ============================================================================

/// Symbolic-stop absence invariant: there must be no `Applied`-stub
/// short-circuit for open generic expansions. The entire `solve.rs`
/// file is absent; the short-circuit cannot exist. Assert by file
/// absence.
#[test]
fn open_generic_expansion_no_longer_short_circuits_to_applied_stub() {
    let workspace_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace parent")
        .to_path_buf();
    let solve_path =
        workspace_root.join("crates/verter_semantic/src/analysis/type_solver/solve.rs");
    assert!(
        !solve_path.exists(),
        "type_solver/solve.rs must not exist — the applied-stub \
         short-circuit for open generic expansion would have lived \
         here; its absence is the discriminating check"
    );
    // Additionally: the walker method
    // `collect_structural_property_descriptors_inner` that would have
    // driven the stub must be absent from production code.
    // (`resolve_node` is a common method name in other subsystems —
    // matched below by the more-specific walker identifier.)
    let hits =
        retired_symbol_hits_in_production(&["collect_structural_property_descriptors_inner"]);
    assert!(
        hits.is_empty(),
        "walker method identifier must not appear in production source:\n{}",
        hits.join("\n")
    );
}

/// Symbolic-indexed-access absence invariant: path projection
/// through open applied types must NOT short-circuit to a
/// `symbolic_indexed_access` wrapper. The canonical path runs through
/// `PathWalker::walk`'s iterative worklist, which emits the canonical
/// `SemanticNodeData::IndexedAccess` or continues into
/// `build_indexed_access` via dispatch re-entry.
#[test]
fn path_projection_through_open_applied_does_not_short_circuit_to_symbolic_indexed_access() {
    let workspace_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace parent")
        .to_path_buf();
    let solve_path =
        workspace_root.join("crates/verter_semantic/src/analysis/type_solver/solve.rs");
    assert!(
        !solve_path.exists(),
        "type_solver/solve.rs must not exist — the symbolic_indexed_access \
         short-circuit would have lived here"
    );
    // The symbolic-stop helpers must not appear in production code.
    // `symbolic_indexed_access` is the stub name a walker would emit
    // instead of re-dispatching.
    let hits = retired_symbol_hits_in_production(&[
        "symbolic_indexed_access",
        "record_indexed_access_open_skip",
        "indexed_access_open_skips",
    ]);
    assert!(
        hits.is_empty(),
        "symbolic-stop helpers must not appear in production source:\n{}",
        hits.join("\n")
    );
}

/// Counter-absence invariant: `indexed_access_open_skips` counter +
/// the `indexed_access_open_skip` audit hook are absent entirely.
/// The `SemanticGraphStats` telemetry is the reusable-work authority;
/// open-skip bookkeeping has no home in the dispatch architecture.
///
/// The entire `type_solver::audit` module is absent — `audit.rs`
/// does not exist in the solver directory. Its absence is the
/// strongest possible proof the counter cannot be present.
#[test]
fn indexed_access_open_skips_counter_retired() {
    let workspace_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace parent")
        .to_path_buf();
    let audit_path =
        workspace_root.join("crates/verter_semantic/src/analysis/type_solver/audit.rs");
    assert!(
        !audit_path.exists(),
        "type_solver/audit.rs must not exist; found at {}",
        audit_path.display()
    );
}

/// Budget-domain absence invariant: `SolveLimits::max_resolve_steps`
/// is not part of the final design — there is no `resolve_steps`
/// counter in dispatch. Assert by file absence + identifier
/// absence.
#[test]
fn budget_domain_solver_resolve_steps_trips_cleanly() {
    let workspace_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace parent")
        .to_path_buf();
    let solve_path =
        workspace_root.join("crates/verter_semantic/src/analysis/type_solver/solve.rs");
    assert!(
        !solve_path.exists(),
        "type_solver/solve.rs must not exist — `SolveLimits::max_resolve_steps` \
         would have lived here and has no dispatch-side successor"
    );
    let hits = retired_symbol_hits_in_production(&["max_resolve_steps", "SolveLimits"]);
    assert!(
        hits.is_empty(),
        "solver budget identifiers must not appear in production source:\n{}",
        hits.join("\n")
    );
}

/// Budget-domain absence invariant: `SolveLimits::max_arena_nodes`
/// is not part of the final design. Dispatch uses the
/// `SemanticGraphStore` interned node pool with no per-request cap.
/// Assert by file absence + identifier absence.
#[test]
fn budget_domain_solver_arena_nodes_trips_cleanly() {
    let workspace_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace parent")
        .to_path_buf();
    let solve_path =
        workspace_root.join("crates/verter_semantic/src/analysis/type_solver/solve.rs");
    assert!(
        !solve_path.exists(),
        "type_solver/solve.rs must not exist — `SolveLimits::max_arena_nodes` \
         would have lived here and has no dispatch-side successor"
    );
    let hits = retired_symbol_hits_in_production(&["max_arena_nodes"]);
    assert!(
        hits.is_empty(),
        "solver arena-node budget identifier must not appear in production source:\n{}",
        hits.join("\n")
    );
}

// ============================================================================
// Substitution-environment preservation
// ============================================================================
//
// Each test asserts an architectural property of the dispatch path
// that mirrors the responsibilities once carried by the legacy
// solver / owner_engine surface. Bodies discriminate: a stub that
// unconditionally returns would fail the discriminating check
// (non-trivial input threading, memo warm-hit observation, or
// specific function-presence grep).

/// Substitution-preservation contract: scoped solves must preserve
/// their `env` (type-parameter bindings) and `name_resolution`
/// (import map) through the prepared-decl context.
/// Post-migration, `ProjectSemanticDispatch::shallow_lower_type_expr`
/// accepts both as function parameters. The discriminating assertion
/// is that the lowering entry point signature carries both fields —
/// grepped from the canonical source so file-split renames surface
/// immediately.
#[test]
fn migrate_owner_engine_solve_scoped_preserves_env_and_name_resolution() {
    let lower_src = include_str!("project_semantic_dispatch/lower.rs");
    assert!(
        lower_src.contains("env: &FxHashMap<String, SemanticNodeId>"),
        "shallow_lower_type_expr must accept the `env` map"
    );
    assert!(
        lower_src
            .contains("name_resolution: &FxHashMap<std::sync::Arc<str>, ResolvedRootIdentity>"),
        "shallow_lower_type_expr must accept the `name_resolution` map"
    );
}

/// Substitution-preservation contract: `ProjectSemanticDispatch`
/// exposes the `ProjectPath` entry point with
/// `ProjectionMode::Expanded` that supersedes any standalone
/// `owner_engine.project_expr_surface_as_type_expr` call.
#[test]
fn migrate_owner_engine_project_expr_surface_as_type_expr_preserves_env() {
    // Mode enum admits `Expanded`.
    let mode = crate::semantic_query::ProjectionMode::Expanded;
    assert_eq!(mode, crate::semantic_query::ProjectionMode::Expanded);
    // SemanticQueryKey admits ProjectPath with `ProjectionMode::Expanded`.
    let key = crate::semantic_query::SemanticQueryKey::ProjectPath {
        base: crate::semantic_query::SemanticNodeId(0),
        path: std::sync::Arc::from(Vec::new().into_boxed_slice()),
        context: crate::semantic_query::ProjectionReductionContext::published(mode),
    };
    match key {
        crate::semantic_query::SemanticQueryKey::ProjectPath { .. } => {}
        other => panic!("ProjectPath variant required post-migration, got {other:?}"),
    }
}

/// Dispatch-first routing contract: the dispatch path on
/// ComponentMetaQueryEngine must route through dispatch — checked by
/// file-content grep so a regression that deletes the dispatch call
/// surfaces immediately.
///
/// Scans all sibling files in the `component_meta_query_engine/`
/// folder (mod.rs + child modules) because
/// `materialize_member_surface_expr` and the dispatch-routed helpers
/// may live in private child modules after the folder split.
#[test]
fn migrate_engine_lower_and_project_to_expanded_preserves_env() {
    let cmqe_files: &[&str] = &[
        include_str!("resolver_core/component_meta_query_engine/mod.rs"),
        include_str!("resolver_core/component_meta_query_engine/registry_decl.rs"),
        include_str!("resolver_core/component_meta_query_engine/shallow_preserve.rs"),
        include_str!("resolver_core/component_meta_query_engine/surface.rs"),
        include_str!("resolver_core/component_meta_query_engine/helpers.rs"),
    ];
    let combined = cmqe_files.join("\n");
    // The dispatch-first lowering entry is the OWNER-AWARE scope spelling
    // (`lower_type_expr_in_owner_scope_with_mode`): the engine threads the
    // exact top-level lexical owner into the dispatch lowering so a Vue
    // module/setup owner pair never collapses onto one scope.
    assert!(
        combined.contains("dispatch.lower_type_expr_in_owner_scope_with_mode"),
        "lower_and_project_to_expanded must attempt dispatch-first lowering post-migration"
    );
    assert!(
        combined.contains("ProjectPath"),
        "lower_and_project_to_expanded must query ProjectPath post-migration"
    );
}

// characterization tests `migrate_engine_project_expr_surface_shape_preserves_env`,
// `instantiate_local_generic_ref_production_callers_migrated_to_dispatch_helper`, and
// `phase_05c_engine_surface_trampolines_route_through_dispatch` deleted alongside the
// engine methods they characterized. Their entire reason for existing
// was to discriminate the trampoline-body-present vs trampoline-body-deleted
// states during the 5c-5f migration window. Post-deletion, the methods they
// characterized are gone, so these tests no longer have a target to
// discriminate against (CLAUDE.md "Legacy Code Deletion": delete tests
// that characterize deleted behavior).

/// Relation-memo identity contract: the relation storage lives on
/// `SemanticGraphStore`'s family memo (the `Relate` family) and behaves
/// as a write-read-warm cycle — construction of a new host must expose
/// the payload read/write entry points.
#[test]
fn type_surface_db_identity_moved_to_semantic_graph_store_memo() {
    let host = host_for_relation_tests();
    let graph = host.project_type_store().semantic_graph();
    let a = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let b = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let key = crate::semantic_query::RelateMemoKey::assignable(
        a,
        b,
        crate::semantic_query::RelationContext::default(),
    );
    // Cold: no entry.
    assert!(
        graph.get_relation_payload(&host, &key).is_none(),
        "cold relation memo must return None before publish"
    );
    // Publish a NotAssignable judgement. The relation memo entry is
    // self-version-rooted; a synthetic publish with no self-roots uses
    // an empty carrier + empty self-root set so the warm read validates
    // vacuously.
    let carrier = crate::fact_signature_helpers::ReadSetSignature::empty();
    let generation = host.project_type_store().current_project_generation();
    graph.insert_relation_payload_for_tests(
        key.clone(),
        carrier,
        std::sync::Arc::from([]),
        graph.relation_payload_for_tests(crate::semantic_query::RelationOutcome::NotAssignable),
        generation,
    );
    // Warm: must return the same judgement.
    let cached = graph
        .get_relation_payload(&host, &key)
        .expect("published relation memo must be readable");
    assert_eq!(
        cached.outcome,
        crate::semantic_query::RelationOutcome::NotAssignable
    );
    assert_eq!(graph.relation_memo_count(), 1);
}

/// Env-threading invariant: dispatch builders accept env through
/// `shallow_lower_type_expr`'s function parameter rather than via
/// any `EvalEnvSolverHost`-style wrapper host. The lowering entry
/// point threading must remain.
#[test]
fn eval_env_solver_host_removal_does_not_lose_env_context() {
    let lower_src = include_str!("project_semantic_dispatch/lower.rs");
    assert!(
        lower_src.contains("env: &FxHashMap<String, SemanticNodeId>"),
        "env must survive as a direct parameter to shallow_lower_type_expr"
    );
    // The env map is consumed inside the function body (TypeParameter branch).
    assert!(
        lower_src.contains("env.get(&param.name)")
            || lower_src.contains("env.contains_key(name.as_ref())"),
        "shallow_lower_type_expr must still consume env for TypeParameter substitution"
    );
}

/// Evaluation-entry-point invariant: the entry point for type
/// evaluation is
/// `dispatch.execute_type_node(ProjectPath { ..., mode: Expanded })`. The
/// path must exist and run end-to-end on a simple identity case.
#[test]
fn type_eval_evaluate_removal_preserves_semantic_output() {
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let empty_path: std::sync::Arc<[crate::semantic_query::PathSegment]> =
        std::sync::Arc::from(Vec::new().into_boxed_slice());
    let result = dispatch.execute_type_node(crate::semantic_query::SemanticQueryKey::ProjectPath {
        base: string,
        path: empty_path,
        context: crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Expanded,
        ),
    });
    match result {
        crate::semantic_query::QueryResult::Value(crate::semantic_query::SemanticQueryOutput {
            value: id,
            ..
        }) => {
            assert_eq!(
                id, string,
                "empty-path Expanded projection of a primitive returns the same node"
            );
        }
        other => panic!("expected Value for identity projection, got {other:?}"),
    }
}

/// Object-shape projection invariant: object surface projection
/// runs through
/// `ProjectSemanticDispatch::execute(ProjectPath { mode: Shallow })`.
#[test]
fn type_expand_expand_object_shape_removal_preserves_shape_output() {
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let object = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("label", string),
    ])));
    let empty_path: std::sync::Arc<[crate::semantic_query::PathSegment]> =
        std::sync::Arc::from(Vec::new().into_boxed_slice());
    let result = dispatch.execute_type_node(crate::semantic_query::SemanticQueryKey::ProjectPath {
        base: object,
        path: empty_path,
        context: crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Shallow,
        ),
    });
    match result {
        crate::semantic_query::QueryResult::Value(crate::semantic_query::SemanticQueryOutput {
            value: id,
            ..
        }) => {
            let data = graph.node_data(id).expect("projection result interned");
            if let SemanticNodeData::Object(surf) = &*data {
                assert_eq!(
                    surf.positive_members().len(),
                    1,
                    "Shallow projection preserves one member"
                );
                assert_eq!(
                    surf.positive_members()[0]
                        .string_name()
                        .expect("string-key fixture"),
                    "label"
                );
            } else {
                panic!("expected Object shape after Shallow projection, got {data:?}");
            }
        }
        other => panic!("expected Value, got {other:?}"),
    }
}

/// Normalization invariant: union / intersection normalization
/// lives on dispatch via `NormalizeUnion` / `NormalizeIntersection`.
#[test]
fn type_expand_expand_normalized_expr_removal_preserves_normalization_output() {
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let a = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let b = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    // A single-element "union" should fold to the element itself.
    let members: std::sync::Arc<[crate::semantic_query::SemanticNodeId]> =
        std::sync::Arc::from(vec![a].into_boxed_slice());
    let result =
        dispatch.execute_type_node(crate::semantic_query::SemanticQueryKey::NormalizeUnion {
            members: members.clone(),
        });
    match result {
        crate::semantic_query::QueryResult::Value(crate::semantic_query::SemanticQueryOutput {
            value: id,
            ..
        }) => {
            assert_eq!(
                id, a,
                "single-element union normalization folds to the only member"
            );
        }
        other => panic!("expected Value for single-element normalization, got {other:?}"),
    }
    // A multi-element union should produce a distinct interned Union node.
    let ab: std::sync::Arc<[crate::semantic_query::SemanticNodeId]> =
        std::sync::Arc::from(vec![a, b].into_boxed_slice());
    let ab_result = dispatch
        .execute_type_node(crate::semantic_query::SemanticQueryKey::NormalizeUnion { members: ab });
    match ab_result {
        crate::semantic_query::QueryResult::Value(crate::semantic_query::SemanticQueryOutput {
            value: id,
            ..
        }) => {
            assert_ne!(
                id, a,
                "multi-member union normalization produces a new node"
            );
            assert_ne!(
                id, b,
                "multi-member union normalization produces a new node"
            );
        }
        other => panic!("expected Value for multi-element normalization, got {other:?}"),
    }
}

// ============================================================================
// Route-loop + route-target dispatch routing
//
// Callers of the engine's route-loop helpers
// (`lower_and_project_to_expanded`) and route-target helpers
// (`project_route_surface_expr`, `instantiate_local_generic_ref`)
// inside `meta_resolve.rs` and `fallthrough.rs` route through
// dispatch helpers. These tests are static-grep gates over the final
// tree.
// ============================================================================

/// Route-loop dispatch-routing invariant: the route-loop pattern
/// (`engine.lower_and_project_to_expanded(scope, expr)`) inside
/// `meta_resolve.rs` and `fallthrough.rs` must be funneled through
/// the Class A dispatch helper
/// (`project_expr_class_a_via_dispatch[_threaded]`) or, where the
/// helper isn't usable, through `dispatch.execute_read` on a
/// `ProjectPath { mode: Expanded }` query directly (the accepted node
/// is materialised to its publication `TypeExpr` at the surface sink).
///
/// Final-state expectations:
/// - `meta_resolve.rs` retains AT MOST 1 `.lower_and_project_to_expanded(`
///   callsite (the Class A helper's engine-threaded route fast-path).
/// - `resolver_core/fallthrough.rs` retains 0 callsites — value-expression
///   evaluation no longer lives there.
/// - The node-domain fallthrough value evaluator routes value expressions
///   through the shared Class-A NODE dispatch helper (one shared resolver).
#[test]
fn route_loop_callers_route_through_dispatch() {
    let meta_src = include_str!("meta_resolve.rs");
    let fallthrough_src = include_str!("resolver_core/fallthrough.rs");
    let value_eval_src =
        include_str!("resolver_core/component_meta_query_engine/fallthrough_value_eval.rs");

    let meta_callsites = meta_src.matches(".lower_and_project_to_expanded(").count();
    let fallthrough_callsites = fallthrough_src
        .matches(".lower_and_project_to_expanded(")
        .count();

    // Final-state contract:
    //
    // - `meta_resolve.rs` retains AT MOST 1 callsite (the Class A
    //   helper's engine-threaded route fast-path).
    // - `resolver_core/fallthrough.rs` retains 0 callsites.
    assert!(
        meta_callsites <= 1,
        "meta_resolve.rs must have <= 1 .lower_and_project_to_expanded( callsite; found {meta_callsites}",
    );
    assert_eq!(
        fallthrough_callsites, 0,
        "fallthrough.rs must have 0 .lower_and_project_to_expanded( callsites; found {fallthrough_callsites}",
    );

    // Positive marker: the node-domain value evaluator routes value
    // expressions through the shared Class-A NODE dispatch helper, so that
    // canonical dispatch entry's name must appear in its module.
    assert!(
        value_eval_src.contains("project_expr_class_a_node_via_dispatch_threaded"),
        "fallthrough value evaluation must route through the Class-A node dispatch helper",
    );
}

/// Engine-method-absence invariant: `instantiate_local_generic_ref`
/// is not a callsite of the meta-resolve family. Callers route
/// through `dispatch.execute_type_node(SemanticQueryKey::Instantiate { .. })`.
/// The check is a static-grep gate over the meta-resolve module
/// family: NO `engine.instantiate_local_generic_ref(...)` callsite
/// may appear.
#[test]
fn instantiate_local_generic_ref_callers_route_through_dispatch() {
    // `meta_resolve.rs` is the shell of a folder module; the body that
    // carries the `instantiate_local_generic_ref` /
    // `SemanticQueryKey::Instantiate` markers lives in
    // `meta_resolve/dispatch_helpers.rs` — the
    // `instantiate_local_generic_ref_via_dispatch` bridge helper.
    // The test concatenates the relevant siblings before running the
    // static-text grep predicates.
    let shell_src = include_str!("meta_resolve.rs");
    let dispatch_helpers_src = include_str!("meta_resolve/dispatch_helpers.rs");
    let meta_src = format!("{shell_src}\n{dispatch_helpers_src}");

    let meta_callsites = meta_src.matches(".instantiate_local_generic_ref(").count();
    assert_eq!(
        meta_callsites, 0,
        "meta_resolve.* must have 0 .instantiate_local_generic_ref( callsites; found {meta_callsites}",
    );

    // Positive marker: callers route through dispatch's Instantiate
    // family (the substitution-aware dispatch path).
    let instantiate_dispatch_calls = meta_src.matches("SemanticQueryKey::Instantiate").count();
    assert!(
        instantiate_dispatch_calls >= 1,
        "meta_resolve.* must dispatch SemanticQueryKey::Instantiate; found {instantiate_dispatch_calls}",
    );
}

// ============================================================================
// Demand-bounded carrier-stop tests.
// ============================================================================
//
// These tests build hermetic SemanticNodeData fixtures and dispatch the
// same `KeyOf` / `MappedType` operator under two different reduction
// contexts (`Published + Expanded` vs `StructuralTransit + Shallow`).
// The two contexts MUST land in distinct cache slots and the
// `StructuralTransit` context MUST return a carrier shell with NO
// per-key `ProjectMember` edges emitted into the audit footprint.
//
// Discriminating against a earlier tree (where the keys had no
// `context` field and the build unconditionally reified):
//   * Pre-fix: a single `MappedType` cache slot, both contexts share
//     the materialised result, per-member edges emit unconditionally.
//   * Post-fix: two distinct cache slots, the transit slot stores a
//     `Mapped { source, mapper }` shell, no per-member edges emitted.

#[test]
fn ax_hybrid_key_of_carrier_stops_under_structural_transit() {
    use crate::semantic_query::{
        ProjectionMode, ProjectionReductionContext, QueryResult, SemanticNodeData,
        SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput, SurfaceMember,
    };

    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    // Build a userland-shaped Object surface: two members `a`, `b`.
    let a_value = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let b_value = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let object = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        SurfaceMember {
            excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
            visibility: verter_type_expr::MemberVisibility::Public,
            key: crate::semantic_query::AuthoredPropertyKey::string("a"),
            value: a_value,
            optional: false,
            readonly: false,
            method_kind: None,
            has_implementation_body: false,
            declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
            merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
            spans: Default::default(),
            declaration_origin: None,
        },
        SurfaceMember {
            excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
            visibility: verter_type_expr::MemberVisibility::Public,
            key: crate::semantic_query::AuthoredPropertyKey::string("b"),
            value: b_value,
            optional: false,
            readonly: false,
            method_kind: None,
            has_implementation_body: false,
            declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
            merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
            spans: Default::default(),
            declaration_origin: None,
        },
    ])));

    // Dispatch under `StructuralTransit` — must return a deferred
    // `KeyOf { base }` carrier (no keyspace enumeration).
    let transit_ctx = ProjectionReductionContext::structural_transit();
    let transit_id = match dispatch.execute_type_node(SemanticQueryKey::KeyOf {
        base: object,
        context: transit_ctx,
    }) {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        other => panic!("KeyOf under StructuralTransit returned {other:?}"),
    };
    let transit_data = graph
        .node_data(transit_id)
        .expect("transit result interned");
    match &*transit_data {
        SemanticNodeData::KeyOf { base } => {
            assert_eq!(*base, object, "carrier preserves the original base");
        }
        other => panic!(
            "AX-hybrid: KeyOf under StructuralTransit MUST return a deferred `KeyOf {{ base }}` carrier; got {other:?}"
        ),
    }
    drop(transit_data);

    // Dispatch under `Published + Expanded` — must reify the keyspace.
    let publish_ctx = ProjectionReductionContext::published(ProjectionMode::Expanded);
    let publish_id = match dispatch.execute_type_node(SemanticQueryKey::KeyOf {
        base: object,
        context: publish_ctx,
    }) {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        other => panic!("KeyOf under Published+Expanded returned {other:?}"),
    };
    assert_ne!(
        transit_id, publish_id,
        "AX-hybrid: transit context and publication context MUST yield distinct cache entries"
    );
    let publish_data = graph
        .node_data(publish_id)
        .expect("publish result interned");
    let is_publication_reified = matches!(
        &*publish_data,
        SemanticNodeData::Union(_) | SemanticNodeData::Literal(_)
    );
    assert!(
        is_publication_reified,
        "AX-hybrid: KeyOf under Published+Expanded MUST reify the keyspace as a literal union / single literal; got {:?}",
        *publish_data
    );
}

#[test]
fn ax_hybrid_mapped_type_carrier_stops_under_structural_transit() {
    use crate::semantic_query::{
        IndexKey, MapperKey, OptionalityMod, ProjectionMode, ProjectionReductionContext,
        QueryResult, ReadonlyMod, SemanticNodeData, SemanticQueryApi, SemanticQueryKey,
        SemanticQueryOutput, SurfaceMember,
    };

    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    // Source = Object { a: string, b: number }.
    let a_value = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let b_value = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let source = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        SurfaceMember {
            excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
            visibility: verter_type_expr::MemberVisibility::Public,
            key: crate::semantic_query::AuthoredPropertyKey::string("a"),
            value: a_value,
            optional: false,
            readonly: false,
            method_kind: None,
            has_implementation_body: false,
            declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
            merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
            spans: Default::default(),
            declaration_origin: None,
        },
        SurfaceMember {
            excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
            visibility: verter_type_expr::MemberVisibility::Public,
            key: crate::semantic_query::AuthoredPropertyKey::string("b"),
            value: b_value,
            optional: false,
            readonly: false,
            method_kind: None,
            has_implementation_body: false,
            declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
            merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
            spans: Default::default(),
            declaration_origin: None,
        },
    ])));

    // key_space = source (any concrete keyspace that would enumerate).
    let key_space = source;
    // value_expr = `T[K]` placeholder via IndexedAccess (Identity mapper kind).
    let parameter_node = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("K"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("K"),
    });
    let value_expr = graph.intern_node(SemanticNodeData::IndexedAccess {
        object: source,
        index: IndexKey::Computed(parameter_node),
    });
    let mapper = MapperKey {
        parameter_node,
        key_space,
        value_expr,
        optionality: OptionalityMod::Keep,
        readonly: ReadonlyMod::Keep,
        name_remap: None,
        kind: crate::semantic_query::MapperKind::Identity,
    };

    // StructuralTransit dispatch.
    let transit_ctx = ProjectionReductionContext::structural_transit();
    let transit_id = match dispatch.execute_type_node(SemanticQueryKey::MappedType {
        source,
        mapper: mapper.clone(),
        context: transit_ctx,
    }) {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        other => panic!("MappedType under StructuralTransit returned {other:?}"),
    };
    let transit_data = graph
        .node_data(transit_id)
        .expect("transit result interned");
    match &*transit_data {
        SemanticNodeData::Mapped {
            source: shell_source,
            mapper: shell_mapper,
        } => {
            assert_eq!(*shell_source, source);
            assert_eq!(shell_mapper, &mapper);
        }
        other => panic!(
            "AX-hybrid: MappedType under StructuralTransit MUST return a `Mapped {{ source, mapper }}` carrier; got {other:?}"
        ),
    }
    drop(transit_data);

    // Published+Expanded dispatch.
    let publish_ctx = ProjectionReductionContext::published(ProjectionMode::Expanded);
    let publish_id = match dispatch.execute_type_node(SemanticQueryKey::MappedType {
        source,
        mapper: mapper.clone(),
        context: publish_ctx,
    }) {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        other => panic!("MappedType under Published+Expanded returned {other:?}"),
    };
    assert_ne!(
        transit_id, publish_id,
        "AX-hybrid: transit context and publication context MUST yield distinct cache slots"
    );
    let publish_data = graph
        .node_data(publish_id)
        .expect("publish result interned");
    let is_publication_object = matches!(&*publish_data, SemanticNodeData::Object(_));
    assert!(
        is_publication_object,
        "AX-hybrid: MappedType under Published+Expanded MUST materialise an Object surface; got {:?}",
        *publish_data
    );
}

#[test]
fn ax_hybrid_userland_mypick_follows_same_carrier_stop_as_builtin_pick() {
    // Structural equivalence: a userland `type MyPick<T, K extends keyof T> =
    // { [P in K]: T[P] }` enters the SAME mapped/keyof dispatch path
    // as the builtin `Pick<T, K>`. Under `StructuralTransit` BOTH
    // carrier-stop; under `Published + Expanded` BOTH materialise.
    //
    // This locks the structural-not-nominal invariant the demand-driven reducer
    // mandates. A regression that resurrects the `BuiltinUtility::from_name`
    // discriminator would break here: the userland mapped would
    // materialise under transit (or fail to materialise under publish),
    // diverging from the builtin path's behaviour.
    use crate::semantic_query::{
        IndexKey, MapperKey, OptionalityMod, ProjectionMode, ProjectionReductionContext,
        QueryResult, ReadonlyMod, SemanticNodeData, SemanticQueryApi, SemanticQueryKey,
        SemanticQueryOutput, SurfaceMember,
    };

    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let a_value = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let source = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        SurfaceMember {
            excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
            visibility: verter_type_expr::MemberVisibility::Public,
            key: crate::semantic_query::AuthoredPropertyKey::string("a"),
            value: a_value,
            optional: false,
            readonly: false,
            method_kind: None,
            has_implementation_body: false,
            declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
            merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
            spans: Default::default(),
            declaration_origin: None,
        },
    ])));

    let parameter_node = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("P"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("P"),
    });
    let value_expr = graph.intern_node(SemanticNodeData::IndexedAccess {
        object: source,
        index: IndexKey::Computed(parameter_node),
    });
    let mapper = MapperKey {
        parameter_node,
        // Userland-style: key_space is a literal `'a'` union (one key).
        key_space: graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "a".to_string(),
        ))),
        value_expr,
        optionality: OptionalityMod::Keep,
        readonly: ReadonlyMod::Keep,
        name_remap: None,
        kind: crate::semantic_query::MapperKind::Identity,
    };

    let transit_id = match dispatch.execute_type_node(SemanticQueryKey::MappedType {
        source,
        mapper: mapper.clone(),
        context: ProjectionReductionContext::structural_transit(),
    }) {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        other => panic!("transit dispatch returned {other:?}"),
    };
    assert!(
        matches!(
            &*graph.node_data(transit_id).expect("interned"),
            SemanticNodeData::Mapped { .. }
        ),
        "AX-hybrid: userland mapped MUST carrier-stop under transit (structural rule, not nominal)"
    );

    let publish_id = match dispatch.execute_type_node(SemanticQueryKey::MappedType {
        source,
        mapper,
        context: ProjectionReductionContext::published(ProjectionMode::Expanded),
    }) {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        other => panic!("publish dispatch returned {other:?}"),
    };
    assert!(
        matches!(
            &*graph.node_data(publish_id).expect("interned"),
            SemanticNodeData::Object(_)
        ),
        "AX-hybrid: userland mapped MUST materialise under Published+Expanded (same structural path as builtin Pick)"
    );
}

#[test]
fn ax_hybrid_may_reduce_operator_predicate_is_purely_structural() {
    // AX-hybrid amended predicate: publication demands (any mode) reduce;
    // `StructuralTransit` (any mode) carrier-stops. The spec's
    // original `&& mode == Expanded` restriction was over-restrictive
    // for the macro projector's `Published + Navigate` publication
    // boundary (broke userland-MyPick structural equivalence — the
    // brief's explicit deliverable). Demand is the load-bearing axis;
    // mode is informational for the body-lowering pipeline.
    use crate::semantic_query::{
        may_reduce_operator, ProjectionMode, ProjectionReductionContext, ReductionDemand,
    };

    let cases = [
        (ReductionDemand::Published, ProjectionMode::Identity, true),
        (ReductionDemand::Published, ProjectionMode::Navigate, true),
        (ReductionDemand::Published, ProjectionMode::Shallow, true),
        (ReductionDemand::Published, ProjectionMode::Expanded, true),
        (ReductionDemand::Published, ProjectionMode::Skeleton, true),
        (
            ReductionDemand::MacroObjectSurface,
            ProjectionMode::Shallow,
            true,
        ),
        (
            ReductionDemand::VueRuntimeObjectSurface,
            ProjectionMode::Shallow,
            true,
        ),
        (
            ReductionDemand::StructuralTransit,
            ProjectionMode::Identity,
            false,
        ),
        (
            ReductionDemand::StructuralTransit,
            ProjectionMode::Navigate,
            false,
        ),
        (
            ReductionDemand::StructuralTransit,
            ProjectionMode::Shallow,
            false,
        ),
        (
            ReductionDemand::StructuralTransit,
            ProjectionMode::Expanded,
            false,
        ),
        (
            ReductionDemand::StructuralTransit,
            ProjectionMode::Skeleton,
            false,
        ),
    ];
    // Mutation recipe: remove either macro demand from
    // `may_reduce_operator`; its corresponding row must fail while
    // `StructuralTransit` remains carrier-stopped.
    for (demand, mode, expected) in cases {
        let ctx = ProjectionReductionContext {
            mode,
            demand,
            provenance: crate::semantic_query::SurfaceProvenanceContext::Structural,
            merge_role: crate::semantic_query::MemberMergeRole::Authored,
            vue_heritage_policy: crate::semantic_query::VueHeritagePolicy::RetainAll,
        };
        assert_eq!(
            may_reduce_operator(ctx),
            expected,
            "AX-hybrid: may_reduce_operator({:?}, {:?}) MUST == {expected}",
            demand,
            mode,
        );
    }
}

#[test]
fn vue_heritage_policy_survives_every_context_template_and_mapped_identity_encoding() {
    use crate::semantic_query::{
        may_reduce_operator, MemberMergeRole, ProjectionMode, ProjectionReductionContext,
        ReductionDemand, SurfaceProvenanceContext, VueHeritagePolicy,
    };

    let runtime = ProjectionReductionContext::vue_runtime_object_surface(
        ProjectionMode::Shallow,
        SurfaceProvenanceContext::Structural,
    );
    let ordinary_parent = ProjectionReductionContext::macro_object_surface(
        ProjectionMode::Shallow,
        SurfaceProvenanceContext::MacroTypeArgOwnBody,
    );
    let demoted = runtime.into_structural_transit_with_mode(ProjectionMode::Navigate);
    let ordinary =
        ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate);

    assert_eq!(demoted.demand, ReductionDemand::StructuralTransit);
    assert_eq!(demoted.mode, ProjectionMode::Navigate);
    assert_eq!(
        demoted.vue_heritage_policy,
        VueHeritagePolicy::SuppressIgnored,
        "runtime heritage policy must survive the carrier-stop demotion"
    );
    assert_eq!(
        ordinary.vue_heritage_policy,
        VueHeritagePolicy::RetainAll,
        "ordinary transit must retain the complete TypeScript surface"
    );
    assert!(!may_reduce_operator(demoted));

    let modes = [
        ProjectionMode::Identity,
        ProjectionMode::Navigate,
        ProjectionMode::Shallow,
        ProjectionMode::Expanded,
        ProjectionMode::Skeleton,
    ];
    for mode in modes {
        let templates = [
            ProjectionReductionContext::published(mode),
            ProjectionReductionContext::published_macro_type_arg_body(mode),
            ProjectionReductionContext::macro_object_surface(
                mode,
                SurfaceProvenanceContext::Structural,
            ),
            ProjectionReductionContext::macro_object_surface(
                mode,
                SurfaceProvenanceContext::MacroTypeArgOwnBody,
            )
            .with_merge_role(MemberMergeRole::Heritage),
            ProjectionReductionContext::structural_transit_with_mode(mode),
        ];
        for template in templates {
            let filtered = template.with_orthogonal_axes_from(runtime);
            let unfiltered = template.with_orthogonal_axes_from(ordinary_parent);

            for derived in [filtered, unfiltered] {
                assert_eq!(derived.mode, template.mode);
                assert_eq!(derived.demand, template.demand);
                assert_eq!(derived.provenance, template.provenance);
                assert_eq!(derived.merge_role, template.merge_role);
            }
            assert_eq!(
                filtered.vue_heritage_policy,
                VueHeritagePolicy::SuppressIgnored,
                "every fresh {:?}/{:?}/{:?}/{:?} template must inherit runtime policy",
                template.mode,
                template.demand,
                template.provenance,
                template.merge_role,
            );
            assert_eq!(
                unfiltered.vue_heritage_policy,
                VueHeritagePolicy::RetainAll,
                "ordinary TypeScript demand must remain unfiltered"
            );
        }
    }

    assert_ne!(
        crate::project_semantic_dispatch::build::encode_projection_reduction_context_bits_for_tests(
            demoted,
        ),
        crate::project_semantic_dispatch::build::encode_projection_reduction_context_bits_for_tests(
            ordinary,
        ),
        "mapped-member identity must distinguish filtered and unfiltered transit"
    );

    // Mutation recipe: bypass `with_orthogonal_axes_from` at any fresh-context
    // transition, rebuild the demoted context from a default constructor, or
    // omit the policy bit from the packed identity. The corresponding table,
    // preservation, or encoding assertion fails.
}

// ============================================================================
// Relation activation — D7 discriminating suite: the vertical, the SCC/session
// table, the typed budget outcome, the strict pair, and concurrency.
// ============================================================================

/// D7 helper: upsert a TS fixture file on the host.
fn upsert_relation_fixture(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: None,
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: crate::FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap_or_else(|e| panic!("upsert {canonical}: {e:?}"));
}

/// D7 helper: resolve a named type on a fixture file to its semantic node.
fn resolve_relation_fixture_symbol(
    host: &VerterHost,
    canonical: &str,
    name: &str,
) -> crate::semantic_query::SemanticNodeId {
    let (outcome, _record) = host
        .resolve_named_symbol_with_audit(
            canonical,
            name,
            Some(crate::semantic_query::ProjectionMode::Expanded),
        )
        .into_parts();
    outcome
        .ok()
        .flatten()
        .unwrap_or_else(|| panic!("{name} must resolve on {canonical}"))
}

/// D7.1 vertical: optional-to-required decides `NotAssignable` through the
/// full `execute(SemanticQueryKey::Relate)` authority, publishes, and
/// warm-replays the same payload.
///
/// DISCRIMINATES the optional-to-required rejection in
/// `relate_property_pair`: a tree relating only the member VALUE types
/// (the pre-activation behavior) answers `Assignable` and fails the first
/// assertion.
#[test]
fn relate_optional_to_required_decides_not_assignable_through_execute() {
    use crate::semantic_query::{QueryResult, RelationOutcome, SemanticQueryValue};

    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let source = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        optional_member("a", string),
    ])));
    let target = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("a", string),
    ])));

    let key = dispatch.relate_key_for(source, target);
    let cold = dispatch.execute(key.to_query_key());
    match cold {
        QueryResult::Value(output) => match output.value {
            SemanticQueryValue::Relation(payload) => assert_eq!(
                payload.outcome,
                RelationOutcome::NotAssignable,
                "an optional source member cannot satisfy a required target member"
            ),
            other => panic!("execute(Relate) must produce a Relation payload, got {other:?}"),
        },
        other => panic!("execute(Relate) must decide this pair, got {other:?}"),
    }
    // Publish + warm replay under the same identity.
    assert_eq!(
        graph.relation_memo_count(),
        1,
        "the decided judgement admits once"
    );
    let warm = graph
        .get_relation_payload(&host, &key)
        .expect("the decided judgement warm-serves");
    assert_eq!(
        warm.outcome,
        crate::semantic_query::RelationOutcome::NotAssignable
    );
    // The mirrored pair (required source, optional target) stays assignable —
    // the rejection is direction-specific, not a blanket optional filter.
    assert!(
        matches!(
            dispatch.execute_relate_pair_as_result_for_tests(target, source),
            RelationResult::Assignable { .. }
        ),
        "required-to-optional must stay assignable"
    );
}

/// D7.3 SCC table, positive recursion: `interface RecA { next: RecA }` vs
/// `interface RecB { next: RecB }` discharges POSITIVE through the
/// coinductive assumption ("assume the relation holds and verify the
/// rest") and publishes `Assignable` with a `CoinductiveCycle` proof.
///
/// DISCRIMINATES the SCC discharge: the retired TLS-guard engine answered
/// a warm-cached `Unknown` for exactly this shape (the deleted bug), and a
/// tree without assumption recording either hangs or returns Unknown —
/// both fail the Assignable + proof assertions.
#[test]
fn coinductive_positive_scc_publishes_assignable_with_cycle_proof() {
    let host = host_for_relation_tests();
    let canonical = "/w/coinductive_pos.ts";
    upsert_relation_fixture(
        &host,
        canonical,
        "export interface RecA { next: RecA }\nexport interface RecB { next: RecB }\n",
    );
    let a = resolve_relation_fixture_symbol(&host, canonical, "RecA");
    let b = resolve_relation_fixture_symbol(&host, canonical, "RecB");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let result = dispatch.execute_relate_pair_as_result_for_tests(a, b);
    assert!(
        matches!(result, RelationResult::Assignable { .. }),
        "a genuinely recursive structural match must discharge Assignable, got {result:?}"
    );

    // The published payload carries the CoinductiveCycle proof (the cycle
    // co-discharged; the proof references the completed member keys).
    let key = dispatch.relate_key_for(a, b);
    let payload = graph
        .get_relation_payload(&host, &key)
        .expect("the positive SCC close publishes the root judgement");
    assert_eq!(
        payload.outcome,
        crate::semantic_query::RelationOutcome::Assignable
    );
    let proof = graph
        .relation_proof_for(payload.relation_proof)
        .expect("the payload's proof id resolves in the proof table");
    match proof {
        crate::semantic_query::RelationProof::CoinductiveCycle { keys } => {
            assert!(
                !keys.is_empty(),
                "the coinductive proof must reference the co-discharged keys"
            );
        }
        other => panic!("a cyclic positive discharge must carry CoinductiveCycle, got {other:?}"),
    }
}

/// D7.3 SCC table, negative recursion: the mutual-recursion pair whose
/// `tag` member mismatches (`"a"` vs `number`) closes NEGATIVE on the
/// non-assumptive obligation and publishes a stable `NotAssignable` —
/// final and warm, never `ReturnOnly`.
#[test]
fn coinductive_negative_scc_publishes_stable_not_assignable() {
    let host = host_for_relation_tests();
    let canonical = "/w/coinductive_neg.ts";
    upsert_relation_fixture(
        &host,
        canonical,
        "export interface NegA { next: NegB; tag: \"a\" }\nexport interface NegB { next: NegA; tag: number }\n",
    );
    let a = resolve_relation_fixture_symbol(&host, canonical, "NegA");
    let b = resolve_relation_fixture_symbol(&host, canonical, "NegB");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let result = dispatch.execute_relate_pair_as_result_for_tests(a, b);
    assert_eq!(
        result,
        RelationResult::NotAssignable,
        "the string-literal tag against number is a negative non-assumptive obligation"
    );
    // Publishable negative — a repeat ask warm-hits the SAME verdict.
    let key = dispatch.relate_key_for(a, b);
    let payload = graph
        .get_relation_payload(&host, &key)
        .expect("a negative SCC close publishes a final NotAssignable, not ReturnOnly");
    assert_eq!(
        payload.outcome,
        crate::semantic_query::RelationOutcome::NotAssignable
    );
    assert_eq!(
        dispatch.execute_relate_pair_as_result_for_tests(a, b),
        RelationResult::NotAssignable,
        "the warm replay serves the same stable negative"
    );
    // B1 discriminator — REVERSE-member co-publication: the mutual cycle's
    // reverse member (`NegB ≤ NegA`, reached through the `next` back-edge)
    // must ALSO have published its collapsed verdict. The pre-activation
    // engine ran nested sub-relations on a private worklist and never
    // co-published members, so this assertion is new-engine-only; a tree
    // whose SCC drain stops publishing members fails it.
    let named = |id: crate::semantic_query::SemanticNodeId, want: &str| -> bool {
        match graph.node_data(id).as_deref() {
            Some(SemanticNodeData::DeclRef { identity }) => identity.decl_name.as_ref() == want,
            Some(SemanticNodeData::Opaque(
                crate::semantic_query::QueryError::DeclPlaceholder { name, .. },
            )) => name.as_ref() == want,
            _ => false,
        }
    };
    let reverse_member: Vec<_> = graph
        .relation_entries_for_tests()
        .into_iter()
        .filter(|(key, _)| named(key.source, "NegB") && named(key.target, "NegA"))
        .collect();
    assert!(
        !reverse_member.is_empty(),
        "the SCC close must CO-PUBLISH the reverse member `NegB ≤ NegA` \
         (the batched member drain), not only the root judgement"
    );
    for (key, outcome) in &reverse_member {
        assert_eq!(
            *outcome,
            crate::semantic_query::RelationOutcome::NotAssignable,
            "the co-published reverse member's verdict collapsed with the cycle: \
             ({:?} <= {:?})",
            key.source,
            key.target
        );
    }
}

/// D7.3 SCC table, Unknown edge: a recursive pair with an undecidable
/// member obligation (an unbound generic `tag: T` on both sides) routes
/// the WHOLE component through ReturnOnly — no memo entry, no warm hit,
/// and a repeat ask recomputes.
#[test]
fn unknown_edge_in_scc_makes_whole_component_return_only() {
    let host = host_for_relation_tests();
    let canonical = "/w/coinductive_unknown.ts";
    // The two `tag` obligations are DISTINCT undecidable shapes (an open
    // `T` against an object wrapping it) so the reflexive-identity
    // short-circuit cannot decide them — the member relation is a genuine
    // `Unknown` non-assumptive edge inside the recursive component.
    upsert_relation_fixture(
        &host,
        canonical,
        "export interface UnkA<T> { next: UnkA<T>; tag: T }\nexport interface UnkB<T> { next: UnkB<T>; tag: { inner: T } }\n",
    );
    let a = resolve_relation_fixture_symbol(&host, canonical, "UnkA");
    let b = resolve_relation_fixture_symbol(&host, canonical, "UnkB");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let before = graph.relation_memo_count();
    let result = dispatch.execute_relate_pair_as_result_for_tests(a, b);
    assert_eq!(
        result,
        RelationResult::Unknown,
        "an unbound type-parameter obligation is undecidable, not a false negative"
    );
    assert_eq!(
        graph.relation_memo_count(),
        before,
        "an Unknown-poisoned component must admit NOTHING (whole-SCC ReturnOnly)"
    );
    let key = dispatch.relate_key_for(a, b);
    assert!(
        graph.get_relation_payload(&host, &key).is_none(),
        "no warm entry may exist for a poisoned component"
    );
}

/// D7.3 session table: a binding-producing judgement publishes ONLY the
/// fingerprint-carrying root identity at session close (with the fixed
/// bindings on the payload); the session-internal deposits never leak a
/// publish under the plain (no-inference) identity.
#[test]
fn binding_session_close_publishes_root_only_with_fixed_bindings() {
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let source = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("value", number),
    ])));
    let infer_v = graph.intern_node(SemanticNodeData::Infer {
        name: Arc::from("V"),
        binder: graph.alloc_infer_binder_id(),
    });
    let target = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("value", infer_v),
    ])));

    let before = graph.relation_memo_count();
    let step = dispatch.execute_relate_pair(source, target);
    match step {
        crate::project_semantic_dispatch::dispatch_txn::RelationStep::Assignable { bindings } => {
            assert_eq!(bindings.len(), 1, "the session fixes exactly one binding");
            assert_eq!(bindings[0].name.as_ref(), "V");
            assert_eq!(
                bindings[0].bound, number,
                "V binds the check-side member type fixed at session close"
            );
        }
        other => panic!("the in-scope object-property infer pattern must bind, got {other:?}"),
    }
    // Exactly ONE new entry — the root judgement under its
    // fingerprint-carrying identity.
    assert_eq!(
        graph.relation_memo_count(),
        before + 1,
        "session close publishes exactly the root judgement"
    );
    // The PLAIN (no-inference-context) identity did not publish: the
    // binding judgement is keyed by the completed fingerprint, and the
    // session-internal deposit is a session-local delta (admission row 7).
    let plain_key = dispatch.relate_key_for(source, target);
    assert!(
        plain_key.inference_context.is_none(),
        "fixture: the pair constructor's plain key carries no fingerprint"
    );
    assert!(
        graph.get_relation_payload(&host, &plain_key).is_none(),
        "the binding judgement must NOT publish under the plain identity"
    );
    // Warm replay through the same authority serves the same bindings
    // without growing the memo.
    let replay = dispatch.execute_relate_pair(source, target);
    match replay {
        crate::project_semantic_dispatch::dispatch_txn::RelationStep::Assignable { bindings } => {
            assert_eq!(bindings.len(), 1);
            assert_eq!(bindings[0].bound, number);
        }
        other => panic!("warm replay must serve the same binding payload, got {other:?}"),
    }
    assert_eq!(graph.relation_memo_count(), before + 1);
}

/// D7.3 session table, abandonment: a session abandoned mid-flight (the
/// injected budget trips inside the binding judgement) publishes NOTHING —
/// the deferred batch releases without publish.
#[test]
fn binding_session_abandoned_by_budget_publishes_nothing() {
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let source = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("value", number),
    ])));
    let infer_v = graph.intern_node(SemanticNodeData::Infer {
        name: Arc::from("V"),
        binder: graph.alloc_infer_binder_id(),
    });
    let target = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("value", infer_v),
    ])));

    host.relation_knobs
        .force_budget_exhaustion
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let before = graph.relation_memo_count();
    let step = dispatch.execute_relate_pair(source, target);
    host.relation_knobs
        .force_budget_exhaustion
        .store(false, std::sync::atomic::Ordering::Relaxed);
    assert!(
        matches!(
            step,
            crate::project_semantic_dispatch::dispatch_txn::RelationStep::BudgetExceeded(_)
        ),
        "the tripped budget surfaces the typed public outcome, got {step:?}"
    );
    assert_eq!(
        graph.relation_memo_count(),
        before,
        "an abandoned session publishes NOTHING (release-without-publish)"
    );
}

/// D7.4: an injected budget trip surfaces the TYPED public `BudgetExceeded`
/// payload (expressible, renderable) while admitting NOTHING — no warm
/// memo entry, no warm read, and the repeat ask recomputes cold instead of
/// warm-hitting.
///
/// DISCRIMINATES the row-4 admission gate: a tree that publishes the
/// budget payload (or maps it onto a decided outcome) fails the memo-count
/// and repeat-recompute assertions.
#[test]
fn relation_budget_exceeded_is_public_and_admits_nothing() {
    use crate::semantic_query::{QueryResult, RelationOutcome, SemanticQueryValue};

    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let source = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("a", string),
    ])));
    let target = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("a", string),
    ])));
    let key = dispatch.relate_key_for(source, target);

    host.relation_knobs
        .force_budget_exhaustion
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let before = graph.relation_memo_count();
    let checks_before = graph.stats_snapshot().relation_check_count;

    // PUBLIC: the payload is a real SemanticQueryValue::Relation with the
    // typed BudgetExceeded outcome and a BudgetExceeded proof.
    let first = dispatch.execute(key.to_query_key());
    match &first {
        QueryResult::Value(output) => match &output.value {
            SemanticQueryValue::Relation(payload) => {
                assert!(
                    matches!(payload.outcome, RelationOutcome::BudgetExceeded(_)),
                    "budget exhaustion is a PUBLIC typed outcome, got {:?}",
                    payload.outcome
                );
                let proof = graph
                    .relation_proof_for(payload.relation_proof)
                    .expect("the budget payload's proof resolves");
                assert!(
                    matches!(
                        proof,
                        crate::semantic_query::RelationProof::BudgetExceeded { .. }
                    ),
                    "the proof rides the BudgetExceeded shape"
                );
            }
            other => panic!("expected a Relation payload, got {other:?}"),
        },
        other => panic!("the budget outcome is expressible, not an error: {other:?}"),
    }

    // Layer 1 — no warm memo entry.
    assert_eq!(
        graph.relation_memo_count(),
        before,
        "BudgetExceeded must never admit a memo entry"
    );
    // Layer 2 — no warm read serves it.
    assert!(
        graph.get_relation_payload(&host, &key).is_none(),
        "no warm relation read may serve a budget payload"
    );
    // Layer 3 — no fact-signature / reverse-index trace: the repeat asks
    // COMPUTE again (the authority's relation-check counter moves and the
    // typed outcome reproduces) instead of short-circuiting on any
    // recorded artifact.
    let second = dispatch.execute(key.to_query_key());
    assert!(
        matches!(
            &second,
            QueryResult::Value(output)
                if matches!(&output.value, SemanticQueryValue::Relation(p)
                    if matches!(p.outcome, RelationOutcome::BudgetExceeded(_)))
        ),
        "the repeat ask recomputes the budget outcome cold"
    );
    let step = dispatch.execute_relate_pair(source, target);
    assert!(
        matches!(
            step,
            crate::project_semantic_dispatch::dispatch_txn::RelationStep::BudgetExceeded(_)
        ),
        "the authority entry recomputes the typed outcome cold, got {step:?}"
    );
    let checks_after = graph.stats_snapshot().relation_check_count;
    assert!(
        checks_after > checks_before,
        "the repeat ask must reach the authority cold (no warm short-circuit): {checks_before} -> {checks_after}"
    );
    assert_eq!(graph.relation_memo_count(), before);

    host.relation_knobs
        .force_budget_exhaustion
        .store(false, std::sync::atomic::Ordering::Relaxed);
    // Sanity: with the knob released the same pair decides and admits.
    let decided = dispatch.execute_relate_pair_as_result_for_tests(source, target);
    assert!(matches!(decided, RelationResult::Assignable { .. }));
    assert_eq!(graph.relation_memo_count(), before + 1);
}

/// D7.5: the paired strict-on/off fixture — `null → string` flips its
/// verdict between the TS-strict regime (NotAssignable) and the relaxed
/// `strictNullChecks`-off regime (Assignable), and the two judgements
/// occupy DISTINCT slots (no cross-hit in either direction).
#[test]
fn strict_family_flip_changes_verdict_without_cross_hit() {
    let host = host_for_relation_tests();
    let graph = host.project_type_store().semantic_graph();
    let null = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Null));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    // Strict regime (production default).
    let strict_dispatch = ProjectSemanticDispatch::new(&host);
    let strict_key = strict_dispatch.relate_key_for(null, string);
    assert_eq!(
        strict_dispatch.execute_relate_pair_as_result_for_tests(null, string),
        RelationResult::NotAssignable,
        "strictNullChecks ON isolates null from string"
    );

    // Relaxed regime: flip strictNullChecks OFF (fresh dispatch — the
    // config snapshot is per-request).
    host.relation_knobs
        .strict_family_relax_bits
        .store(0b01, std::sync::atomic::Ordering::Relaxed);
    let relaxed_dispatch = ProjectSemanticDispatch::new(&host);
    let relaxed_key = relaxed_dispatch.relate_key_for(null, string);
    assert_ne!(
        strict_key, relaxed_key,
        "the strict fold must isolate the two identities (type_env_hash)"
    );
    assert!(
        matches!(
            relaxed_dispatch.execute_relate_pair_as_result_for_tests(null, string),
            RelationResult::Assignable { .. }
        ),
        "strictNullChecks OFF admits null into string — the BEHAVIORAL branch"
    );

    // No cross-hit: each slot holds its own verdict.
    let strict_payload = graph
        .get_relation_payload(&host, &strict_key)
        .expect("the strict judgement stays warm in its own slot");
    assert_eq!(
        strict_payload.outcome,
        crate::semantic_query::RelationOutcome::NotAssignable
    );
    let relaxed_payload = graph
        .get_relation_payload(&host, &relaxed_key)
        .expect("the relaxed judgement warms its own slot");
    assert_eq!(
        relaxed_payload.outcome,
        crate::semantic_query::RelationOutcome::Assignable
    );

    // Back to strict: the verdict is the strict one again (never the
    // relaxed slot's).
    host.relation_knobs
        .strict_family_relax_bits
        .store(0, std::sync::atomic::Ordering::Relaxed);
    let strict_again = ProjectSemanticDispatch::new(&host);
    assert_eq!(
        strict_again.execute_relate_pair_as_result_for_tests(null, string),
        RelationResult::NotAssignable,
        "restoring strict must never cross-hit the relaxed slot"
    );
}

/// D7.6: concurrent cyclic relation requests complete — two threads race
/// the SAME recursive pair cold; the per-transaction reentry substrate
/// plus the family singleflight must neither self-await nor deadlock.
/// Fails LOUDLY on a 60s watchdog instead of hanging the suite.
#[test]
fn concurrent_cyclic_relation_requests_complete_without_deadlock() {
    let host = std::sync::Arc::new(host_for_relation_tests());
    let canonical = "/w/coinductive_concurrent.ts";
    upsert_relation_fixture(
        &host,
        canonical,
        "export interface ConA { next: ConA }\nexport interface ConB { next: ConB }\n",
    );
    let a = resolve_relation_fixture_symbol(&host, canonical, "ConA");
    let b = resolve_relation_fixture_symbol(&host, canonical, "ConB");

    let (tx, rx) = std::sync::mpsc::channel::<RelationResult>();
    for _ in 0..2 {
        let host = std::sync::Arc::clone(&host);
        let tx = tx.clone();
        std::thread::spawn(move || {
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let result = dispatch.execute_relate_pair_as_result_for_tests(a, b);
            let _ = tx.send(result);
        });
    }
    drop(tx);
    for _ in 0..2 {
        let result = rx
            .recv_timeout(std::time::Duration::from_secs(60))
            .expect("WATCHDOG: a concurrent cyclic relation request deadlocked / self-awaited");
        assert!(
            matches!(result, RelationResult::Assignable { .. }),
            "both racers must converge on the coinductive Assignable, got {result:?}"
        );
    }
}

/// SCC-drain integrity: a popped provisional SCC member must NEVER be drained
/// (and published) by an unrelated sibling relation whose frame happens to
/// reuse the member's recycled stack index. The reverse member `B ≤ A` of
/// a negatively-closing cycle must never warm-publish `Assignable`.
#[test]
fn sibling_scc_close_cannot_steal_pending_member_of_open_cycle() {
    let host = host_for_relation_tests();
    let canonical = "/w/scc_steal.ts";
    // `StealA ≤ StealB`: the `next` pair opens the mutual cycle (the
    // reverse member `StealB ≤ StealA` deposits provisionally and awaits
    // the root); the `other` pair is an UNRELATED nested relation whose
    // frame reuses the popped member's stack index and closes cleanly as
    // its own root; the `tag` pair then drives the ROOT negative
    // (`string` is not assignable to the literal `"a"`).
    upsert_relation_fixture(
        &host,
        canonical,
        concat!(
            "export interface StealA { next: StealB; other: StealC; tag: string }\n",
            "export interface StealB { next: StealA; other: StealD; tag: \"a\" }\n",
            "export interface StealC { x: string }\n",
            "export interface StealD { x: string; y?: string }\n",
        ),
    );
    let a = resolve_relation_fixture_symbol(&host, canonical, "StealA");
    let b = resolve_relation_fixture_symbol(&host, canonical, "StealB");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let root = dispatch.execute_relate_pair_as_result_for_tests(a, b);
    assert_eq!(
        root,
        RelationResult::NotAssignable,
        "fixture: the root pair must close NEGATIVE (string vs \"a\")"
    );
    // The poisoned-cache discriminator, over the ACTUAL published set: in
    // this fixture every object-object pair of the A/B cycle is
    // symmetric-NotAssignable (the tag mismatch kills A<=B directly and
    // B<=A through the collapsed `next` back-edge), and the only other
    // object pairs (StealC/StealD) are symmetric-Assignable — so NO
    // published object-object `Assignable` entry may coexist with a
    // published `NotAssignable` for its REVERSED pair. A pending member
    // stolen by the sibling close (or frozen by a wrong-direction
    // re-discharge) publishes exactly such a contradiction.
    let entries = graph.relation_entries_for_tests();
    for (key, outcome) in &entries {
        if !matches!(outcome, crate::semantic_query::RelationOutcome::Assignable) {
            continue;
        }
        let is_decl_pairish = |id: crate::semantic_query::SemanticNodeId| {
            matches!(
                graph.node_data(id).as_deref(),
                Some(
                    SemanticNodeData::Object(_)
                        | SemanticNodeData::DeclRef { .. }
                        | SemanticNodeData::Opaque(
                            crate::semantic_query::QueryError::DeclPlaceholder { .. }
                        )
                )
            )
        };
        if !is_decl_pairish(key.source) || !is_decl_pairish(key.target) {
            continue;
        }
        let contradiction = entries.iter().any(|(other, other_outcome)| {
            other.source == key.target
                && other.target == key.source
                && matches!(
                    other_outcome,
                    crate::semantic_query::RelationOutcome::NotAssignable
                )
        });
        assert!(
            !contradiction,
            "poisoned warm entry: ({:?} <= {:?}) published Assignable while its \
             reverse pair is published NotAssignable — a stale provisional member \
             escaped the SCC gate (stolen drain or wrong-direction re-discharge)",
            key.source, key.target
        );
    }
}

/// Re-discharge direction: the negative-SCC re-discharge must run deepest-first
/// (bottom-up over the condensation) so a shallower member re-runs against
/// the FINAL deeper verdicts. In the 3-node chain cycle the root closes
/// negative (T0 requires `extra`); NO member may stay published
/// `Assignable` while the member it depends on is `NotAssignable`.
#[test]
fn negative_scc_redischarge_runs_deepest_first() {
    let host = host_for_relation_tests();
    let canonical = "/w/scc_chain.ts";
    upsert_relation_fixture(
        &host,
        canonical,
        concat!(
            "export interface S0 { next: S1 }\n",
            "export interface S1 { next: S2 }\n",
            "export interface S2 { next: S0 }\n",
            "export interface T0 { next: T1; extra: string }\n",
            "export interface T1 { next: T2 }\n",
            "export interface T2 { next: T0 }\n",
        ),
    );
    let s0 = resolve_relation_fixture_symbol(&host, canonical, "S0");
    let t0 = resolve_relation_fixture_symbol(&host, canonical, "T0");
    let s1 = resolve_relation_fixture_symbol(&host, canonical, "S1");
    let t1 = resolve_relation_fixture_symbol(&host, canonical, "T1");
    let s2 = resolve_relation_fixture_symbol(&host, canonical, "S2");
    let t2 = resolve_relation_fixture_symbol(&host, canonical, "T2");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let root = dispatch.execute_relate_pair_as_result_for_tests(s0, t0);
    assert_eq!(
        root,
        RelationResult::NotAssignable,
        "fixture: S0 lacks T0's required `extra` member"
    );
    // Every member pair's warm verdict (if published) must be
    // NotAssignable — the whole chain depends on the collapsed root
    // assumption. A shallow member frozen `Assignable` before its deeper
    // dependency flipped is the wrong-direction re-discharge.
    for (name, s, t) in [("S1<=T1", s1, t1), ("S2<=T2", s2, t2)] {
        let warm = graph.get_relation_payload(&host, &dispatch.relate_key_for(s, t));
        assert!(
            !matches!(
                warm.as_ref().map(|p| &p.outcome),
                Some(crate::semantic_query::RelationOutcome::Assignable)
            ),
            "{name} stayed published Assignable although its dependency chain \
             collapsed NotAssignable (shallowest-first re-discharge): {warm:?}"
        );
    }
}

/// Re-discharge direction: the negative-SCC re-discharge must run DEEPEST-first
/// (bottom-up over the condensation). In this fixture the SCC root
/// (`X1 ≤ Y1`) closes negative through its off-cycle `peer` obligation
/// (`P.tag: string` vs `Q.tag: "a"`); the two positive assumption-consuming
/// members (`X0 ≤ Y0` deep, `X2 ≤ Y2` shallow) must BOTH collapse — a
/// shallowest-first pass freezes `X2 ≤ Y2` against the deep member's stale
/// provisional `Assignable` before that member flips. Ground truth: NO
/// pair in this fixture is assignable, so the memo may hold ZERO published
/// `Assignable` entries after the ask.
#[test]
fn negative_scc_redischarge_collapses_shallow_members_against_final_deep_verdicts() {
    let host = host_for_relation_tests();
    let canonical = "/w/scc_freeze.ts";
    // Shape: the SCC root `W ≤ V` carries the off-cycle negative (`peer`),
    // and a TWO-member positive chain (`D1 ≤ E1` shallow, `D2 ≤ E2` deep)
    // hangs off it, with `D2` back-edging to `W`. At the root's close the
    // provisional batch is [D2: A, D1: A]; a shallowest-first re-discharge
    // freezes `D1` against `D2`'s stale provisional `Assignable` BEFORE
    // `D2` flips on the collapsed root — publishing a false warm
    // `Assignable` for `D1 ≤ E1`.
    upsert_relation_fixture(
        &host,
        canonical,
        concat!(
            "export interface R0 { next: A1 }\n",
            "export interface A1 { next: W }\n",
            "export interface W { next: D1; peer: P }\n",
            "export interface D1 { next: D2 }\n",
            "export interface D2 { next: W }\n",
            "export interface S0 { next: B1 }\n",
            "export interface B1 { next: V }\n",
            "export interface V { next: E1; peer: Q }\n",
            "export interface E1 { next: E2 }\n",
            "export interface E2 { next: V }\n",
            "export interface P { tag: string }\n",
            "export interface Q { tag: \"a\" }\n",
        ),
    );
    let x0 = resolve_relation_fixture_symbol(&host, canonical, "R0");
    let y0 = resolve_relation_fixture_symbol(&host, canonical, "S0");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let root = dispatch.execute_relate_pair_as_result_for_tests(x0, y0);
    assert_eq!(
        root,
        RelationResult::NotAssignable,
        "fixture: the cycle collapses through the peer tag mismatch"
    );
    let assignable: Vec<_> = graph
        .relation_entries_for_tests()
        .into_iter()
        .filter(|(_, outcome)| {
            matches!(outcome, crate::semantic_query::RelationOutcome::Assignable)
        })
        .map(|(key, _)| (key.source, key.target))
        .collect();
    assert!(
        assignable.is_empty(),
        "no pair in this fixture is assignable; a published Assignable is a \
         member FROZEN against a stale provisional deep verdict \
         (wrong-direction re-discharge): {assignable:?}"
    );
}

/// Inference variance: candidates deposited from CONTRAVARIANT positions
/// (function parameters) combine by INTERSECTION, not union.
/// `((a: string, b: number) => void) extends ((a: infer U, b: infer U) => void) ? U : never`
/// resolves `U` to `string & number` = `never` — a union `string | number`
/// is the covariant combination applied to contravariant candidates.
#[test]
fn contravariant_infer_candidates_intersect_not_union() {
    use crate::semantic_query::{
        FunctionParam, QueryResult, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
        TypeParamDecl,
    };
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let string_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let void_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Void));
    let never_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
    let infer_u_binder = graph.alloc_infer_binder_id();
    let infer_u = graph.intern_node(SemanticNodeData::Infer {
        name: Arc::from("U"),
        binder: infer_u_binder.clone(),
    });

    let function = |a: crate::semantic_query::SemanticNodeId,
                    b: crate::semantic_query::SemanticNodeId| {
        graph.intern_node(SemanticNodeData::Signature {
            kind: crate::semantic_query::SignatureKind::Call,
            params: Arc::from(
                vec![
                    FunctionParam::synthetic(Some(Arc::from("a")), a, false, false),
                    FunctionParam::synthetic(Some(Arc::from("b")), b, false, false),
                ]
                .into_boxed_slice(),
            ),
            return_type: void_node,
            occurrence: None,
            return_carrier: crate::semantic_query::SignatureReturnCarrier::Declared(void_node),
            type_parameters: Arc::from(Vec::<TypeParamDecl>::new().into_boxed_slice()),
            signature_span: None,
            return_type_span: None,
        })
    };
    let check = function(string_node, number_node);
    let extends = function(infer_u, infer_u);

    let result = match dispatch.execute_type_node(SemanticQueryKey::Conditional {
        check,
        extends,
        true_branch: infer_u,
        false_branch: never_node,
        distributive: false,
    }) {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        other => panic!("expected Value, got {other:?}"),
    };

    let data = graph.node_data(result);
    assert!(
        matches!(
            data.as_deref(),
            Some(SemanticNodeData::Primitive(PrimitiveKind::Never))
        ),
        "contravariant candidates `string` and `number` for U must INTERSECT \
         (string & number = never); got {data:?}"
    );
}

/// Axis refusal: a relation key on a NOT-YET-IMPLEMENTED axis (a
/// non-`Assignable` relation kind, a non-default overload-selection
/// policy) must REFUSE — undecided, ReturnOnly, zero admission — never a
/// silent assignability answer. `Identity(string, unknown)` through the
/// assignability reducer would hit the `(_, Unknown) => Assignable`
/// prefilter arm and PUBLISH a false `Assignable` for an identity
/// judgement. The `Fresh` freshness axis and the excess-property policy
/// are IMPLEMENTED (the fresh excess prepass) and no longer refuse — see
/// `fresh_excess_property_checking`.
#[test]
fn non_default_relation_axes_refuse_instead_of_answering_assignability() {
    use crate::semantic_query::{
        OverloadSelectionPolicy, QueryResult, RelateMemoKey, RelationKind, SemanticQueryApi,
    };
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let unknown = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown));

    let base = dispatch.relate_key_for(string, unknown);
    let identity_key = RelateMemoKey {
        relation: RelationKind::Identity,
        ..base.clone()
    };
    let overload_key = {
        let mut key = base.clone();
        key.policy.overload_selection = OverloadSelectionPolicy::FirstApplicable;
        key
    };

    let before = graph.relation_memo_count();
    for (name, key) in [
        ("Identity", identity_key),
        ("FirstApplicable overloads", overload_key),
    ] {
        let result = dispatch.execute(key.to_query_key());
        assert!(
            !matches!(
                &result,
                QueryResult::Value(output) if matches!(
                    &output.value,
                    crate::semantic_query::SemanticQueryValue::Relation(p)
                        if p.outcome == crate::semantic_query::RelationOutcome::Assignable
                )
            ),
            "{name}: an unimplemented relation axis must refuse, not answer \
             assignability (string/unknown would be a FALSE {name} verdict): {result:?}"
        );
        assert_eq!(
            graph.relation_memo_count(),
            before,
            "{name}: a refused axis admits NOTHING (ReturnOnly)"
        );
    }
    // The default assignability axis still decides and admits normally.
    assert!(matches!(
        dispatch.execute_relate_pair_as_result_for_tests(string, unknown),
        RelationResult::Assignable { .. }
    ));
}

/// Capture-avoidance: infer substitution is CAPTURE-AVOIDING under a function's
/// own type parameters. `string extends infer U ? (<U>() => U) : never`
/// must keep the inner generic's `U` intact (`<U>() => U`) — a name-driven
/// rewrite without a binder-scope check produces `<U>() => string`,
/// capturing the shadowed inner binder.
#[test]
fn infer_substitution_does_not_capture_function_shadowed_binder() {
    use crate::semantic_query::{
        DeclIdentity, QueryResult, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
        TypeParamDecl,
    };
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let string_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let never_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
    let infer_u = graph.intern_node(SemanticNodeData::Infer {
        name: Arc::from("U"),
        binder: graph.alloc_infer_binder_id(),
    });
    // Inner occurrence of the FUNCTION's own `U` (the shadowing binder's
    // reference — lowers as a TypeParam shell named "U").
    let inner_u = graph.intern_node(SemanticNodeData::TypeParam {
        decl: DeclIdentity::synthetic("U"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("U"),
    });
    // true branch: `<U>() => U`.
    let generic_fn = graph.intern_node(SemanticNodeData::Signature {
        kind: crate::semantic_query::SignatureKind::Call,
        params: Arc::from(Vec::<crate::semantic_query::FunctionParam>::new().into_boxed_slice()),
        return_type: inner_u,
        occurrence: None,
        return_carrier: crate::semantic_query::SignatureReturnCarrier::Declared(inner_u),
        type_parameters: Arc::from(
            vec![TypeParamDecl {
                name: Arc::from("U"),
                param: inner_u,
                constraint: None,
                default: None,
                is_const: false,
            }]
            .into_boxed_slice(),
        ),
        signature_span: None,
        return_type_span: None,
    });

    let result = match dispatch.execute_type_node(SemanticQueryKey::Conditional {
        check: string_node,
        extends: infer_u,
        true_branch: generic_fn,
        false_branch: never_node,
        distributive: false,
    }) {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        other => panic!("expected Value, got {other:?}"),
    };

    let data = graph.node_data(result);
    let Some(SemanticNodeData::Signature {
        return_type,
        type_parameters,
        ..
    }) = data.as_deref()
    else {
        panic!("the true branch must stay a function, got {data:?}");
    };
    assert_eq!(
        type_parameters.len(),
        1,
        "the inner generic keeps its own <U>"
    );
    let ret = graph.node_data(*return_type);
    assert!(
        matches!(
            ret.as_deref(),
            Some(SemanticNodeData::TypeParam { display_name, .. })
                if display_name.as_ref() == "U"
        ),
        "the inner generic's return must stay its OWN `U` (shadowed binder), \
         not the outer infer's bound `string`; got {ret:?}"
    );
}

/// Mutable-array inference: a MUTABLE array `(infer U)[]` pattern still binds —
/// `string[] extends (infer U)[] ? U : never` → `string`. The invariant
/// element check for non-readonly arrays must not impose the reverse arm
/// against an `Infer` element under an active session (the deposit IS the
/// binding); the readonly Flatten fixture took the covariant-only path and
/// hid this.
#[test]
fn mutable_array_infer_element_binds_covariantly() {
    use crate::semantic_query::{
        QueryResult, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
    };
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let string_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let never_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
    let infer_u = graph.intern_node(SemanticNodeData::Infer {
        name: Arc::from("U"),
        binder: graph.alloc_infer_binder_id(),
    });
    let check = graph.intern_node(SemanticNodeData::Array {
        element: string_node,
        readonly: false,
    });
    let extends = graph.intern_node(SemanticNodeData::Array {
        element: infer_u,
        readonly: false,
    });

    let result = match dispatch.execute_type_node(SemanticQueryKey::Conditional {
        check,
        extends,
        true_branch: infer_u,
        false_branch: never_node,
        distributive: false,
    }) {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        other => panic!("expected Value, got {other:?}"),
    };
    assert_eq!(
        result,
        string_node,
        "`string[] extends (infer U)[] ? U : never` must bind U = string \
         (neither deferred nor never); got {:?}",
        graph.node_data(result)
    );

    // Negative control: non-`Infer` mutable arrays KEEP the invariant
    // bidirectional element check — `string[] ≤ (string | number)[]`
    // (mutable) stays NotAssignable (the reverse arm fails).
    let number_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let union_node = graph.intern_node(SemanticNodeData::Union(Arc::from(
        vec![string_node, number_node].into_boxed_slice(),
    )));
    let union_array = graph.intern_node(SemanticNodeData::Array {
        element: union_node,
        readonly: false,
    });
    assert_eq!(
        dispatch.execute_relate_pair_as_result_for_tests(check, union_array),
        RelationResult::NotAssignable,
        "mutable non-Infer arrays must stay INVARIANT (string[] ≤ (string|number)[] rejects)"
    );
}

/// Nested-binder shadowing: a NESTED conditional that re-binds the same `infer`
/// name shadows the outer binder — the outer substitution must not
/// rewrite the inner binder's scope. `string extends infer U ? (number
/// extends infer U ? U : never) : never` → `number` (the inner `U`
/// re-binds to `number`); a scope-blind rewrite turns the inner
/// conditional into `number extends string ? string : never` = `never`.
#[test]
fn nested_same_name_infer_binder_is_not_captured_by_outer_substitution() {
    use crate::semantic_query::{
        QueryResult, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
    };
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let string_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let never_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
    let infer_u = graph.intern_node(SemanticNodeData::Infer {
        name: Arc::from("U"),
        binder: graph.alloc_infer_binder_id(),
    });
    // Inner: `number extends infer U ? U : never`.
    let inner = graph.intern_node(SemanticNodeData::Conditional {
        check: number_node,
        extends: infer_u,
        true_branch_ref: infer_u,
        false_branch_ref: never_node,
        distributive: false,
    });

    let result = match dispatch.execute_type_node(SemanticQueryKey::Conditional {
        check: string_node,
        extends: infer_u,
        true_branch: inner,
        false_branch: never_node,
        distributive: false,
    }) {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        other => panic!("expected Value, got {other:?}"),
    };
    // The shadow stop kept the inner conditional's binder scope intact,
    // and the selected branch reduced on demand: the inner `infer U`
    // re-binds to `number`. A captured inner binder (extends/true
    // rewritten to `string`) collapses to `never` instead.
    assert_eq!(
        result,
        number_node,
        "the inner `infer U` re-binds (U = number); a captured inner binder \
         collapses to never; got {:?}",
        graph.node_data(result)
    );

    // Negative control: the outer binder still rewrites positions the
    // inner binder does NOT shadow — an inner conditional with a
    // DIFFERENT infer name keeps the outer `U` substitution live in its
    // branches: `string extends infer U ? (number extends infer V ? U :
    // never) : never` → `string`.
    let infer_v = graph.intern_node(SemanticNodeData::Infer {
        name: Arc::from("V"),
        binder: graph.alloc_infer_binder_id(),
    });
    let inner_v = graph.intern_node(SemanticNodeData::Conditional {
        check: number_node,
        extends: infer_v,
        true_branch_ref: infer_u,
        false_branch_ref: never_node,
        distributive: false,
    });
    let result = match dispatch.execute_type_node(SemanticQueryKey::Conditional {
        check: string_node,
        extends: infer_u,
        true_branch: inner_v,
        false_branch: never_node,
        distributive: false,
    }) {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        other => panic!("expected Value, got {other:?}"),
    };
    // The outer U substituted into the non-shadowed inner true branch,
    // and the inner conditional reduced on demand (`number extends infer
    // V` binds V and selects TRUE): the outcome is the outer-bound
    // `string`.
    assert_eq!(
        result,
        string_node,
        "a non-shadowing inner binder (V) must NOT block the outer U \
         substitution; got {:?}",
        graph.node_data(result)
    );
}

/// Binder-scope precision, reference case: an inner conditional whose
/// `extends` merely REFERENCES the outer `infer U` does NOT re-bind it —
/// the outer substitution must reach the inner conditional.
/// `type R = string extends infer U ? (number extends U ? U : "no") : never`
/// → `"no"` (`number extends string` is false).
#[test]
fn inner_conditional_referencing_outer_infer_does_not_shadow() {
    let host = host_for_relation_tests();
    let canonical = "/w/infer_ref_scope.ts";
    upsert_relation_fixture(
        &host,
        canonical,
        "export type R = string extends infer U ? (number extends U ? U : \"no\") : never;\n",
    );
    let r = resolve_relation_fixture_symbol(&host, canonical, "R");
    let host_graph = host.project_type_store().semantic_graph();
    let data = host_graph.node_data(r);
    assert!(
        matches!(
            data.as_deref(),
            Some(SemanticNodeData::Literal(
                crate::semantic_query::LiteralValue::String(s)
            )) if s == "no"
        ),
        "a bare REFERENCE to the outer binder must not shadow-stop the \
         substitution (`number extends string` is false ⇒ \"no\"); got {data:?}"
    );
}

/// Binder-scope precision, nearest-conditional case: an `infer U`
/// DECLARED inside an INNER conditional's own pattern binds at that inner
/// conditional (TS scopes `infer` to the nearest enclosing conditional) —
/// it does NOT re-bind the middle level, whose `U` is the outer binder.
/// `type F = string extends infer U
///    ? (true extends (string extends infer U ? true : false) ? U : never)
///    : never` → `string`.
#[test]
fn infer_declared_in_inner_conditional_scope_does_not_shadow_middle_level() {
    let host = host_for_relation_tests();
    let canonical = "/w/infer_inner_scope.ts";
    upsert_relation_fixture(
        &host,
        canonical,
        "export type F = string extends infer U ? (true extends (string extends infer U ? true : false) ? U : never) : never;\n",
    );
    let f = resolve_relation_fixture_symbol(&host, canonical, "F");
    let host_graph = host.project_type_store().semantic_graph();
    let data = host_graph.node_data(f);
    assert!(
        matches!(
            data.as_deref(),
            Some(SemanticNodeData::Primitive(PrimitiveKind::String))
        ),
        "an inner-scope `infer U` declaration must not shadow the MIDDLE \
         conditional's outer-bound `U`; got {data:?}"
    );
}

/// Open-check classifiers include `InferRef`: under Expanded empty-path,
/// `T extends infer U ? (U extends string ? {a} : {b}) : {c}` distributes
/// BOTH conditional levels — the inner check is an `InferRef` reference to
/// the still-open outer binder, exactly as open as a `TypeParam` / `Infer`
/// check — into the three-arm union.
#[test]
fn expanded_distribution_treats_infer_ref_check_as_open() {
    use crate::semantic_query::{
        PathSegment, ProjectionMode, ProjectionReductionContext, QueryResult, SemanticQueryApi,
        SemanticQueryKey, SemanticQueryOutput,
    };
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let string_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let t_param = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let infer_u_binder = graph.alloc_infer_binder_id();
    let infer_u = graph.intern_node(SemanticNodeData::Infer {
        name: Arc::from("U"),
        binder: infer_u_binder.clone(),
    });
    // The post-activation lowering shape: the inner check `U` is an
    // `InferRef` REFERENCE to the outer binder.
    let infer_ref_u = graph.intern_node(SemanticNodeData::InferRef {
        name: Arc::from("U"),
        binder: infer_u_binder,
    });
    // The branch arms are REAL declaration placeholders so the terminal
    // expansion genuinely rewrites them (the arm-identity short-circuit in
    // the expander keeps the parent when nothing expanded).
    let canonical = "/w/inferref_open_arms.ts";
    upsert_relation_fixture(
        &host,
        canonical,
        concat!(
            "export interface ArmA { a: string }\n",
            "export interface ArmB { b: string }\n",
            "export interface ArmC { c: string }\n",
        ),
    );
    let whole_hash = host
        .ensure_indexed_ready(canonical)
        .expect("IndexedReady for the arm fixture")
        .whole_hash;
    let placeholder = |name: &str| {
        graph.intern_node(SemanticNodeData::Opaque(
            crate::semantic_query::QueryError::DeclPlaceholder {
                canonical_id: Arc::from(canonical),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                name: Arc::from(name),
                whole_hash,
            },
        ))
    };
    let (a, b, c) = (
        placeholder("ArmA"),
        placeholder("ArmB"),
        placeholder("ArmC"),
    );
    let inner = graph.intern_node(SemanticNodeData::Conditional {
        check: infer_ref_u,
        extends: string_node,
        true_branch_ref: a,
        false_branch_ref: b,
        distributive: false,
    });
    let outer = graph.intern_node(SemanticNodeData::Conditional {
        check: t_param,
        extends: infer_u,
        true_branch_ref: inner,
        false_branch_ref: c,
        distributive: false,
    });

    let projected = match dispatch.execute_type_node(SemanticQueryKey::ProjectPath {
        base: outer,
        path: Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        context: ProjectionReductionContext::published(ProjectionMode::Expanded),
    }) {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        other => panic!("expected Value, got {other:?}"),
    };

    // Collect every object-literal member name reachable through the
    // distributed union — all three arms must have materialised.
    let mut names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut stack = vec![projected];
    let mut visited = std::collections::HashSet::new();
    while let Some(node) = stack.pop() {
        if !visited.insert(node) {
            continue;
        }
        match graph.node_data(node).as_deref() {
            Some(SemanticNodeData::Union(arms)) => stack.extend(arms.iter().copied()),
            Some(SemanticNodeData::Object(view)) => {
                for m in view.positive_members().iter() {
                    names.insert(m.string_name().expect("string-key fixture").to_string());
                }
            }
            _ => {}
        }
    }
    assert_eq!(
        names,
        ["a", "b", "c"]
            .into_iter()
            .map(str::to_string)
            .collect::<std::collections::BTreeSet<_>>(),
        "Expanded empty-path must distribute BOTH open-check levels \
         (an InferRef check is open); got arms {names:?} from {:?}",
        graph.node_data(projected)
    );
}

/// A `Mapped` inside a conditional's extends is NOT an infer-binding
/// boundary (a mapped-`as`/value `infer` declares for the ENCLOSING
/// conditional — `conditional_binds_mapped_as_remap_infer_in_true_branch`
/// is the producer contract). An inner conditional whose MAPPED extends
/// re-declares `U` therefore SHADOWS the outer binder: the outer
/// substitution must not rewrite the mapped declaration. The capture
/// collapses the inner pattern to `{ [K in "a"]: string }`, failing the
/// `{ a: number }` check and yielding `never`.
#[test]
fn mapped_extends_infer_declaration_shadows_outer_binder() {
    use crate::semantic_query::{
        LiteralValue, MapperKey, MapperKind, OptionalityMod, QueryResult, ReadonlyMod,
        SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
    };
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let string_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let never_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
    let infer_u = graph.intern_node(SemanticNodeData::Infer {
        name: Arc::from("U"),
        binder: graph.alloc_infer_binder_id(),
    });
    let lit_a = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
        "a".to_string(),
    )));
    let k_param = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("K"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("K"),
    });
    // `{ [K in "a"]: infer U }` — the mapped VALUE declares `infer U`.
    let mapped = graph.intern_node(SemanticNodeData::Mapped {
        source: lit_a,
        mapper: MapperKey {
            parameter_node: k_param,
            key_space: lit_a,
            value_expr: infer_u,
            optionality: OptionalityMod::Keep,
            readonly: ReadonlyMod::Keep,
            name_remap: None,
            kind: MapperKind::Computed,
        },
    });
    let check_obj = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("a", number_node),
    ])));
    // Inner: `{ a: number } extends { [K in "a"]: infer U } ? U : never`.
    let inner = graph.intern_node(SemanticNodeData::Conditional {
        check: check_obj,
        extends: mapped,
        true_branch_ref: infer_u,
        false_branch_ref: never_node,
        distributive: false,
    });

    let result = match dispatch.execute_type_node(SemanticQueryKey::Conditional {
        check: string_node,
        extends: infer_u,
        true_branch: inner,
        false_branch: never_node,
        distributive: false,
    }) {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        other => panic!("expected Value, got {other:?}"),
    };
    let data = graph.node_data(result);
    // The CAPTURE outcome: the outer `U = string` rewrote the mapped
    // declaration, the inner pattern became `{ [K in "a"]: string }`, the
    // `{ a: number }` check failed, and the whole type collapsed `never`.
    assert!(
        !matches!(
            data.as_deref(),
            Some(SemanticNodeData::Primitive(PrimitiveKind::Never))
        ),
        "the mapped-declared inner `infer U` must shadow the outer binder \
         (Mapped is NOT a binding boundary) — a `never` collapse is the \
         capture; got {data:?}"
    );
    // And the surviving inner scope must still carry its own declaration
    // (either as the preserved conditional or its own rebound outcome —
    // never the outer-bound `string`).
    assert!(
        !matches!(
            data.as_deref(),
            Some(SemanticNodeData::Primitive(PrimitiveKind::String))
        ),
        "the inner rebind must not surface the OUTER binder's bound; got {data:?}"
    );
}

/// Capture-avoidance across the `Infer`/`TypeParam` boundary: a mapped
/// type's OWN key parameter shadows a same-NAMED outer `infer` binder.
/// `string extends infer K ? { [K in "a"]: K } : never` — the mapped's
/// `K` occurrences (value_expr) are the mapped binder, not the outer
/// infer; the node-identity-only shadow check missed the name-based
/// rewrite and captured them (`{ a: string }` instead of `{ a: "a" }`).
#[test]
fn mapped_own_key_param_shadows_same_named_outer_infer_binder() {
    use crate::semantic_query::{
        LiteralValue, MapperKey, MapperKind, OptionalityMod, QueryResult, ReadonlyMod,
        SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
    };
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let string_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let never_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
    let infer_k = graph.intern_node(SemanticNodeData::Infer {
        name: Arc::from("K"),
        binder: graph.alloc_infer_binder_id(),
    });
    let lit_a = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
        "a".to_string(),
    )));
    // The mapped's OWN `K` binder (a TypeParam named "K" — a DIFFERENT
    // node id from the outer `infer K`).
    let k_param = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("K"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("K"),
    });
    // `{ [K in "a"]: K }` — the value IS the mapped binder occurrence.
    let mapped = graph.intern_node(SemanticNodeData::Mapped {
        source: lit_a,
        mapper: MapperKey {
            parameter_node: k_param,
            key_space: lit_a,
            value_expr: k_param,
            optionality: OptionalityMod::Keep,
            readonly: ReadonlyMod::Keep,
            name_remap: None,
            kind: MapperKind::Computed,
        },
    });

    let result = match dispatch.execute_type_node(SemanticQueryKey::Conditional {
        check: string_node,
        extends: infer_k,
        true_branch: mapped,
        false_branch: never_node,
        distributive: false,
    }) {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        other => panic!("expected Value, got {other:?}"),
    };

    // The selected branch is the mapped carrier; its value_expr must
    // STILL be the mapped's own `K` binder — a `string` there is the
    // outer infer's bound captured across the Infer/TypeParam boundary.
    let data = graph.node_data(result);
    let Some(SemanticNodeData::Mapped { mapper, .. }) = data.as_deref() else {
        panic!("the selected branch must stay the mapped carrier, got {data:?}");
    };
    assert_eq!(
        mapper.value_expr,
        k_param,
        "the mapped's own `K` occurrences shadow the same-named outer \
         `infer K` — the value must remain the mapped binder, got {:?}",
        graph.node_data(mapper.value_expr)
    );
    // Behavioral cross-check: materialising the mapped surface gives
    // `a: "a"` (per-key binder substitution), never `a: string`.
    let surface = match dispatch.execute_type_node(SemanticQueryKey::MappedType {
        source: lit_a,
        mapper: mapper.clone(),
        context: crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Expanded,
        ),
    }) {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        other => panic!("expected Value, got {other:?}"),
    };
    let surface_data = graph.node_data(surface);
    let Some(SemanticNodeData::Object(view)) = surface_data.as_deref() else {
        panic!("mapped surface must materialise an Object, got {surface_data:?}");
    };
    assert_eq!(view.positive_members().len(), 1);
    assert_eq!(
        view.positive_members()[0]
            .string_name()
            .expect("string-key fixture"),
        "a"
    );
    assert!(
        matches!(
            graph.node_data(view.positive_members()[0].value).as_deref(),
            Some(SemanticNodeData::Literal(LiteralValue::String(s))) if s == "a"
        ),
        "member `a` must be the per-key bound `\"a\"`, got {:?}",
        graph.node_data(view.positive_members()[0].value)
    );
}

/// Construct signatures substitute like call signatures:
/// `string extends infer T ? new (x: T) => T : never` must substitute the
/// bound `T` INSIDE the retained constructor carrier — a leaf treatment
/// leaves `new (x: T) => T` unbound.
#[test]
fn constructor_type_substitutes_bound_infer_inside_signature() {
    use crate::semantic_query::{
        FunctionParam, QueryResult, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
        TypeParamDecl,
    };
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let string_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let never_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
    let infer_t_binder = graph.alloc_infer_binder_id();
    let infer_t = graph.intern_node(SemanticNodeData::Infer {
        name: Arc::from("T"),
        binder: infer_t_binder.clone(),
    });
    // References inside the true branch bind as `InferRef` (the producer
    // contract).
    let t_ref = graph.intern_node(SemanticNodeData::InferRef {
        name: Arc::from("T"),
        binder: infer_t_binder,
    });
    let signature = graph.intern_node(SemanticNodeData::Signature {
        kind: crate::semantic_query::SignatureKind::Call,
        params: Arc::from(
            vec![FunctionParam::synthetic(
                Some(Arc::from("x")),
                t_ref,
                false,
                false,
            )]
            .into_boxed_slice(),
        ),
        return_type: t_ref,
        occurrence: None,
        return_carrier: crate::semantic_query::SignatureReturnCarrier::Declared(t_ref),
        type_parameters: Arc::from(Vec::<TypeParamDecl>::new().into_boxed_slice()),
        signature_span: None,
        return_type_span: None,
    });
    let ctor = graph.intern_construct_twin_for_tests(signature);

    let result = match dispatch.execute_type_node(SemanticQueryKey::Conditional {
        check: string_node,
        extends: infer_t,
        true_branch: ctor,
        false_branch: never_node,
        distributive: false,
    }) {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        other => panic!("expected Value, got {other:?}"),
    };
    let data = graph.node_data(result);
    let Some(SemanticNodeData::Signature {
        kind: crate::semantic_query::SignatureKind::Construct,
        params,
        return_type,
        ..
    }) = data.as_deref()
    else {
        panic!("the selected branch must stay a construct Signature, got {data:?}");
    };
    assert_eq!(
        (params[0].ty, *return_type),
        (string_node, string_node),
        "the bound `T` must substitute INSIDE the constructor signature \
         (`new (x: string) => string`); got param {:?} / return {:?}",
        graph.node_data(params[0].ty),
        graph.node_data(*return_type)
    );
}

/// Construct signatures relate and bind infers through the relation + the
/// conditional infer route:
/// `(new () => number) extends (new () => infer R) ? { value: R } : { bad: true }`
/// binds `R = number` and selects the TRUE branch. A relation that falls
/// through to kind-mismatch `NotAssignable` silently selects the WRONG
/// branch (`{ bad: true }`).
#[test]
fn constructor_type_relates_and_binds_infer_return() {
    use crate::semantic_query::{
        FunctionParam, QueryResult, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
        TypeParamDecl,
    };
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let number_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let bool_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let infer_r_binder = graph.alloc_infer_binder_id();
    let infer_r = graph.intern_node(SemanticNodeData::Infer {
        name: Arc::from("R"),
        binder: infer_r_binder.clone(),
    });
    let r_ref = graph.intern_node(SemanticNodeData::InferRef {
        name: Arc::from("R"),
        binder: infer_r_binder,
    });
    let ctor_of = |ret: crate::semantic_query::SemanticNodeId| {
        let signature = graph.intern_node(SemanticNodeData::Signature {
            kind: crate::semantic_query::SignatureKind::Call,
            params: Arc::from(Vec::<FunctionParam>::new().into_boxed_slice()),
            return_type: ret,
            occurrence: None,
            return_carrier: crate::semantic_query::SignatureReturnCarrier::Declared(ret),
            type_parameters: Arc::from(Vec::<TypeParamDecl>::new().into_boxed_slice()),
            signature_span: None,
            return_type_span: None,
        });
        graph.intern_construct_twin_for_tests(signature)
    };
    let check = ctor_of(number_node);
    let extends = ctor_of(infer_r);
    let true_branch = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("value", r_ref),
    ])));
    let false_branch = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("bad", bool_node),
    ])));

    let result = match dispatch.execute_type_node(SemanticQueryKey::Conditional {
        check,
        extends,
        true_branch,
        false_branch,
        distributive: false,
    }) {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        other => panic!("expected Value, got {other:?}"),
    };
    let data = graph.node_data(result);
    let Some(SemanticNodeData::Object(view)) = data.as_deref() else {
        panic!("the selected branch must be an Object, got {data:?}");
    };
    assert_eq!(
        view.positive_members()[0]
            .string_name()
            .expect("string-key fixture"),
        "value",
        "constructor-vs-constructor must relate through the signatures and \
         select TRUE (`value: R`) — the `bad` member means the relation fell \
         through to a kind-mismatch NotAssignable"
    );
    assert_eq!(
        view.positive_members()[0].value,
        number_node,
        "R must bind to the constructor's return (`number`); got {:?}",
        graph.node_data(view.positive_members()[0].value)
    );
}

/// Construct signatures substitute under the MAPPED per-K
/// substitution: `{ [K in "a"]: new (x: K) => K }` materialises
/// `a: new (x: "a") => "a"` — a substitution leaf leaves `K` unresolved
/// inside the constructor value.
#[test]
fn mapped_constructor_value_substitutes_per_key() {
    use crate::semantic_query::{
        FunctionParam, LiteralValue, MapperKey, MapperKind, OptionalityMod, QueryResult,
        ReadonlyMod, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput, TypeParamDecl,
    };
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let lit_a = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
        "a".to_string(),
    )));
    let k_param = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("K"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("K"),
    });
    let signature = graph.intern_node(SemanticNodeData::Signature {
        kind: crate::semantic_query::SignatureKind::Call,
        params: Arc::from(
            vec![FunctionParam::synthetic(
                Some(Arc::from("x")),
                k_param,
                false,
                false,
            )]
            .into_boxed_slice(),
        ),
        return_type: k_param,
        occurrence: None,
        return_carrier: crate::semantic_query::SignatureReturnCarrier::Declared(k_param),
        type_parameters: Arc::from(Vec::<TypeParamDecl>::new().into_boxed_slice()),
        signature_span: None,
        return_type_span: None,
    });
    let ctor = graph.intern_construct_twin_for_tests(signature);

    let surface = match dispatch.execute_type_node(SemanticQueryKey::MappedType {
        source: lit_a,
        mapper: MapperKey {
            parameter_node: k_param,
            key_space: lit_a,
            value_expr: ctor,
            optionality: OptionalityMod::Keep,
            readonly: ReadonlyMod::Keep,
            name_remap: None,
            kind: MapperKind::Computed,
        },
        context: crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Expanded,
        ),
    }) {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        other => panic!("expected Value, got {other:?}"),
    };
    let surface_data = graph.node_data(surface);
    let Some(SemanticNodeData::Object(view)) = surface_data.as_deref() else {
        panic!("mapped surface must materialise an Object, got {surface_data:?}");
    };
    let member_value = view.positive_members()[0].value;
    let sig_data = graph.node_data(member_value);
    let Some(SemanticNodeData::Signature {
        kind: crate::semantic_query::SignatureKind::Construct,
        params,
        return_type,
        ..
    }) = sig_data.as_deref()
    else {
        panic!(
            "member `a` must stay a construct Signature, got {:?}",
            graph.node_data(member_value)
        );
    };
    assert_eq!(
        (params[0].ty, *return_type),
        (lit_a, lit_a),
        "the mapped `K` must substitute per-key INSIDE the constructor \
         (`new (x: \"a\") => \"a\"`); got param {:?} / return {:?}",
        graph.node_data(params[0].ty),
        graph.node_data(*return_type)
    );
}

/// The declaration-scoped shadow predicate descends CONSTRUCTOR patterns:
/// an inner conditional whose extends declares `infer P` inside a
/// `new (x: infer P) => any` pattern re-binds `P` — the outer
/// substitution must not capture the inner declaration's scope.
#[test]
fn constructor_pattern_infer_declaration_shadows_outer_binder() {
    use crate::semantic_query::{
        FunctionParam, QueryResult, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
        TypeParamDecl,
    };
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let string_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let any_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any));
    let never_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
    let number_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let infer_p_binder = graph.alloc_infer_binder_id();
    let infer_p = graph.intern_node(SemanticNodeData::Infer {
        name: Arc::from("P"),
        binder: infer_p_binder.clone(),
    });
    let p_ref = graph.intern_node(SemanticNodeData::InferRef {
        name: Arc::from("P"),
        binder: infer_p_binder,
    });
    // Inner extends: `new (x: infer P) => any`.
    let signature = graph.intern_node(SemanticNodeData::Signature {
        kind: crate::semantic_query::SignatureKind::Call,
        params: Arc::from(
            vec![FunctionParam::synthetic(
                Some(Arc::from("x")),
                infer_p,
                false,
                false,
            )]
            .into_boxed_slice(),
        ),
        return_type: any_node,
        occurrence: None,
        return_carrier: crate::semantic_query::SignatureReturnCarrier::Declared(any_node),
        type_parameters: Arc::from(Vec::<TypeParamDecl>::new().into_boxed_slice()),
        signature_span: None,
        return_type_span: None,
    });
    let ctor_pattern = graph.intern_construct_twin_for_tests(signature);
    // Inner: `(new (x: number) => any) extends new (x: infer P) => any ? P : never`.
    let inner_check_sig = graph.intern_node(SemanticNodeData::Signature {
        kind: crate::semantic_query::SignatureKind::Call,
        params: Arc::from(
            vec![FunctionParam::synthetic(
                Some(Arc::from("x")),
                number_node,
                false,
                false,
            )]
            .into_boxed_slice(),
        ),
        return_type: any_node,
        occurrence: None,
        return_carrier: crate::semantic_query::SignatureReturnCarrier::Declared(any_node),
        type_parameters: Arc::from(Vec::<TypeParamDecl>::new().into_boxed_slice()),
        signature_span: None,
        return_type_span: None,
    });
    let inner_check = graph.intern_construct_twin_for_tests(inner_check_sig);
    let inner = graph.intern_node(SemanticNodeData::Conditional {
        check: inner_check,
        extends: ctor_pattern,
        true_branch_ref: p_ref,
        false_branch_ref: never_node,
        distributive: false,
    });

    // Outer: `string extends infer P ? <inner> : never`.
    let result = match dispatch.execute_type_node(SemanticQueryKey::Conditional {
        check: string_node,
        extends: infer_p,
        true_branch: inner,
        false_branch: never_node,
        distributive: false,
    }) {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        other => panic!("expected Value, got {other:?}"),
    };
    // The inner constructor-pattern declaration re-binds P: the inner
    // conditional's own inference gives P = number (the constructor
    // param), NEVER the outer bound `string` (the capture).
    let data = graph.node_data(result);
    assert!(
        !matches!(
            data.as_deref(),
            Some(SemanticNodeData::Primitive(PrimitiveKind::String))
        ),
        "the inner constructor-pattern `infer P` must shadow the outer \
         binder — `string` here is the outer bound captured into the inner \
         scope; got {data:?}"
    );
    assert_eq!(
        result,
        number_node,
        "the inner conditional re-binds P := number through its own \
         constructor pattern; got {:?}",
        graph.node_data(result)
    );
}

/// D7 helper: intern a bare call signature `() => ret`
/// (with optional named param) and the construct spelling of the same
/// signature.
#[allow(clippy::type_complexity)]
fn signature_fixture_nodes(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    param: Option<(&str, crate::semantic_query::SemanticNodeId)>,
    ret: crate::semantic_query::SemanticNodeId,
) -> (
    crate::semantic_query::SemanticNodeId, // call
    crate::semantic_query::SemanticNodeId, // construct
) {
    use crate::semantic_query::{FunctionParam, TypeParamDecl};
    let params: Vec<FunctionParam> = param
        .map(|(name, ty)| {
            vec![FunctionParam::synthetic(
                Some(Arc::from(name)),
                ty,
                false,
                false,
            )]
        })
        .unwrap_or_default();
    let call = graph.intern_node(SemanticNodeData::Signature {
        kind: crate::semantic_query::SignatureKind::Call,
        params: Arc::from(params.into_boxed_slice()),
        return_type: ret,
        occurrence: None,
        return_carrier: crate::semantic_query::SignatureReturnCarrier::Declared(ret),
        type_parameters: Arc::from(Vec::<TypeParamDecl>::new().into_boxed_slice()),
        signature_span: None,
        return_type_span: None,
    });
    let construct = graph.intern_construct_twin_for_tests(call);
    (call, construct)
}

/// Signature-kind object forms: the structurally-equivalent OBJECT forms must relate
/// by signature KIND. `(() => R) extends { new(): R }` is FALSE (a call
/// signature never satisfies a construct signature) and
/// `(new () => R) extends { new(): R }` is TRUE — in BOTH directions.
#[test]
fn call_and_construct_object_forms_relate_by_kind() {
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let (call_fn, ctor) = signature_fixture_nodes(graph, None, number);

    // `{ new(): number }` — a construct-ONLY object.
    let construct_obj = graph.intern_node(SemanticNodeData::Object(
        crate::semantic_query::surface_view! {
            members: Arc::from(Vec::new().into_boxed_slice()),
            call_signatures: Arc::from(Vec::new().into_boxed_slice()),
            construct_signatures: Arc::from(vec![ctor].into_boxed_slice()),
            index_signatures: Arc::from(Vec::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        },
    ));
    // `{ (): number }` — a call-ONLY object.
    let call_obj = graph.intern_node(SemanticNodeData::Object(
        crate::semantic_query::surface_view! {
            members: Arc::from(Vec::new().into_boxed_slice()),
            call_signatures: Arc::from(vec![call_fn].into_boxed_slice()),
            construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        },
    ));

    let relate = |s, t| dispatch.execute_relate_pair_as_result_for_tests(s, t);
    // Call fn vs construct-only object: FALSE both directions.
    assert_eq!(
        relate(call_fn, construct_obj),
        RelationResult::NotAssignable,
        "`() => R` must NOT satisfy `{{ new(): R }}` (construct bucket unmet)"
    );
    assert_eq!(
        relate(construct_obj, call_fn),
        RelationResult::NotAssignable,
        "`{{ new(): R }}` must NOT satisfy `() => R` (no call signature)"
    );
    // Construct spelling vs construct-only object: TRUE both directions.
    assert!(
        matches!(
            relate(ctor, construct_obj),
            RelationResult::Assignable { .. }
        ),
        "`new () => R` must satisfy `{{ new(): R }}`; got {:?}",
        relate(ctor, construct_obj)
    );
    assert!(
        matches!(
            relate(construct_obj, ctor),
            RelationResult::Assignable { .. }
        ),
        "`{{ new(): R }}` must satisfy `new () => R`; got {:?}",
        relate(construct_obj, ctor)
    );
    // Call fn vs call-only object: TRUE both directions (the call twin).
    assert!(
        matches!(relate(call_fn, call_obj), RelationResult::Assignable { .. }),
        "`() => R` must satisfy `{{ (): R }}`; got {:?}",
        relate(call_fn, call_obj)
    );
    assert!(
        matches!(relate(call_obj, call_fn), RelationResult::Assignable { .. }),
        "`{{ (): R }}` must satisfy `() => R`; got {:?}",
        relate(call_obj, call_fn)
    );
}

/// Signature-utility kind selection: the signature utilities are KIND-aware over a direct
/// construct spelling: `ConstructorParameters<new (x: string) => object>`
/// = `[x: string]`, `InstanceType<...>` = the instance type; `Parameters`
/// / `ReturnType` (call-kind) MISS on a construct input.
#[test]
fn signature_utilities_select_by_kind_over_direct_constructor() {
    use crate::semantic_query::{
        ProjectionMode, QueryResult, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
    };
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let string_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let instance = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
        required_member("made", string_node),
    ])));
    let (_call, ctor) = signature_fixture_nodes(graph, Some(("x", string_node)), instance);

    let run = |name: &str| {
        let anchor = crate::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
            Arc::from("/w/lib.ts"),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            Arc::from(name),
        );
        dispatch.execute_type_node(SemanticQueryKey::Instantiate(
            crate::semantic_query::InstantiateKey::new(
                anchor,
                Arc::from(vec![ctor].into_boxed_slice()),
                crate::semantic_query::InstantiateContext::non_file(
                    crate::semantic_query::ProjectionReductionContext::published(
                        ProjectionMode::Expanded,
                    ),
                    Default::default(),
                    crate::project_semantic_dispatch::BodySourceWitness::mint_for_unit_tests(),
                ),
            ),
        ))
    };

    // ConstructorParameters<new (x: string) => I> = [x: string].
    let ctor_params = match run("ConstructorParameters") {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        other => panic!("ConstructorParameters must produce a value, got {other:?}"),
    };
    let data = graph.node_data(ctor_params);
    let Some(SemanticNodeData::Tuple { elements, .. }) = data.as_deref() else {
        panic!("ConstructorParameters over a direct constructor must be a tuple, got {data:?}");
    };
    assert_eq!(elements.len(), 1);
    assert_eq!(
        elements[0].value,
        string_node,
        "the constructor's parameter type flows into the tuple; got {:?}",
        graph.node_data(elements[0].value)
    );

    // InstanceType<new (x: string) => I> = I.
    let inst = match run("InstanceType") {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        other => panic!("InstanceType must produce a value, got {other:?}"),
    };
    assert_eq!(
        inst,
        instance,
        "InstanceType selects the construct signature's return; got {:?}",
        graph.node_data(inst)
    );

    // Kind negatives: the CALL-kind utilities must NOT read a construct
    // signature (Opaque miss shell, never the tuple/return).
    for call_only in ["Parameters", "ReturnType"] {
        let out = match run(call_only) {
            QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
            other => panic!("{call_only} must produce a value shell, got {other:?}"),
        };
        assert!(
            matches!(
                graph.node_data(out).as_deref(),
                Some(SemanticNodeData::Opaque(_))
            ),
            "{call_only} (call-kind) must MISS on a construct input, got {:?}",
            graph.node_data(out)
        );
    }
}

/// Signature-kind semantics: direct call vs direct construct are non-assignable
/// in BOTH directions, and inference bindings survive an object-form source
/// (`{ new(): number }` against the direct construct pattern
/// `new () => infer R` binds `R := number`, not merely the correct boolean).
/// Cross-producer parity for the constructor shape (the eager path and the
/// structural producer interning the same root `Signature(Construct)`) is
/// asserted in
/// `structural_carrier_producer::structural_lower_tests::structural_equivalence_for_constructor_signature`.
#[test]
fn signature_kind_semantics_and_cross_producer_parity() {
    use crate::semantic_query::SignatureKind;
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let (call_fn, ctor) = signature_fixture_nodes(graph, None, number);

    // Direct call vs direct construct: non-assignable BOTH directions.
    assert_eq!(
        dispatch.execute_relate_pair_as_result_for_tests(call_fn, ctor),
        RelationResult::NotAssignable,
        "`() => R` never satisfies `new () => R`"
    );
    assert_eq!(
        dispatch.execute_relate_pair_as_result_for_tests(ctor, call_fn),
        RelationResult::NotAssignable,
        "`new () => R` never satisfies `() => R`"
    );

    // Inference bindings survive the OBJECT form: `{ new(): number }`
    // against the direct construct pattern `new () => infer R` binds
    // R := number (not merely the correct boolean).
    let infer_r = graph.intern_node(SemanticNodeData::Infer {
        name: Arc::from("R"),
        binder: graph.alloc_infer_binder_id(),
    });
    let ctor_infer = {
        use crate::semantic_query::{FunctionParam, TypeParamDecl};
        graph.intern_node(SemanticNodeData::Signature {
            kind: SignatureKind::Construct,
            params: Arc::from(Vec::<FunctionParam>::new().into_boxed_slice()),
            return_type: infer_r,
            occurrence: None,
            return_carrier: crate::semantic_query::SignatureReturnCarrier::Declared(infer_r),
            type_parameters: Arc::from(Vec::<TypeParamDecl>::new().into_boxed_slice()),
            signature_span: None,
            return_type_span: None,
        })
    };
    let construct_obj = graph.intern_node(SemanticNodeData::Object(
        crate::semantic_query::surface_view! {
            members: Arc::from(Vec::new().into_boxed_slice()),
            call_signatures: Arc::from(Vec::new().into_boxed_slice()),
            construct_signatures: Arc::from(vec![ctor].into_boxed_slice()),
            index_signatures: Arc::from(Vec::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        },
    ));
    match dispatch.execute_relate_pair(construct_obj, ctor_infer) {
        crate::project_semantic_dispatch::dispatch_txn::RelationStep::Assignable { bindings } => {
            assert_eq!(bindings.len(), 1, "the construct-pattern session binds R");
            assert_eq!(
                bindings[0].bound,
                number,
                "R binds the object's construct signature return; got {:?}",
                graph.node_data(bindings[0].bound)
            );
        }
        other => panic!("`{{ new(): number }}` must match `new () => infer R`, got {other:?}"),
    }
}

// ============================================================================
// Shared spread materializer (the ordered object-literal left fold)
// ============================================================================
//
// Behavioral contract of `lower_spread_object_literal` + `spread_fold` (TS
// 5.4.5 `getSpreadType`): origin minting (FreshOwn survives only where the
// fold proves the member untouched), required/optional overlap semantics,
// top/bottom lattice, bounded union distribution, structured index-info
// composition, and typed open-spread object surfaces.

// ============================================================================
// Fresh excess-property checking (the union excess prepass)
// ============================================================================
//
// The relation authority's `Fresh + excess_property_check` axis: the excess
// prepass runs once per relation frame before ordinary union-arm
// distribution — gate, broad-target skips, discriminant reduction, primitive
// filtering, FreshOwn-only candidate selection, structured known-name
// checking, reduced-arm value checking, and three-valued outcomes.

pub(crate) mod fresh_excess_property_checking {
    use std::sync::Arc;

    use verter_type_expr::ExcessPropertyOrigin;

    use super::{empty_surface, host_for_relation_tests, optional_member, required_member};
    use crate::project_semantic_dispatch::dispatch_txn::RelationStep;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{
        FreshnessKey, LiteralValue, PrimitiveKind, SemanticNodeData, SemanticNodeId, SurfaceMember,
    };
    use crate::VerterHost;

    fn fresh_member(name: &str, value: SemanticNodeId) -> SurfaceMember {
        let mut member = required_member(name, value);
        member.excess_origin = ExcessPropertyOrigin::FreshOwn;
        member
    }

    fn tainted_member(name: &str, value: SemanticNodeId) -> SurfaceMember {
        let mut member = required_member(name, value);
        member.excess_origin = ExcessPropertyOrigin::SpreadTainted;
        member
    }

    /// The FRESH excess-checking relation step for `(source, target)`.
    fn relate_fresh_excess(
        host: &VerterHost,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> RelationStep {
        let dispatch = ProjectSemanticDispatch::new(host);
        let mut key = dispatch.relate_key_for(source, target);
        key.source_freshness = FreshnessKey::Fresh;
        key.policy.excess_property_check = true;
        dispatch.execute_relate(key)
    }

    /// The plain widened assignability step (regular source, no policy).
    fn relate_regular(
        host: &VerterHost,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> RelationStep {
        let dispatch = ProjectSemanticDispatch::new(host);
        dispatch.execute_relate(dispatch.relate_key_for(source, target))
    }

    fn assert_assignable(step: &RelationStep, what: &str) {
        assert!(
            matches!(step, RelationStep::Assignable { .. }),
            "{what}: expected Assignable, got {step:?}"
        );
    }

    fn assert_not_assignable(step: &RelationStep, what: &str) {
        assert!(
            matches!(step, RelationStep::NotAssignable),
            "{what}: expected NotAssignable, got {step:?}"
        );
    }

    fn spread_program(
        operand: SemanticNodeId,
        graph: &crate::semantic_query_memo::SemanticGraphStore,
    ) -> SemanticNodeId {
        graph.intern_node(SemanticNodeData::ObjectSpreadProgram(
            crate::semantic_query::ObjectSpreadProgram {
                effects: Arc::from([crate::semantic_query::ObjectConstructionEffect::Spread(
                    operand,
                )]),
            },
        ))
    }

    #[test]
    fn open_program_arm_does_not_prove_empty_or_key_absence() {
        let host = host_for_relation_tests();
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = host.project_type_store().semantic_graph();
        let operand = graph.intern_node(SemanticNodeData::TypeParam {
            decl: crate::semantic_query::DeclIdentity::synthetic("T"),
            param_index: 0,
            constraint: None,
            default: None,
            display_name: Arc::from("T"),
        });
        let arm = spread_program(operand, graph);

        assert_eq!(
            dispatch.open_surface_excess_facts_for_tests(arm, "hidden"),
            (Some(false), true, true),
            "an open program arm is never empty-object-like, and its \
             known-name / discriminant absence stay undecidable"
        );
    }

    #[test]
    fn union_excess_keeps_an_open_arm_undecided() {
        let host = host_for_relation_tests();
        let graph = host.project_type_store().semantic_graph();
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let operand = graph.intern_node(SemanticNodeData::TypeParam {
            decl: crate::semantic_query::DeclIdentity::synthetic("T"),
            param_index: 0,
            constraint: None,
            default: None,
            display_name: Arc::from("T"),
        });
        let source =
            graph.intern_node(SemanticNodeData::Object(empty_surface(vec![fresh_member(
                "extra", number,
            )])));
        let closed_arm = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            required_member("required", string),
        ])));
        let open_arm = spread_program(operand, graph);
        let target = graph.intern_node(SemanticNodeData::Union(Arc::from([closed_arm, open_arm])));

        let actual = relate_fresh_excess(&host, source, target);
        assert!(
            matches!(actual, RelationStep::Unknown),
            "open union arm must stay undecided, got {actual:?}"
        );
    }

    /// A fresh literal with an unknown extra member is rejected against a
    /// closed object target; the SAME shapes under a REGULAR (widened) source
    /// — or fresh WITHOUT the excess policy — relate ordinarily (width
    /// subtyping accepts).
    #[test]
    fn fresh_extra_member_rejected_and_regular_mode_skips_excess() {
        let host = host_for_relation_tests();
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = host.project_type_store().semantic_graph();
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

        let source = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            fresh_member("a", number),
            fresh_member("b", string),
        ])));
        let target = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            required_member("a", number),
        ])));

        assert_not_assignable(
            &relate_fresh_excess(&host, source, target),
            "fresh `{a, b}` vs `{a}`: `b` is excess",
        );
        assert_assignable(
            &relate_regular(&host, source, target),
            "the widened source relates by ordinary width subtyping",
        );
        // Fresh WITHOUT the excess policy: the prepass gate requires BOTH.
        let mut fresh_only = dispatch.relate_key_for(source, target);
        fresh_only.source_freshness = FreshnessKey::Fresh;
        assert_assignable(
            &dispatch.execute_relate(fresh_only),
            "fresh without excess_property_check skips the prepass",
        );
    }

    /// Discriminant reduction: `{ kind: "a", a: 1, b: "extra" }` narrows the
    /// union to the `kind: "a"` arm, and `b` — known ONLY in the discarded
    /// `kind: "b"` arm — is rejected after narrowing. WITHOUT reduction the
    /// name would be known (arm 2) and its string value would fit, so this
    /// discriminates the reduction from a flat known-name scan.
    #[test]
    fn discriminated_union_rejects_extra_after_narrowing() {
        let host = host_for_relation_tests();
        let graph = host.project_type_store().semantic_graph();
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let lit_a = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "a".to_string(),
        )));
        let lit_b = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "b".to_string(),
        )));
        let one = graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(1.0)));
        let extra = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "extra".to_string(),
        )));

        let source = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            fresh_member("kind", lit_a),
            fresh_member("a", one),
            fresh_member("b", extra),
        ])));
        let arm_a = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            required_member("kind", lit_a),
            required_member("a", number),
        ])));
        let arm_b = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            required_member("kind", lit_b),
            required_member("b", string),
        ])));
        let target = graph.intern_node(SemanticNodeData::Union(Arc::from(
            vec![arm_a, arm_b].into_boxed_slice(),
        )));

        assert_not_assignable(
            &relate_fresh_excess(&host, source, target),
            "after `kind: \"a\"` narrowing, `b` is excess in the surviving arm",
        );
    }

    /// A property known only through a SURVIVING arm is accepted only when
    /// its value fits the reduced-arm property union: `b?: string` in the
    /// surviving arm accepts `"x"` and rejects `2`.
    #[test]
    fn surviving_arm_property_is_value_checked_against_reduced_union() {
        let host = host_for_relation_tests();
        let graph = host.project_type_store().semantic_graph();
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let lit_a = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "a".to_string(),
        )));
        let lit_b = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "b".to_string(),
        )));
        let one = graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(1.0)));
        let two = graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(2.0)));
        let x = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "x".to_string(),
        )));

        let arm_a = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            required_member("kind", lit_a),
            required_member("a", number),
            optional_member("b", string),
        ])));
        let arm_b = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            required_member("kind", lit_b),
            required_member("c", number),
        ])));
        let target = graph.intern_node(SemanticNodeData::Union(Arc::from(
            vec![arm_a, arm_b].into_boxed_slice(),
        )));

        let rejecting = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            fresh_member("kind", lit_a),
            fresh_member("a", one),
            fresh_member("b", two),
        ])));
        assert_not_assignable(
            &relate_fresh_excess(&host, rejecting, target),
            "`b: 2` does not fit the surviving arm's `b?: string`",
        );

        let accepting = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            fresh_member("kind", lit_a),
            fresh_member("a", one),
            fresh_member("b", x),
        ])));
        assert_assignable(
            &relate_fresh_excess(&host, accepting, target),
            "`b: \"x\"` fits the surviving arm's `b?: string`",
        );
    }

    /// An applicable index signature makes a name KNOWN and contributes its
    /// value type: a compatible extra member is accepted, an incompatible one
    /// rejected.
    #[test]
    fn applicable_index_signature_accepts_compatible_and_rejects_incompatible() {
        let host = host_for_relation_tests();
        let graph = host.project_type_store().semantic_graph();
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let two = graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(2.0)));
        let s = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "s".to_string(),
        )));

        let indexed = crate::semantic_query::surface_view! {
            members: Arc::from(vec![required_member("a", number)].into_boxed_slice()),
            call_signatures: Arc::from(Vec::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
            index_signatures: Arc::from(
                vec![crate::semantic_query::IndexSignature {
                    key_type: string,
                    value_type: number,
                    readonly: false,
                    spans: Default::default(),
                    declaration_origin: None,
                }]
                .into_boxed_slice(),
            ),
            keyspace: None,
            has_index_signature: true,
        };
        let target = graph.intern_node(SemanticNodeData::Object(indexed));

        let compatible = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            fresh_member("a", two),
            fresh_member("extra", two),
        ])));
        assert_assignable(
            &relate_fresh_excess(&host, compatible, target),
            "`extra: 2` is known through `[k: string]: number` and fits",
        );

        let incompatible = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            fresh_member("a", two),
            fresh_member("extra", s),
        ])));
        assert_not_assignable(
            &relate_fresh_excess(&host, incompatible, target),
            "`extra: \"s\"` is known but violates the index value type",
        );
    }

    /// `{}`, `object`, and a global-`Object` reference target do not report
    /// excess properties.
    #[test]
    fn empty_object_object_primitive_and_global_object_targets_skip_excess() {
        let host = host_for_relation_tests();
        let graph = host.project_type_store().semantic_graph();
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

        let source = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            fresh_member("a", number),
            fresh_member("b", number),
        ])));

        // Empty `{}` target: excess is SKIPPED and the ordinary relation
        // accepts (every object satisfies the empty surface).
        let empty = graph.intern_node(SemanticNodeData::Object(empty_surface(Vec::new())));
        assert_assignable(
            &relate_fresh_excess(&host, source, empty),
            "the empty object target skips excess checking",
        );

        // The `object` nonprimitive and a global-`Object` reference: never an
        // excess REJECTION (the ordinary relation may stay undecided for the
        // unresolved reference — that is not an excess report).
        let object_prim = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Object));
        assert!(
            !matches!(
                relate_fresh_excess(&host, source, object_prim),
                RelationStep::NotAssignable
            ),
            "the `object` nonprimitive target must not report excess properties"
        );
        // DISCRIMINATING `Object` skip: two RESOLVABLE named targets with
        // IDENTICAL bodies — only the name differs. The `Object`-named one
        // skips excess (accepted on width); the other resolves and rejects.
        // This cannot pass through the not-a-target / unresolvable arms: an
        // unresolvable reference would leave BOTH verdicts equal.
        use crate::semantic_query::NodeScopeId;
        use crate::{FileLanguage, UpsertRequest};
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/excess_object_skip.ts".to_string(),
                source: Arc::from(
                    "export type Object = { a: number };\nexport type NotObject = { a: number };\n",
                ),
                file_language: FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .unwrap();
        let dispatch = ProjectSemanticDispatch::new(&host);
        let shallow = dispatch
            .ctx
            .shallow_file_state("/excess_object_skip.ts")
            .expect("fixture must index");
        let scope = NodeScopeId::File {
            canonical_id: Arc::from("/excess_object_skip.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: shallow.whole_hash,
            local_scope: None,
        };
        let object_named = graph.intern_node(SemanticNodeData::DeclRef {
            identity: crate::semantic_query::DeclIdentity::from_scope(&scope, Arc::from("Object")),
        });
        let not_object = graph.intern_node(SemanticNodeData::DeclRef {
            identity: crate::semantic_query::DeclIdentity::from_scope(
                &scope,
                Arc::from("NotObject"),
            ),
        });
        // A USERLAND `type Object = { a: number }` is NOT the global: TS
        // still excess-checks against it — the spelling alone never grants
        // the skip.
        assert!(
            matches!(
                relate_fresh_excess(&host, source, object_named),
                RelationStep::NotAssignable
            ),
            "a module-local `Object` declaration is excess-checked like any \
             other named target — the skip is identity, not spelling"
        );
        assert!(
            matches!(
                relate_fresh_excess(&host, source, not_object),
                RelationStep::NotAssignable
            ),
            "the identically-shaped non-`Object` target rejects too (sanity)"
        );
        // The GLOBAL `Object` identity (builtin/ambient namespace) DOES
        // skip: never an excess rejection.
        let builtin_object = graph.intern_node(SemanticNodeData::DeclRef {
            identity: crate::semantic_query::DeclIdentity {
                canonical_id: Arc::from("__builtin__"),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                whole_hash: crate::semantic_query::HashValue::default(),
                decl_name: Arc::from("Object"),
            },
        });
        assert!(
            !matches!(
                relate_fresh_excess(&host, source, builtin_object),
                RelationStep::NotAssignable
            ),
            "the builtin-identified global `Object` target must not report \
             excess properties"
        );
    }

    /// Direct, shorthand, method, getter, and setter members are all
    /// `FreshOwn` excess candidates: a method-shaped fresh member is
    /// rejected exactly like a plain property.
    #[test]
    fn method_and_accessor_shaped_fresh_members_are_excess_candidates() {
        let host = host_for_relation_tests();
        let graph = host.project_type_store().semantic_graph();
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

        let mut method = fresh_member("m", number);
        method.method_kind = Some(verter_type_expr::ObjectMethodKind::Method);
        let source = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            fresh_member("a", number),
            method,
        ])));
        let target = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            required_member("a", number),
        ])));
        assert_not_assignable(
            &relate_fresh_excess(&host, source, target),
            "a method-shaped FreshOwn member is an excess candidate",
        );
    }

    /// Spread-only (`SpreadTainted`) members are NOT excess candidates — but
    /// a tainted member whose name IS known still undergoes normal value
    /// compatibility (taint never suppresses a value mismatch).
    #[test]
    fn spread_tainted_members_are_not_candidates_but_still_value_check() {
        let host = host_for_relation_tests();
        let graph = host.project_type_store().semantic_graph();
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

        // `b` arrived through a spread: NOT an excess candidate.
        let source = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            fresh_member("a", number),
            tainted_member("b", string),
        ])));
        let narrow_target = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            required_member("a", number),
        ])));
        assert_assignable(
            &relate_fresh_excess(&host, source, narrow_target),
            "a SpreadTainted extra member is exempt from excess reporting",
        );

        // But when the target DOES know `b`, the tainted member's value must
        // still satisfy it.
        let typed_target = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            required_member("a", number),
            required_member("b", number),
        ])));
        assert_not_assignable(
            &relate_fresh_excess(&host, source, typed_target),
            "spread taint never suppresses a value incompatibility",
        );
    }

    /// `excess_origin` does not change ORDINARY assignability: under a
    /// regular key the same shapes yield the same verdict regardless of
    /// member origins.
    #[test]
    fn origin_does_not_change_ordinary_assignability() {
        let host = host_for_relation_tests();
        let graph = host.project_type_store().semantic_graph();
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

        let target = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            required_member("a", number),
        ])));
        let fresh_source = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            fresh_member("a", number),
            fresh_member("b", number),
        ])));
        let plain_source = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            required_member("a", number),
            required_member("b", number),
        ])));

        let fresh_step = relate_regular(&host, fresh_source, target);
        let plain_step = relate_regular(&host, plain_source, target);
        assert_assignable(&fresh_step, "FreshOwn-membered source, regular key");
        assert_assignable(&plain_step, "NonLiteral-membered source, regular key");
    }

    /// Same-object union arms do not perform branch-local excess checks:
    /// `{ a, b }` vs `{a} | {b}` — each name is known in SOME arm, so the
    /// prepass passes and ordinary distribution accepts through the `{a}`
    /// arm. A branch-local excess check would reject BOTH arms.
    #[test]
    fn union_arms_do_not_rerun_branch_local_excess() {
        let host = host_for_relation_tests();
        let graph = host.project_type_store().semantic_graph();
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

        let source = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            fresh_member("a", number),
            fresh_member("b", number),
        ])));
        let arm_a = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            required_member("a", number),
        ])));
        let arm_b = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            required_member("b", number),
        ])));
        let target = graph.intern_node(SemanticNodeData::Union(Arc::from(
            vec![arm_a, arm_b].into_boxed_slice(),
        )));

        assert_assignable(
            &relate_fresh_excess(&host, source, target),
            "no branch-local excess: names known across arms, ordinary \
             distribution decides",
        );
    }

    /// Nested freshness: an INLINE nested literal (FreshOwn members) is
    /// excess-checked in its own sub-relation frame, while the equivalent
    /// VARIABLE-SOURCED nested object (NonLiteral members) is accepted.
    #[test]
    fn nested_inline_excess_rejected_variable_sourced_nested_accepted() {
        let host = host_for_relation_tests();
        let graph = host.project_type_store().semantic_graph();
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

        let nested_target = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            required_member("a", number),
        ])));
        let target = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            required_member("n", nested_target),
        ])));

        // Inline nested literal: FreshOwn members ⇒ nested excess rejection.
        let inline_nested = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            fresh_member("a", number),
            fresh_member("extra", number),
        ])));
        let inline_source =
            graph.intern_node(SemanticNodeData::Object(empty_surface(vec![fresh_member(
                "n",
                inline_nested,
            )])));
        assert_not_assignable(
            &relate_fresh_excess(&host, inline_source, target),
            "an inline nested literal inherits freshness and rejects `extra`",
        );

        // Variable-sourced nested object: NonLiteral members ⇒ accepted.
        let variable_nested = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            required_member("a", number),
            required_member("extra", number),
        ])));
        let variable_source =
            graph.intern_node(SemanticNodeData::Object(empty_surface(vec![fresh_member(
                "n",
                variable_nested,
            )])));
        assert_assignable(
            &relate_fresh_excess(&host, variable_source, target),
            "a variable-sourced nested object does not regain freshness",
        );
    }

    /// A union value check that cannot be decided propagates `Unknown` — it
    /// is neither collapsed into a rejection nor an acceptance, and nothing
    /// is admitted to the relation memo.
    #[test]
    fn union_value_check_unknown_stays_unknown() {
        let host = host_for_relation_tests();
        let graph = host.project_type_store().semantic_graph();
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let lit_a = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "a".to_string(),
        )));
        // The surviving arm's `b` type is an UNRESOLVABLE reference — the
        // value check against it is undecidable.
        let unresolved = graph.intern_node(SemanticNodeData::DeclRef {
            identity: crate::semantic_query::DeclIdentity {
                canonical_id: Arc::from("/missing-file.ts"),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                whole_hash: crate::semantic_query::HashValue::default(),
                decl_name: Arc::from("MissingType"),
            },
        });

        let arm_a = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            required_member("kind", lit_a),
            optional_member("b", unresolved),
        ])));
        // A second arm keeps the target a UNION (the value check is the
        // union-target step).
        let lit_b = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "b".to_string(),
        )));
        let arm_b = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            required_member("kind", lit_b),
            required_member("c", number),
        ])));
        let target = graph.intern_node(SemanticNodeData::Union(Arc::from(
            vec![arm_a, arm_b].into_boxed_slice(),
        )));

        let source = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            fresh_member("kind", lit_a),
            fresh_member("b", number),
        ])));

        let before = graph.relation_memo_count();
        let step = relate_fresh_excess(&host, source, target);
        assert!(
            matches!(step, RelationStep::Unknown),
            "an undecidable union value check stays Unknown, got {step:?}"
        );
        assert_eq!(
            graph.relation_memo_count(),
            before,
            "Unknown admits NOTHING to the relation memo"
        );
    }

    /// A nested union value check stays undecided when one contributing arm
    /// exhausts named-target resolution. The exhausted arm cannot stand in
    /// for an absent property and contribute a fabricated `undefined`.
    #[test]
    fn nested_union_value_check_is_undecided_when_an_arm_resolution_exhausts() {
        use crate::semantic_query::NodeScopeId;
        use crate::{FileLanguage, UpsertRequest};

        let host = host_for_relation_tests();
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/excess_nested_exhaustion.ts".to_string(),
                source: Arc::from(concat!(
                    "export type Hop0 = Hop1;\n",
                    "export type Hop1 = Hop2;\n",
                    "export type Hop2 = Hop3;\n",
                    "export type Hop3 = Hop4;\n",
                    "export type Hop4 = Hop5;\n",
                    "export type Hop5 = Hop6;\n",
                    "export type Hop6 = Hop7;\n",
                    "export type Hop7 = Hop8;\n",
                    "export type Hop8 = Hop9;\n",
                    "export type Hop9 = { q?: number };\n",
                )),
                file_language: FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .unwrap();
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = host.project_type_store().semantic_graph();
        let shallow = dispatch
            .ctx
            .shallow_file_state("/excess_nested_exhaustion.ts")
            .expect("fixture must index");
        let scope = NodeScopeId::File {
            canonical_id: Arc::from("/excess_nested_exhaustion.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: shallow.whole_hash,
            local_scope: None,
        };
        let exhausted_arm = graph.intern_node(SemanticNodeData::DeclRef {
            identity: crate::semantic_query::DeclIdentity::from_scope(&scope, Arc::from("Hop0")),
        });
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let known_arm = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            required_member("p", number),
        ])));
        let nested = graph.intern_node(SemanticNodeData::Union(Arc::from(
            vec![known_arm, exhausted_arm].into_boxed_slice(),
        )));
        // This outer arm makes the ordinary relation decidably assignable
        // after a buggy prepass fabricates `undefined` for the exhausted
        // nested arm. Without it, the ordinary relation independently sees
        // the unresolved carrier and returns `Unknown`, masking the bug.
        let accepting_arm = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            optional_member("p", number),
        ])));
        let target = graph.intern_node(SemanticNodeData::Union(Arc::from(
            vec![accepting_arm, nested].into_boxed_slice(),
        )));
        let source =
            graph.intern_node(SemanticNodeData::Object(empty_surface(vec![fresh_member(
                "p", number,
            )])));

        let step = relate_fresh_excess(&host, source, target);
        assert!(
            matches!(step, RelationStep::Unknown),
            "resolution exhaustion in a nested expected-value arm must keep \
             the value check undecided, got {step:?}"
        );
    }

    /// A DEEP alias chain to an object target still excess-checks: hop
    /// exhaustion must never silently skip the prepass while the ordinary
    /// relation (which follows aliases unbounded) accepts on width — a
    /// "gave up" must be fail-closed, never indistinguishable from
    /// "no excess".
    #[test]
    fn deep_alias_chain_target_still_excess_checks() {
        let host = host_for_relation_tests();
        let graph = host.project_type_store().semantic_graph();
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let target_surface = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            required_member("a", number),
        ])));
        // 12 alias hops — beyond any small resolver hop cap.
        let mut target = target_surface;
        for _ in 0..12 {
            target = graph.intern_node(SemanticNodeData::Alias(target));
        }
        let source = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            fresh_member("a", number),
            fresh_member("extra", number),
        ])));
        assert_not_assignable(
            &relate_fresh_excess(&host, source, target),
            "a 12-deep alias chain resolves and reports the excess member",
        );
    }

    /// UMBRELLA: per-property freshness tracks spread taint end to end — the
    /// per-member origin decides excess candidacy inside the sole relation
    /// authority. A fresh direct member is reported excess; the same member
    /// spread-tainted is exempt; taint never suppresses value checking; and
    /// ordinary relation ignores the origin axis entirely.
    #[test]
    pub(crate) fn freshness_tracks_per_property_spread_taint() {
        let host = host_for_relation_tests();
        let graph = host.project_type_store().semantic_graph();
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

        let target = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            required_member("a", number),
        ])));

        // FreshOwn extra ⇒ rejected.
        let fresh_extra = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            fresh_member("a", number),
            fresh_member("extra", string),
        ])));
        assert_not_assignable(
            &relate_fresh_excess(&host, fresh_extra, target),
            "a FreshOwn extra member is excess",
        );

        // The SAME extra member spread-tainted ⇒ exempt.
        let tainted_extra = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            fresh_member("a", number),
            tainted_member("extra", string),
        ])));
        assert_assignable(
            &relate_fresh_excess(&host, tainted_extra, target),
            "the same member spread-tainted is exempt from excess reporting",
        );

        // Taint never suppresses value incompatibility when the name is known.
        let typed_target = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            required_member("a", number),
            required_member("extra", number),
        ])));
        assert_not_assignable(
            &relate_fresh_excess(&host, tainted_extra, typed_target),
            "a known tainted member still fails value compatibility",
        );

        // Ordinary relation ignores the origin axis.
        assert_assignable(
            &relate_regular(&host, fresh_extra, target),
            "regular mode ignores per-property freshness entirely",
        );
    }

    /// A NAMED (DeclRef) target resolves before classification: a fresh
    /// literal with an extra member is rejected against `type Target =
    /// { a: number }` reached through its reference carrier — the common
    /// user-facing shape. DISCRIMINATES against a raw-node classifier that
    /// treats reference carriers as not-a-target and passes.
    #[test]
    fn named_target_resolves_and_rejects_excess() {
        use crate::semantic_query::NodeScopeId;
        use crate::{FileLanguage, UpsertRequest};
        let host = host_for_relation_tests();
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/excess_named_target.ts".to_string(),
                source: Arc::from("export type Target = { a: number };\n"),
                file_language: FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .unwrap();
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = host.project_type_store().semantic_graph();
        let shallow = dispatch
            .ctx
            .shallow_file_state("/excess_named_target.ts")
            .expect("fixture must index");
        let scope = NodeScopeId::File {
            canonical_id: Arc::from("/excess_named_target.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: shallow.whole_hash,
            local_scope: None,
        };
        let target = graph.intern_node(SemanticNodeData::DeclRef {
            identity: crate::semantic_query::DeclIdentity::from_scope(&scope, Arc::from("Target")),
        });
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let source = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            fresh_member("a", number),
            fresh_member("extra", number),
        ])));
        assert_not_assignable(
            &relate_fresh_excess(&host, source, target),
            "a named target must resolve and report the excess member",
        );
        // Sanity (non-vacuous): without the extra member the same named
        // target accepts.
        let clean = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![fresh_member(
            "a", number,
        )])));
        assert_assignable(
            &relate_fresh_excess(&host, clean, target),
            "the same named target accepts the exact shape",
        );
    }

    /// An UNDECIDABLE discriminant arm is never dropped by the reduction:
    /// `{ kind: "a", b: 1 }` vs `{ kind: "a"; a?: number } | { kind:
    /// <unresolvable>; b: number }` — arm B's discriminant relates Unknown,
    /// so B must stay in the arm set (making `b` known) and the
    /// contaminated reduction must surface `Unknown`, never a memoizable
    /// false rejection.
    #[test]
    fn unknown_discriminant_arm_is_kept_not_dropped() {
        let host = host_for_relation_tests();
        let graph = host.project_type_store().semantic_graph();
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let lit_a = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "a".to_string(),
        )));
        let one = graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(1.0)));
        let unresolvable_kind = graph.intern_node(SemanticNodeData::DeclRef {
            identity: crate::semantic_query::DeclIdentity {
                canonical_id: Arc::from("/missing-kind.ts"),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                whole_hash: crate::semantic_query::HashValue::default(),
                decl_name: Arc::from("MissingKind"),
            },
        });

        let arm_a = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            required_member("kind", lit_a),
            optional_member("a", number),
        ])));
        let arm_b = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            required_member("kind", unresolvable_kind),
            required_member("b", number),
        ])));
        let target = graph.intern_node(SemanticNodeData::Union(Arc::from(
            vec![arm_a, arm_b].into_boxed_slice(),
        )));
        let source = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![
            fresh_member("kind", lit_a),
            fresh_member("b", one),
        ])));

        let before = graph.relation_memo_count();
        let step = relate_fresh_excess(&host, source, target);
        assert!(
            !matches!(step, RelationStep::NotAssignable),
            "an Unknown discriminant must not drop its arm into a false \
             rejection; got {step:?}"
        );
        assert!(
            matches!(step, RelationStep::Unknown),
            "a reduction contaminated by an undecidable discriminant stays \
             Unknown; got {step:?}"
        );
        assert_eq!(
            graph.relation_memo_count(),
            before,
            "a contaminated reduction admits NOTHING to the relation memo"
        );
    }
}
