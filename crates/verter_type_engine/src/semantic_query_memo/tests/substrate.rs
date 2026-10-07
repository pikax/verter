use crate::semantic_query::*;
use crate::semantic_query_memo::arena::{shard_index_for, NUM_SHARDS};
use crate::semantic_query_memo::family::carrier_facts_reference_canonical;
use crate::semantic_query_memo::interner::{DepSignatureInterner, SWEEP_INTERVAL};
use crate::semantic_query_memo::*;
use std::sync::Arc;
use verter_type_expr::TopLevelOwnerId;

fn scope(canonical: &str) -> ScopeId {
    ScopeId {
        canonical_id: Arc::from(canonical),
        owner: TopLevelOwnerId::ordinary_file(),
        local_scope: None,
        binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(
            TopLevelOwnerId::ordinary_file(),
        ),
    }
}

/// Build a `ReadSetSignature` whose fact rail names exactly
/// `canonical` via a `FileWholeHash` fact — so a publish through
/// `publish_with_carrier_for_tests` registers a `canonical_to_entries`
/// reverse-index entry under that canonical. `hash` keeps each
/// carrier's `FileWholeHash` distinct.
fn carrier_naming(
    canonical: &str,
    hash: u8,
) -> verter_session_query::facts::fact_cache::ReadSetSignature {
    verter_session_query::facts::fact_cache::ReadSetSignature::new(Arc::from(vec![
        verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash {
            canonical_id: canonical.to_string(),
            hash: [hash; 16],
        },
    ]))
}

// ──────────────────────────────────────────────────────────────────
// Family-memo backfill matrix
// ──────────────────────────────────────────────────────────────────

fn family_test_path() -> Arc<[PathSegment]> {
    Arc::from(
        vec![PathSegment::Member(
            crate::semantic_query::PropertyKey::identifier("foo"),
        )]
        .into_boxed_slice(),
    )
}

// ──────────────────────────────────────────────────────────────────
// B2 derivation/origin layer + telemetry tests
// ──────────────────────────────────────────────────────────────────

fn dep_sig_for(canonical: &str, hash: u8) -> DepSignature {
    Arc::from(
        vec![(
            Arc::<str>::from(canonical),
            crate::semantic_query::DepVersion::WholeHash([hash; 16]),
        )]
        .into_boxed_slice(),
    )
}

// ──────────────────────────────────────────────────────────────────
// Document-close release (`release_canonical`)
// ──────────────────────────────────────────────────────────────────

fn release_file_scope(canonical: &str, hash: u8) -> NodeScopeId {
    NodeScopeId::File {
        canonical_id: Arc::from(canonical),
        owner: TopLevelOwnerId::ordinary_file(),
        whole_hash: [hash; 16],
        local_scope: None,
    }
}

fn release_type_param(canonical: &str, hash: u8, name: &str) -> SemanticNodeData {
    SemanticNodeData::TypeParam {
        decl: DeclIdentity {
            canonical_id: Arc::from(canonical),
            owner: TopLevelOwnerId::ordinary_file(),
            whole_hash: [hash; 16],
            decl_name: Arc::from(name),
        },
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from(name),
    }
}

fn release_member(name: &str, value: SemanticNodeId) -> crate::semantic_query::SurfaceMember {
    crate::semantic_query::SurfaceMember {
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

fn release_object_view(
    members: Vec<crate::semantic_query::SurfaceMember>,
) -> crate::semantic_query::SurfaceView {
    crate::surface_view! {
        members: Arc::from(members.into_boxed_slice()),
        call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        index_signatures: Arc::from(
            Vec::<crate::semantic_query::IndexSignature>::new().into_boxed_slice()
        ),
        keyspace: None,
        has_index_signature: false,
    }
}

/// One closed document's substrate: a `File{canonical}` type-parameter
/// node, an object surface over it (plus a shared Global primitive), and a
/// Global alias shell over the object — the shape a consumer's rebuilt
/// node takes. Returns `(param, object, alias, view)`.
fn release_intern_document(
    store: &SemanticGraphStore,
    canonical: &str,
    hash: u8,
    shared: SemanticNodeId,
) -> (
    SemanticNodeId,
    SemanticNodeId,
    SemanticNodeId,
    crate::semantic_query::SurfaceView,
) {
    let scope = release_file_scope(canonical, hash);
    let param =
        store.intern_node_with_scope(release_type_param(canonical, hash, "T"), scope.clone());
    let view = release_object_view(vec![
        release_member("own", param),
        release_member("shared", shared),
    ]);
    let object = store.intern_node_with_scope(SemanticNodeData::Object(view.clone()), scope);
    let alias = store.intern_node(SemanticNodeData::Alias(object));
    (param, object, alias, view)
}

fn release_decl_key(canonical: &str, name: &str) -> SemanticQueryKey {
    SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope(canonical),
        name: Arc::from(name),
    })
}

/// The inert-structure bit and the unresolved-reach bit of a node live in
/// ONE per-node entry, so the two memos share one lifecycle: an entry
/// released for a node id takes both bits with it, and no second node-id
/// map exists to outlive it.
#[test]
fn a_nodes_structural_bits_share_one_sidecar_entry() {
    let store = SemanticGraphStore::new();
    let number = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let array = store.intern_node(SemanticNodeData::Array {
        element: number,
        readonly: false,
    });
    assert_eq!(store.node_structure_bits_for_tests(array), None);
    assert_eq!(store.unresolved_reach_count(), 0);
    assert!(store.node_is_inert_structure(array));
    assert_eq!(
        store.node_structure_bits_for_tests(array),
        Some(unresolved_reach::NodeStructureBits {
            unresolved: None,
            inert: Some(true),
        })
    );
    assert!(!store.node_reaches_unresolved(array));
    assert_eq!(
        store.node_structure_bits_for_tests(array),
        Some(unresolved_reach::NodeStructureBits {
            unresolved: Some(false),
            inert: Some(true),
        }),
        "both bits sit in the node's one entry"
    );
    assert_eq!(
        store.node_structure_bits_for_tests(number),
        Some(unresolved_reach::NodeStructureBits {
            unresolved: Some(false),
            inert: Some(true),
        })
    );
    assert_eq!(
        store.unresolved_reach_count(),
        2,
        "one sidecar entry per node, whatever bits it holds"
    );
}

#[test]
fn interning_returns_unique_stable_ids() {
    let store = SemanticGraphStore::new();
    let a = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let b = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    assert_ne!(a, b);
    assert_eq!(a.0 + 1, b.0);
}

/// Structural-interning positive invariant — two
/// `intern_node_with_scope` calls for the same `(payload, scope)`
/// pair must share one [`SemanticNodeId`]. An append-only allocator
/// would return distinct ids and break dedup.
#[test]
fn intern_dedups_structural_values_across_contexts() {
    let store = SemanticGraphStore::new();
    let first = store.intern_node_with_scope(
        SemanticNodeData::Primitive(PrimitiveKind::Number),
        NodeScopeId::Global,
    );
    let second = store.intern_node_with_scope(
        SemanticNodeData::Primitive(PrimitiveKind::Number),
        NodeScopeId::Global,
    );
    assert_eq!(
        first, second,
        "structurally-identical (payload, scope) pairs must dedup \
         to one SemanticNodeId under C7 compound-key interning",
    );

    // Scope axis still disambiguates: same payload in a different
    // scope produces a distinct id.
    let scoped = store.intern_node_with_scope(
        SemanticNodeData::Primitive(PrimitiveKind::Number),
        NodeScopeId::File {
            canonical_id: Arc::from("/w/a.ts"),
            owner: TopLevelOwnerId::ordinary_file(),
            whole_hash: [0u8; 16],
            local_scope: None,
        },
    );
    assert_ne!(
        first, scoped,
        "cross-scope same-payload interns must stay distinct — C7 \
         preserves the scope disambiguation axis",
    );
}

/// Owner is a semantic identity axis, not presentation metadata. The same
/// payload at the same canonical/hash/local scope must intern separately when
/// its authored module/instance owner differs; an ordinary-file control still
/// deduplicates.
#[test]
fn intern_identity_discriminates_top_level_owner() {
    let store = SemanticGraphStore::new();
    let scope = |owner| NodeScopeId::File {
        canonical_id: Arc::from("/w/Component.vue"),
        owner,
        whole_hash: [7u8; 16],
        local_scope: None,
    };
    let payload = SemanticNodeData::Primitive(PrimitiveKind::Number);

    let module = store.intern_node_with_scope(payload.clone(), scope(TopLevelOwnerId::module(0)));
    let instance =
        store.intern_node_with_scope(payload.clone(), scope(TopLevelOwnerId::instance(0)));
    let ordinary_first =
        store.intern_node_with_scope(payload.clone(), scope(TopLevelOwnerId::ordinary_file()));
    let ordinary_second =
        store.intern_node_with_scope(payload, scope(TopLevelOwnerId::ordinary_file()));

    assert_ne!(
        module, instance,
        "module and instance scopes must not alias"
    );
    assert_eq!(
        module, ordinary_first,
        "ordinary files are explicitly Module(0)"
    );
    assert_eq!(ordinary_first, ordinary_second);
    assert_eq!(store.node_count(), 2);
}

/// Declaration payload identity and content-free slot identity both retain
/// owner. Removing either owner field makes module/instance declarations with
/// the same canonical/name/space collide before they reach the memo.
#[test]
fn declaration_and_slot_identity_discriminate_top_level_owner() {
    let decl = |owner| DeclIdentity {
        canonical_id: Arc::from("/w/Component.vue"),
        owner,
        whole_hash: [9u8; 16],
        decl_name: Arc::from("Shared"),
    };
    let module_decl = decl(TopLevelOwnerId::module(0));
    let instance_decl = decl(TopLevelOwnerId::instance(0));
    assert_ne!(module_decl, instance_decl);

    let slot = |owner| {
        ResolvedDeclSlotIdentity::type_slot(
            Arc::from("/w/Component.vue"),
            owner,
            Arc::from("Shared"),
            3,
            [4u8; 16],
            [5u8; 16],
        )
    };
    let module_slot = slot(TopLevelOwnerId::module(0));
    let instance_slot = slot(TopLevelOwnerId::instance(0));
    assert_ne!(module_slot, instance_slot);

    let mut declarations = std::collections::HashMap::new();
    declarations.insert(module_decl, "module");
    declarations.insert(instance_decl, "instance");
    assert_eq!(declarations.len(), 2);

    let mut slots = std::collections::HashMap::new();
    slots.insert(module_slot, "module");
    slots.insert(instance_slot, "instance");
    assert_eq!(slots.len(), 2);
}

#[test]
fn node_data_is_readable_via_graph_read_trait() {
    let store = SemanticGraphStore::new();
    let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let read: &dyn SemanticGraphRead = &store;
    let data = read.node_data(id);
    assert!(matches!(
        *data,
        SemanticNodeData::Primitive(PrimitiveKind::Boolean)
    ));
}

/// Sharded-dedup invariant — sharded dedup produces the same `SemanticNodeId`
/// across threads for identical `(payload, scope)` pairs. The
/// invariant is strong: two threads interning the same payload at
/// the same scope must observe equal ids immediately (no visibility
/// gap from the per-shard Mutex). The threads race; the second
/// arrival finds the first's entry in the shard index and returns
/// the same id rather than allocating a duplicate.
#[test]
fn intern_identity_invariant_holds_across_threads() {
    use std::thread;
    let store = Arc::new(SemanticGraphStore::new());
    let store_a = Arc::clone(&store);
    let store_b = Arc::clone(&store);
    let handle_a = thread::spawn(move || {
        store_a.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String))
    });
    let handle_b = thread::spawn(move || {
        store_b.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String))
    });
    let id_a = handle_a.join().expect("thread A joined");
    let id_b = handle_b.join().expect("thread B joined");
    assert_eq!(
        id_a, id_b,
        "C17 sharded intern must produce identical SemanticNodeId across \
         threads for the same (payload, scope) pair — found {id_a:?} vs {id_b:?}",
    );
}

/// Spans are part of node identity. Two `Function` payloads that differ
/// ONLY in `signature_span` must intern to DISTINCT ids; two identical
/// (span-included) shapes must dedup. Discriminating against the
/// fingerprint interner dropping spans from the content-`Eq` authority
/// (which would alias provenance-distinct signatures).
#[test]
fn intern_span_participates_in_identity() {
    let store = SemanticGraphStore::new();
    let ret = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let mk = |end: u32| SemanticNodeData::Signature {
        kind: crate::semantic_query::SignatureKind::Call,
        params: Arc::from(Vec::<crate::semantic_query::FunctionParam>::new()),
        return_type: ret,
        occurrence: None,
        return_carrier: crate::semantic_query::SignatureReturnCarrier::Declared(ret),
        type_parameters: Arc::from(Vec::<crate::semantic_query::TypeParamDecl>::new()),
        signature_span: Some(verter_span::Span::new(0, end)),
        return_type_span: None,
        predicate: None,
        is_abstract: false,
    };
    let id_a = store.intern_node(mk(10));
    let id_a_again = store.intern_node(mk(10));
    let id_b = store.intern_node(mk(20));
    assert_eq!(
        id_a, id_a_again,
        "identical function shape (incl signature_span) must dedup to one id",
    );
    assert_ne!(
        id_a, id_b,
        "functions differing only in signature_span must stay distinct — \
         spans are part of node identity",
    );
}

/// `TypeParam::display_name` is EXCLUDED from node identity (F11): two
/// `TypeParam` nodes with the same `decl` / `param_index` / `constraint`
/// / `default` but different `display_name` must dedup to one id.
/// Discriminating against the fingerprint including `display_name` (a
/// derived `Hash` would), which would route the two renames into
/// different buckets and defeat dedup even though their `Eq` is equal.
#[test]
fn intern_typeparam_display_name_excluded_from_identity() {
    use crate::semantic_query::DeclIdentity;
    let store = SemanticGraphStore::new();
    let mk = |display: &str| SemanticNodeData::TypeParam {
        decl: DeclIdentity {
            canonical_id: Arc::from("/w/a.ts"),
            owner: TopLevelOwnerId::ordinary_file(),
            whole_hash: [3u8; 16],
            decl_name: Arc::from("T"),
        },
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from(display),
    };
    let id1 = store.intern_node(mk("T"));
    let id2 = store.intern_node(mk("TRenamed"));
    assert_eq!(
        id1, id2,
        "TypeParam display_name is excluded from identity — same \
         decl/index/constraint/default must dedup regardless of display_name",
    );
}

/// Sharded-dedup invariant — `shard_index_for` is deterministic: identical
/// `(data, scope)` pairs route to the same shard regardless of
/// calling thread or program run. This is load-bearing for the
/// sharded-dedup correctness: a payload's shard must not drift
/// across invocations or the second intern would land on a
/// different shard and allocate a duplicate id.
#[test]
fn shard_routing_is_deterministic_per_payload_and_scope() {
    let data_a = SemanticNodeData::Primitive(PrimitiveKind::String);
    let data_b = SemanticNodeData::Primitive(PrimitiveKind::String);
    let scope_global = NodeScopeId::Global;
    let scope_file = NodeScopeId::File {
        canonical_id: Arc::from("/w/x.ts"),
        owner: TopLevelOwnerId::ordinary_file(),
        whole_hash: [0u8; 16],
        local_scope: None,
    };
    assert_eq!(
        shard_index_for(&data_a, &scope_global),
        shard_index_for(&data_b, &scope_global),
        "shard routing must be stable for identical payloads at identical scopes",
    );
    // Different scope → may route differently, but the result is
    // still deterministic per call.
    let s1 = shard_index_for(&data_a, &scope_file);
    let s2 = shard_index_for(&data_a, &scope_file);
    assert_eq!(s1, s2, "shard routing must be stable across repeat calls");
    assert!(s1 < NUM_SHARDS, "shard index must stay within NUM_SHARDS");
}

// ──────────────────────────────────────────────────────────────────
// DepSignatureInterner
// ──────────────────────────────────────────────────────────────────

/// Interner returns the SAME
/// `Arc` for two distinct calls with equivalent payload.
/// Discriminating: pre-fix tree has no interner, every publish
/// builds a fresh Arc. Post-fix tree: dedup via content hash.
#[test]
fn dep_signature_interner_returns_same_arc_for_equivalent_payloads() {
    let interner = DepSignatureInterner::new();
    let payload_a = vec![
        (
            Arc::<str>::from("/w/a.ts"),
            DepVersion::WholeHash([1u8; 16]),
        ),
        (
            Arc::<str>::from("/w/b.ts"),
            DepVersion::WholeHash([2u8; 16]),
        ),
    ];
    // Reordered with a duplicate — must normalise to the same
    // canonical form.
    let payload_b = vec![
        (
            Arc::<str>::from("/w/b.ts"),
            DepVersion::WholeHash([2u8; 16]),
        ),
        (
            Arc::<str>::from("/w/a.ts"),
            DepVersion::WholeHash([1u8; 16]),
        ),
        (
            Arc::<str>::from("/w/a.ts"),
            DepVersion::WholeHash([1u8; 16]),
        ),
    ];
    let arc_a = interner.intern(&payload_a);
    let arc_b = interner.intern(&payload_b);
    assert!(
        Arc::ptr_eq(&arc_a, &arc_b),
        "equivalent payloads (modulo order + dups) must intern to the same Arc"
    );
    // Different content → different Arc.
    let payload_c = vec![(
        Arc::<str>::from("/w/c.ts"),
        DepVersion::WholeHash([3u8; 16]),
    )];
    let arc_c = interner.intern(&payload_c);
    assert!(
        !Arc::ptr_eq(&arc_a, &arc_c),
        "different payloads must intern to different Arcs"
    );
}

/// Sweep removes empty buckets and dead-Weak buckets.
/// Memory-hygiene invariant: the interner's sweep pass must reclaim
/// buckets whose `Weak` entries have all been dropped, otherwise the
/// hash-cons table grows unbounded.
#[test]
fn dep_signature_intern_sweep_removes_empty_buckets() {
    let interner = DepSignatureInterner::new();
    let payload = vec![(
        Arc::<str>::from("/w/sweep.ts"),
        DepVersion::WholeHash([7u8; 16]),
    )];

    // Intern, drop the strong ref, sweep — bucket must be removed.
    {
        let _arc = interner.intern(&payload);
        assert!(
            interner.bucket_count() >= 1,
            "intern must populate the bucket"
        );
        assert_eq!(
            interner.live_signature_count(),
            1,
            "interned signature must be live"
        );
    } // _arc dropped here.

    // Strong ref gone; bucket entry now contains a dead Weak.
    // Sweep() must reclaim the empty bucket.
    assert_eq!(
        interner.live_signature_count(),
        0,
        "after dropping the strong ref, the Weak is dead"
    );
    interner.sweep();
    assert_eq!(
        interner.bucket_count(),
        0,
        "sweep() must reclaim the empty bucket"
    );
}

/// Auto-sweep trigger fires every `SWEEP_INTERVAL`
/// inserts. Discriminating: drop strong refs, then intern enough
/// distinct signatures to trip the auto-sweep. The bucket count
/// stays bounded.
#[test]
fn dep_signature_intern_auto_sweep_keeps_bucket_count_bounded() {
    let interner = DepSignatureInterner::new();
    // Insert and drop SWEEP_INTERVAL+1 distinct signatures — each
    // bucket becomes orphaned immediately because the Arc never
    // escapes the loop body. Auto-sweep is triggered when the
    // counter hits SWEEP_INTERVAL.
    for i in 0..(SWEEP_INTERVAL + 1) {
        let canonical: Arc<str> = Arc::from(format!("/w/n{i}.ts"));
        let _arc = interner.intern_canonical(canonical, DepVersion::ProjectGeneration(i));
    }
    // After auto-sweep, dead-Weak buckets should be reclaimed.
    // Tolerate up to SWEEP_INTERVAL stragglers (the buckets that
    // landed after the auto-sweep tick; counter resumes counting).
    assert!(
        interner.bucket_count() <= SWEEP_INTERVAL as usize,
        "auto-sweep must keep bucket count bounded; got {}",
        interner.bucket_count()
    );
}

/// Mandatory test gate. `invalidate_canonical(c)` must drop
/// `NodeArena` shard-dedup entries whose origin scope is
/// `NodeScopeId::File { canonical_id: c, .. }` while preserving:
///   1. `NodeScopeId::Global` entries (purely structural nodes).
///   2. `NodeScopeId::File { canonical_id: other, .. }` entries
///      keyed at any unrelated canonical.
///
/// Discriminating: re-intern after invalidation. A preserved
/// shard-dedup entry returns the same `SemanticNodeId`; an evicted
/// shard-dedup entry forces a new arena allocation (the arena is
/// append-only — node ids never compress).
///
/// Pre-fix tree (no arena invalidation): the shard index for the
/// File-scope node is preserved; re-intern returns the SAME id, the
/// `assert_ne!` for the invalidated canonical FAILS.
/// Post-fix tree: shard entry dropped; re-intern allocates a fresh
/// id, the `assert_ne!` PASSES while the Global / unrelated File
/// scope `assert_eq!` PASS.
#[test]
fn node_arena_invalidation_preserves_global_scope() {
    use crate::semantic_query::DeclIdentity;
    use verter_session_query::analysis::types::Hash16;

    let store = SemanticGraphStore::new();

    // Distinct payload per scope so dedup operates per scope key.
    let global_payload = || SemanticNodeData::Primitive(PrimitiveKind::String);
    let canonical_a: Arc<str> = Arc::from("/w/a.ts");
    let canonical_b: Arc<str> = Arc::from("/w/b.ts");
    let whole_a: Hash16 = [1u8; 16];
    let whole_b: Hash16 = [2u8; 16];
    let scope_a = NodeScopeId::File {
        canonical_id: Arc::clone(&canonical_a),
        owner: TopLevelOwnerId::ordinary_file(),
        whole_hash: whole_a,
        local_scope: None,
    };
    let scope_b = NodeScopeId::File {
        canonical_id: Arc::clone(&canonical_b),
        owner: TopLevelOwnerId::ordinary_file(),
        whole_hash: whole_b,
        local_scope: None,
    };
    // File-scope nodes need a payload that varies per scope (so
    // dedup keys are unique). Use TypeParam{decl} keyed on the
    // canonical so the (payload, scope) pair lands in distinct
    // shard entries.
    let file_a_payload = SemanticNodeData::TypeParam {
        decl: DeclIdentity {
            canonical_id: Arc::clone(&canonical_a),
            owner: TopLevelOwnerId::ordinary_file(),
            whole_hash: whole_a,
            decl_name: Arc::from("Param_A"),
        },
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("Param_A"),
    };
    let file_b_payload = SemanticNodeData::TypeParam {
        decl: DeclIdentity {
            canonical_id: Arc::clone(&canonical_b),
            owner: TopLevelOwnerId::ordinary_file(),
            whole_hash: whole_b,
            decl_name: Arc::from("Param_B"),
        },
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("Param_B"),
    };

    let global_id_first = store.intern_node_with_scope(global_payload(), NodeScopeId::Global);
    let file_a_id_first = store.intern_node_with_scope(file_a_payload.clone(), scope_a.clone());
    let file_b_id_first = store.intern_node_with_scope(file_b_payload.clone(), scope_b.clone());

    // Sanity: re-interning before invalidation deduplicates per
    // scope. Without a pre-invalidation hit, the post-invalidation
    // test cannot tell "drop happened" from "never deduped".
    let global_id_second = store.intern_node_with_scope(global_payload(), NodeScopeId::Global);
    let file_a_id_second = store.intern_node_with_scope(file_a_payload.clone(), scope_a.clone());
    let file_b_id_second = store.intern_node_with_scope(file_b_payload.clone(), scope_b.clone());
    assert_eq!(
        global_id_first, global_id_second,
        "pre-invalidation Global re-intern must dedup"
    );
    assert_eq!(
        file_a_id_first, file_a_id_second,
        "pre-invalidation File(/w/a.ts) re-intern must dedup"
    );
    assert_eq!(
        file_b_id_first, file_b_id_second,
        "pre-invalidation File(/w/b.ts) re-intern must dedup"
    );

    // Invalidate /w/a.ts. Only File { canonical_id:
    // /w/a.ts, .. } shard entries are dropped. Global entries and
    // File { canonical_id: /w/b.ts, .. } entries are preserved.
    let _ = store.invalidate_canonical(canonical_a.as_ref());

    // Discriminating assertions:
    let global_id_post = store.intern_node_with_scope(global_payload(), NodeScopeId::Global);
    let file_a_id_post = store.intern_node_with_scope(file_a_payload, scope_a);
    let file_b_id_post = store.intern_node_with_scope(file_b_payload, scope_b);

    assert_eq!(
        global_id_post, global_id_first,
        "Global-scope shard entry must SURVIVE invalidate_canonical \
         (invariant — invalidation does NOT drop Global)"
    );
    assert_eq!(
        file_b_id_post, file_b_id_first,
        "File(/w/b.ts) shard entry must SURVIVE invalidation of /w/a.ts \
         (invariant — invalidation drops only the matching canonical's File scope)"
    );
    assert_ne!(
        file_a_id_post, file_a_id_first,
        "File(/w/a.ts) shard entry must be DROPPED by invalidate_canonical(/w/a.ts); \
         re-intern must allocate a new SemanticNodeId (the arena is append-only — \
         ids never compress)"
    );
}

/// §3.4 PATH-AXIS discrimination: `cached_satisfies` is path-EXACT, never
/// prefix-containment. A DEEP recorded materialised point
/// (`A['c']['full']['bar']`) must NOT satisfy a request at a strict PREFIX
/// of that path (`A['c']`), and a SHALLOW recorded point must NOT satisfy
/// a DEEPER request.
///
/// This pins the §3.4 silent-warm-hit crux the MODE-axis guard
/// `cache_satisfaction_is_materialized_point_not_nominal_demand` does NOT
/// cover: that guard records AND requests the SAME `[foo]` path, so it
/// exercises only the mode axis and would STILL PASS under a
/// prefix-dominance `cached_satisfies`.
///
/// Why this is a PURE-FUNCTION probe, not a store-level publish: at the
/// memo level a prefix request maps to a DIFFERENT `FamilyKey` (the
/// projection path is part of the family identity — see
/// `FamilyKey::ProjectPath { path, .. }`), so a store-level probe can
/// never reach the deep entry's slot to begin with. The path-exactness of
/// the predicate is only observable on `cached_satisfies` itself, which
/// BOTH the warm-hit gate and the directional backfill gate consult.
///
/// DISCRIMINATING: FAILS against a `cached_satisfies` mutated to
/// `requested.path().is_prefix_of(m.path())` (or to drop the path clause
/// entirely). Under either mutant the deep `Expanded@[c,full,bar]` record
/// would dominate the shallow `Expanded@[c]` request — the mode is equal
/// and `[c]` is a prefix of `[c,full,bar]`, which the internal
/// `semantically_dominates` path check (`requested.path` is-prefix-of
/// `recorded.path`) already accepts — so the first assertion below would
/// wrongly hold. PASSES against the landed path-EXACT predicate
/// (`m.path() == requested.path()`).
#[test]
fn cache_satisfaction_requires_path_exact_not_prefix() {
    use crate::semantic_query::demand::{
        cached_satisfies, Demand, MaterializedPoint, MaterializedSet, ProjectionPath,
    };

    let deep = ProjectionPath::from_segments([
        PathSegment::Member(crate::semantic_query::PropertyKey::identifier("c")),
        PathSegment::Member(crate::semantic_query::PropertyKey::identifier("full")),
        PathSegment::Member(crate::semantic_query::PropertyKey::identifier("bar")),
    ]);
    let shallow_prefix = ProjectionPath::from_segments([PathSegment::Member(
        crate::semantic_query::PropertyKey::identifier("c"),
    )]);

    let expanded_at = |path: ProjectionPath| {
        let mut d = Demand::from(ProjectionMode::Expanded);
        d.projection.path = path;
        MaterializedPoint::new(d)
    };

    // A DEEP recorded `Expanded` point must NOT satisfy a SHALLOW request
    // at a strict PREFIX of the deep path. Under a prefix-dominance mutant
    // this would wrongly HIT (the bug class: a deep compute's record
    // serving a shallow surface it never materialised at that path).
    let deep_record = MaterializedSet::single(expanded_at(deep.clone()));
    let shallow_request = expanded_at(shallow_prefix.clone());
    assert!(
        !cached_satisfies(&deep_record, &shallow_request),
        "a DEEP recorded point must NOT satisfy a SHALLOW (strict-prefix) request — \
         cached_satisfies is path-EXACT, never prefix-containment",
    );

    // Vice-versa: a SHALLOW recorded point must NOT satisfy a DEEPER
    // request (the shallow record never reached the deep path).
    let shallow_record = MaterializedSet::single(expanded_at(shallow_prefix.clone()));
    let deep_request = expanded_at(deep.clone());
    assert!(
        !cached_satisfies(&shallow_record, &deep_request),
        "a SHALLOW recorded point must NOT satisfy a DEEPER request",
    );

    // POSITIVE CONTROL: an EXACT-path request at a dominated mode HITS —
    // proves the misses above are path-exactness, not a blanket reject.
    assert!(
        cached_satisfies(&deep_record, &expanded_at(deep.clone())),
        "an EXACT-path request at a dominated mode MUST hit",
    );
}

/// `invalidate_all` ID-KEYED-CACHE CLEAR — a project-generation bump
/// MUST drop every `SemanticNodeId`-keyed semantic cache (the relation
/// memo and the `DerivationStore` edges + signature pool), not just the
/// family memo.
///
/// DISCRIMINATES: this test populates a derivation edge and a relation
/// judgement, then calls `invalidate_all` and asserts the relation-memo
/// count, the derivation edge-bucket count, and the derivation edge
/// count are all zero. Against a tree whose `invalidate_all` cleared the
/// family memo but skipped `relation_memo.clear()` /
/// `derivation.clear()`, those counters stay non-zero and the assertion
/// fails — a stale judgement would survive the project-generation bump.
#[test]
fn invalidate_all_clears_id_keyed_semantic_caches() {
    let store = SemanticGraphStore::new();

    // Intern two nodes, record an origin edge for one, and publish a
    // relation judgement keyed on its id pair.
    let result = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let src = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    store.record_origin_edge(
        result,
        OriginEdgeKind::Normalize,
        Arc::from(vec![src].into_boxed_slice()),
        crate::semantic_query::OriginMeta::None,
        dep_sig_for("/w/a.ts", 1),
    );
    store.insert_relation_payload_for_tests(
        crate::semantic_query::RelateMemoKey::assignable(
            result,
            result,
            crate::semantic_query::RelationContext::default(),
        ),
        verter_session_query::facts::fact_cache::ReadSetSignature::empty(),
        Arc::from(Vec::<Arc<str>>::new().into_boxed_slice()),
        store.relation_payload_for_tests(crate::semantic_query::RelationOutcome::NotAssignable),
        0,
    );
    // Sanity-check the pre-bump state is actually populated, else the
    // test would not discriminate.
    assert_eq!(
        store.origin_edge_count(),
        1,
        "pre-bump: the derivation edge is recorded",
    );
    assert_eq!(
        store.relation_memo_count(),
        1,
        "pre-bump: the relation judgement is cached",
    );

    // Project-generation bump.
    let _ = store.invalidate_all();

    // The id-keyed semantic caches are genuinely empty post-bump.
    assert_eq!(
        store.relation_memo_count(),
        0,
        "CLEAR BUG: invalidate_all must clear relation_memo on a \
         project-generation bump",
    );
    assert_eq!(
        store.origin_edge_count(),
        0,
        "CLEAR BUG: invalidate_all must clear the DerivationStore edges \
         on a project-generation bump",
    );
    assert_eq!(
        store.derivation_bucket_count(),
        0,
        "CLEAR BUG: invalidate_all must clear the DerivationStore edge \
         buckets on a project-generation bump",
    );
}

/// MAP/BUDGET LIFECYCLE FENCE (P2, clear side) — `invalidate_all` MUST
/// clear the `memo_budget` retention ledger UNDER the `entries` lock
/// that performed `entries.clear()`, so the two clears are one atomic
/// step against a concurrent publisher.
///
/// Without the fence `invalidate_all` clears `entries` under the lock,
/// releases it, then clears `memo_budget` separately — a publisher can
/// land an `entries` family + `memo_budget` admission in that gap, and
/// the trailing `memo_budget.clear()` then strands a live family with no
/// ledger record (invisible to FIFO eviction → the retention cap can be
/// exceeded).
///
/// Deterministic. `invalidate_all` is parked, via the pre-`memo_budget`-
/// clear injection point, right before the `memo_budget` clear. With it
/// pinned there the test asserts `entries.try_lock()` is `None`: a
/// publisher reaching `entries_lock_diagnosed()` right now WOULD block.
///
/// DISCRIMINATES: against an un-fenced `invalidate_all` (the
/// `memo_budget` clear runs after the `entries` lock is released)
/// `try_lock()` succeeds (`Some`) and the assertion FAILS. With the
/// fence the `memo_budget` clear runs while the `entries` lock is held,
/// `try_lock()` is `None`, and the assertion PASSES.
#[test]
fn invalidate_all_clears_memo_budget_under_entries_lock() {
    use std::sync::Barrier;
    use std::thread;

    let store = Arc::new(SemanticGraphStore::new());
    // Seed one family so `entries` and `memo_budget` are both non-empty.
    let node = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    store.publish_with_carrier_for_tests(
        SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/seed.ts"),
            name: Arc::from("Seed"),
        }),
        QueryResult::Value(node),
        verter_session_query::facts::fact_cache::ReadSetSignature::empty(),
        Arc::from([]),
    );
    assert_eq!(store.memo_family_count_for_test(), 1, "seeded one family");
    assert_eq!(
        store.memo_budget_tracked_len_for_test(),
        1,
        "seed recorded one budget admission",
    );

    let clear_parked = Arc::new(Barrier::new(2));
    let _gate_guard =
        store.test_invalidate_all_pre_memo_budget_clear_gate(Arc::clone(&clear_parked));

    let store_i = Arc::clone(&store);
    let invalidator = thread::spawn(move || store_i.invalidate_all());

    // `invalidate_all` has cleared `entries` and parked right before the
    // `memo_budget` clear. With the fence in place the `entries` lock is
    // STILL held — a concurrent publisher would block.
    clear_parked.wait();
    assert!(
        store.entries_lock_is_held_for_tests(),
        "MAP/BUDGET DESYNC: `invalidate_all` does NOT hold the `entries` \
         lock while clearing `memo_budget` — a concurrent publish could \
         land an `entries` family + `memo_budget` admission between the \
         `entries` clear and the `memo_budget` clear, stranding a live \
         family with no ledger record. The `memo_budget` clear must run \
         under the `entries` lock.",
    );
    clear_parked.wait();
    let _ = invalidator.join().expect("invalidator thread");

    assert_eq!(
        store.memo_family_count_for_test(),
        0,
        "invalidate_all cleared every family",
    );
    assert_eq!(
        store.memo_budget_tracked_len_for_test(),
        0,
        "invalidate_all cleared the budget ledger — map and budget consistent",
    );
}

/// MAP/BUDGET LIFECYCLE FENCE (P2, publish side) — a warm-slot publish
/// MUST record the `memo_budget` admission UNDER the `entries` lock that
/// landed the slot, so the slot landing and the ledger record are one
/// atomic step against a concurrent `invalidate_all`.
///
/// Without the fence the publish lands the `entries` slot under the
/// lock, releases it, then records `memo_budget` separately — a
/// concurrent `invalidate_all` can clear both structures in that gap,
/// and the publish's trailing `memo_budget` record then re-populates the
/// ledger for an `entries` slot the reset dropped (or, symmetrically,
/// the reset's `memo_budget.clear()` erases the record for a live slot).
///
/// Deterministic. A publisher is parked, via the post-`memo_budget`-
/// record injection point, right after the `memo_budget` admission
/// lands. With it pinned there the test asserts `entries.try_lock()` is
/// `None`: an `invalidate_all` reaching `entries_lock_diagnosed()` right
/// now WOULD block.
///
/// DISCRIMINATES: against an un-fenced publish (the `memo_budget` record
/// runs after the `entries` lock is released) `try_lock()` succeeds
/// (`Some`) and the assertion FAILS. With the fence the `memo_budget`
/// record runs while the `entries` lock is held, `try_lock()` is
/// `None`, and the assertion PASSES.
#[test]
fn warm_publish_records_memo_budget_under_entries_lock() {
    use std::sync::Barrier;
    use std::thread;

    let store = Arc::new(SemanticGraphStore::new());
    let node = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

    let publish_parked = Arc::new(Barrier::new(2));
    let _gate_guard = store.test_publish_post_memo_budget_record_gate(Arc::clone(&publish_parked));

    let store_p = Arc::clone(&store);
    let publisher = thread::spawn(move || {
        store_p.publish_with_carrier_for_tests(
            SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                scope: scope("/w/publish.ts"),
                name: Arc::from("Pub"),
            }),
            QueryResult::Value(node),
            verter_session_query::facts::fact_cache::ReadSetSignature::empty(),
            Arc::from([]),
        )
    });

    // The publisher has landed its `entries` slot and recorded the
    // `memo_budget` admission, and is parked. With the fence in place
    // the `entries` lock is STILL held — a concurrent `invalidate_all`
    // would block.
    publish_parked.wait();
    assert!(
        store.entries_lock_is_held_for_tests(),
        "MAP/BUDGET DESYNC: a warm-slot publish does NOT hold the \
         `entries` lock while recording the `memo_budget` admission — a \
         concurrent `invalidate_all` could clear `entries` + `memo_budget` \
         between the slot landing and the admission record. The \
         `memo_budget` admission must be recorded under the `entries` lock.",
    );
    publish_parked.wait();
    let populated = publisher.join().expect("publisher thread");
    assert_eq!(populated, 1, "the publish landed one slot");

    // Map and budget agree once the publish completes.
    assert_eq!(store.memo_family_count_for_test(), 1);
    assert_eq!(
        store.memo_budget_tracked_len_for_test(),
        1,
        "the family has exactly one budget ledger record",
    );
}

/// FINDING A — FENCE THE REVERSE-INDEX CLEAR AGAINST NEW
/// PUBLISHES. `invalidate_all` MUST clear the `canonical_to_entries`
/// reverse index UNDER the `entries` lock that performed
/// `entries.clear()` + `memo_budget.clear()`, so all three members of
/// the family-memo consistency cluster are cleared atomically against a
/// concurrent publisher.
///
/// Without the fence `invalidate_all` clears `entries` + `memo_budget`
/// under the lock, RELEASES it, then clears `canonical_to_entries` in a
/// tail. A query admitted in that window publishes a fresh memo entry
/// and registers it in `canonical_to_entries`; the trailing
/// `canonical_to_entries.clear()` then deletes only the reverse-index
/// registration while the memo entry + budget record stay live — or,
/// depending on timing, leaves the live memo entry with no registration.
/// Either way a later `invalidate_canonical` cannot find or abort that
/// entry.
///
/// Deterministic. `invalidate_all` is parked, via the
/// pre-`canonical_to_entries`-clear injection point, right before the
/// reverse-index clear. With it pinned there the test asserts
/// `entries.try_lock()` is `None`: a publisher reaching
/// `entries_lock_diagnosed()` right now WOULD block, so it cannot
/// register into `canonical_to_entries` between the `entries` clear and
/// the reverse-index clear.
///
/// DISCRIMINATES: against an un-fenced `invalidate_all` (the
/// `canonical_to_entries` clear runs after the `entries` lock is
/// released) `try_lock()` succeeds (`Some`) and the assertion FAILS.
/// With the fence the reverse-index clear runs while the `entries` lock
/// is held, `try_lock()` is `None`, and the assertion PASSES. The
/// post-join end-state assertions confirm all three cluster members end
/// empty and consistent.
#[test]
fn invalidate_all_clears_reverse_index_under_entries_lock() {
    use std::sync::Barrier;
    use std::thread;

    let store = Arc::new(SemanticGraphStore::new());
    // Seed one family whose carrier names `/w/seed.ts` — so `entries`,
    // `memo_budget`, AND the `canonical_to_entries` reverse index are
    // all non-empty.
    let node = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    store.publish_with_carrier_for_tests(
        SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/seed.ts"),
            name: Arc::from("Seed"),
        }),
        QueryResult::Value(node),
        carrier_naming("/w/seed.ts", 1),
        Arc::from([]),
    );
    assert_eq!(store.memo_family_count_for_test(), 1, "seeded one family");
    assert_eq!(
        store.memo_budget_tracked_len_for_test(),
        1,
        "seed recorded one budget admission",
    );
    assert_eq!(
        store.canonical_to_entries_count("/w/seed.ts"),
        1,
        "seed registered one reverse-index entry",
    );

    let clear_parked = Arc::new(Barrier::new(2));
    let _gate_guard =
        store.test_invalidate_all_pre_reverse_index_clear_gate(Arc::clone(&clear_parked));

    let store_i = Arc::clone(&store);
    let invalidator = thread::spawn(move || store_i.invalidate_all());

    // `invalidate_all` has cleared `entries` + `memo_budget` and parked
    // right before the `canonical_to_entries` clear. With the fence in
    // place the `entries` lock is STILL held — a concurrent publisher
    // would block.
    clear_parked.wait();
    assert!(
        store.entries_lock_is_held_for_tests(),
        "REVERSE-INDEX DESYNC: `invalidate_all` does NOT hold the \
         `entries` lock while clearing `canonical_to_entries` — a \
         concurrent publish could register a fresh reverse-index entry \
         between the `entries` clear and the reverse-index clear, \
         leaving a live memo entry with no `canonical_to_entries` \
         registration (or a stranded registration with no entry), \
         invisible to a later `invalidate_canonical`. The \
         `canonical_to_entries` clear must run under the `entries` lock.",
    );
    clear_parked.wait();
    let _ = invalidator.join().expect("invalidator thread");

    // End-state: all three cluster members are empty and consistent.
    assert_eq!(
        store.memo_family_count_for_test(),
        0,
        "invalidate_all cleared every family",
    );
    assert_eq!(
        store.memo_budget_tracked_len_for_test(),
        0,
        "invalidate_all cleared the budget ledger",
    );
    assert_eq!(
        store.canonical_to_entries_count("/w/seed.ts"),
        0,
        "invalidate_all cleared the reverse index — no stranded \
         registration survives the project-generation reset",
    );
    assert_eq!(
        store.canonical_to_entries_shard_count_for_test(),
        0,
        "no reverse-index shard survives the clear",
    );
}

/// FINDING B — SCOPE BUDGET REVERSE-INDEX CLEANUP TO EVICTED
/// ENTRIES. The FIFO budget-eviction MUST prune the evicted victim's
/// `canonical_to_entries` reverse-index registration UNDER the `entries`
/// lock that performed the victim's `entries` removal, so a concurrent
/// fresh same-`(family, slot)` re-publish cannot interleave its
/// reverse-index registration between the victim's `entries` removal and
/// the victim's reverse-index pruning.
///
/// Without the fence the FIFO eviction removes the victim from `entries`
/// under the lock, RELEASES it, then prunes the victim's
/// `canonical_to_entries` registration in a deferred key-only cleanup.
/// An already-in-flight build for the same `(family, slot)` that
/// publishes and registers before that loop runs has its FRESH
/// registration removed by the key-only cleanup, leaving the live
/// re-published memo slot invisible to future `invalidate_canonical`
/// drains.
///
/// Deterministic. The store is pinned to a `memo_budget` cap of 2.
/// Publishing the THIRD distinct family FIFO-evicts the first; that
/// publish is parked, via the post-reverse-index-prune injection point,
/// right after the evicted victim's reverse-index registration is
/// pruned. With it pinned there the test asserts `entries.try_lock()` is
/// `None`: a fresh re-publisher reaching `entries_lock_diagnosed()`
/// right now WOULD block, so it cannot register a fresh
/// `canonical_to_entries` entry between the victim's `entries` removal
/// and the victim's reverse-index prune.
///
/// DISCRIMINATES: against an un-fenced eviction (the victim's
/// reverse-index pruning runs after the `entries` lock is released)
/// `try_lock()` succeeds (`Some`) and the assertion FAILS. With the
/// fence the prune runs while the `entries` lock is held, `try_lock()`
/// is `None`, and the assertion PASSES. The post-join end-state
/// assertions confirm the evicted victim's registration is gone and the
/// two surviving families' registrations are intact.
#[test]
fn budget_eviction_prunes_reverse_index_under_entries_lock() {
    use std::sync::Barrier;
    use std::thread;

    // Cap of 2: the third distinct family evicts the first (FIFO).
    let store = Arc::new(SemanticGraphStore::new_with_memo_budget_for_test(2));

    // Publish family A (carrier names /w/a.ts) and family B (/w/b.ts).
    // Ledger after both: [A, B] — at the cap, no eviction yet.
    for (name, canonical, hash) in [("A", "/w/a.ts", 1u8), ("B", "/w/b.ts", 2u8)] {
        let node = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        store.publish_with_carrier_for_tests(
            SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                scope: scope(canonical),
                name: Arc::from(name),
            }),
            QueryResult::Value(node),
            carrier_naming(canonical, hash),
            Arc::from([]),
        );
    }
    assert_eq!(store.memo_family_count_for_test(), 2, "A and B both warm");
    assert_eq!(
        store.canonical_to_entries_count("/w/a.ts"),
        1,
        "A registered a reverse-index entry",
    );
    assert_eq!(
        store.canonical_to_entries_count("/w/b.ts"),
        1,
        "B registered a reverse-index entry",
    );

    // Arm the post-reverse-index-prune gate; the next publish that
    // FIFO-evicts a victim parks right after pruning the victim's
    // reverse-index registration.
    let prune_parked = Arc::new(Barrier::new(2));
    let _gate_guard = store.test_publish_post_reverse_index_prune_gate(Arc::clone(&prune_parked));

    // Publish family C (/w/c.ts) — ledger overflows [A, B] → C, victim A
    // is FIFO-evicted. The publish parks after pruning A's reverse-index
    // registration, with the `entries` lock still held.
    let store_p = Arc::clone(&store);
    let publisher = thread::spawn(move || {
        let node = store_p.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        store_p.publish_with_carrier_for_tests(
            SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                scope: scope("/w/c.ts"),
                name: Arc::from("C"),
            }),
            QueryResult::Value(node),
            carrier_naming("/w/c.ts", 3),
            Arc::from([]),
        )
    });

    // The publisher has evicted victim A, pruned A's reverse-index
    // registration, and parked. With the fence in place the `entries`
    // lock is STILL held — a concurrent fresh re-publisher would block.
    prune_parked.wait();
    assert!(
        store.entries_lock_is_held_for_tests(),
        "REVERSE-INDEX DESYNC: a FIFO budget-eviction does NOT hold the \
         `entries` lock while pruning the evicted victim's \
         `canonical_to_entries` registration — a concurrent fresh \
         same-`(family, slot)` re-publish could register into \
         `canonical_to_entries` between the victim's `entries` removal \
         and the deferred key-only prune, and the prune would then \
         delete the fresh registration, leaving the live re-published \
         memo slot invisible to `invalidate_canonical`. The victim's \
         reverse-index prune must run under the `entries` lock.",
    );
    prune_parked.wait();
    let populated = publisher.join().expect("publisher thread");
    assert_eq!(populated, 1, "C's publish landed one slot");

    // End-state: victim A's reverse-index registration is gone; B and C
    // — the two families within the cap — keep theirs intact.
    assert_eq!(
        store.memo_family_count_for_test(),
        2,
        "the memo holds exactly the two families within the cap (B, C)",
    );
    assert_eq!(
        store.canonical_to_entries_count("/w/a.ts"),
        0,
        "the FIFO-evicted victim A's reverse-index registration is pruned",
    );
    assert_eq!(
        store.canonical_to_entries_count("/w/b.ts"),
        1,
        "surviving family B's reverse-index registration is intact",
    );
    assert_eq!(
        store.canonical_to_entries_count("/w/c.ts"),
        1,
        "freshly-published family C's reverse-index registration is intact",
    );
}

/// FINDING 2 — register/cleanup symmetry. `register_reverse_index`
/// walks the UNION of the carrier's `canonical_ids()` and the
/// `dispatch_dep_signature`'s canonicals — a published entry whose
/// `dispatch_dep_signature` names a canonical the carrier rail does
/// NOT (notably the common `<project>` from
/// `project_generation_signature()`) registers a reverse-index entry
/// under that dispatch-only canonical too. The FIFO-eviction prune in
/// `record_family_admission_locked` MUST walk the SAME union; pruning
/// only the carrier's `canonical_ids()` strands the dispatch-only
/// registration after the family is FIFO-evicted.
///
/// Fixture:
///
/// - `memo_budget` cap pinned to 2.
/// - Family A's carrier names canonical `/w/a.ts` AND its
///   `dispatch_dep_signature` names canonical `<project>` (the
///   production `KeyOf`/`ProjectPath`/normalization-builder pattern
///   where the dispatch fence is a `project_generation_signature()`
///   — a single `(<project>, ProjectGeneration { g })` entry).
/// - Family B's carrier names `/w/b.ts`; dispatch is empty.
///
/// After A and B both published, `canonical_to_entries` holds shards
/// for `/w/a.ts`, `<project>`, and `/w/b.ts`. Publishing family C
/// FIFO-evicts A (the oldest).
///
/// Pre-fix prune iterates only `entry.read_set_signature.canonical_ids()`,
/// which yields `/w/a.ts` alone — `<project>`'s reverse-index shard
/// survives.
/// Post-fix prune walks `canonical_ids()` UNION
/// `entry.dispatch_dep_signature` canonicals — `<project>` is pruned
/// alongside `/w/a.ts`.
///
/// DISCRIMINATES: the assertion `canonical_to_entries_count("<project>")
/// == 0` after FIFO eviction FAILS pre-fix (registration survives) and
/// PASSES post-fix (registration pruned). The same shape the
/// register / cleanup symmetry rule enforces for the
/// cooperative-admission caches.
#[test]
fn fifo_eviction_prunes_dispatch_only_reverse_index_registration() {
    use crate::semantic_query::DepVersion;
    use verter_session_query::facts::fact_cache::FactVersionRef;

    // Cap of 2: the third distinct family evicts the first (FIFO).
    let store = Arc::new(SemanticGraphStore::new_with_memo_budget_for_test(2));

    // Family A: carrier rail names `/w/a.ts`; dispatch fence names
    // `<project>` (production `KeyOf` / `ProjectPath` /
    // normalization-builder dispatch shape — every such builder emits
    // a `project_generation_signature()` fence on top of whatever the
    // carrier's traced cross-file facts capture).
    let dispatch_fence_a: DepSignature = Arc::from(
        vec![(
            Arc::<str>::from("<project>"),
            DepVersion::ProjectGeneration(0),
        )]
        .into_boxed_slice(),
    );
    let node_a = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let key_a = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/a.ts"),
        name: Arc::from("A"),
    });
    let carrier_a =
        verter_session_query::facts::fact_cache::ReadSetSignature::new(Arc::from(vec![
            FactVersionRef::FileWholeHash {
                canonical_id: "/w/a.ts".to_string(),
                hash: [1u8; 16],
            },
        ]));
    store.publish_with_carrier_and_dispatch_for_tests(
        key_a,
        QueryResult::Value(node_a),
        carrier_a,
        Arc::from([]),
        dispatch_fence_a,
    );

    // Family B: carrier rail names `/w/b.ts`; dispatch is empty (a
    // builder whose fence is `empty_signature()`, contrast with A).
    let node_b = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let key_b = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/b.ts"),
        name: Arc::from("B"),
    });
    let carrier_b =
        verter_session_query::facts::fact_cache::ReadSetSignature::new(Arc::from(vec![
            FactVersionRef::FileWholeHash {
                canonical_id: "/w/b.ts".to_string(),
                hash: [2u8; 16],
            },
        ]));
    store.publish_with_carrier_for_tests(
        key_b,
        QueryResult::Value(node_b),
        carrier_b,
        Arc::from([]),
    );

    // Fixture invariants — A registered under `/w/a.ts` AND
    // `<project>`; B registered under `/w/b.ts`.
    assert_eq!(
        store.memo_family_count_for_test(),
        2,
        "fixture invariant: A and B are both warm",
    );
    assert_eq!(
        store.canonical_to_entries_count("/w/a.ts"),
        1,
        "fixture invariant: A's carrier registered a reverse-index entry \
         under /w/a.ts",
    );
    assert_eq!(
        store.canonical_to_entries_count("<project>"),
        1,
        "fixture invariant: A's dispatch_dep_signature registered a \
         reverse-index entry under <project> via register_reverse_index's \
         dispatch-fence union step",
    );
    assert_eq!(
        store.canonical_to_entries_count("/w/b.ts"),
        1,
        "fixture invariant: B's carrier registered a reverse-index entry \
         under /w/b.ts",
    );

    // Family C: a third distinct family. Publishing it FIFO-evicts
    // the oldest admission — family A.
    let node_c = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let key_c = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/c.ts"),
        name: Arc::from("C"),
    });
    let carrier_c =
        verter_session_query::facts::fact_cache::ReadSetSignature::new(Arc::from(vec![
            FactVersionRef::FileWholeHash {
                canonical_id: "/w/c.ts".to_string(),
                hash: [3u8; 16],
            },
        ]));
    let populated = store.publish_with_carrier_for_tests(
        key_c,
        QueryResult::Value(node_c),
        carrier_c,
        Arc::from([]),
    );
    assert_eq!(
        populated, 1,
        "C's publish landed one slot, triggering FIFO eviction of A",
    );

    // End-state — A's CARRIER reverse-index entry (`/w/a.ts`) is
    // pruned (the pre-existing FIFO prune path covers this), and A's
    // DISPATCH-ONLY reverse-index entry (`<project>`) is also pruned
    // post-fix. B and C survive intact (within the budget cap).
    assert_eq!(
        store.canonical_to_entries_count("/w/a.ts"),
        0,
        "the FIFO-evicted victim A's carrier-rail reverse-index entry \
         is pruned (existing behavior)",
    );
    assert_eq!(
        store.canonical_to_entries_count("<project>"),
        0,
        "REVERSE-INDEX DESYNC: the FIFO eviction's prune loop iterates \
         only the carrier's `read_set_signature.canonical_ids()` — it \
         SKIPS canonicals named exclusively in the victim's \
         `dispatch_dep_signature`. A's `<project>` reverse-index \
         registration (created via `register_reverse_index`'s \
         dispatch-fence union step) survives FIFO eviction, leaving a \
         stale `(family, slot)` pair under `<project>` in \
         `canonical_to_entries`. Across many bare \
         `bump_project_generation()` cycles every family memoising a \
         builder that emits `project_generation_signature()` (the \
         common `KeyOf` / `ProjectPath` / normalization-builder shape) \
         leaks a `<project>` reverse-index registration past its own \
         FIFO eviction, growing `canonical_to_entries` beyond the memo \
         budget. The prune path must walk the SAME union as \
         `register_reverse_index` — `canonical_ids()` PLUS \
         dispatch-fence canonicals — so register/cleanup are symmetric \
         on every path (the register/cleanup symmetry rule the \
         cooperative-admission caches enforce).",
    );
    assert_eq!(
        store.canonical_to_entries_count("/w/b.ts"),
        1,
        "surviving family B's reverse-index entry is intact",
    );
    assert_eq!(
        store.canonical_to_entries_count("/w/c.ts"),
        1,
        "freshly-published family C's reverse-index entry is intact",
    );
}

/// Multiple edges of the same kind on the same result are stored as a
/// list — walkers see all of them. This is the multi-derivation
/// support the contract requires.
#[test]
fn origin_multiple_edges_same_kind() {
    let store = SemanticGraphStore::new();
    let result = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let src_a = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let src_b = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));

    store.record_origin_edge(
        result,
        OriginEdgeKind::Normalize,
        Arc::from(vec![src_a].into_boxed_slice()),
        crate::semantic_query::OriginMeta::None,
        dep_sig_for("/w/a.ts", 1),
    );
    store.record_origin_edge(
        result,
        OriginEdgeKind::Normalize,
        Arc::from(vec![src_b].into_boxed_slice()),
        crate::semantic_query::OriginMeta::None,
        dep_sig_for("/w/b.ts", 2),
    );

    let edges = store.origins_of_kind(result, OriginEdgeKind::Normalize);
    assert_eq!(edges.len(), 2, "both Normalize derivations preserved");
    assert_eq!(store.origin_edge_count(), 2);
}

/// `origins(node)` returns every edge across kinds. Sources are
/// preserved verbatim from the recording call.
#[test]
fn origin_walk_returns_all_sources() {
    let store = SemanticGraphStore::new();
    let result = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let decl = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
    let arg = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    store.record_origin_edge(
        result,
        OriginEdgeKind::Instantiate,
        Arc::from(vec![decl, arg].into_boxed_slice()),
        crate::semantic_query::OriginMeta::None,
        dep_sig_for("/w/a.ts", 1),
    );

    let edges = store.origins(result);
    assert_eq!(edges.len(), 1);
    let (kind, edge) = &edges[0];
    assert_eq!(*kind, OriginEdgeKind::Instantiate);
    assert_eq!(edge.sources.as_ref(), &[decl, arg]);
}

/// `AliasResolve` edges from the unwrapped target back to the alias
/// declaration identity are walkable. Each hop emits one edge so a
/// chain is reconstructible.
#[test]
fn alias_resolve_edge_walk_returns_declaration_identity() {
    let store = SemanticGraphStore::new();
    let target = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let alias_decl = store.intern_node(SemanticNodeData::Alias(target));

    store.record_origin_edge(
        target,
        OriginEdgeKind::AliasResolve,
        Arc::from(vec![alias_decl].into_boxed_slice()),
        crate::semantic_query::OriginMeta::AliasName(Arc::from("AliasName")),
        dep_sig_for("/w/a.ts", 1),
    );

    let alias_edges = store.origins_of_kind(target, OriginEdgeKind::AliasResolve);
    assert_eq!(alias_edges.len(), 1);
    assert_eq!(alias_edges[0].sources.as_ref(), &[alias_decl]);
    assert!(matches!(
        &alias_edges[0].meta,
        crate::semantic_query::OriginMeta::AliasName(name) if name.as_ref() == "AliasName"
    ));
}

/// A barrel/re-export alias chain `X → Y → A` emits one
/// `AliasResolve` edge per hop and the chain is walkable end-to-end.
#[test]
fn alias_chain_multiple_hops_walk() {
    let store = SemanticGraphStore::new();
    let final_target = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let middle_alias = store.intern_node(SemanticNodeData::Alias(final_target));
    let outer_alias = store.intern_node(SemanticNodeData::Alias(middle_alias));

    // final_target ← middle_alias (one hop)
    store.record_origin_edge(
        final_target,
        OriginEdgeKind::AliasResolve,
        Arc::from(vec![middle_alias].into_boxed_slice()),
        crate::semantic_query::OriginMeta::None,
        dep_sig_for("/w/a.ts", 1),
    );
    // middle_alias ← outer_alias (second hop)
    store.record_origin_edge(
        middle_alias,
        OriginEdgeKind::AliasResolve,
        Arc::from(vec![outer_alias].into_boxed_slice()),
        crate::semantic_query::OriginMeta::None,
        dep_sig_for("/w/b.ts", 2),
    );

    // Walk from final_target — caller follows sources transitively.
    let mut chain: Vec<SemanticNodeId> = vec![final_target];
    let mut current = final_target;
    loop {
        let edges = store.origins_of_kind(current, OriginEdgeKind::AliasResolve);
        if edges.is_empty() {
            break;
        }
        current = edges[0].sources[0];
        chain.push(current);
    }
    assert_eq!(chain, vec![final_target, middle_alias, outer_alias]);
}

/// Multiple derivations of the SAME structural result store as
/// distinct edges with distinct dep-signatures. Walkers see all of
/// them — there is no "canonical publisher" shortcut.
#[test]
fn multiple_derivations_of_same_node_all_contribute_their_edges() {
    let store = SemanticGraphStore::new();
    let result = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let src1 = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let src2 = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

    // Two distinct Instantiate derivations producing the same result.
    store.record_origin_edge(
        result,
        OriginEdgeKind::Instantiate,
        Arc::from(vec![src1].into_boxed_slice()),
        crate::semantic_query::OriginMeta::None,
        dep_sig_for("/w/p1.ts", 1),
    );
    store.record_origin_edge(
        result,
        OriginEdgeKind::Instantiate,
        Arc::from(vec![src2].into_boxed_slice()),
        crate::semantic_query::OriginMeta::None,
        dep_sig_for("/w/p2.ts", 2),
    );

    let edges = store.origins_of_kind(result, OriginEdgeKind::Instantiate);
    assert_eq!(edges.len(), 2);
    let canonicals: Vec<&str> = edges
        .iter()
        .flat_map(|e| e.edge_dep_signature.iter().map(|(c, _)| c.as_ref()))
        .collect();
    assert!(canonicals.contains(&"/w/p1.ts"));
    assert!(canonicals.contains(&"/w/p2.ts"));
}

/// A purely structural node that no builder ever recorded an edge for
/// has zero origins — the walk yields nothing. Structural / primitive /
/// shared-literal nodes have no version identity, so this is correct.
#[test]
fn structural_node_has_zero_origin_edges() {
    let store = SemanticGraphStore::new();
    let primitive = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    let visited = store.origins(primitive);
    assert!(
        visited.is_empty(),
        "structural primitive node must have zero origin edges"
    );
    assert_eq!(store.origin_edge_count(), 0);
}

/// Edge dep-signature interning: two edges committed with identical
/// fences share one `Arc<DepSignature>` allocation.
#[test]
fn edge_dep_signatures_intern_identical_fences() {
    let store = SemanticGraphStore::new();
    let result = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let src = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    let sig = dep_sig_for("/w/shared.ts", 1);
    store.record_origin_edge(
        result,
        OriginEdgeKind::Instantiate,
        Arc::from(vec![src].into_boxed_slice()),
        crate::semantic_query::OriginMeta::None,
        sig.clone(),
    );
    store.record_origin_edge(
        result,
        OriginEdgeKind::Normalize,
        Arc::from(vec![src].into_boxed_slice()),
        crate::semantic_query::OriginMeta::None,
        sig.clone(),
    );

    let edges = store.origins(result);
    assert_eq!(edges.len(), 2);
    let arc1 = &edges[0].1.edge_dep_signature;
    let arc2 = &edges[1].1.edge_dep_signature;
    assert!(
        Arc::ptr_eq(arc1, arc2),
        "identical fences must share one interned Arc<DepSignature>"
    );
}

/// `record_path_length` and `record_projection_depth` push samples
/// into reservoirs whose p50 / p95 surface on the next snapshot.
#[test]
fn record_path_length_and_projection_depth_drive_percentiles() {
    let store = SemanticGraphStore::new();
    // Path lengths 1..=100 → p50 ≈ 50, p95 ≈ 95.
    for n in 1..=100u32 {
        store.record_path_length(n);
        store.record_projection_depth(n * 2);
    }
    let stats = store.stats_snapshot();
    // Nearest-rank percentile (R-3 / PERCENTILE.INC):
    //   idx = round((N-1) * p)
    // For N=100 samples sorted 1..=100:
    //   p50 → round(99 * 0.5) = round(49.5) = 50 → sorted[50] = 51
    //   p95 → round(99 * 0.95) = round(94.05) = 94 → sorted[94] = 95
    assert_eq!(stats.path_length_p50, 51);
    assert_eq!(stats.path_length_p95, 95);
    // projection_depth samples are 2..=200 step 2 (100 samples):
    //   sorted[50] = 2 * 51 = 102; sorted[94] = 2 * 95 = 190.
    assert_eq!(stats.projection_depth_p50, 102);
    assert_eq!(stats.projection_depth_p95, 190);
}

/// `origin_edges_per_node_p50/p95` are computed at snapshot time
/// from the derivation store directly — no separate sample
/// reservoir is needed because the store already records the full
/// per-node edge layout.
///
/// **Fixture rationale.** Minting 10 "distinct" nodes by calling
/// `intern_node(Primitive(Number))` ten times only works under an
/// append-only allocator. Under structural dedup, all 10 calls
/// converge on one [`SemanticNodeId`] and the per-node edge counts
/// collapse into a single `[1, 2, …, 10]`-edge list on one node.
///
/// The rewrite interns ten structurally-distinct payloads so the
/// implementation produces ten result nodes with a
/// `(1, 2, …, 10)` edge distribution. The assertion-intent — that
/// `origin_edges_per_node_p50/p95` derive correctly across N
/// distinct result nodes — is preserved; only the setup technique
/// changed.
#[test]
fn origin_edges_per_node_percentiles_derive_from_derivation_store() {
    use verter_type_expr::LiteralValue;
    let store = SemanticGraphStore::new();
    let src = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    // Ten structurally-distinct payloads. Under C7 compound-key
    // interning each returns its own [`SemanticNodeId`]. The same
    // assertion-intent is preserved: per-node edge counts sorted
    // ascending are [1, 2, …, 10] → p50 = 6, p95 = 10.
    let distinct_payloads: [SemanticNodeData; 10] = [
        SemanticNodeData::Primitive(PrimitiveKind::Number),
        SemanticNodeData::Primitive(PrimitiveKind::Boolean),
        SemanticNodeData::Primitive(PrimitiveKind::Symbol),
        SemanticNodeData::Primitive(PrimitiveKind::BigInt),
        SemanticNodeData::Primitive(PrimitiveKind::Never),
        SemanticNodeData::Literal(LiteralValue::String(String::from("a"))),
        SemanticNodeData::Literal(LiteralValue::String(String::from("b"))),
        SemanticNodeData::Literal(LiteralValue::Number(1.0)),
        SemanticNodeData::Literal(LiteralValue::Boolean(true)),
        SemanticNodeData::Literal(LiteralValue::Boolean(false)),
    ];
    let mut seen_ids: Vec<SemanticNodeId> = Vec::with_capacity(10);
    for (i, payload) in distinct_payloads.into_iter().enumerate() {
        let result = store.intern_node(payload);
        // Guard: the mechanism requires distinct ids. If any pair
        // aliases, the assertion below would silently pass because
        // origin-edge counts would cluster differently.
        assert!(
            !seen_ids.contains(&result),
            "fixture payload #{i} collided with an earlier one — \
             rewrite invalid",
        );
        seen_ids.push(result);
        for j in 0..=(i as u32) {
            // Each emission must carry a
            // distinct edge identity so the per-node ledger
            // observes (i+1) edges. Vary the dep_signature hash
            // per emission so the dedup at `record_origin_edge`
            // does NOT collapse them — the assertion-intent is
            // per-node edge counts across genuinely-distinct
            // derivations, which the dedup must NOT touch.
            let hash_byte = (j as u8).saturating_add(1);
            store.record_origin_edge(
                result,
                OriginEdgeKind::Instantiate,
                Arc::from(vec![src].into_boxed_slice()),
                crate::semantic_query::OriginMeta::None,
                dep_sig_for("/w/x.ts", hash_byte),
            );
        }
    }
    let stats = store.stats_snapshot();
    // Counts ascending = [1,2,3,4,5,6,7,8,9,10]; nearest-rank
    // p50 → idx round(9 * 0.5) = 5 → 6; p95 → idx round(9 * 0.95) = 9 → 10.
    assert_eq!(stats.origin_edges_per_node_p50, 6);
    assert_eq!(stats.origin_edges_per_node_p95, 10);
}

/// `walk_origin_chain` must release the derivation lock before
/// invoking the visitor — otherwise a visitor that walks the chain
/// transitively (e.g. by calling `origins_of_kind` to follow
/// sources) would deadlock on the non-reentrant `parking_lot::Mutex`.
/// The test materialises edges, then has the visitor call back into
/// the store; if the lock is still held when the visitor runs, the
/// re-entry hangs and the test times out.
#[test]
fn walk_origin_chain_releases_derivation_lock_before_visitor() {
    let store = SemanticGraphStore::new();
    let target = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let alias_decl = store.intern_node(SemanticNodeData::Alias(target));
    store.record_origin_edge(
        target,
        OriginEdgeKind::AliasResolve,
        Arc::from(vec![alias_decl].into_boxed_slice()),
        crate::semantic_query::OriginMeta::None,
        dep_sig_for("/w/x.ts", 1),
    );

    let mut visited_count = 0usize;
    store.walk_origin_chain(target, |_kind, _edge| {
        // Recursive call back into the store from inside the
        // visitor — would deadlock if the visitor still held the
        // derivation lock.
        let _ = store.origins(target);
        let _ = store.origins_of_kind(target, OriginEdgeKind::AliasResolve);
        visited_count += 1;
    });
    assert_eq!(visited_count, 1, "the single recorded edge was visited");
}

/// BOUND PROOF — the derivation store's `edges` map MUST NOT grow
/// monotonically with the content-edit count.
///
/// Each content edit interns fresh `SemanticNodeId`s, so each edit's
/// origin edges land in fresh `(result, kind)` buckets. Without a
/// retention bound the bucket count grew +N per edit forever (the
/// identity-tuple dedup only suppresses a re-publish of the SAME node
/// id, never a new content version's fresh ids).
///
/// DISCRIMINATES: against the pre-fix tree the `DerivationStore` had no
/// bound — recording 4096 + 600 distinct buckets left all of them
/// resident. After the fix the FIFO `edge_budget` caps the bucket count
/// at `DERIVATION_EDGE_BUCKET_CAP`. The assertion bound is the store's
/// own published cap, so the test stays correct if the cap is tuned.
#[test]
fn derivation_store_bounds_edge_bucket_growth() {
    use crate::semantic_query_memo::derivation::DERIVATION_EDGE_BUCKET_CAP;

    let store = SemanticGraphStore::new();
    // Record more distinct `(result, kind)` buckets than the cap. Each
    // `result` is a fresh node id (a distinct `Alias`-chain link), so
    // every `record_origin_edge` opens a brand-new bucket — exactly the
    // "fresh ids per content version" growth the bound must contain.
    let bucket_count = DERIVATION_EDGE_BUCKET_CAP + 600;
    let mut prev = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    for _ in 0..bucket_count {
        let result = store.intern_node(SemanticNodeData::Alias(prev));
        store.record_origin_edge(
            result,
            OriginEdgeKind::Normalize,
            Arc::from(vec![prev].into_boxed_slice()),
            crate::semantic_query::OriginMeta::None,
            dep_sig_for("/w/a.ts", 1),
        );
        prev = result;
    }

    let live_buckets = store.derivation_bucket_count();
    assert!(
        live_buckets <= DERIVATION_EDGE_BUCKET_CAP,
        "bounded retention proof: after recording {bucket_count} distinct \
         derivation buckets the DerivationStore must stay bounded by its \
         edge-bucket cap ({DERIVATION_EDGE_BUCKET_CAP}), not grow with the \
         edit count. Observed live buckets={live_buckets}.",
    );
    // Discrimination floor — the store is still retaining its newest
    // buckets (it is not empty), so the bound is a cap, not a wipe.
    assert!(
        live_buckets >= 1,
        "the derivation store must still retain its most recent buckets — \
         observed live buckets={live_buckets}",
    );
    // The most-recently recorded bucket survived (FIFO evicts oldest).
    assert_eq!(
        store.origins_of_kind(prev, OriginEdgeKind::Normalize).len(),
        1,
        "the newest derivation bucket must be retained under FIFO eviction",
    );
}

/// BOUND PROOF — the derivation store's `signature_pool` interning map
/// MUST NOT grow monotonically with the count of distinct fences. The
/// pool stores `Weak` values whose lifetime is tied to the edges that
/// reference them; the `edges` map is itself bounded by `edge_budget`, so
/// the count of LIVE pooled signatures is bounded by the live-edge count.
///
/// DISCRIMINATES: an unbounded `FxHashMap` of strong `Arc`s would keep
/// every distinct `DepSignature` fence resident forever — recording
/// `DERIVATION_EDGE_BUCKET_CAP + 600` distinct fences would leave all of
/// them live. With the `Weak`-valued pool, evicting an edge bucket (FIFO
/// past `edge_budget`) drops the strong `Arc`s its edges held, so the
/// corresponding pooled `Weak`s go dead and stop counting toward the live
/// pool size.
#[test]
fn derivation_store_bounds_signature_pool_growth() {
    use crate::semantic_query_memo::derivation::DERIVATION_EDGE_BUCKET_CAP;

    let store = SemanticGraphStore::new();
    // Emit more distinct fences than the edge-bucket cap. Each edge gets
    // a distinct fence AND a fresh `result` node (a distinct `Alias`-chain
    // link), so every emission opens its own `(result, kind)` bucket. Once
    // the bucket count exceeds `edge_budget`, the oldest buckets are
    // FIFO-evicted — dropping the only strong `Arc`s to their fences, so
    // those pooled `Weak`s go dead.
    let fence_count = DERIVATION_EDGE_BUCKET_CAP + 600;
    let mut prev = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    for i in 0..fence_count {
        let result = store.intern_node(SemanticNodeData::Alias(prev));
        let canonical = format!("/w/f{i}.ts");
        store.record_origin_edge(
            result,
            OriginEdgeKind::Normalize,
            Arc::from(vec![prev].into_boxed_slice()),
            crate::semantic_query::OriginMeta::None,
            dep_sig_for(&canonical, (i % 251) as u8),
        );
        prev = result;
    }
    // Live pooled signatures = signatures still reachable from a surviving
    // edge bucket. `edges` is capped at `DERIVATION_EDGE_BUCKET_CAP`, and
    // each surviving bucket holds exactly one edge → one live fence — so
    // the live pool size cannot exceed the bucket cap.
    let live_pool_size = store.derivation_signature_pool_size();
    assert!(
        live_pool_size <= DERIVATION_EDGE_BUCKET_CAP,
        "bounded retention proof: after interning {fence_count} distinct \
         fences the DerivationStore's LIVE signature pool must stay \
         bounded by the edge-bucket cap ({DERIVATION_EDGE_BUCKET_CAP}) — \
         a `Weak` goes dead when its edge bucket is FIFO-evicted. Observed \
         live pool size={live_pool_size}.",
    );
    // Discrimination floor — the pool still retains its newest live
    // signatures (it is not a wipe).
    assert!(
        live_pool_size >= 1,
        "the signature pool must still retain the fences of its surviving \
         edge buckets — observed live pool size={live_pool_size}",
    );
}

/// ORIGIN-EDGE DEDUP DURABILITY — re-emitting an origin edge whose fence
/// was driven out of the interning pool's reclamation reach by a flood of
/// other distinct fences MUST still deduplicate. `record_origin_edge`
/// probes for an existing edge with `Arc::ptr_eq` on the interned
/// `edge_dep_signature`; the interner therefore has to keep handing back
/// the SAME `Arc<DepSignature>` for an identical fence value for as long
/// as a live edge references it.
///
/// DISCRIMINATES: an interner that bounded `signature_pool` with an
/// independent FIFO cap would, when flooded with `cap + N` other distinct
/// fences, evict the original fence's pool entry even though the first
/// edge still held its `Arc`. The re-emit's `intern_signature` would then
/// allocate a FRESH `Arc`, `Arc::ptr_eq` would miss, and the edge would
/// be recorded a SECOND time — the bucket would grow to two `OriginEdge`s
/// and `origin_edges_emitted` would double-count. With the `Weak`-valued
/// pool the original fence's entry upgrades successfully (the first edge
/// keeps it alive), the same `Arc` is reused, and the re-emit
/// deduplicates.
#[test]
fn origin_edge_dedup_survives_signature_pool_flood() {
    let store = SemanticGraphStore::new();

    // Distinct, stable nodes for the edge under test.
    let result = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let source = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let original_fence = dep_sig_for("/w/original.ts", 7);

    // Step 1 — emit the original edge. Its interned dep-signature `Arc`
    // is now held by the stored `OriginEdge`.
    store.record_origin_edge(
        result,
        OriginEdgeKind::Normalize,
        Arc::from(vec![source].into_boxed_slice()),
        crate::semantic_query::OriginMeta::None,
        original_fence.clone(),
    );
    assert_eq!(
        store
            .origins_of_kind(result, OriginEdgeKind::Normalize)
            .len(),
        1,
        "pre-flood: the original edge is recorded exactly once",
    );
    assert_eq!(
        store.stats_snapshot().origin_edges_emitted,
        1,
        "pre-flood: exactly one origin-edge emission counted",
    );

    // Step 2 — drive a flood of distinct fences through the SAME store so
    // any independent FIFO cap on the pool would evict the original
    // fence's pool entry. Each flood edge targets ONE shared `result`
    // node so the flood adds a single extra `(result, kind)` bucket
    // (never enough to evict the bucket under test) — this isolates the
    // test to signature-pool reclamation, not edge-bucket eviction. A
    // count well past any plausible pool cap guarantees the original
    // fence would be FIFO-evicted under the pre-fix mechanism.
    let flood_target = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let flood_count = 4096 + 1024;
    for i in 0..flood_count {
        let canonical = format!("/w/flood-{i}.ts");
        store.record_origin_edge(
            flood_target,
            OriginEdgeKind::Normalize,
            Arc::from(vec![source].into_boxed_slice()),
            crate::semantic_query::OriginMeta::None,
            dep_sig_for(&canonical, (i % 211) as u8),
        );
    }

    // Step 3 — re-emit the ORIGINAL edge with a fresh-but-equal fence
    // value. The interner must upgrade the original fence's still-live
    // `Weak` and hand back the SAME `Arc` the first edge holds, so the
    // `Arc::ptr_eq` dedup probe matches and the re-emit is suppressed.
    store.record_origin_edge(
        result,
        OriginEdgeKind::Normalize,
        Arc::from(vec![source].into_boxed_slice()),
        crate::semantic_query::OriginMeta::None,
        dep_sig_for("/w/original.ts", 7),
    );

    // The `(result, Normalize)` bucket must still hold exactly ONE edge.
    let bucket = store.origins_of_kind(result, OriginEdgeKind::Normalize);
    assert_eq!(
        bucket.len(),
        1,
        "DEDUP BUG: re-emitting an identical origin edge after a \
         signature-pool flood must deduplicate — the `(result, Normalize)` \
         bucket grew to {} edges. A FIFO-capped pool evicted the original \
         fence's interned `Arc`, so the re-emit allocated a fresh `Arc`, \
         `Arc::ptr_eq` missed, and the duplicate was recorded.",
        bucket.len(),
    );
    // …and the cumulative `origin_edges_emitted` counter must reflect
    // exactly `1 (original) + flood_count` ledger writes — the re-emit
    // was deduplicated, so it must NOT have bumped the counter. Every
    // flood fence is distinct, so all `flood_count` flood edges are
    // genuine (non-duplicate) emissions.
    let emitted = store.stats_snapshot().origin_edges_emitted;
    assert_eq!(
        emitted,
        1 + flood_count as u64,
        "DEDUP BUG: `origin_edges_emitted` must be 1 + {flood_count} \
         after the dedup'd re-emit — observed {emitted}. A higher count \
         means the re-emitted original edge double-counted because its \
         pooled signature `Arc` was evicted and re-allocated.",
    );
}

/// BOUND PROOF — a SINGLE `(result, kind)` derivation bucket's
/// `Vec<OriginEdge>` MUST NOT grow without bound.
///
/// `record` (the write-side of `record_origin_edge`) appends one
/// `OriginEdge` per distinct derivation of the same structural
/// `result` for the same `kind`. Distinct derivations carry distinct
/// fences, so the identity-tuple dedup at `record_origin_edge` never
/// suppresses them — they all land in the SAME `(result, kind)`
/// bucket. In a long-lived session that re-derives one result many
/// times, that one bucket grows monotonically, and each retained
/// `OriginEdge` keeps its interned dep-signature `Arc` alive, so the
/// `Weak`-based signature pool stays live for every same-bucket fence.
///
/// DISCRIMINATES: against HEAD `397a51211` the per-bucket edge growth
/// is unbounded — the FIFO `edge_budget` only records an admission for
/// a NEWLY-KEYED bucket (`if is_new_bucket`), so appending another
/// distinct edge to an EXISTING bucket bypasses the budget entirely.
/// Recording `DERIVATION_EDGES_PER_BUCKET_CAP + 600` distinct edges
/// into one bucket leaves all of them resident (and all their fences
/// pool-live). After the fix the per-bucket FIFO cap evicts the oldest
/// edge on every append past the cap, so the bucket length stays at /
/// under `DERIVATION_EDGES_PER_BUCKET_CAP` and an evicted edge drops
/// its `Arc<DepSignature>` — once the last edge holding a pooled fence
/// is evicted that fence's `Weak` goes dead and stops counting toward
/// the live pool size.
#[test]
fn derivation_store_bounds_per_bucket_edge_growth() {
    use crate::semantic_query_memo::derivation::{
        DERIVATION_EDGES_PER_BUCKET_CAP, DERIVATION_EDGE_BUCKET_CAP,
    };

    let store = SemanticGraphStore::new();
    // ONE shared `(result, kind)` bucket. `result` and the lone
    // `source` are fixed, stable node ids — every emission targets the
    // SAME `(result, Normalize)` key, so this exercises per-bucket
    // growth in isolation (the bucket COUNT stays at 1, well under
    // `DERIVATION_EDGE_BUCKET_CAP`, so bucket-level FIFO never fires).
    let result = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let source = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    // Emit far more DISTINCT edges into that one bucket than the
    // per-bucket cap. Each edge carries a distinct fence, so the
    // `record_origin_edge` identity-tuple dedup never suppresses it —
    // every emission is a genuine `store.record` append into the SAME
    // bucket.
    let edge_count = DERIVATION_EDGES_PER_BUCKET_CAP + 600;
    for i in 0..edge_count {
        let canonical = format!("/w/edge-{i}.ts");
        store.record_origin_edge(
            result,
            OriginEdgeKind::Normalize,
            Arc::from(vec![source].into_boxed_slice()),
            crate::semantic_query::OriginMeta::None,
            dep_sig_for(&canonical, (i % 251) as u8),
        );
    }

    // (1) The single bucket's `Vec<OriginEdge>` must stay at / under
    // the per-bucket cap — NOT grow to `edge_count`.
    let bucket_len = store
        .origins_of_kind(result, OriginEdgeKind::Normalize)
        .len();
    assert!(
        bucket_len <= DERIVATION_EDGES_PER_BUCKET_CAP,
        "bounded retention proof: after recording {edge_count} distinct \
         origin edges into ONE (result, Normalize) bucket the bucket's \
         Vec<OriginEdge> must stay bounded by the per-bucket cap \
         ({DERIVATION_EDGES_PER_BUCKET_CAP}), not grow with the \
         derivation count. Observed bucket length={bucket_len}.",
    );
    // Discrimination floor — the bucket still retains its newest edges
    // (it is a cap, not a wipe).
    assert!(
        bucket_len >= 1,
        "the derivation bucket must still retain its most recent edges — \
         observed bucket length={bucket_len}",
    );
    // The total derivation edge count across the whole store is the
    // same single bucket — also bounded by the per-bucket cap.
    let total_edges = store.origin_edge_count();
    assert!(
        total_edges <= DERIVATION_EDGES_PER_BUCKET_CAP,
        "the store's total origin-edge count must equal the one bounded \
         bucket — observed total_edges={total_edges}",
    );

    // (2) The `Weak`-based signature pool must not retain entries for
    // edges the per-bucket cap evicted. Every emitted fence is
    // distinct; once an edge is FIFO-evicted from the bucket its
    // `Arc<DepSignature>` drops, so its pooled `Weak` goes dead. The
    // count of LIVE pooled signatures therefore cannot exceed the
    // surviving edge count, which is bounded by the per-bucket cap.
    let live_pool_size = store.derivation_signature_pool_size();
    assert!(
        live_pool_size <= DERIVATION_EDGES_PER_BUCKET_CAP,
        "bounded retention proof: after interning {edge_count} distinct \
         fences into ONE bucket the LIVE signature pool must stay \
         bounded by the per-bucket cap ({DERIVATION_EDGES_PER_BUCKET_CAP}) \
         — a `Weak` goes dead when its edge is FIFO-evicted from the \
         bucket. Observed live pool size={live_pool_size}.",
    );
    // The bucket count stayed at 1 throughout — this test isolates
    // per-bucket growth, never the bucket-level FIFO budget.
    assert_eq!(
        store.derivation_bucket_count(),
        1,
        "this test exercises ONE bucket — the bucket count must stay 1, \
         well under DERIVATION_EDGE_BUCKET_CAP ({DERIVATION_EDGE_BUCKET_CAP})",
    );
}

// ──────────────────────────────────────────────────────────────────
// NodeScopeId origin-scope sidecar
//
// The sidecar records where each non-exempt node was first interned.
// Dispatch builders query `node_scope(id)` to reconstruct the
// originating scope and route per-base-scope lookups through the
// correct `SessionSolverHost`.
// ──────────────────────────────────────────────────────────────────

/// Every non-exempt `intern_node_with_scope` call populates the
/// sidecar at intern time. Plain `intern_node` records `Global`.
#[test]
fn node_scope_sidecar_populated_at_intern_time_for_every_decl_origin_node() {
    let store = SemanticGraphStore::new();

    // Non-exempt scope-bound origin (e.g. `build_resolve_decl` /
    // `build_instantiate` result).
    let scope = NodeScopeId::File {
        canonical_id: Arc::from("/w/decl.ts"),
        owner: TopLevelOwnerId::ordinary_file(),
        whole_hash: [7u8; 16],
        local_scope: None,
    };
    let decl_id = store.intern_node_with_scope(
        SemanticNodeData::Primitive(PrimitiveKind::String),
        scope.clone(),
    );
    assert_eq!(
        store.node_scope(decl_id),
        Some(scope.clone()),
        "decl-origin node must record its scope in the sidecar",
    );

    // Helper intermediate / structural node (no scope-bound origin).
    let global_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    assert_eq!(
        store.node_scope(global_id),
        Some(NodeScopeId::Global),
        "scope-less intern_node must record Global",
    );

    // Multiple non-exempt nodes get independent sidecar slots.
    let scope_b = NodeScopeId::File {
        canonical_id: Arc::from("/w/other.ts"),
        owner: TopLevelOwnerId::ordinary_file(),
        whole_hash: [8u8; 16],
        local_scope: Some(3),
    };
    let decl_b_id = store.intern_node_with_scope(
        SemanticNodeData::Primitive(PrimitiveKind::Never),
        scope_b.clone(),
    );
    assert_eq!(store.node_scope(decl_b_id), Some(scope_b));
    // First node's scope is unchanged (the sidecar is per-id, not
    // shared across interns).
    assert_eq!(store.node_scope(decl_id), Some(scope));
}

/// `node_scope(id)` returns the **origin** scope (where the node was
/// first interned), not the reader's scope. Dispatch builders on
/// scope B who query a node interned in scope A observe scope A.
#[test]
fn node_scope_returns_origin_not_reader_scope() {
    let store = SemanticGraphStore::new();
    let scope_a = NodeScopeId::File {
        canonical_id: Arc::from("/w/a.ts"),
        owner: TopLevelOwnerId::ordinary_file(),
        whole_hash: [1u8; 16],
        local_scope: None,
    };
    let scope_b = NodeScopeId::File {
        canonical_id: Arc::from("/w/b.ts"),
        owner: TopLevelOwnerId::ordinary_file(),
        whole_hash: [2u8; 16],
        local_scope: None,
    };

    // Node interned from scope A.
    let id = store.intern_node_with_scope(
        SemanticNodeData::Primitive(PrimitiveKind::String),
        scope_a.clone(),
    );

    // Reader from scope B queries the sidecar — the sidecar returns
    // scope A, not scope B.
    let observed = store.node_scope(id);
    assert_eq!(observed, Some(scope_a));
    assert_ne!(observed, Some(scope_b));
}

/// Counter taxonomy cross-check: the three new fields appear on the
/// debug-dump snapshot and are zero by default. Complements the
/// `counter_taxonomy_matches_plan` test in
/// `crates/verter_type_engine/src/semantic_query.rs` which enforces
/// the §6.3 bidirectional equality.
#[test]
fn counter_taxonomy_matches_plan_covers_new_counters() {
    let stats = SemanticGraphStats::default();
    let debug = format!("{stats:?}");
    for field in [
        "joined_waits",
        "inflight_aborted_retries",
        "cold_aborts_swept",
    ] {
        assert!(
            debug.contains(&format!("{field}: 0")),
            "SemanticGraphStats default must publish `{field}: 0` — missing \
             field indicates the counter extension did not ship",
        );
    }

    // Live store must expose the same defaults via stats_snapshot.
    let store = SemanticGraphStore::new();
    let snap = store.stats_snapshot();
    assert_eq!(snap.joined_waits, 0);
    assert_eq!(snap.inflight_aborted_retries, 0);
    assert_eq!(snap.cold_aborts_swept, 0);
}

/// `invalidate_canonical`'s fact-rail drain discriminates entries via
/// `carrier_facts_reference_canonical`; a `FileSourceEnv` contributor
/// fact must make the contributor canonical a reverse-index member so
/// evicting the contributor reaches the entry.
#[test]
fn carrier_facts_reference_canonical_matches_file_source_env_contributor() {
    let facts = [
        verter_session_query::facts::fact_cache::FactVersionRef::FileSourceEnv {
            canonical_id: "/contrib.d.ts".to_string(),
            parse_env_hash: verter_session_query::facts::fact_cache::ParseEnvHash::from_env_hash(
                [3u8; 16],
            ),
            parse_key: verter_session_query::source::toolchain::parse_key_for_test("/contrib.d.ts", 2),
            file_language_id:
                verter_session_query::source::artifact_key::FileArtifactKey::synthetic_file_language_for_test(
                    "/contrib.d.ts",
                ),
        },
    ];
    assert!(
        carrier_facts_reference_canonical(&facts, "/contrib.d.ts"),
        "a FileSourceEnv fact must register its contributor canonical for the drain"
    );
    assert!(
        !carrier_facts_reference_canonical(&facts, "/other.d.ts"),
        "an unrelated canonical must not match the contributor fact"
    );
}

/// The fact-rail drain reaches an entry through a consumed result's
/// receipt however deep the edited canonical sits: a change to the file a
/// 1,024-level receipt chain bottoms out in reaches the entry holding its
/// top, and a file no level names does not.
#[test]
fn carrier_facts_reference_a_canonical_deep_in_a_receipt_chain() {
    use verter_session_query::facts::fact_cache::FactVersionRef;
    let whole = |canonical: String| FactVersionRef::FileWholeHash {
        canonical_id: canonical,
        hash: [7u8; 16],
    };
    let mut receipt = verter_session_query::facts::fact_cache::ResultReceipt::new(vec![whole(
        "/deep/0.ts".into(),
    )]);
    for level in 1..1_024 {
        receipt = verter_session_query::facts::fact_cache::ResultReceipt::new(vec![
            FactVersionRef::Receipt(receipt),
            whole(format!("/deep/{level}.ts")),
        ]);
    }
    let facts = [FactVersionRef::Receipt(receipt)];
    assert!(
        carrier_facts_reference_canonical(&facts, "/deep/0.ts"),
        "the deepest level's canonical must reach the entry"
    );
    assert!(carrier_facts_reference_canonical(&facts, "/deep/1023.ts"));
    assert!(
        !carrier_facts_reference_canonical(&facts, "/deep/unrelated.ts"),
        "a canonical no level names must not reach the entry"
    );
}

/// A document close releases EVERYTHING the semantic substrate retained
/// for the closed canonical — its memo entries (drained through the
/// reverse index AND through the released-id key / result sweep), its
/// node payloads plus the nodes embedding them (the cascade reaches a
/// Global alias shell over the closed object), its `unresolved_reach`
/// bits, its member-ordinal index, its origin edges, and the relation
/// proofs / relate keys naming its nodes — while the neighbour document's
/// entries, nodes, sidecars and proofs, and the shared Global primitive,
/// stay intact and still dedup.
///
/// On the old code a close reached only `invalidate_canonical`: the
/// three `/w/a.ts` nodes stayed live (`node_count` would read 7, not 4),
/// `unresolved_reach` kept every bit (7, not 4), the member-ordinal index
/// kept both entries, both relate keys and all four proofs stayed
/// interned, and the two entries whose carriers name only `/w/b.ts` but
/// whose key / result hold `/w/a.ts` nodes kept serving them.
#[test]
fn release_canonical_reclaims_the_closed_documents_substrate_and_keeps_the_neighbours() {
    use crate::semantic_query::{
        BudgetExceededKind, OriginEdgeKind, OriginMeta, RecursionOrBudgetCap, RelateMemoKey,
        RelationContext, RelationFailureCode, RelationProof, SubRelationPosition, SubRelationRef,
    };

    let store = SemanticGraphStore::new();
    let shared = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let (a_param, a_obj, a_alias, a_view) = release_intern_document(&store, "/w/a.ts", 1, shared);
    let (b_param, b_obj, b_alias, b_view) = release_intern_document(&store, "/w/b.ts", 2, shared);
    assert_eq!(store.node_count(), 7);
    assert_eq!(store.node_slot_count(), 7);

    // Memo entries. `key_a` / `key_b` are the ordinary self-rooted
    // entries the reverse index drains. `key_b_holding_a` holds A's node
    // as its RESULT and `key_path_on_a` embeds A's node in its KEY, yet
    // both carriers name only `/w/b.ts` — the shapes only the released-id
    // sweep can find.
    let key_a = release_decl_key("/w/a.ts", "A");
    let key_b = release_decl_key("/w/b.ts", "B");
    let key_b_holding_a = release_decl_key("/w/b.ts", "ViaA");
    let key_path_on_a = SemanticQueryKey::ProjectPath {
        base: a_obj,
        path: family_test_path(),
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Shallow,
        ),
    };
    let roots_a: Arc<[Arc<str>]> = Arc::from(vec![Arc::<str>::from("/w/a.ts")]);
    let roots_b: Arc<[Arc<str>]> = Arc::from(vec![Arc::<str>::from("/w/b.ts")]);
    store.publish_with_carrier_for_tests(
        key_a.clone(),
        QueryResult::Value(a_obj),
        carrier_naming("/w/a.ts", 1),
        Arc::clone(&roots_a),
    );
    store.publish_with_carrier_for_tests(
        key_b.clone(),
        QueryResult::Value(b_obj),
        carrier_naming("/w/b.ts", 2),
        Arc::clone(&roots_b),
    );
    store.publish_with_carrier_for_tests(
        key_b_holding_a.clone(),
        QueryResult::Value(a_obj),
        carrier_naming("/w/b.ts", 2),
        Arc::clone(&roots_b),
    );
    store.publish_with_carrier_for_tests(
        key_path_on_a.clone(),
        QueryResult::Value(shared),
        carrier_naming("/w/b.ts", 2),
        Arc::clone(&roots_b),
    );
    // Four publishes; the `Shallow` path publish also backfills its
    // narrower sibling slot, so the populated-slot count is one higher.
    let memo_entries_before = store.memo_entry_count();
    assert_eq!(memo_entries_before, 5);

    // Per-node sidecars.
    assert!(!store.node_reaches_unresolved(a_alias));
    assert!(!store.node_reaches_unresolved(b_alias));
    assert_eq!(
        store.unresolved_reach_count(),
        7,
        "alias + object + param per document, plus the shared primitive once"
    );
    let _ = store.member_ordinal_index(a_obj, &a_view);
    let _ = store.member_ordinal_index(b_obj, &b_view);
    store.record_origin_edge(
        a_obj,
        OriginEdgeKind::Instantiate,
        Arc::from(vec![a_param].into_boxed_slice()),
        OriginMeta::None,
        dep_sig_for("/w/a.ts", 1),
    );
    store.record_origin_edge(
        b_obj,
        OriginEdgeKind::Instantiate,
        Arc::from(vec![b_param].into_boxed_slice()),
        OriginMeta::None,
        dep_sig_for("/w/b.ts", 2),
    );

    // Relation tables: one relate key and one negative proof per
    // document, a cycle proof over A's key, and a budget proof with no
    // node at all.
    let key_id_a = store.intern_relate_key(RelateMemoKey::assignable(
        a_obj,
        shared,
        RelationContext::default(),
    ));
    let key_id_b = store.intern_relate_key(RelateMemoKey::assignable(
        b_obj,
        shared,
        RelationContext::default(),
    ));
    let proof_a = store.intern_relation_proof(RelationProof::NotAssignable {
        reason: RelationFailureCode::Structural,
        failing_sub: SubRelationRef {
            source: a_param,
            target: shared,
            position: SubRelationPosition::Root,
        },
    });
    let proof_b = store.intern_relation_proof(RelationProof::NotAssignable {
        reason: RelationFailureCode::Structural,
        failing_sub: SubRelationRef {
            source: b_param,
            target: shared,
            position: SubRelationPosition::Root,
        },
    });
    let proof_cycle_a = store.intern_relation_proof(RelationProof::CoinductiveCycle {
        keys: Arc::from(vec![key_id_a].into_boxed_slice()),
    });
    let proof_budget = store.intern_relation_proof(RelationProof::BudgetExceeded {
        cap: RecursionOrBudgetCap {
            kind: BudgetExceededKind::RelationBudget,
            limit: 0,
        },
    });
    assert_eq!(store.relate_key_count(), 2);
    assert_eq!(store.relation_proof_count(), 4);

    let report = store.release_canonical("/w/a.ts");

    assert_eq!(
        report.nodes_released, 3,
        "A's param + object, plus the Global alias embedding the object: {report:?}"
    );
    for dead in [a_param, a_obj, a_alias] {
        assert!(!store.node_is_live(dead), "{dead:?} must be released");
        assert!(
            matches!(
                store.node_data(dead).as_deref(),
                Some(SemanticNodeData::Opaque(QueryError::Miss))
            ),
            "a released id reads as the Opaque(Miss) placeholder"
        );
        assert_eq!(store.node_scope(dead), None);
    }
    for live in [b_param, b_obj, b_alias, shared] {
        assert!(store.node_is_live(live), "{live:?} must stay live");
    }
    assert_eq!(store.node_count(), 4, "live nodes: B's three + shared");
    assert_eq!(store.node_slot_count(), 7, "ids are never reused");

    assert!(
        store.get_unvalidated(&key_a).is_none(),
        "A's own entry drained"
    );
    assert!(
        store.get_unvalidated(&key_b_holding_a).is_none(),
        "an entry whose RESULT is a released node is swept even though its carrier never named /w/a.ts"
    );
    assert!(
        store.get_unvalidated(&key_path_on_a).is_none(),
        "an entry whose KEY embeds a released node is swept even though its carrier never named /w/a.ts"
    );
    assert!(store.get_unvalidated(&key_b).is_some(), "B's entry intact");
    assert_eq!(store.memo_entry_count(), 1);
    assert_eq!(
        report.memo_entries_evicted,
        memo_entries_before - 1,
        "every populated slot but B's own was evicted"
    );
    assert_eq!(store.canonical_to_entries_count("/w/a.ts"), 0);
    assert_eq!(store.canonical_to_entries_count("/w/b.ts"), 1);
    assert_eq!(
        store.memo_family_count_for_test(),
        store.memo_budget_tracked_len_for_test(),
        "the family budget ledger tracks exactly the surviving families"
    );

    assert_eq!(store.unresolved_reach_count(), 4, "B's three bits + shared");
    assert_eq!(report.unresolved_reach_dropped, 3);
    assert_eq!(report.member_indexes_dropped, 1);
    assert_eq!(report.derivation_buckets_dropped, 1);
    assert!(store.origins(a_obj).is_empty(), "A's origin edges dropped");
    assert_eq!(store.origins(b_obj).len(), 1, "B's origin edge kept");

    assert_eq!(store.relate_key_count(), 1);
    assert_eq!(store.relation_proof_count(), 2);
    assert_eq!(report.relate_keys_released, 1);
    assert_eq!(report.relation_proofs_released, 2);
    assert!(store.relate_key_for_id(key_id_a).is_none());
    assert!(store.relate_key_for_id(key_id_b).is_some());
    assert!(store.relation_proof_for(proof_a).is_none());
    assert!(store.relation_proof_for(proof_cycle_a).is_none());
    assert!(store.relation_proof_for(proof_b).is_some());
    assert!(store.relation_proof_for(proof_budget).is_some());

    // Dedup: A's content mints fresh ids; B's and the shared primitive
    // still dedup to their existing ids.
    let a_param_again = store.intern_node_with_scope(
        release_type_param("/w/a.ts", 1, "T"),
        release_file_scope("/w/a.ts", 1),
    );
    assert_ne!(
        a_param_again, a_param,
        "a released id is never handed out again"
    );
    assert_eq!(
        store.intern_node_with_scope(
            release_type_param("/w/b.ts", 2, "T"),
            release_file_scope("/w/b.ts", 2)
        ),
        b_param
    );
    assert_eq!(
        store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String)),
        shared
    );
}

/// Twenty open / edit-to-identical-content / close cycles of one
/// document keep the LIVE substrate flat: after the first cycle the live
/// node count, memo entries, reach bits and member indexes at the "open"
/// point are exactly the first cycle's, and every close returns them to
/// the baseline. Only the append-only id space grows — by exactly the
/// document's three nodes per cycle, because a released id is never
/// reused.
///
/// On the old code the close path could only reach `invalidate_canonical`,
/// which dropped the dedup entries but never a payload: `node_count`
/// (then the slot count) grew by three every cycle — 4, 7, 10, … — so the
/// flat assertion failed on cycle 1, and the reach bits / member indexes
/// accumulated with it.
#[test]
fn release_canonical_keeps_the_live_substrate_flat_across_open_close_cycles() {
    let store = SemanticGraphStore::new();
    let shared = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let baseline_live = store.node_count();
    let baseline_slots = store.node_slot_count();
    let roots_a: Arc<[Arc<str>]> = Arc::from(vec![Arc::<str>::from("/w/a.ts")]);
    let mut first_open: Option<(usize, usize, usize)> = None;

    for cycle in 0..20usize {
        let slots_before = store.node_slot_count();
        let (a_param, a_obj, a_alias, a_view) =
            release_intern_document(&store, "/w/a.ts", 1, shared);
        store.publish_with_carrier_for_tests(
            release_decl_key("/w/a.ts", "A"),
            QueryResult::Value(a_obj),
            carrier_naming("/w/a.ts", 1),
            Arc::clone(&roots_a),
        );
        assert!(!store.node_reaches_unresolved(a_alias));
        let _ = store.member_ordinal_index(a_obj, &a_view);
        assert!(
            a_param.0 as usize >= slots_before,
            "cycle {cycle}: identical content must mint FRESH ids after a release, never a released one"
        );

        let open = (
            store.node_count(),
            store.memo_entry_count(),
            store.unresolved_reach_count(),
        );
        assert_eq!(
            open.0,
            baseline_live + 3,
            "cycle {cycle}: three live nodes per open"
        );
        match first_open {
            None => first_open = Some(open),
            Some(first) => assert_eq!(
                open, first,
                "cycle {cycle}: the live substrate at 'open' must equal the first cycle's"
            ),
        }

        let report = store.release_canonical("/w/a.ts");
        assert_eq!(report.nodes_released, 3, "cycle {cycle}: {report:?}");
        assert_eq!(
            store.node_count(),
            baseline_live,
            "cycle {cycle}: live nodes back to baseline"
        );
        assert_eq!(store.memo_entry_count(), 0, "cycle {cycle}");
        assert_eq!(
            store.unresolved_reach_count(),
            1,
            "cycle {cycle}: only the shared bit"
        );
        assert_eq!(
            store.node_slot_count(),
            baseline_slots + (cycle + 1) * 3,
            "cycle {cycle}: the id space grows by exactly the released nodes"
        );
    }
}

/// The content-bound class a node-id / reverse-index drain cannot see:
/// (a) nodes interned under the CONSUMER's scope whose payload carries the
/// closed document's content identity (a `DeclRef` to one of its
/// declarations, the `DeclPlaceholder` refusal for one) — no node id
/// inside, consumer scope outside; (b) candidates whose carrier was
/// COMPACTED to a domain aggregate, which `canonical_ids()` reports as
/// naming no canonical, so the reverse index registers them nowhere and
/// `invalidate_canonical` leaves them (the count assertion pins that
/// under-approximation); (c) candidates whose FAMILY names the document
/// through a content-free slot / scope. A close drops all three, and keeps
/// a neighbour's precisely-registered candidate.
///
/// On the release before the content-bound drain, both consumer-scoped
/// nodes stayed live (`nodes_released` was 0), the two aggregate-only
/// candidates and the scope-keyed candidate survived, and
/// `memo_entry_counts_by_family` reported `ResolveDecl` at 4 instead of 1.
#[test]
fn release_canonical_drops_content_bound_nodes_and_compacted_candidates() {
    use verter_session_query::facts::fact_cache::FactVersionRef;
    use verter_session_query::facts::fact_cache::{
        AggregatePopulation, AggregateStamp, CompactionDomain, DomainGenerationFact,
    };
    use verter_session_query::resolution::{ResolutionPopulation, ResolutionWorldId};

    let store = SemanticGraphStore::new();
    let shared = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let scope_c = release_file_scope("/w/consumer.vue", 9);
    let decl_ref_in_consumer = store.intern_node_with_scope(
        SemanticNodeData::DeclRef {
            identity: DeclIdentity {
                canonical_id: Arc::from("/w/a.ts"),
                owner: TopLevelOwnerId::ordinary_file(),
                whole_hash: [1u8; 16],
                decl_name: Arc::from("T"),
            },
        },
        scope_c.clone(),
    );
    let placeholder_in_consumer = store.intern_node_with_scope(
        SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
            canonical_id: Arc::from("/w/a.ts"),
            owner: TopLevelOwnerId::ordinary_file(),
            name: Arc::from("T"),
            whole_hash: [1u8; 16],
        }),
        scope_c,
    );

    let aggregate_only = || {
        verter_session_query::facts::fact_cache::ReadSetSignature::new(Arc::from(vec![
            FactVersionRef::DomainGeneration(DomainGenerationFact {
                domain: CompactionDomain::Resolution,
                population: AggregatePopulation::Resolution(ResolutionPopulation::Base),
                stamp: AggregateStamp::ResolutionRoots {
                    base: ResolutionWorldId::from_raw(1),
                    session: None,
                },
            }),
        ]))
    };
    let no_roots: Arc<[Arc<str>]> = Arc::from(Vec::<Arc<str>>::new());
    let roots_b: Arc<[Arc<str>]> = Arc::from(vec![Arc::<str>::from("/w/b.ts")]);

    // (b) compacted carriers: one keyed in A's scope, one in B's scope.
    let key_a_compacted = release_decl_key("/w/a.ts", "T");
    let key_b_compacted = release_decl_key("/w/b.ts", "ViaAggregate");
    store.publish_with_carrier_for_tests(
        key_a_compacted.clone(),
        QueryResult::Value(shared),
        aggregate_only(),
        Arc::clone(&no_roots),
    );
    store.publish_with_carrier_for_tests(
        key_b_compacted.clone(),
        QueryResult::Value(shared),
        aggregate_only(),
        Arc::clone(&roots_b),
    );
    // (c) a family naming A through its lookup scope, with a carrier that
    // names only B precisely.
    let key_a_scoped = release_decl_key("/w/a.ts", "U");
    store.publish_with_carrier_for_tests(
        key_a_scoped.clone(),
        QueryResult::Value(shared),
        carrier_naming("/w/b.ts", 2),
        Arc::clone(&roots_b),
    );
    // The neighbour's own, precisely registered candidate.
    let key_b = release_decl_key("/w/b.ts", "B");
    store.publish_with_carrier_for_tests(
        key_b.clone(),
        QueryResult::Value(shared),
        carrier_naming("/w/b.ts", 2),
        roots_b,
    );
    assert_eq!(
        store.canonical_to_entries_count("/w/a.ts"),
        0,
        "an aggregate-only carrier names no canonical: the reverse index never registers it under A"
    );
    assert_eq!(
        store.memo_entry_counts_by_family(),
        vec![("ResolveDecl", 4)]
    );

    let report = store.release_canonical("/w/a.ts");

    assert_eq!(
        report.nodes_released, 2,
        "the consumer-scoped DeclRef and DeclPlaceholder bind A's content: {report:?}"
    );
    assert!(!store.node_is_live(decl_ref_in_consumer));
    assert!(!store.node_is_live(placeholder_in_consumer));
    assert!(store.node_is_live(shared));
    assert!(
        store.get_unvalidated(&key_a_compacted).is_none(),
        "a compacted candidate in A's scope is dropped"
    );
    assert!(
        store.get_unvalidated(&key_b_compacted).is_none(),
        "a compacted candidate is dropped on ANY close: the reload is movement in its domain"
    );
    assert!(
        store.get_unvalidated(&key_a_scoped).is_none(),
        "a family whose lookup scope names A is dropped even though its carrier names only B"
    );
    assert!(
        store.get_unvalidated(&key_b).is_some(),
        "B's own candidate stays"
    );
    assert_eq!(
        store.memo_entry_counts_by_family(),
        vec![("ResolveDecl", 1)]
    );
    assert_eq!(report.memo_entries_evicted, 3);
    assert_eq!(
        store.memo_family_count_for_test(),
        store.memo_budget_tracked_len_for_test()
    );
}

/// A union member view (the per-store `union_views` cache) is released with
/// its union: a view of a closed document's union is keyed by a union id the
/// reload never mints again, so the close drops it, while a view whose union
/// is live stays resident and is served unchanged.
///
/// Discriminating: without the sweep the view count grew by one per distinct
/// union the churn built and never came back, and a stale holder could read
/// released members through the resident view.
#[test]
fn release_canonical_drops_the_union_views_of_the_closed_document() {
    use crate::semantic_query::composite::{CompositeList, UnionKind};
    use crate::semantic_query::stable_key::semantic_union_members;
    use crate::semantic_query::SemanticContext;

    let store = SemanticGraphStore::new();
    let ctx = SemanticContext::production();
    let shared = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let (a_param, _, _, _) = release_intern_document(&store, "/w/a.ts", 1, shared);
    let (b_param, _, _, _) = release_intern_document(&store, "/w/b.ts", 1, shared);
    let union_of = |canonical: &str, member: SemanticNodeId| {
        store.intern_node_with_scope(
            SemanticNodeData::Union(CompositeList::<UnionKind>::authored_shell_for_tests(
                Arc::from([member, shared]),
            )),
            release_file_scope(canonical, 1),
        )
    };
    let a_union = union_of("/w/a.ts", a_param);
    let b_union = union_of("/w/b.ts", b_param);
    let _ = semantic_union_members(&store, a_union, &ctx);
    let b_view = semantic_union_members(&store, b_union, &ctx);
    assert_eq!(
        store.union_view_count(),
        2,
        "one resident view per union built"
    );

    let report = store.release_canonical("/w/a.ts");
    assert_eq!(report.union_views_released, 1, "{report:?}");
    assert_eq!(store.union_view_count(), 1);
    assert!(
        Arc::ptr_eq(&semantic_union_members(&store, b_union, &ctx), &b_view),
        "the live union's view stays resident and is served unchanged"
    );
    assert!(
        !store.node_is_live(a_union),
        "the closed document's union is released"
    );

    // A late reader of the released union id gets the placeholder's view but
    // does not re-admit it: nothing would ever release that residue.
    let _ = semantic_union_members(&store, a_union, &ctx);
    assert_eq!(
        store.union_view_count(),
        1,
        "a view of a released union is served, never kept"
    );
}
