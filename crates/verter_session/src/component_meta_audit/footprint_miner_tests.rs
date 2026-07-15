//! Tests for the [`super`] footprint miner — the deterministic
//! `AccumulatorState` + [`SemanticGraphStore`] → [`RequestFootprintAudit`]
//! conversion, with the content-only structural fingerprint at its core.
//!
//! Extracted from the inline `#[cfg(test)] mod tests` to keep the production
//! module within the per-file line budget; the assertions are unchanged.

use super::*;
use crate::component_meta_audit::accumulator::{DerivationEdgeRaw, RequestFootprintAccumulator};
use crate::semantic_query::OriginEdge;

fn make_ctx(id: u64) -> Arc<RequestContext> {
    let acc = Arc::new(RequestFootprintAccumulator::new());
    RequestContext::new(id, Arc::from("/x.vue"), true, Some(acc))
}

fn synth_edge(
    result_raw: u64,
    sources: &[u64],
    kind: CoreOriginEdgeKind,
    meta: OriginMeta,
) -> DerivationEdgeRaw {
    DerivationEdgeRaw {
        result: SemanticNodeId(result_raw),
        kind,
        edge: OriginEdge {
            sources: sources.iter().copied().map(SemanticNodeId).collect(),
            meta,
            edge_dep_signature: Arc::new(
                Arc::<[(Arc<str>, crate::semantic_query::DepVersion)]>::from(Vec::<(
                    Arc<str>,
                    crate::semantic_query::DepVersion,
                )>::new(
                )),
            ),
        },
    }
}

fn empty_graph() -> SemanticGraphStore {
    SemanticGraphStore::new()
}

#[test]
fn mine_footprint_empty_state_yields_empty_subgraph_and_zero_counters() {
    let graph = empty_graph();
    let ctx = make_ctx(1);
    let state = AccumulatorState::default();
    let fp = mine_footprint(&graph, state, &ctx, 10_000, &AuditCaps::default());
    assert_eq!(fp.derivation_subgraph.nodes.len(), 0);
    assert_eq!(fp.derivation_subgraph.edges.len(), 0);
    assert_eq!(fp.cache_outcomes.cold_builds, 0);
    assert!(!fp.graph_completeness.has_orphan_edges);
}

#[test]
fn mine_footprint_cache_outcomes_read_from_per_context_atomic_counters() {
    let graph = empty_graph();
    let ctx = make_ctx(2);
    ctx.cold_builds.store(3, Ordering::Relaxed);
    ctx.warm_hits.store(5, Ordering::Relaxed);
    ctx.joined_waits.store(2, Ordering::Relaxed);
    ctx.sentinels.store(1, Ordering::Relaxed);
    ctx.inflight_aborted_retries.store(7, Ordering::Relaxed);
    ctx.cold_aborts_swept.store(11, Ordering::Relaxed);
    let fp = mine_footprint(
        &graph,
        AccumulatorState::default(),
        &ctx,
        10_000,
        &AuditCaps::default(),
    );
    assert_eq!(fp.cache_outcomes.cold_builds, 3);
    assert_eq!(fp.cache_outcomes.warm_hits, 5);
    assert_eq!(fp.cache_outcomes.joined_waits, 2);
    assert_eq!(fp.cache_outcomes.sentinels, 1);
    assert_eq!(fp.cache_outcomes.inflight_aborted_retries, 7);
    assert_eq!(fp.cache_outcomes.cold_aborts_swept, 11);
}

#[test]
fn mine_footprint_truncates_at_max_derivation_edges_sets_orphan_flag() {
    let graph = empty_graph();
    let ctx = make_ctx(3);
    let mut state = AccumulatorState::default();
    for i in 0..10u64 {
        state.derivation_edges_raw.push(synth_edge(
            i,
            &[i + 100],
            CoreOriginEdgeKind::AliasResolve,
            OriginMeta::None,
        ));
    }
    let fp = mine_footprint(&graph, state, &ctx, 5, &AuditCaps::default());
    assert_eq!(fp.derivation_subgraph.edges.len(), 5);
    assert!(fp.graph_completeness.has_orphan_edges);
    assert_eq!(fp.graph_completeness.edges_truncated, 5);
}

#[test]
fn mine_footprint_identical_inputs_produce_byte_identical_outputs() {
    let graph = empty_graph();
    let ctx_a = make_ctx(1);
    let ctx_b = make_ctx(1);
    let mut state_a = AccumulatorState::default();
    let mut state_b = AccumulatorState::default();
    for i in 0..6u64 {
        state_a.derivation_edges_raw.push(synth_edge(
            i,
            &[i + 100],
            CoreOriginEdgeKind::ProjectMember,
            OriginMeta::ProjectedMember {
                name: Arc::from(format!("m{i}")),
                provenance: verter_audit::MemberEdgeProvenance::PathProjection,
            },
        ));
        state_b.derivation_edges_raw.push(synth_edge(
            i,
            &[i + 100],
            CoreOriginEdgeKind::ProjectMember,
            OriginMeta::ProjectedMember {
                name: Arc::from(format!("m{i}")),
                provenance: verter_audit::MemberEdgeProvenance::PathProjection,
            },
        ));
    }
    let fp_a = mine_footprint(&graph, state_a, &ctx_a, 10_000, &AuditCaps::default());
    let fp_b = mine_footprint(&graph, state_b, &ctx_b, 10_000, &AuditCaps::default());
    let bytes_a = serde_json::to_vec(&fp_a).expect("serialise a");
    let bytes_b = serde_json::to_vec(&fp_b).expect("serialise b");
    assert_eq!(
        bytes_a, bytes_b,
        "identical inputs must produce byte-identical mined footprints"
    );
}

#[test]
fn mine_footprint_conditional_decisions_distinguish_true_false_deferred() {
    let graph = empty_graph();
    let ctx = make_ctx(4);
    let mut state = AccumulatorState::default();
    state.derivation_edges_raw.push(synth_edge(
        1,
        &[10],
        CoreOriginEdgeKind::ConditionalSelect,
        OriginMeta::Branch(BranchSelection::True),
    ));
    state.derivation_edges_raw.push(synth_edge(
        2,
        &[10],
        CoreOriginEdgeKind::ConditionalSelect,
        OriginMeta::Branch(BranchSelection::False),
    ));
    state.derivation_edges_raw.push(synth_edge(
        3,
        &[10],
        CoreOriginEdgeKind::ConditionalSelect,
        OriginMeta::None,
    ));
    let fp = mine_footprint(&graph, state, &ctx, 10_000, &AuditCaps::default());
    let branches: Vec<ConditionalBranch> =
        fp.conditional_decisions.iter().map(|c| c.branch).collect();
    assert!(branches.contains(&ConditionalBranch::True));
    assert!(branches.contains(&ConditionalBranch::False));
    assert!(branches.contains(&ConditionalBranch::Deferred));
}

#[test]
fn mine_footprint_alias_resolve_emits_one_record_per_hop() {
    let graph = empty_graph();
    let ctx = make_ctx(5);
    let mut state = AccumulatorState::default();
    for hop in 0..3u64 {
        state.derivation_edges_raw.push(synth_edge(
            hop,
            &[hop + 1],
            CoreOriginEdgeKind::AliasResolve,
            OriginMeta::AliasName(Arc::from(format!("alias_{hop}"))),
        ));
    }
    let fp = mine_footprint(&graph, state, &ctx, 10_000, &AuditCaps::default());
    assert_eq!(fp.alias_resolutions.len(), 3);
    for rec in &fp.alias_resolutions {
        assert!(rec.alias_name.starts_with("alias_"));
    }
}

#[test]
fn mine_footprint_path_segments_preserve_member_index_distinction() {
    let graph = empty_graph();
    let ctx = make_ctx(6);
    let mut state = AccumulatorState::default();
    let path = [
        PathSegment::Member(Arc::from("a")),
        PathSegment::Index(IndexKey::String(Arc::from("b"))),
        PathSegment::Index(IndexKey::Number(
            crate::semantic_query::CanonicalIndexInt::from_canonical_i64(7).expect("canonical"),
        )),
    ];
    state.derivation_edges_raw.push(synth_edge(
        1,
        &[10],
        CoreOriginEdgeKind::ProjectPath,
        OriginMeta::Path(Arc::from(path)),
    ));
    let fp = mine_footprint(&graph, state, &ctx, 10_000, &AuditCaps::default());
    assert_eq!(fp.projections.len(), 1);
    let segs = &fp.projections[0].path;
    assert!(matches!(&segs[0], ProjectPathSegment::Member { name } if name.as_ref() == "a"));
    assert!(matches!(&segs[1], ProjectPathSegment::Index { key } if key.as_ref() == "b"));
    assert!(matches!(&segs[2], ProjectPathSegment::Index { key } if key.as_ref() == "7"));
}

#[test]
fn mine_footprint_indexed_ready_builds_extracted_from_structured_events() {
    let graph = empty_graph();
    let ctx = make_ctx(7);
    let mut state = AccumulatorState::default();
    state
        .structured_events
        .push(StructuredAuditEvent::IndexedReadyBuilt {
            canonical_id: Arc::from("/a.ts"),
            whole_hash: [9u8; 16],
        });
    state
        .structured_events
        .push(StructuredAuditEvent::IndexedReadyBuilt {
            canonical_id: Arc::from("/b.ts"),
            whole_hash: [10u8; 16],
        });
    let fp = mine_footprint(&graph, state, &ctx, 10_000, &AuditCaps::default());
    assert_eq!(fp.indexed_ready_builds.len(), 2);
    assert_eq!(fp.indexed_ready_builds[0].canonical_id.as_ref(), "/a.ts");
    assert_eq!(fp.indexed_ready_builds[1].canonical_id.as_ref(), "/b.ts");
}

#[test]
fn mine_footprint_multiple_derivations_for_same_result_produce_multiple_edges() {
    let graph = empty_graph();
    let ctx = make_ctx(8);
    let mut state = AccumulatorState::default();
    // Two derivations of the same result via different alias hops.
    state.derivation_edges_raw.push(synth_edge(
        42,
        &[10],
        CoreOriginEdgeKind::AliasResolve,
        OriginMeta::AliasName(Arc::from("path_a")),
    ));
    state.derivation_edges_raw.push(synth_edge(
        42,
        &[20],
        CoreOriginEdgeKind::AliasResolve,
        OriginMeta::AliasName(Arc::from("path_b")),
    ));
    let fp = mine_footprint(&graph, state, &ctx, 10_000, &AuditCaps::default());
    assert_eq!(fp.derivation_subgraph.edges.len(), 2);
    assert_eq!(
        fp.derivation_subgraph.edges[0].result, fp.derivation_subgraph.edges[1].result,
        "both derivations of the same SemanticNodeId must produce the same NodeId result"
    );
}

use crate::semantic_query::PrimitiveKind;

/// Build a `Foo<arg_child>` bare-ref carrier node (head `Foo` in
/// `NodeScopeId::Global`, a single structural type argument). The carrier's
/// `type_args` are carried IN at construction through the sanctioned
/// `SemanticNodeData::new_bare_ref` constructor — never hand-bound — so this
/// is a REAL carrier node, not a faked `SemanticNodeData`.
fn bare_ref_with_arg(arg_child: SemanticNodeId) -> SemanticNodeData {
    SemanticNodeData::new_bare_ref(
        Arc::from("Foo"),
        NodeScopeId::Global,
        Arc::from(vec![arg_child].into_boxed_slice()),
    )
}

/// `structural_hash_of` must DISCRIMINATE a carrier's type arguments: two
/// `Foo<…>` carriers that share a head but differ ONLY in their single type
/// argument child must produce DIFFERENT fingerprints. This is the contract
/// the old `format!("{data:?}")` fingerprint could not guarantee once the
/// carrier's private `type_args` layout changed; the variant-aware walk
/// hashes the head plus the ordered recursive child hashes, so distinct args
/// never collapse to one footprint.
#[test]
fn structural_hash_discriminates_carrier_type_args_foo_a_vs_foo_b() {
    let graph = empty_graph();
    // Two structurally-DISTINCT argument children (`A` = string, `B` =
    // number), each a real interned node.
    let child_a = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let child_b = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    assert_ne!(
        child_a, child_b,
        "the two argument children must be distinct interned nodes"
    );

    let foo_a = bare_ref_with_arg(child_a); // Foo<A>
    let foo_b = bare_ref_with_arg(child_b); // Foo<B>

    let hash_a = structural_hash_of(&graph, &foo_a);
    let hash_b = structural_hash_of(&graph, &foo_b);
    assert_ne!(
        hash_a, hash_b,
        "DISCRIMINATION: Foo<A> and Foo<B> (same head, different single type argument) must \
         yield DIFFERENT structural fingerprints — the carrier walk must hash the arg child \
         structure, not drop it. A fingerprint that ignored the args (or rendered them through \
         a representation-sensitive Debug that collapsed distinct children) would make these \
         equal and this assertion fail."
    );
}

/// Inverse stability contract: two structurally-IDENTICAL `Foo<A>` carriers
/// (same head, same single argument child) must produce the SAME fingerprint.
/// Together with the discrimination test this pins "distinct args ⇒ distinct
/// footprint, identical args ⇒ identical footprint".
#[test]
fn structural_hash_is_stable_for_identical_carriers() {
    let graph = empty_graph();
    let child_a = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    // Two independently-constructed but structurally-identical Foo<A>.
    let foo_a1 = bare_ref_with_arg(child_a);
    let foo_a2 = bare_ref_with_arg(child_a);

    let hash_a1 = structural_hash_of(&graph, &foo_a1);
    let hash_a2 = structural_hash_of(&graph, &foo_a2);
    assert_eq!(
        hash_a1, hash_a2,
        "STABILITY: two structurally-identical Foo<A> carriers must yield the SAME structural \
         fingerprint."
    );
}

/// The discriminating signal must be the carrier ARGUMENT, not merely the
/// carrier HEAD. `Foo<A>` and `Foo<B>` share an identical head (`Foo` /
/// `Global`), so a head-only fingerprint would make them equal; this test
/// pins that the head is shared yet the fingerprints diverge — proving the
/// arg child is what discriminates.
#[test]
fn structural_hash_carrier_discrimination_comes_from_args_not_head() {
    let graph = empty_graph();
    let child_a = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let child_b = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

    let foo_a = bare_ref_with_arg(child_a);
    let foo_b = bare_ref_with_arg(child_b);

    // Heads are identical (same name + scope, no args).
    assert_eq!(
        foo_a.bare_ref_head().map(|(n, s)| (n.clone(), s.clone())),
        foo_b.bare_ref_head().map(|(n, s)| (n.clone(), s.clone())),
        "Foo<A> and Foo<B> must share an identical head — the only difference is the arg child"
    );
    // …yet the fingerprints diverge, so the arg child is the discriminator.
    assert_ne!(
        structural_hash_of(&graph, &foo_a),
        structural_hash_of(&graph, &foo_b),
        "with identical heads, the differing fingerprints prove the carrier ARG child is what \
         discriminates the fingerprint."
    );
}

/// Model `Box<inner>` as an `Array { element: inner }` — a NON-carrier child
/// whose distinguishing structure (`element`) is reachable ONLY by descending
/// it. A real interned node.
fn array_of(graph: &SemanticGraphStore, inner: SemanticNodeId) -> SemanticNodeId {
    graph.intern_node(SemanticNodeData::Array {
        element: inner,
        readonly: false,
    })
}

/// FINDING A regression — CROSS-GRAPH CONTENT STABILITY. Two structurally
/// identical `Foo<String>` carriers, built in SEPARATE graphs where graph B
/// interned unrelated nodes FIRST so its `String` child receives a DIFFERENT
/// arena ordinal than graph A's, must produce the SAME fingerprint.
///
/// This FAILS against the pre-fix tree: the old `push_carrier_arg_hashes`
/// folded the raw `SemanticNodeId.0` arena ordinal of the arg child into the
/// bytes, so `String`-at-ordinal-0 and `String`-at-ordinal-2 hashed
/// differently — the exact content-determinism violation. The content-only
/// encoder descends to the `Primitive(String)` content in both graphs, so
/// the ordinal divergence is invisible.
#[test]
fn structural_hash_is_cross_graph_stable_despite_different_child_ordinals() {
    // Graph A: `String` is the FIRST interned node → ordinal 0.
    let graph_a = empty_graph();
    let string_a = graph_a.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let foo_a = bare_ref_with_arg(string_a);

    // Graph B: intern unrelated nodes FIRST, so `String` lands at a HIGHER
    // ordinal than in graph A.
    let graph_b = empty_graph();
    let _filler0 = graph_b.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let _filler1 = graph_b.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Void));
    let string_b = graph_b.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let foo_b = bare_ref_with_arg(string_b);

    // Precondition: the two `String` children genuinely occupy DIFFERENT
    // arena ordinals — otherwise the test would pass vacuously even against
    // the ordinal-folding code.
    assert_ne!(
        string_a.0, string_b.0,
        "test setup must give the `String` child different ordinals across the two graphs"
    );

    assert_eq!(
        structural_hash_of(&graph_a, &foo_a),
        structural_hash_of(&graph_b, &foo_b),
        "CROSS-GRAPH STABILITY: two equivalent `Foo<String>` carriers must hash IDENTICALLY \
         even when the `String` child received a different arena ordinal in each graph. The \
         pre-fix code folded the raw ordinal into the bytes and FAILED this; a content-only \
         fingerprint descends to the `Primitive(String)` content and passes."
    );
}

/// FINDING B regression — NESTED NON-CARRIER DISCRIMINATION ACROSS FRESH
/// GRAPHS WITH COLLIDING ORDINALS. `Foo<Box<String>>` vs `Foo<Box<Number>>`
/// where `Box<_>` is a NON-carrier `Array` child. Both graphs intern in the
/// SAME order, so the outer `Array` node and its inner primitive occupy the
/// SAME ordinals in each graph — the ONLY real difference is the deeply
/// nested primitive (`String` vs `Number`).
///
/// This FAILS against the pre-fix tree: the recursion descended ONLY carrier
/// `type_args` and the non-carrier arm used `format!("{data:?}")`, which for
/// the `Array` renders `Array { element: SemanticNodeId(0), readonly: false }`
/// — IDENTICAL bytes in both graphs because the ordinals collide and Debug
/// does not descend the child. The two carriers therefore COLLIDED. The
/// content-only encoder descends the `Array.element` child and distinguishes
/// `Primitive(String)` from `Primitive(Number)`.
#[test]
fn structural_hash_discriminates_nested_non_carrier_child_across_colliding_ordinals() {
    // Graph A: String(0), Array{element:0}(1) → Foo<Box<String>>.
    let graph_a = empty_graph();
    let string_a = graph_a.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let box_a = array_of(&graph_a, string_a);
    let foo_a = bare_ref_with_arg(box_a);

    // Graph B: Number(0), Array{element:0}(1) → Foo<Box<Number>>.
    let graph_b = empty_graph();
    let number_b = graph_b.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let box_b = array_of(&graph_b, number_b);
    let foo_b = bare_ref_with_arg(box_b);

    // Precondition: the OUTER `Box` node (the carrier's direct arg) AND the
    // inner primitive occupy the SAME ordinals in both graphs — so a
    // fingerprint that distinguished only by ordinal, or that stopped at the
    // direct arg's own Debug, could NOT tell these two apart.
    assert_eq!(
        box_a.0, box_b.0,
        "test setup must collide the outer Box ordinal across the two graphs"
    );
    assert_eq!(
        string_a.0, number_b.0,
        "test setup must collide the inner primitive ordinal across the two graphs"
    );

    assert_ne!(
        structural_hash_of(&graph_a, &foo_a),
        structural_hash_of(&graph_b, &foo_b),
        "NESTED DISCRIMINATION: `Foo<Box<String>>` and `Foo<Box<Number>>` must hash \
         DIFFERENTLY even though the outer Box and inner-primitive ordinals collide. The \
         pre-fix carrier-only recursion + Debug-non-carrier arm rendered the Box identically \
         in both graphs and COLLIDED; the content-only encoder descends Box.element and \
         distinguishes String from Number."
    );
}

/// RECURSION-IS-LOAD-BEARING. The discriminating signal sits TWO non-carrier
/// levels deep: `Foo<Box<Box<String>>>` vs `Foo<Box<Box<Number>>>`, with
/// COLLIDING ordinals at every level across two fresh graphs. The two direct
/// arg children are identical `Array` shells, their `element` children are
/// identical `Array` shells, and ONLY the innermost primitive differs.
///
/// This is the direct answer to the "would the tests still pass if the
/// recursive `graph.node_data` descent were deleted?" objection: it FAILS
/// against the pre-fix tree (Debug does not descend), and it would FAIL
/// against THIS encoder too if `encode_child`'s recursive descent were
/// removed — the two carriers would then collide on the identical outer
/// shells with colliding ordinals. It also defeats an "ordinal-only"
/// shortcut: every ordinal collides, so only descending the content
/// distinguishes them.
#[test]
fn structural_hash_recursion_into_grandchildren_is_load_bearing() {
    // Graph A: String(0), Array{0}(1), Array{1}(2) → Foo<Box<Box<String>>>.
    let graph_a = empty_graph();
    let string_a = graph_a.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let inner_a = array_of(&graph_a, string_a);
    let outer_a = array_of(&graph_a, inner_a);
    let foo_a = bare_ref_with_arg(outer_a);

    // Graph B: Number(0), Array{0}(1), Array{1}(2) → Foo<Box<Box<Number>>>.
    let graph_b = empty_graph();
    let number_b = graph_b.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let inner_b = array_of(&graph_b, number_b);
    let outer_b = array_of(&graph_b, inner_b);
    let foo_b = bare_ref_with_arg(outer_b);

    // Every ordinal collides across the two graphs.
    assert_eq!(outer_a.0, outer_b.0, "outer Box ordinal must collide");
    assert_eq!(inner_a.0, inner_b.0, "inner Box ordinal must collide");
    assert_eq!(
        string_a.0, number_b.0,
        "innermost primitive ordinal must collide"
    );

    assert_ne!(
        structural_hash_of(&graph_a, &foo_a),
        structural_hash_of(&graph_b, &foo_b),
        "LOAD-BEARING RECURSION: the only difference is a primitive TWO non-carrier levels \
         deep behind colliding ordinals. The fingerprints diverge ONLY because `encode_child` \
         recursively descends `graph.node_data` to that primitive. Delete the recursion (or \
         fold ordinals) and this assertion flips to a collision."
    );
}

/// MINED-FOOTPRINT BYTE-STABILITY OVER CARRIERS UNDER ORDINAL SKEW. Build two
/// graphs each containing an equivalent `Foo<String>` carrier referenced by the
/// derivation edges, but intern the carrier's `String` child at a DIFFERENT
/// arena ordinal in graph B (graph B interns unrelated fillers FIRST), then mine
/// each and assert the serialised footprints are byte-identical. This drives the
/// structural encoder through the full `mine_footprint` pipeline (node-record
/// construction + node sort by `structural_hash`) on CARRIER nodes — the
/// determinism contract the file header pins.
///
/// The ordinal skew makes this a genuine cross-run byte-identity discriminator,
/// not a same-order pipeline smoke test: it FAILS against the carrier-arg
/// ordinal-folding tree (the old `push_carrier_arg_hashes` folded the child's
/// raw `SemanticNodeId.0`, so the skewed `String` ordinals produced divergent
/// carrier bytes and the two mined footprints differed). The content-only
/// encoder descends to the `Primitive(String)` content in both graphs, so the
/// ordinal skew is invisible.
#[test]
fn mine_footprint_byte_identical_over_interned_carrier_nodes() {
    /// `skew` controls the carrier child's arena ordinal: when set, unrelated
    /// filler nodes are interned FIRST so the `String` child lands at a HIGHER
    /// ordinal than in the un-skewed graph.
    fn build_graph_with_carrier(
        skew: bool,
    ) -> (SemanticGraphStore, SemanticNodeId, SemanticNodeId) {
        let graph = empty_graph();
        if skew {
            let _f0 = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
            let _f1 = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Void));
        }
        let string_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let carrier_id = graph.intern_node(bare_ref_with_arg(string_id));
        (graph, carrier_id, string_id)
    }

    fn state_referencing(carrier: SemanticNodeId, child: SemanticNodeId) -> AccumulatorState {
        let mut state = AccumulatorState::default();
        state.derivation_edges_raw.push(DerivationEdgeRaw {
            result: carrier,
            kind: CoreOriginEdgeKind::AliasResolve,
            edge: OriginEdge {
                sources: Arc::from(vec![child].into_boxed_slice()),
                meta: OriginMeta::AliasName(Arc::from("Foo")),
                edge_dep_signature: Arc::new(
                    Arc::<[(Arc<str>, crate::semantic_query::DepVersion)]>::from(Vec::<(
                        Arc<str>,
                        crate::semantic_query::DepVersion,
                    )>::new(
                    )),
                ),
            },
        });
        state
    }

    let (graph_a, carrier_a, child_a) = build_graph_with_carrier(false);
    let (graph_b, carrier_b, child_b) = build_graph_with_carrier(true);
    let ctx_a = make_ctx(1);
    let ctx_b = make_ctx(1);

    // Precondition: graph B's `String` child genuinely occupies a DIFFERENT
    // arena ordinal than graph A's — otherwise this would pass vacuously even
    // against the carrier-arg ordinal-folding encoder.
    assert_ne!(
        child_a.0, child_b.0,
        "test setup must give the carrier's `String` child different ordinals across the two graphs"
    );

    let fp_a = mine_footprint(
        &graph_a,
        state_referencing(carrier_a, child_a),
        &ctx_a,
        10_000,
        &AuditCaps::default(),
    );
    let fp_b = mine_footprint(
        &graph_b,
        state_referencing(carrier_b, child_b),
        &ctx_b,
        10_000,
        &AuditCaps::default(),
    );

    // Sanity: the carrier node really did flow through node-record
    // construction (a BareRef → `Other { name: "BareRef" }` kind), so the
    // structural encoder ran on a carrier in the mining pipeline.
    assert!(
        fp_a.derivation_subgraph.nodes.iter().any(
            |n| matches!(&n.kind, SemanticNodeKind::Other { name } if name.as_ref() == "BareRef")
        ),
        "the interned BareRef carrier must appear in the mined node table"
    );

    let bytes_a = serde_json::to_vec(&fp_a).expect("serialise a");
    let bytes_b = serde_json::to_vec(&fp_b).expect("serialise b");
    assert_eq!(
        bytes_a, bytes_b,
        "CARRIER DETERMINISM UNDER ORDINAL SKEW: two graphs containing an equivalent `Foo<String>` \
         carrier must mine to BYTE-IDENTICAL footprints even when the `String` child received a \
         different arena ordinal in each graph. The carrier-arg ordinal-folding code folded the \
         child's raw ordinal into the carrier bytes and FAILED this; the content-only encoder \
         descends to the `Primitive(String)` content and passes."
    );
}

use crate::semantic_query::SyntheticBindingId;
use verter_type_expr::SyntheticCarrierSurfaceKind;

/// Build a content-free [`SyntheticBindingId`] — the four scalar/string fields,
/// NO `value_node`. Two ids built from the same `(slot, binding)` are `Eq`.
fn synthetic_binding_id(slot: &str, binding: &str) -> SyntheticBindingId {
    SyntheticBindingId {
        scope_canonical_id: Arc::from("/Comp.vue"),
        surface_kind: SyntheticCarrierSurfaceKind::SlotBinding,
        slot_name: Some(Arc::from(slot)),
        binding_name: Arc::from(binding),
    }
}

/// Intern a real `SyntheticBinding` node whose `value_node` points at the
/// already-interned `target` node (re-attaching the target's raw ordinal exactly
/// as the production lowering does at `structural_carrier_producer/lower.rs`).
/// The binding's
/// `value_node` therefore RESOLVES in `graph`, so the encoder descends it.
fn intern_synthetic_binding(
    graph: &SemanticGraphStore,
    id: SyntheticBindingId,
    target: SemanticNodeId,
) -> SemanticNodeId {
    graph.intern_node(SemanticNodeData::SyntheticBinding {
        id,
        value_node: target.0,
    })
}

/// CROSS-GRAPH STABILITY OF EQUIVALENT SYNTHETIC BINDINGS UNDER ORDINAL SKEW.
/// Two graphs each hold a `SyntheticBinding` with the SAME content-free
/// [`SyntheticBindingId`] whose `value_node` points at a target of the SAME
/// CONTENT (`Primitive(String)`), but graph B interns unrelated fillers FIRST so
/// its target lands at a DIFFERENT arena ordinal. The two structural fingerprints
/// must be EQUAL.
///
/// This is the discriminating regression for the `value_node` ordinal leak: it
/// FAILS against the tree where the `SyntheticBinding` arm folded
/// `value_node.to_le_bytes()` — the raw `SemanticNodeId` ordinal — straight into
/// the content hash, so a target at ordinal 1 and the same-content target at
/// ordinal 3 produced divergent bytes (a cross-run false-DISTINCTION). Descending
/// `value_node` via `encode_child` hashes the pointed-at `Primitive(String)`
/// content in both graphs, so the ordinal skew is invisible.
#[test]
fn synthetic_binding_is_cross_graph_stable_despite_different_value_node_ordinals() {
    // Graph A: `String` target is the FIRST interned node → low ordinal.
    let graph_a = empty_graph();
    let target_a = graph_a.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let binding_a =
        intern_synthetic_binding(&graph_a, synthetic_binding_id("default", "row"), target_a);

    // Graph B: intern unrelated fillers FIRST, so the same-content `String`
    // target lands at a HIGHER ordinal than in graph A.
    let graph_b = empty_graph();
    let _f0 = graph_b.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let _f1 = graph_b.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Void));
    let target_b = graph_b.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let binding_b =
        intern_synthetic_binding(&graph_b, synthetic_binding_id("default", "row"), target_b);

    // Precondition: the two `value_node` targets genuinely occupy DIFFERENT
    // arena ordinals — otherwise the test would pass vacuously even against the
    // ordinal-folding `value_node.to_le_bytes()` code.
    assert_ne!(
        target_a.0, target_b.0,
        "test setup must give the `value_node` target different ordinals across the two graphs"
    );

    let hash_a = structural_hash_of(
        &graph_a,
        &graph_a.node_data(binding_a).expect("binding_a interned"),
    );
    let hash_b = structural_hash_of(
        &graph_b,
        &graph_b.node_data(binding_b).expect("binding_b interned"),
    );
    assert_eq!(
        hash_a, hash_b,
        "SYNTHETIC-BINDING CROSS-GRAPH STABILITY: two `SyntheticBinding`s with the same \
         content-free id and a `value_node` pointing at the same-content target must hash \
         IDENTICALLY even when that target received a different arena ordinal in each graph. The \
         ordinal-folding code folded the raw `value_node` ordinal and FAILED this; descending it \
         via `encode_child` hashes the target's `Primitive(String)` content and passes."
    );
}

/// DISTINCTION BY VALUE-NODE CONTENT UNDER COLLIDING ORDINALS. Two graphs each
/// hold a `SyntheticBinding` with the SAME content-free [`SyntheticBindingId`],
/// but their `value_node` targets COLLIDE on the arena ordinal while pointing at
/// DIFFERENT content (`Primitive(String)` vs `Primitive(Number)`). The two
/// structural fingerprints must DIFFER.
///
/// This proves the content distinction is preserved and guards against a naive
/// "just drop `value_node`" fix: dropping it would leave only the identical
/// content-free id, so the two semantically-DISTINCT bindings would COLLIDE on
/// the footprint (a false-collision, strictly worse than the ordinal fold's
/// false-distinction). It also defeats an ordinal-only shortcut: the ordinals
/// collide, so only descending `value_node`'s content distinguishes them.
#[test]
fn synthetic_binding_distinguishes_value_node_content_under_colliding_ordinals() {
    // Graph A: `String` target is the FIRST interned node → ordinal N.
    let graph_a = empty_graph();
    let target_a = graph_a.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let binding_a =
        intern_synthetic_binding(&graph_a, synthetic_binding_id("default", "row"), target_a);

    // Graph B: `Number` target is the FIRST interned node → the SAME ordinal N,
    // but DIFFERENT content.
    let graph_b = empty_graph();
    let target_b = graph_b.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let binding_b =
        intern_synthetic_binding(&graph_b, synthetic_binding_id("default", "row"), target_b);

    // Precondition: the two `value_node` targets COLLIDE on the ordinal — so a
    // fingerprint that distinguished only by ordinal (or that dropped
    // `value_node` entirely) could NOT tell these two bindings apart.
    assert_eq!(
        target_a.0, target_b.0,
        "test setup must collide the `value_node` target ordinal across the two graphs"
    );

    let hash_a = structural_hash_of(
        &graph_a,
        &graph_a.node_data(binding_a).expect("binding_a interned"),
    );
    let hash_b = structural_hash_of(
        &graph_b,
        &graph_b.node_data(binding_b).expect("binding_b interned"),
    );
    assert_ne!(
        hash_a, hash_b,
        "SYNTHETIC-BINDING CONTENT DISTINCTION: two `SyntheticBinding`s with the same content-free \
         id whose `value_node` points at DIFFERENT content (`String` vs `Number`) must hash \
         DIFFERENTLY even though the target ordinals collide. Descending `value_node` via \
         `encode_child` distinguishes the targets' content; dropping `value_node` would collide \
         them, and an ordinal-only fold could not tell them apart under colliding ordinals."
    );
}
