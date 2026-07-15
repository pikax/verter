use super::*;
use std::sync::Arc;
use verter_type_expr::{
    LiteralValue, PrimitiveName, RecursiveConditionalBranch, RecursiveConditionalFrame, TypeExpr,
};

/// Own-once + first-encounter-order invariant for the string / node
/// interners (the encode-path de-duplication): each interned string is
/// stored EXACTLY once in `strings`, each structurally-distinct wire node
/// exactly once in `nodes`, ids are 1-based in first-encounter order, and a
/// repeated intern returns the same id WITHOUT growing the owning table.
/// `into_tables` then yields those owning tables verbatim — the equivalence
/// the finalization by-value move relies on.
///
/// Discriminating: a reverse index that failed to dedup (or mishandled a
/// hash collision) would return a fresh id on a repeat and grow the table —
/// both asserted absent; an id-order regression flips the pinned sequence.
#[test]
fn interner_owns_each_string_and_node_once_in_first_encounter_order() {
    let mut builder = GraphBuilder::new();

    // Strings: first-encounter order, 1-based, dedup on repeat.
    assert_eq!(builder.string_id("alpha"), 1);
    assert_eq!(builder.string_id("beta"), 2);
    assert_eq!(
        builder.string_id("alpha"),
        1,
        "a repeated string must dedup to the same id"
    );
    assert_eq!(builder.string_id("gamma"), 3);
    assert_eq!(
        builder.string_id("beta"),
        2,
        "a repeated string must dedup to the same id"
    );
    assert_eq!(
        builder.strings(),
        ["alpha".to_string(), "beta".to_string(), "gamma".to_string()],
        "each string is owned exactly once, in first-encounter order",
    );

    // Nodes: two structurally-equal exprs held in distinct values collapse
    // onto one interned wire node.
    let first = TypeExpr::Primitive(PrimitiveName::String);
    let structurally_equal = TypeExpr::Primitive(PrimitiveName::String);
    let id_first = builder.node_id(&first);
    let strings_len = builder.strings().len();
    let nodes_len = builder.nodes().len();
    let id_equal = builder.node_id(&structurally_equal);
    assert_eq!(
        id_first, id_equal,
        "structurally-identical nodes share one node id"
    );
    assert_eq!(
        builder.nodes().len(),
        nodes_len,
        "a structural-dedup hit must not grow the node table",
    );
    assert_eq!(
        builder.strings().len(),
        strings_len,
        "a node dedup hit must not add strings",
    );
    assert_eq!(
        builder
            .nodes()
            .iter()
            .filter(|node| matches!(node, GraphNode::Primitive { .. }))
            .count(),
        1,
        "the primitive wire node is owned exactly once",
    );

    // `into_tables` returns the interned tables verbatim (own-once move
    // fidelity): the finalization path moves these straight onto the wire.
    let strings_snapshot = builder.strings().to_vec();
    let nodes_snapshot = builder.nodes().to_vec();
    let (strings, nodes) = builder.into_tables();
    assert_eq!(
        strings, strings_snapshot,
        "into_tables yields the interned string table verbatim"
    );
    assert_eq!(
        nodes, nodes_snapshot,
        "into_tables yields the interned node table verbatim"
    );
}

#[test]
fn graph_builder_encodes_recursive_ref_not_unknown() {
    let expr = TypeExpr::RecursiveRef {
        name: std::sync::Arc::from("Tree"),
        type_arguments: std::sync::Arc::from(vec![TypeExpr::Primitive(PrimitiveName::String)]),
        conditional_context: std::sync::Arc::from(vec![RecursiveConditionalFrame {
            branch: RecursiveConditionalBranch::True,
            decided: true,
            check: std::sync::Arc::new(TypeExpr::named("T")),
            extends: std::sync::Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
        }]),
    };

    let mut builder = GraphBuilder::new();
    let node_id = builder.node_id(&expr);
    let nodes = builder.nodes();
    let node = &nodes[(node_id - 1) as usize];

    assert!(
        matches!(node, GraphNode::RecursiveRef { .. }),
        "graph builder must produce GraphNode::RecursiveRef, got {:?}",
        std::mem::discriminant(node)
    );

    if let GraphNode::RecursiveRef {
        name,
        type_arguments,
        conditional_context,
    } = node
    {
        assert!(*name > 0, "name string ID should be set");
        assert_eq!(type_arguments.len(), 1, "should have 1 type argument");
        assert_eq!(
            conditional_context.len(),
            1,
            "should have 1 conditional frame"
        );
        assert_eq!(conditional_context[0].branch, 1, "branch=true should be 1");
        assert!(conditional_context[0].decided);
    }
}

/// Pins the INTENTIONAL constructor-type wire erasure: a
/// `TypeExpr::ConstructorType` serialises to a `GraphNode::Function` (the
/// closed `GraphTypeNode.kind` taxonomy has no dedicated constructor kind).
/// The constructor-vs-function distinction is consumed in the session
/// semantic dispatch before serialisation, so
/// function-like is the contract-correct wire shape.
///
/// Discriminating in two directions:
///
/// * A builder that left the constructor type unhandled (or emitted some
///   non-function node) fails the `GraphNode::Function` assertion, and the
///   byte-equal check fails if the constructor type ever diverged from the
///   same-payload function wire shape.
/// * The memo layer stays distinct: `ExprMemoKey::from_expr` of a
///   constructor type must NOT equal that of a function carrying the same
///   `Arc<FunctionExpr>` — if the `ConstructorType` memo arm were ever
///   collapsed into `Self::Function`, this assertion fails. (The final wire
///   *node id* legitimately dedups, because byte-identical `GraphNode`s
///   share a node-id slot — the erasure is wire-shape-identical by design;
///   the test pins that fact rather than asserting the opposite.)
#[test]
fn constructor_type_serialises_to_function_wire_node() {
    use verter_type_expr::{FunctionExpr, FunctionParam};

    // `new (x: string) => Foo` — one named param + a ref return.
    let function = Arc::new(FunctionExpr::synthetic(
        vec![FunctionParam::synthetic(
            Some("x".to_string()),
            TypeExpr::Primitive(PrimitiveName::String),
            false,
            false,
        )],
        Some(Arc::new(TypeExpr::named("Foo"))),
        Vec::new(),
    ));
    let ctor = TypeExpr::ConstructorType(Arc::clone(&function));

    let mut builder = GraphBuilder::new();
    let ctor_id = builder.node_id(&ctor);
    let nodes = builder.nodes();
    let node = &nodes[(ctor_id - 1) as usize];

    // (1) The wire node is a Function (erasure) carrying the parameter.
    match node {
        GraphNode::Function { parameters, .. } => {
            assert_eq!(
                parameters.len(),
                1,
                "constructor-type parameter must survive the function-node erasure",
            );
        }
        other => panic!(
            "constructor type must serialise to GraphNode::Function (intentional \
             erasure), got {:?}",
            std::mem::discriminant(other)
        ),
    }

    // (2) The wire node is BYTE-IDENTICAL to a plain function with the SAME
    // payload — the erasure is structural, not just same-discriminant.
    let plain = TypeExpr::Function(Arc::clone(&function));
    let mut fn_builder = GraphBuilder::new();
    let fn_id = fn_builder.node_id(&plain);
    let fn_node = fn_builder.nodes()[(fn_id - 1) as usize].clone();
    assert_eq!(
        node, &fn_node,
        "constructor type and same-payload function must produce the same wire \
         node (GraphNode::Function) — the erasure is wire-shape-identical",
    );

    // (3) The MEMO key stays distinct so a constructor type and a function
    // carrying the same `Arc<FunctionExpr>` never share an `expr_ids` entry.
    // This is the invariant the dedicated `ExprMemoKey::ConstructorType`
    // variant exists to enforce: collapsing it into `Self::Function` would
    // make these equal and is a cache-collision bug.
    assert_ne!(
        ExprMemoKey::from_expr(&ctor),
        ExprMemoKey::from_expr(&plain),
        "ExprMemoKey::ConstructorType must stay distinct from \
         ExprMemoKey::Function for the same Arc<FunctionExpr>",
    );

    // (4) The final wire NODE id legitimately dedups: because the two wire
    // nodes are byte-identical (claim 2), `node_ids` collapses them onto one
    // slot. The erasure is wire-shape-identical by design — pin that fact so
    // a future change that diverged the shapes (re-introducing a distinct
    // node id) is caught and re-justified here.
    let mut shared_builder = GraphBuilder::new();
    let ctor_node_id = shared_builder.node_id(&ctor);
    let fn_node_id = shared_builder.node_id(&plain);
    assert_eq!(
        ctor_node_id, fn_node_id,
        "byte-identical constructor/function wire nodes must share one node id \
         (wire erasure is shape-identical)",
    );
}

#[test]
fn ptr_cache_fast_path_avoids_memo_key_on_pointer_based_variants() {
    // Build an expression tree with pointer-based variants (Union, Object,
    // Function, IndexedAccess, Array) and verify that repeat lookups hit
    // the ptr cache without building ExprMemoKey or GraphNode.
    let inner_obj = TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
        properties: vec![],
    }));
    let union = TypeExpr::Union(Arc::from(vec![
        inner_obj.clone(),
        TypeExpr::Primitive(PrimitiveName::String),
    ]));
    let array = TypeExpr::Array {
        element: Arc::new(union.clone()),
        readonly: false,
    };

    let mut builder = GraphBuilder::new();

    // First pass: builds everything.
    let id1 = builder.node_id(&array);
    let builds_after_first = builder.debug_graph_node_build_count();
    assert!(
        builds_after_first > 0,
        "first pass should build graph nodes"
    );
    assert_eq!(
        builder.debug_expr_ptr_cache_hits(),
        0,
        "first pass should have zero ptr cache hits"
    );

    // Second pass: should hit the ptr cache for the top-level Array.
    let id2 = builder.node_id(&array);
    assert_eq!(id1, id2, "same expression must return same node id");
    assert_eq!(
        builder.debug_expr_ptr_cache_hits(),
        1,
        "second lookup on a pointer-based variant should hit the ptr cache"
    );
    assert_eq!(
        builder.debug_graph_node_build_count(),
        builds_after_first,
        "ptr cache hit should not build any new graph nodes"
    );

    // Also verify that value-based variants (Literal) still work correctly.
    let lit = TypeExpr::Literal(LiteralValue::String("hello".to_string()));
    let lit_id1 = builder.node_id(&lit);
    let lit_id2 = builder.node_id(&lit);
    assert_eq!(
        lit_id1, lit_id2,
        "value-based variant should still deduplicate via ExprMemoKey"
    );
}

#[test]
fn graph_builder_reuses_same_expr_reference_without_rewalking_subgraph() {
    let shared = TypeExpr::IndexedAccess {
        object: Arc::new(TypeExpr::named("Accordion")),
        index: Arc::new(TypeExpr::Literal(LiteralValue::String("slots".to_string()))),
    };
    let expr = TypeExpr::Array {
        element: Arc::new(TypeExpr::Union(Arc::from(vec![shared.clone(), shared]))),
        readonly: false,
    };

    let mut builder = GraphBuilder::new();
    let first_id = builder.node_id(&expr);
    let builds_after_first = builder.debug_graph_node_build_count();

    let second_id = builder.node_id(&expr);
    let builds_after_second = builder.debug_graph_node_build_count();

    assert_eq!(
        first_id, second_id,
        "same expression should reuse one node id"
    );
    assert_eq!(
        builds_after_second, builds_after_first,
        "repeat node_id() on the same expression should hit the front cache instead of rebuilding the graph node"
    );
}

/// The graph wire is a public surface and `GraphObjectMember` carries no
/// visibility field, so a non-public class member must never be encoded onto
/// it. The object-node sanitizer drops non-public Property / Method members
/// (recursively, since nested member value types serialize through the same
/// path); index signatures (no accessibility) are kept.
///
/// Discrimination: FAILS on a tree where the object-node builder encodes
/// every member — the protected `b` / private `c` members (and the nested
/// private member) would appear in the emitted `GraphNode::Object` member
/// list.
#[test]
fn object_node_wire_omits_non_public_members() {
    use verter_type_expr::{
        FunctionExpr, MemberVisibility, MethodSignature, ObjectExpr, ObjectMember, ObjectProperty,
    };

    // Inner object surface with a non-public member, used as the value type
    // of the public outer member `a` — exercises recursive sanitisation.
    let inner = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![
            ObjectMember::Property(ObjectProperty::with_visibility(
                "pub_inner".to_string(),
                TypeExpr::Primitive(PrimitiveName::String),
                false,
                false,
                MemberVisibility::Public,
                Default::default(),
            )),
            ObjectMember::Property(ObjectProperty::with_visibility(
                "priv_inner".to_string(),
                TypeExpr::Primitive(PrimitiveName::Number),
                false,
                false,
                MemberVisibility::Private,
                Default::default(),
            )),
        ],
    }));

    let outer = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![
            ObjectMember::Property(ObjectProperty::with_visibility(
                "a".to_string(),
                inner,
                false,
                false,
                MemberVisibility::Public,
                Default::default(),
            )),
            ObjectMember::Property(ObjectProperty::with_visibility(
                "b".to_string(),
                TypeExpr::Primitive(PrimitiveName::Number),
                false,
                false,
                MemberVisibility::Protected,
                Default::default(),
            )),
            ObjectMember::Method(MethodSignature::with_visibility(
                "c".to_string(),
                FunctionExpr::synthetic(Vec::new(), None, Vec::new()),
                false,
                MemberVisibility::Private,
                Default::default(),
            )),
        ],
    }));

    let mut builder = GraphBuilder::new();
    let outer_id = builder.node_id(&outer);
    let nodes = builder.nodes();

    // Resolve a string id back to its source string for name assertions.
    let strings = builder.strings();
    let name_of = |id: u32| -> Option<&str> {
        if id == 0 {
            None
        } else {
            strings.get((id - 1) as usize).map(String::as_str)
        }
    };

    let GraphNode::Object { members } = &nodes[(outer_id - 1) as usize] else {
        panic!("outer must encode to GraphNode::Object");
    };
    let outer_member_names: Vec<&str> = members.iter().filter_map(|m| name_of(m.name)).collect();
    assert_eq!(
        outer_member_names,
        vec!["a"],
        "outer object wire must carry ONLY the public member `a` \
         (protected `b` / private method `c` dropped): {outer_member_names:?}"
    );

    // The nested object (value type of `a`) must also be sanitised.
    let inner_member = &members[0];
    let GraphNode::Object {
        members: inner_members,
    } = &nodes[(inner_member.ty - 1) as usize]
    else {
        panic!("the value type of `a` must encode to GraphNode::Object");
    };
    let inner_member_names: Vec<&str> = inner_members
        .iter()
        .filter_map(|m| name_of(m.name))
        .collect();
    assert_eq!(
        inner_member_names,
        vec!["pub_inner"],
        "nested object wire must carry ONLY `pub_inner` (private `priv_inner` \
         dropped recursively): {inner_member_names:?}"
    );
}

/// A representative nested JSDoc-payload type exercising strings, node
/// children, dedup, and absent-id (0) positions: `Record<string, Foo> |
/// "lit" | (name?: string) => Foo`.
fn representative_jsdoc_type() -> TypeExpr {
    use verter_type_expr::{FunctionExpr, FunctionParam};
    let foo = TypeExpr::Ref {
        name: "Foo".into(),
        type_arguments: Arc::from(Vec::new().into_boxed_slice()),
    };
    let record = TypeExpr::Ref {
        name: "Record".into(),
        type_arguments: Arc::from(vec![
            TypeExpr::Primitive(PrimitiveName::String),
            foo.clone(),
        ]),
    };
    // Absent return annotation exercises the 0-id (absent) position.
    let func = TypeExpr::Function(Arc::new(FunctionExpr::synthetic(
        vec![FunctionParam::synthetic(
            Some("name".to_string()),
            TypeExpr::Primitive(PrimitiveName::String),
            true,
            false,
        )],
        None,
        Vec::new(),
    )));
    TypeExpr::Union(Arc::from(vec![
        record,
        TypeExpr::Literal(LiteralValue::String("lit".into())),
        func,
    ]))
}

/// Seed a builder with unrelated pre-existing content (simulating the
/// surrounding component-meta block conversion that runs BEFORE the JSDoc
/// tags), including a string and a node the JSDoc type will COLLIDE with
/// (dedup parity).
fn seed_builder(builder: &mut GraphBuilder) {
    let _ = builder.string_id("/src/App.vue");
    // `Foo` both as a string and as an interned Ref node — the snapshot
    // re-intern must DEDUP onto these, exactly like the direct walk.
    let _ = builder.node_id(&TypeExpr::Ref {
        name: "Foo".into(),
        type_arguments: Arc::from(Vec::new().into_boxed_slice()),
    });
}

/// GOLDEN wire parity: appending a producer-captured snapshot into a live
/// builder produces BYTE-IDENTICAL node/string tables AND the same root id
/// as the retired direct `node_id(&TypeExpr)` walk — including dedup
/// against pre-existing seeded content. Discriminating: any remap slip
/// (string-vs-node id confusion, missed dedup, order drift) flips the
/// table equality; a perturbed payload flips the root/table too.
#[test]
fn append_snapshot_is_wire_identical_to_direct_node_id_walk() {
    let ty = representative_jsdoc_type();

    // LEGACY path: the direct walk into a seeded live builder.
    let mut legacy = GraphBuilder::new();
    seed_builder(&mut legacy);
    let legacy_root = legacy.node_id(&ty);

    // NEW path: producer-side snapshot capture, then re-intern into an
    // IDENTICALLY seeded live builder.
    let mut producer = GraphBuilder::new();
    let snapshot_root = producer.node_id(&ty);
    let snapshot =
        crate::graph::snapshot::ResolvedTypeGraphSnapshot::from_builder(producer, snapshot_root)
            .expect("valid non-synthetic snapshot");
    let mut live = GraphBuilder::new();
    seed_builder(&mut live);
    let new_root = live.append_snapshot(&snapshot);

    assert_eq!(legacy_root, new_root, "the remapped root id is identical");
    assert_eq!(
        legacy.nodes(),
        live.nodes(),
        "the node tables are byte-identical"
    );
    assert_eq!(
        legacy.strings(),
        live.strings(),
        "the string tables are byte-identical"
    );

    // Appending the SAME snapshot again is fully deduplicated — same root,
    // no table growth (value-level dedup parity with a repeated walk).
    let nodes_before = live.nodes().len();
    let strings_before = live.strings().len();
    let repeat_root = live.append_snapshot(&snapshot);
    assert_eq!(repeat_root, new_root, "a repeated append dedups the root");
    assert_eq!(live.nodes().len(), nodes_before, "no node-table growth");
    assert_eq!(
        live.strings().len(),
        strings_before,
        "no string-table growth"
    );

    // DISCRIMINATION: a perturbed payload produces a different graph.
    let perturbed = TypeExpr::Ref {
        name: "Bar".into(),
        type_arguments: Arc::from(Vec::new().into_boxed_slice()),
    };
    let mut producer2 = GraphBuilder::new();
    let root2 = producer2.node_id(&perturbed);
    let snapshot2 =
        crate::graph::snapshot::ResolvedTypeGraphSnapshot::from_builder(producer2, root2)
            .expect("valid non-synthetic snapshot");
    assert_ne!(
        snapshot2, snapshot,
        "a perturbed payload produces a different snapshot"
    );
    let appended2 = live.append_snapshot(&snapshot2);
    assert_ne!(
        appended2, new_root,
        "a perturbed payload lands on a different node"
    );
}

/// A representative RECURSIVE / MAPPED / nested-OBJECT payload — the node
/// families the flat `representative_jsdoc_type` golden does not reach:
/// a `RecursiveRef` carrying a decided conditional frame, a `Mapped` node
/// (parameter string, keyof source, modifier tags, absent `name_type` = 0),
/// and a two-level nested object whose member values recurse through the
/// same interning path.
fn representative_recursive_mapped_type() -> TypeExpr {
    use verter_type_expr::{
        MappedModifier, MemberVisibility, ObjectExpr, ObjectMember, ObjectProperty,
    };
    // `Tree<string>` with a decided conditional frame — the recursive
    // back-edge shape the builder encodes as `GraphNode::RecursiveRef`.
    let recursive = TypeExpr::RecursiveRef {
        name: Arc::from("Tree"),
        type_arguments: Arc::from(vec![TypeExpr::Primitive(PrimitiveName::String)]),
        conditional_context: Arc::from(vec![RecursiveConditionalFrame {
            branch: RecursiveConditionalBranch::True,
            decided: true,
            check: Arc::new(TypeExpr::named("T")),
            extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
        }]),
    };
    // `{ [K in keyof Foo]?: Tree<string> }` — mapped with an optional-add
    // modifier and NO name-remap (`name_type` exercises the absent id 0).
    let mapped = TypeExpr::Mapped {
        parameter: "K".to_string(),
        source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::named("Foo")))),
        value: Arc::new(recursive.clone()),
        optional: MappedModifier::Add,
        readonly: MappedModifier::None,
        name_type: None,
    };
    // `{ deep: { tree: Tree<string> } }` — nested object members whose
    // value types recurse through the same node-interning path.
    let inner_object = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![ObjectMember::Property(ObjectProperty::with_visibility(
            "tree".to_string(),
            recursive,
            false,
            false,
            MemberVisibility::Public,
            Default::default(),
        ))],
    }));
    let outer_object = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![ObjectMember::Property(ObjectProperty::with_visibility(
            "deep".to_string(),
            inner_object,
            true,
            false,
            MemberVisibility::Public,
            Default::default(),
        ))],
    }));
    TypeExpr::Intersection(Arc::from(vec![mapped, outer_object]))
}

/// GOLDEN wire parity for the RECURSIVE / MAPPED / nested-OBJECT node
/// families through `append_snapshot`: byte-identical node/string tables
/// and the same root id as the retired direct `node_id(&TypeExpr)` walk,
/// including dedup against pre-seeded content (the flat-golden sibling
/// covers Union/Ref/Function/Literal/Primitive; this one proves the remap
/// arms for `RecursiveRef` conditional frames, `Mapped` modifier/absent-id
/// positions, and nested object members). DISCRIMINATING: perturbing the
/// mapped payload flips the captured snapshot, the appended root, and the
/// table equality.
#[test]
fn append_snapshot_recursive_mapped_object_is_wire_identical_to_direct_walk() {
    let ty = representative_recursive_mapped_type();

    // LEGACY path: the direct walk into a seeded live builder.
    let mut legacy = GraphBuilder::new();
    seed_builder(&mut legacy);
    let legacy_root = legacy.node_id(&ty);

    // NEW path: producer-side snapshot capture, then re-intern into an
    // IDENTICALLY seeded live builder.
    let mut producer = GraphBuilder::new();
    let snapshot_root = producer.node_id(&ty);
    let snapshot =
        crate::graph::snapshot::ResolvedTypeGraphSnapshot::from_builder(producer, snapshot_root)
            .expect("valid non-synthetic snapshot");
    let mut live = GraphBuilder::new();
    seed_builder(&mut live);
    let new_root = live.append_snapshot(&snapshot);

    assert_eq!(legacy_root, new_root, "the remapped root id is identical");
    assert_eq!(
        legacy.nodes(),
        live.nodes(),
        "the node tables are byte-identical"
    );
    assert_eq!(
        legacy.strings(),
        live.strings(),
        "the string tables are byte-identical"
    );
    // The payload genuinely reached the deep families (not a degenerate
    // encoding): the appended table carries a RecursiveRef WITH its
    // conditional frame, a Mapped node with the absent name_type id 0,
    // and a nested Object.
    assert!(
        live.nodes().iter().any(|n| matches!(
            n,
            GraphNode::RecursiveRef { conditional_context, .. } if conditional_context.len() == 1
        )),
        "the recursive-ref node (with its conditional frame) must be present"
    );
    assert!(
        live.nodes()
            .iter()
            .any(|n| matches!(n, GraphNode::Mapped { name_type: 0, .. })),
        "the mapped node (absent name_type = 0) must be present"
    );
    assert!(
        live.nodes()
            .iter()
            .filter(|n| matches!(n, GraphNode::Object { members } if !members.is_empty()))
            .count()
            >= 2,
        "both nested object levels must be present"
    );

    // Appending the SAME snapshot again is fully deduplicated — same
    // root, no table growth.
    let nodes_before = live.nodes().len();
    let strings_before = live.strings().len();
    let repeat_root = live.append_snapshot(&snapshot);
    assert_eq!(repeat_root, new_root, "a repeated append dedups the root");
    assert_eq!(live.nodes().len(), nodes_before, "no node-table growth");
    assert_eq!(
        live.strings().len(),
        strings_before,
        "no string-table growth"
    );

    // DISCRIMINATION: perturbing the mapped VALUE payload (the recursive
    // arm swapped for a primitive) produces a different captured snapshot
    // and lands on a different appended node than the unperturbed payload.
    let perturbed = {
        use verter_type_expr::MappedModifier;
        TypeExpr::Mapped {
            parameter: "K".to_string(),
            source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::named("Foo")))),
            value: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
            optional: MappedModifier::Add,
            readonly: MappedModifier::None,
            name_type: None,
        }
    };
    let mut producer2 = GraphBuilder::new();
    let root2 = producer2.node_id(&perturbed);
    let snapshot2 =
        crate::graph::snapshot::ResolvedTypeGraphSnapshot::from_builder(producer2, root2)
            .expect("valid non-synthetic snapshot");
    assert_ne!(
        snapshot2, snapshot,
        "a perturbed mapped payload produces a different snapshot"
    );
    let appended2 = live.append_snapshot(&snapshot2);
    assert_ne!(
        appended2, new_root,
        "a perturbed mapped payload lands on a different node"
    );
}

/// The `SyntheticSlotBinding` carrier is LIVE-WIRE-ONLY, and both halves
/// of that contract are pinned here:
///
/// * **Capture fails closed.** A `GraphBuilder` whose table holds a
///   slot-binding carrier CANNOT be captured as a persisted snapshot —
///   `from_builder` returns the exact typed
///   `SnapshotCaptureError::NonPersistableNode`, never a partial
///   snapshot. The carrier's `value_node` is a generation-local SESSION
///   semantic id; a persisted snapshot (session cache value / FFI JSON
///   DTO) outlives that generation, so the persisted vocabulary
///   structurally excludes the carrier.
///
/// * **The LIVE wire truth is unchanged.** A direct
///   `node_id(&TypeExpr::synthetic_slot_binding(..))` walk into a live
///   (pre-seeded) builder still encodes `GraphNode::SyntheticSlotBinding`
///   with the VERBATIM `value_node` — a large foreign id (424242) is
///   copied as-is, never wire-arena-remapped, never u32-truncated — while
///   the carrier's three STRING fields intern through the live wire
///   string table (non-zero ids past the seed; absent slot names keep the
///   id-0 sentinel). The sibling
///   `graph_builder_synthetic_carrier_roundtrip` integration test pins
///   the same verbatim-copy invariant on an unseeded builder.
#[test]
fn synthetic_slot_binding_is_live_wire_only_and_capture_fails_closed() {
    use crate::graph::snapshot::SnapshotCaptureError;
    use verter_type_expr::{SyntheticCarrierKey, SyntheticCarrierSurfaceKind};

    let non_zero = TypeExpr::synthetic_slot_binding(SyntheticCarrierKey {
        scope_canonical_id: Arc::from("/abs/Foo.vue"),
        surface_kind: SyntheticCarrierSurfaceKind::SlotBinding,
        slot_name: Some(Arc::from("default")),
        binding_name: Arc::from("controls"),
        // A large SESSION id proving no u32 truncation / no wire-arena remap.
        value_node: 424242,
    });
    let zero = TypeExpr::synthetic_slot_binding(SyntheticCarrierKey {
        scope_canonical_id: Arc::from("/abs/Bar.vue"),
        surface_kind: SyntheticCarrierSurfaceKind::Binding,
        slot_name: None,
        binding_name: Arc::from("model"),
        // The legitimate absent sentinel.
        value_node: 0,
    });
    let ty = TypeExpr::Union(Arc::from(vec![non_zero, zero]));

    // (a) CAPTURE FAILS CLOSED: a carrier-bearing builder never becomes
    // a persisted snapshot — the exact typed error, not a partial table.
    let mut producer = GraphBuilder::new();
    let snapshot_root = producer.node_id(&ty);
    assert_eq!(
        crate::graph::snapshot::ResolvedTypeGraphSnapshot::from_builder(producer, snapshot_root,)
            .expect_err("a slot-binding-bearing capture must fail closed"),
        SnapshotCaptureError::NonPersistableNode,
    );

    // (b) LIVE WIRE: the direct walk into a seeded live builder encodes
    // the carrier with the verbatim value_node and remapped string ids.
    let mut live = GraphBuilder::new();
    seed_builder(&mut live);
    let _root = live.node_id(&ty);

    let carriers: Vec<(u64, u32, u32, u32)> = live
        .nodes()
        .iter()
        .filter_map(|n| match n {
            GraphNode::SyntheticSlotBinding {
                value_node,
                scope_canonical_id_id,
                slot_name_id,
                binding_name_id,
                ..
            } => Some((
                *value_node,
                *scope_canonical_id_id,
                *slot_name_id,
                *binding_name_id,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        carriers.len(),
        2,
        "both slot-binding carriers must be present on the live wire"
    );
    let non_zero_carrier = carriers
        .iter()
        .find(|(v, ..)| *v == 424242)
        .expect("the large foreign value_node must survive verbatim");
    assert!(
        non_zero_carrier.1 > 0 && non_zero_carrier.2 > 0 && non_zero_carrier.3 > 0,
        "present string fields (scope / slot / binding) intern to non-zero ids"
    );
    let zero_carrier = carriers
        .iter()
        .find(|(v, ..)| *v == 0)
        .expect("the absent value_node sentinel (0) must be preserved");
    assert_eq!(
        zero_carrier.2, 0,
        "an absent slot_name stays the id-0 sentinel"
    );
    assert!(
        zero_carrier.1 > 0 && zero_carrier.3 > 0,
        "the present scope / binding fields of the sentinel carrier still intern"
    );
}
