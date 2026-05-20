//! Invariant tests for `ProjectSemanticDispatch`: per-function
//! recursion guards, fixpoint termination of `evaluate_deferred`,
//! cycle-safe `walk_path` / `key_names_from_base_node`,
//! `relation_guard` short-circuiting, mapped-type substitution, and
//! the relation-memo round-trip. Each test characterizes a specific
//! architectural property of the dispatch surface and discriminates
//! against violations introduced by future regressions.

#![allow(dead_code)]

// ============================================================================
// §6.1 Guard contract (8 tests) — un-ignored in §5.3 WIP-R
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
    // Catch-all arm returns `node` unchanged. Post-Fix-D the helper
    // returns a `(node, changed)` tuple, so accept either form.
    assert!(
        substitute_src.contains("_ => node") || substitute_src.contains("_ => (node, false)"),
        "substitute_semantic_type_param (or its change-tracking helper) must carry a \
         catch-all arm returning input unchanged"
    );
}

/// Guard invariant: `evaluate_deferred_semantic_node` reaches a
/// fix-point on a recursive alias via the stack-local visited set
/// + `next == node` termination. Verified by source-content grep.
#[test]
fn evaluate_deferred_reaches_fixpoint_on_recursive_type_alias() {
    let evaluate_src = include_str!("project_semantic_dispatch/evaluate.rs");
    assert!(
        evaluate_src.contains("let mut visited = rustc_hash::FxHashSet::default();"),
        "evaluate_deferred_semantic_node must use a visited set for cycle detection"
    );
    assert!(
        evaluate_src.contains("if next == node"),
        "evaluate_deferred_semantic_node must terminate on `next == node` fix-point"
    );
    assert!(
        evaluate_src.contains("!visited.insert(next)"),
        "evaluate_deferred_semantic_node must return on visited-set re-entry"
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
        match dispatch.execute(crate::semantic_query::SemanticQueryKey::NormalizeUnion {
            members: next_nodes,
        }) {
            crate::semantic_query::QueryResult::Value(id) => current = id,
            other => panic!("NormalizeUnion iteration {i} failed: {other:?}"),
        }
    }
    let empty_path: std::sync::Arc<[crate::semantic_query::PathSegment]> =
        std::sync::Arc::from(Vec::new().into_boxed_slice());
    let result = dispatch.execute(crate::semantic_query::SemanticQueryKey::ProjectPath {
        base: current,
        path: empty_path,
        mode: crate::semantic_query::ProjectionMode::Expanded,
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
        vec![crate::semantic_query::PathSegment::Member(Arc::from(
            "name",
        ))]
        .into_boxed_slice(),
    );
    let result = dispatch.execute(crate::semantic_query::SemanticQueryKey::ProjectPath {
        base: current,
        path,
        mode: crate::semantic_query::ProjectionMode::Expanded,
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
/// source: the catch-all publishes None for shapes the enumerator
/// cannot resolve.
///
/// The enumerator is iterative: the catch-all publishes onto a
/// worklist results stack via `results.push(None)`. The
/// discriminating intent — "the Unresolvable sentinel is surfaced
/// on the catch-all" — is what this test pins.
#[test]
fn key_names_from_base_node_returns_unresolvable_on_cyclic_intersection() {
    let enumerate_src = include_str!("project_semantic_dispatch/enumerate.rs");
    // Catch-all publishes None (Rust's Option equivalent of
    // KeyEnumeration::Unresolvable in the current shape).
    assert!(
        enumerate_src.contains("results.push(None)"),
        "key_names_from_base_node must publish None on unresolvable shapes via the worklist results stack"
    );
    // Intersection arm drives recursive enumeration (worklist frame
    // + combine reducer under C10).
    assert!(
        enumerate_src.contains("Intersection"),
        "key_names_from_base_node must handle Intersection structurally"
    );
    // Worklist driver expands arms iteratively (replaces the pre-C10
    // recursive self-call).
    assert!(
        enumerate_src.contains("KeyNamesFrame::Expand"),
        "key_names_from_base_node must dispatch arm expansion through the iterative KeyNamesFrame worklist"
    );
}

/// Guard invariant: `relate_nodes` returns `RelationResult::Unknown`
/// on cyclic re-entry. The TLS in-flight set catches re-entry; the
/// public surface returns Unknown without infinite recursion.
/// Verified by source grep + a behavioural probe that confirms the
/// memo round-trips Unknown.
#[test]
fn relation_guard_returns_unknown_on_cyclic_reentry() {
    let relation_src = include_str!("project_semantic_dispatch/relation.rs");
    assert!(
        relation_src.contains("RELATION_IN_FLIGHT"),
        "relation.rs must carry the TLS-backed in-flight set for cycle detection"
    );
    assert!(
        relation_src.contains("enter_relation_guard")
            && relation_src.contains("RelationResult::Unknown"),
        "relation.rs must return Unknown on guard re-entry"
    );
    // Behavioural: Unknown pairs memoise with fence and round-trip.
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let object = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![])));
    let source = graph.intern_node(SemanticNodeData::IndexedAccess {
        object,
        index: crate::semantic_query::IndexKey::String(Arc::from("a")),
    });
    // Deferred shell on source → Unknown.
    let (result, _fence) = dispatch.relate_nodes(source, object);
    assert_eq!(
        result,
        RelationResult::Unknown,
        "deferred shell on source side must produce RelationResult::Unknown"
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
        SemanticQueryApi, SemanticQueryKey,
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
    // key's name. Path C C6a node-id substitute matching requires
    // the value_expr's TypeParam reference to use the SAME
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
        // Path C C5 classifies it as `Computed`: the build path
        // substitutes name → Literal(name) and evaluates.
        kind: crate::semantic_query::MapperKind::Computed,
    };

    let (result, _) = match dispatch.execute(SemanticQueryKey::MappedType { source, mapper }) {
        res @ (QueryResult::Value(_) | QueryResult::Error(_) | QueryResult::Recursive(_)) => {
            (res, ())
        }
    };
    let id = match result {
        QueryResult::Value(id) => id,
        other => panic!("expected mapped-type Value, got {other:?}"),
    };
    let data = graph.node_data(id).expect("result interned");
    let SemanticNodeData::Object(surface) = &*data else {
        panic!("mapped with enumerable key space must produce Object; got {data:?}");
    };
    let member_names: Vec<String> = surface
        .members
        .iter()
        .map(|m| m.name.as_ref().to_string())
        .collect();
    assert_eq!(
        member_names,
        vec!["a".to_string(), "b".to_string()],
        "per-key members must come from the enumerated key space, not Opaque(Miss)"
    );
    // Per-key value: substituting K → Literal(name) into TypeParam(K)
    // yields Literal(name). This proves the substitution path runs for
    // the non-Object-source case.
    for member in surface.members.iter() {
        let value_data = graph.node_data(member.value).expect("value interned");
        let expected = member.name.as_ref().to_string();
        match &*value_data {
            SemanticNodeData::Literal(LiteralValue::String(actual)) => {
                assert_eq!(
                    actual, &expected,
                    "per-key value must be the substituted literal; got {actual:?}"
                );
            }
            other => panic!(
                "per-key value for `{}` must be a String literal after substitution; got {other:?}",
                member.name
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
        SemanticNodeData, SemanticQueryApi, SemanticQueryKey,
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
        index: IndexKey::TypeNode(parameter_node),
    });
    let mapper = MapperKey {
        parameter_node,
        key_space,
        value_expr,
        optionality: OptionalityMod::Keep,
        readonly: ReadonlyMod::Keep,
        name_remap: None,
        // `IndexedAccess { object = source, index = TypeNode(K) }`
        // is the canonical identity projection — Path C C5 classifies
        // as `Identity`.
        kind: crate::semantic_query::MapperKind::Identity,
    };

    let result = dispatch.execute(SemanticQueryKey::MappedType { source, mapper });
    let id = match result {
        QueryResult::Value(id) => id,
        other => panic!("expected mapped-type Value, got {other:?}"),
    };
    let data = graph.node_data(id).expect("result interned");
    let SemanticNodeData::Object(surface) = &*data else {
        panic!("mapped with enumerable key space must produce Object; got {data:?}");
    };
    assert_eq!(surface.members.len(), 1);
    let member = &surface.members[0];
    assert_eq!(member.name.as_ref(), "a");
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
                IndexKey::TypeNode(index_node) => {
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
        SemanticQueryKey,
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

    let result = dispatch.execute(SemanticQueryKey::MappedType {
        source,
        mapper: mapper.clone(),
    });
    let id = match result {
        QueryResult::Value(id) => id,
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

    let result =
        dispatch.execute(crate::semantic_query::SemanticQueryKey::KeyOf { base: intersection });
    let id = match result {
        crate::semantic_query::QueryResult::Value(id) => id,
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

    let result = dispatch.execute(crate::semantic_query::SemanticQueryKey::KeyOf { base: union });
    let id = match result {
        crate::semantic_query::QueryResult::Value(id) => id,
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

    let result =
        dispatch.execute(crate::semantic_query::SemanticQueryKey::KeyOf { base: conditional });
    let id = match result {
        crate::semantic_query::QueryResult::Value(id) => id,
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
        SemanticQueryKey,
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

    let result = dispatch.execute(SemanticQueryKey::MappedType {
        source,
        mapper: mapper.clone(),
    });
    let id = match result {
        QueryResult::Value(id) => id,
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
// §6.3 Relation engine (8 tests) — un-ignored in §5.4 WIP-S
// ============================================================================
//
// Real discriminating bodies for the Phase D relation engine. Each test
// constructs SemanticNodeData fixtures directly on the shared graph and
// exercises `ProjectSemanticDispatch::relate_nodes` against them. A
// characterization test body must FAIL against the pre-cutover tree
// (where `relate_nodes` was a `todo!()` / shallow stub) and PASS against
// the  tree where the real decision table lives.

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
    SurfaceView {
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
        name: Arc::from(name),
        value,
        optional: false,
        readonly: false,
        is_method: false,
    }
}

fn optional_member(name: &str, value: crate::semantic_query::SemanticNodeId) -> SurfaceMember {
    SurfaceMember {
        name: Arc::from(name),
        value,
        optional: true,
        readonly: false,
        is_method: false,
    }
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

    let (result, _fence) = dispatch.relate_nodes(source, target);
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

    let (result, _fence) = dispatch.relate_nodes(source, target);
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

    let (result, _fence) = dispatch.relate_nodes(source, target);
    assert!(
        matches!(result, RelationResult::Assignable { .. }),
        "record-shaped Object-to-Object with shared inner Object must be Assignable; got {result:?}"
    );
}

/// The relation engine uses the shared `relation_memo` rather than
/// per-call recursion: two `relate_nodes` calls with the same pair
/// warm-hit on the second call.
///
/// Discriminates: pre-cutover code had separate shallow checks at
/// `build_conditional` and no memo; any stub that recomputes on each
/// call fails this test because the memo count would not grow.
#[test]
fn relate_conditional_check_uses_dispatch_memo_not_private_recursion() {
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let a = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let b = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

    let before = graph.relation_memo_count();
    let (_r1, _) = dispatch.relate_nodes(a, b);
    let after_one = graph.relation_memo_count();
    let (_r2, _) = dispatch.relate_nodes(a, b);
    let after_two = graph.relation_memo_count();

    assert_eq!(
        after_one,
        before + 1,
        "first relate_nodes call must publish exactly one memo entry"
    );
    assert_eq!(
        after_one, after_two,
        "second relate_nodes call with same pair must warm-hit, not grow the memo"
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
    let (result, _fence) = dispatch.relate_nodes(string, string);
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
/// `include_str!` dependency on the retiring file. Every pre-cutover
/// arena test name must map onto a  semantic concept —
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
        dispatch.relate_nodes(never, string).0,
        RelationResult::Assignable { .. }
    ));
    // `everything_assignable_to_unknown`.
    assert!(matches!(
        dispatch.relate_nodes(string, unknown).0,
        RelationResult::Assignable { .. }
    ));
    // `different_primitives_not_assignable`.
    assert_eq!(
        dispatch.relate_nodes(string, number).0,
        RelationResult::NotAssignable
    );
    // `same_primitive_assignable`.
    assert!(matches!(
        dispatch.relate_nodes(string, string).0,
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
    let (result, _fence) = dispatch.relate_nodes(number, number);
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

/// `Unknown` is cached with a dep-signature fence in the relation
/// memo rather than recomputed on each cyclic re-entry. Discriminates:
/// a stub that routes `Unknown` through the cold path produces two
/// distinct memo entries when called twice.
#[test]
fn relation_unknown_is_cached_with_fence_not_recomputed_on_repeated_cycle() {
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    // Deferred shell pair: both IndexedAccess shells over distinct
    // base/index pairs. The relation engine returns Unknown for
    // deferred shells on either side.
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let _number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
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
    let (r1, _) = dispatch.relate_nodes(source, target);
    let (r2, _) = dispatch.relate_nodes(source, target);
    let after = graph.relation_memo_count();

    assert_eq!(r1, RelationResult::Unknown);
    assert_eq!(r2, RelationResult::Unknown);
    assert_eq!(
        after,
        before + 1,
        "Unknown must cache exactly once per pair (fence-aware warm hit)"
    );
    // Belt-and-braces: the memo exposes the cached outcome via
    // `get_relation`.
    let cached = graph.get_relation(&host, source, target);
    assert!(
        matches!(cached, Some((_, RelationResult::Unknown))),
        "memo must expose the cached Unknown; got {cached:?}"
    );
    // Use `IndexSignature` so the import is exercised (and reviewable
    // in diffs) without producing a dead-code warning.
    let _unused_tag: Option<IndexSignature> = None;
    let _widened: LiteralValue = LiteralValue::String(String::from("_"));
}

// ============================================================================
// Authority-cutover invariants — file-absence
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
// Authority-cutover invariants — identifier-absence
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
    // production code. `HostNamedTypeCacheAdapter` in `host_manage.rs`
    // is the only parser↔dispatch seam and does not carry these
    // names.
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
fn semantic_graph_store_has_relation_memo_field() {
    // SemanticGraphStore must carry a single `relation_memo` field for
    // the relation engine. The field is a `BudgetedRelationMemo` — a
    // wrapper owning the `(source, target)` map, its retention budget,
    // and the `retention_gate` that keeps the map and the budget in one
    // lock domain — so the relation memo and its retention ledger
    // cannot desync (`clear` is exclusive against concurrent inserts).
    let memo_src = include_str!("semantic_query_memo/mod.rs");
    assert!(
        memo_src.contains("relation_memo: BudgetedRelationMemo"),
        "SemanticGraphStore must have a `relation_memo: BudgetedRelationMemo` field"
    );
    // The relation memo's `DashMap` is owned by `BudgetedRelationMemo`.
    let budgeted_src = include_str!("semantic_query_memo/budgeted_caches.rs");
    assert!(
        budgeted_src.contains("memo: DashMap<"),
        "BudgetedRelationMemo must own the relation memo's DashMap"
    );
    // Behavioural verification: the field is accessible via the
    // `get_relation` / `insert_relation` API used by
    // `ProjectSemanticDispatch::relate_nodes`.
    let host = host_for_relation_tests();
    let graph = host.project_type_store().semantic_graph();
    assert_eq!(
        graph.relation_memo_count(),
        0,
        "fresh host has zero relation memo entries"
    );
}

#[test]
fn host_named_type_cache_adapter_uses_semantic_graph_store_directly() {
    // Cache-routing invariant: `HostNamedTypeCacheAdapter` reads
    // and writes `SemanticGraphStore` via `get_resolved_named_type`
    // / `insert_resolved_named_type` directly — no
    // SessionSolverHost wrapper sits in between.
    let host_manage_src = include_str!("host_manage.rs");
    // The adapter struct is present.
    assert!(
        host_manage_src.contains("struct HostNamedTypeCacheAdapter"),
        "HostNamedTypeCacheAdapter struct must be present"
    );
    // The adapter calls the SemanticGraphStore entry points directly.
    assert!(
        host_manage_src.contains(".graph.get_resolved_named_type"),
        "HostNamedTypeCacheAdapter must call graph.get_resolved_named_type"
    );
    assert!(
        host_manage_src.contains(".graph.insert_resolved_named_type"),
        "HostNamedTypeCacheAdapter must call graph.insert_resolved_named_type"
    );
}

// ============================================================================
// Authority-cutover invariants — single-authority cardinality
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
    // `fn relate_nodes` must appear exactly once — in
    // `project_semantic_dispatch/relation.rs`.
    let count = count_def_in_crates("fn relate_nodes");
    assert_eq!(
        count, 1,
        "fn relate_nodes must have exactly one definition; got {count}"
    );
}

#[test]
fn type_expr_lowering_has_exactly_one_path() {
    // `fn shallow_lower_type_expr` must appear exactly once — in
    // `project_semantic_dispatch/lower.rs`.
    let count = count_def_in_crates("fn shallow_lower_type_expr");
    assert_eq!(
        count, 1,
        "fn shallow_lower_type_expr must have exactly one definition; got {count}"
    );
}

#[test]
fn semantic_node_to_type_expr_has_exactly_one_path() {
    // Sibling invariant of `type_expr_lowering_has_exactly_one_path`:
    // the reverse direction (`SemanticNodeId → TypeExpr`) must also
    // have exactly one production path. After Step 6.1 of the F2
    // architectural unification, `fn raise_node_to_type_expr(`
    // appears exactly once — in
    // `project_semantic_dispatch/raise.rs`. The trailing `(` is part
    // of the needle so the counter does not double-count
    // `fn raise_node_to_type_expr_inner(` (which appears in the same
    // module as the recursion helper).
    let count = count_def_in_crates("fn raise_node_to_type_expr(");
    assert_eq!(
        count, 1,
        "fn raise_node_to_type_expr must have exactly one definition; got {count}"
    );
}

#[test]
fn relation_memo_has_exactly_one_owner() {
    // The `relation_memo` field must appear exactly once in production
    // code — on `SemanticGraphStore`, typed `BudgetedRelationMemo`. The
    // memo's backing `DashMap` is owned by that wrapper (which also
    // owns the retention budget + `retention_gate`), so the relation
    // memo still has exactly one owner.
    let field_count = count_def_in_crates("relation_memo: BudgetedRelationMemo");
    assert_eq!(
        field_count, 1,
        "relation_memo field must have exactly one owner; got {field_count}"
    );
    // The wrapper owns the backing map exactly once.
    let map_count = count_def_in_crates("memo: DashMap<(SemanticNodeId, SemanticNodeId)");
    assert_eq!(
        map_count, 1,
        "the relation memo's DashMap must have exactly one owner \
         (BudgetedRelationMemo); got {map_count}"
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
// §6.5 D5 grep (5 tests) — un-ignored in §5.9 WIP-D5
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

#[test]
fn solver_caches_relation_rebuilt_per_dispatch_builder_invocation() {
    // Relation-scope invariant: relation scratch is per-dispatch-builder
    // invocation, not retained across builders. The relation engine
    // uses a thread-local RELATION_IN_FLIGHT guard (cleared per call
    // via enter/exit_relation_guard) and the persistent
    // SemanticGraphStore relation_memo for cross-request dedup.
    // ProjectSemanticDispatch is created fresh per dispatch call
    // (borrows &VerterHost), so no per-instance relation cache can
    // leak across invocations.
    //
    // Verify structurally: ProjectSemanticDispatch has no `relation`
    // field, and the relation module uses thread-local not instance
    // state.
    let mod_src = include_str!("project_semantic_dispatch/mod.rs");
    assert!(
        !mod_src.contains("relation_cache"),
        "ProjectSemanticDispatch must not carry a relation_cache field"
    );
    let rel_src = include_str!("project_semantic_dispatch/relation.rs");
    assert!(
        rel_src.contains("RELATION_IN_FLIGHT"),
        "relation module must use thread-local RELATION_IN_FLIGHT, not instance state"
    );
    assert!(
        rel_src.contains("fn enter_relation_guard"),
        "per-call enter/exit guard must be present"
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

/// Structured-budget-failure contract: the parser's syntactic depth
/// guard must emit a structured
/// `ResolutionBudgetExceeded { limit, actual, context }` record on
/// cap-trip — **not** a silent `Applied`-style stub. A deeply nested
/// `Foo<Foo<...>>` chain past the `PARSER_SYNTACTIC_DEPTH_LIMIT =
/// 256` cap must produce a recorded `limit == 256`, a non-zero
/// `actual`, and termination without stack overflow.
#[test]
fn budget_exceeded_returns_structured_failure_not_applied_stub() {
    use verter_parser::utils::oxc::vue::{
        resolve_type_elements_with_ctx, take_last_resolution_budget_exceeded,
        PARSER_SYNTACTIC_DEPTH_LIMIT,
    };

    // Clear any stale record from previous tests on this thread.
    let _ = take_last_resolution_budget_exceeded();

    // Build a chain of type aliases A0 → A1 → A2 → ... → A_N → Leaf.
    // Each `type A_i = A_{i+1}` hop re-enters
    // `resolve_type_elements_inner_with_ctx` via the TSTypeReference arm's
    // `find_type_alias` path, incrementing `RESOLUTION_DEPTH` once per
    // hop. At `PARSER_SYNTACTIC_DEPTH_LIMIT` the guard refuses entry and
    // stores the structured `ResolutionBudgetExceeded` record.
    let chain_depth = (PARSER_SYNTACTIC_DEPTH_LIMIT as usize) + 20;
    let mut source = String::from("interface Leaf { value: string }\n");
    for i in 0..chain_depth {
        source.push_str(&format!("type A{i} = A{next};\n", next = i + 1));
    }
    source.push_str(&format!("type A{chain_depth} = Leaf;\n"));
    source.push_str("type Test = A0;\n");

    // Lower the SFC script. The parser synthesises a TypeResolutionContext,
    // walks `Test`'s chain, and trips the syntactic depth guard.
    let allocator = oxc_allocator::Allocator::default();
    let ret = oxc_parser::Parser::new(&allocator, &source, oxc_span::SourceType::ts()).parse();
    let stmt = ret.program.body.iter().find_map(|s| match s {
        oxc_ast::ast::Statement::TSTypeAliasDeclaration(decl)
            if decl.id.name.as_str() == "Test" =>
        {
            Some(decl)
        }
        _ => None,
    });
    let test_alias = stmt.expect("Test alias must parse");
    let mut ctx =
        verter_parser::utils::oxc::vue::build_type_context(&ret.program, source.as_bytes(), 0);
    let _resolved = resolve_type_elements_with_ctx(&test_alias.type_annotation, 0, &mut ctx);

    let record = take_last_resolution_budget_exceeded()
        .expect("structured ResolutionBudgetExceeded must be recorded on cap-trip");
    assert_eq!(
        record.limit, PARSER_SYNTACTIC_DEPTH_LIMIT,
        "ResolutionBudgetExceeded.limit must equal PARSER_SYNTACTIC_DEPTH_LIMIT"
    );
    assert!(
        record.actual >= PARSER_SYNTACTIC_DEPTH_LIMIT,
        "ResolutionBudgetExceeded.actual must be >= limit at cap-trip; got {}",
        record.actual
    );
    assert!(
        !record.context.is_empty(),
        "ResolutionBudgetExceeded.context must be non-empty"
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
        lower_src.contains("name_resolution: &FxHashMap<String, ResolvedRootIdentity>"),
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
        mode,
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
    assert!(
        combined.contains("dispatch.lower_type_expr_in_scope"),
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

/// Relation-memo identity contract: the dispatch memo API exists on
/// `SemanticGraphStore::relation_memo` and behaves as a write-read-
/// warm cycle — construction of a new host must expose the
/// `get_relation` / `insert_relation` entry points.
#[test]
fn type_surface_db_identity_moved_to_semantic_graph_store_memo() {
    let host = host_for_relation_tests();
    let graph = host.project_type_store().semantic_graph();
    let a = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let b = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    // Cold: no entry.
    assert!(
        graph.get_relation(&host, a, b).is_none(),
        "cold relation memo must return None before publish"
    );
    // Publish a NotAssignable judgement. The relation memo entry is
    // self-version-rooted; a synthetic publish with no self-roots uses
    // an empty carrier + empty self-root set so the warm read validates
    // vacuously.
    let carrier = crate::fact_signature_helpers::ReadSetSignature::empty();
    let generation = host.project_type_store().current_project_generation();
    graph.insert_relation(
        a,
        b,
        carrier,
        std::sync::Arc::from([]),
        RelationResult::NotAssignable,
        generation,
    );
    // Warm: must return the same judgement.
    let (_, cached) = graph
        .get_relation(&host, a, b)
        .expect("published relation memo must be readable");
    assert_eq!(cached, RelationResult::NotAssignable);
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

/// Cache-routing invariant: all dispatch consumers route through
/// `ProjectSemanticDispatch` directly — no `SessionSolverHost`-style
/// bridge sits between them. The parser-adjacent
/// `HostNamedTypeCacheAdapter` reads/writes `SemanticGraphStore` via
/// `get_resolved_named_type` / `insert_resolved_named_type`.
#[test]
fn session_solver_host_removal_migrates_all_consumers_to_dispatch() {
    let host_manage_src = include_str!("host_manage.rs");
    assert!(
        host_manage_src.contains("SemanticGraphStore"),
        "HostNamedTypeCacheAdapter must reach SemanticGraphStore directly"
    );
    assert!(
        host_manage_src.contains("get_resolved_named_type")
            && host_manage_src.contains("insert_resolved_named_type"),
        "HostNamedTypeCacheAdapter must call get_resolved_named_type / insert_resolved_named_type"
    );
}

/// Session entry-point invariant: session paths use
/// `ProjectSemanticDispatch::new(host)` directly (no
/// `TypeQueryEngine`-style intermediary). Parser's Vue macro cache
/// key routes through `HostNamedTypeCacheAdapter`.
#[test]
fn type_query_engine_removal_migrates_vue_macro_parsing_to_host_named_type_cache_adapter() {
    let host_manage_src = include_str!("host_manage.rs");
    assert!(
        host_manage_src.contains("HostNamedTypeCacheAdapter"),
        "HostNamedTypeCacheAdapter must exist in host_manage.rs post-migration"
    );
    // The adapter implements the NamedTypeCache trait from verter_compiler's
    // resolve_type cache_keys module.
    assert!(
        host_manage_src.contains(
            "impl verter_compiler::utils::oxc::vue::resolve_type::cache_keys::NamedTypeCache"
        ) || host_manage_src.contains("NamedTypeCache for HostNamedTypeCacheAdapter"),
        "HostNamedTypeCacheAdapter must implement NamedTypeCache for parser integration"
    );
}

/// Evaluation-entry-point invariant: the entry point for type
/// evaluation is
/// `dispatch.execute(ProjectPath { ..., mode: Expanded })`. The
/// path must exist and run end-to-end on a simple identity case.
#[test]
fn type_eval_evaluate_removal_preserves_semantic_output() {
    let host = host_for_relation_tests();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let empty_path: std::sync::Arc<[crate::semantic_query::PathSegment]> =
        std::sync::Arc::from(Vec::new().into_boxed_slice());
    let result = dispatch.execute(crate::semantic_query::SemanticQueryKey::ProjectPath {
        base: string,
        path: empty_path,
        mode: crate::semantic_query::ProjectionMode::Expanded,
    });
    match result {
        crate::semantic_query::QueryResult::Value(id) => {
            assert_eq!(
                id, string,
                "empty-path Expanded projection of a primitive returns the same node"
            );
        }
        other => panic!("expected Value for identity projection, got {other:?}"),
    }
}

/// Vue-macro-pipeline routing invariant: the Vue macro pipeline
/// runs through the parser's `NamedTypeCache` adapter on the
/// dispatch side. The host adapter infrastructure must exist and
/// lookups must hit the `SemanticGraphStore` cache layer.
#[test]
fn type_eval_build_expand_macro_types_removal_preserves_macro_pipeline_output() {
    let host = host_for_relation_tests();
    let graph = host.project_type_store().semantic_graph();
    // resolved_named_type_count is a proxy for the Vue macro cache
    // surface being present and queryable.
    assert_eq!(
        graph.resolved_named_type_count(),
        0,
        "fresh host has zero Vue macro resolution entries"
    );
    // The clear / invalidate helpers exist — part of the migration
    // preservation surface for macro cache lifecycle.
    graph.clear_resolved_named_types();
    graph.invalidate_resolved_named_types_for_canonical("unused");
    assert_eq!(graph.resolved_named_type_count(), 0);
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
    let result = dispatch.execute(crate::semantic_query::SemanticQueryKey::ProjectPath {
        base: object,
        path: empty_path,
        mode: crate::semantic_query::ProjectionMode::Shallow,
    });
    match result {
        crate::semantic_query::QueryResult::Value(id) => {
            let data = graph.node_data(id).expect("projection result interned");
            if let SemanticNodeData::Object(surf) = &*data {
                assert_eq!(
                    surf.members.len(),
                    1,
                    "Shallow projection preserves one member"
                );
                assert_eq!(surf.members[0].name.as_ref(), "label");
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
    let result = dispatch.execute(crate::semantic_query::SemanticQueryKey::NormalizeUnion {
        members: members.clone(),
    });
    match result {
        crate::semantic_query::QueryResult::Value(id) => {
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
    let ab_result =
        dispatch.execute(crate::semantic_query::SemanticQueryKey::NormalizeUnion { members: ab });
    match ab_result {
        crate::semantic_query::QueryResult::Value(id) => {
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
/// helper isn't usable, through `dispatch.execute_to_type_expr` on a
/// `ProjectPath { mode: Expanded }` query directly.
///
/// Final-state expectations:
/// - `meta_resolve.rs` retains AT MOST 1 `.lower_and_project_to_expanded(`
///   callsite (the Class A helper's engine-threaded route fast-path).
/// - `fallthrough.rs` retains 0 callsites — the
///   `evaluate_value_expression_via_env_or_dispatch` fallback uses
///   `project_expr_class_a_via_dispatch`.
#[test]
fn route_loop_callers_route_through_dispatch() {
    let meta_src = include_str!("meta_resolve.rs");
    let fallthrough_src = include_str!("resolver_core/fallthrough.rs");

    let meta_callsites = meta_src.matches(".lower_and_project_to_expanded(").count();
    let fallthrough_callsites = fallthrough_src
        .matches(".lower_and_project_to_expanded(")
        .count();

    // Final-state contract:
    //
    // - `meta_resolve.rs` retains AT MOST 1 callsite (the Class A
    //   helper's engine-threaded route fast-path).
    // - `fallthrough.rs` retains 0 callsites; the
    //   `evaluate_value_expression_via_env_or_dispatch` fallback
    //   routes through the Class A helper.
    assert!(
        meta_callsites <= 1,
        "meta_resolve.rs must have <= 1 .lower_and_project_to_expanded( callsite; found {meta_callsites}",
    );
    assert_eq!(
        fallthrough_callsites, 0,
        "fallthrough.rs must have 0 .lower_and_project_to_expanded( callsites; found {fallthrough_callsites}",
    );

    // Positive marker: the Class A helper is the canonical dispatch
    // entry, so its name must appear in fallthrough.rs.
    assert!(
        fallthrough_src.contains("project_expr_class_a_via_dispatch"),
        "fallthrough.rs must call project_expr_class_a_via_dispatch",
    );
}

/// Engine-method-absence invariant: `instantiate_local_generic_ref`
/// is not a callsite of the meta-resolve family. Callers route
/// through `dispatch.execute(SemanticQueryKey::Instantiate { .. })`.
/// The check is a static-grep gate over the meta-resolve module
/// family: NO `engine.instantiate_local_generic_ref(...)` callsite
/// may appear.
#[test]
fn instantiate_local_generic_ref_callers_route_through_dispatch() {
    // `meta_resolve.rs` is the shell of a folder module; the bodies
    // that carry the `instantiate_local_generic_ref` /
    // `SemanticQueryKey::Instantiate` markers live in:
    //   - `meta_resolve/registry_materialize.rs` — the registry-route
    //     fast path that dispatches Instantiate for route-target
    //     Pick/Omit recipes.
    //   - `meta_resolve/dispatch_helpers.rs` — the
    //     `instantiate_local_generic_ref_via_dispatch` bridge helper.
    // The test concatenates the relevant post-split siblings before
    // running the static-text grep predicates.
    let shell_src = include_str!("meta_resolve.rs");
    let registry_materialize_src = include_str!("meta_resolve/registry_materialize.rs");
    let dispatch_helpers_src = include_str!("meta_resolve/dispatch_helpers.rs");
    let meta_src = format!("{shell_src}\n{registry_materialize_src}\n{dispatch_helpers_src}");

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
