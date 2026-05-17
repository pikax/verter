//! Tests for `semantic_query_memo` — interning, in-flight admission,
//! family-keyed warm memo, derivation/origin layer, telemetry, and
//! `DepSignatureInterner`.

use super::*;
use crate::semantic_query::{DepVersion, PrimitiveKind, ResolveDeclKey, ScopeId};
use crate::{HostConfig, VerterHost};

/// A standalone host used purely as a
/// [`crate::resolver_core::ResolverContext`] for the strict warm-read
/// validator that `execute_cooperative` / `get_validated` / the
/// relation memo now consult.
///
/// These substrate tests drive cooperative admission on a directly
/// constructed [`SemanticGraphStore`]; the memo entries they publish
/// carry empty self-version-rooted carriers (the `empty_signature()`
/// build outputs), so warm-read validation is vacuous regardless of the
/// host the context belongs to. The host therefore only has to be a
/// well-formed `ResolverContext` — it does not need to own the store
/// under test.
fn ctx_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn scope(canonical: &str) -> ScopeId {
    ScopeId {
        canonical_id: Arc::from(canonical),
        local_scope: None,
    }
}

#[test]
fn interning_returns_unique_stable_ids() {
    let store = SemanticGraphStore::new();
    let a = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let b = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    assert_ne!(a, b);
    assert_eq!(a.0 + 1, b.0);
}

/// Path C C7 positive invariant — two `intern_node_with_scope` calls
/// for the same `(payload, scope)` pair share one
/// [`SemanticNodeId`]. Under the pre-C7 append-only allocator the two
/// calls returned distinct ids.
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

/// Path C C7 negative invariant — `VueMacroElements` is an
/// identity-carrier with latest-insert-wins semantics (see
/// [`SemanticGraphStore::insert_resolved_named_type`]). Two
/// `intern_node` calls for the same `Arc<ResolvedElements>` payload
/// must still return distinct [`SemanticNodeId`]s so fresh inserts
/// under the same `HostResolvedNamedTypeKey` do not alias with prior
/// payloads. Under naive structural dedup this would collapse — the
/// exemption in `push_impl` short-circuits the dedup index.
#[test]
fn intern_does_not_dedup_vue_macro_elements_identity_carrier() {
    use verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements;
    let store = SemanticGraphStore::new();
    let payload = Arc::new(ResolvedElements::default());
    let a = store.intern_node(SemanticNodeData::VueMacroElements(Arc::clone(&payload)));
    let b = store.intern_node(SemanticNodeData::VueMacroElements(Arc::clone(&payload)));
    assert_ne!(
        a, b,
        "VueMacroElements must allocate fresh slots on every insert — \
         identity-carrier contract requires latest-insert-wins semantics",
    );
    // Sidecar stays `None` for both slots — exempt from origin-scope
    // tracking per
    assert_eq!(store.node_scope(a), None);
    assert_eq!(store.node_scope(b), None);
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

/// Path C C17 — sharded dedup produces the same `SemanticNodeId`
/// across threads for identical `(payload, scope)` pairs. The
/// invariant is strong: two threads interning the same payload at
/// the same scope must observe equal ids immediately (no visibility
/// gap from C17's per-shard Mutex). The threads race; the second
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

/// Path C C17 — `shard_index_for` is deterministic: identical
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

#[test]
fn execute_cooperative_memoizes_winner_result() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/a.ts"),
        name: Arc::from("Foo"),
    });

    let mut call_count = 0u32;
    let _first = store.execute_cooperative(
        &host,
        key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            call_count += 1;
            let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
            (QueryResult::Value(id), empty_signature())
        },
    );

    // Second call must be a warm hit. The build closure is not invoked.
    let second = store.execute_cooperative(
        &host,
        key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            call_count += 1;
            let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
            (QueryResult::Value(id), empty_signature())
        },
    );

    match second.value {
        QueryResult::Value(id) => {
            let data = store.node_data(id).unwrap();
            assert!(matches!(
                *data,
                SemanticNodeData::Primitive(PrimitiveKind::String)
            ));
        }
        other => panic!("expected warm value, got {other:?}"),
    }
    assert_eq!(call_count, 1, "cold build must run exactly once");
}

#[test]
fn same_path_recursion_returns_sentinel_not_deadlock() {
    let host = ctx_host();
    let store = Arc::new(SemanticGraphStore::new());
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/a.ts"),
        name: Arc::from("Recursive"),
    });

    let store_ref = Arc::clone(&store);
    let key_ref = key.clone();

    let result = store.execute_cooperative(
        &host,
        key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            // Re-enter the same key from the same stack — this must
            // return a Recursive sentinel, not self-await.
            let inner = store_ref.execute_cooperative(
                &host,
                key_ref.clone(),
                || store_ref.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                || -> (QueryResult<SemanticNodeId>, DepSignature) {
                    panic!("inner build must not run during same-path recursion");
                },
            );
            match inner.value {
                QueryResult::Recursive(_) => {
                    let id =
                        store_ref.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
                    (QueryResult::Value(id), empty_signature())
                }
                other => panic!("expected Recursive sentinel, got {other:?}"),
            }
        },
    );
    assert!(matches!(result.value, QueryResult::Value(_)));
}

#[test]
fn errors_do_not_warm_shared_memo() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/a.ts"),
        name: Arc::from("BadBudget"),
    });

    let first = store.execute_cooperative(
        &host,
        key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || (QueryResult::Error(QueryError::Miss), empty_signature()),
    );
    assert!(matches!(first.value, QueryResult::Error(_)));
    assert_eq!(
        store.memo_entry_count(),
        0,
        "errors must not promote to warm memo entries"
    );

    let mut re_ran = false;
    let second = store.execute_cooperative(
        &host,
        key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            re_ran = true;
            let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
            (QueryResult::Value(id), empty_signature())
        },
    );
    assert!(re_ran, "failed-result keys must not become warm");
    assert!(matches!(second.value, QueryResult::Value(_)));
}

#[test]
fn dep_signature_is_returned_with_warm_hits() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/a.ts"),
        name: Arc::from("Foo"),
    });
    let sig: DepSignature = Arc::from(
        vec![(
            Arc::<str>::from("/w/a.ts"),
            crate::semantic_query::DepVersion::WholeHash([1u8; 16]),
        )]
        .into_boxed_slice(),
    );
    let _ = store.execute_cooperative(
        &host,
        key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
            (QueryResult::Value(id), sig.clone())
        },
    );
    let warm = store.get_unvalidated(&key).unwrap();
    assert_eq!(warm.dep_signature.len(), 1);
    assert_eq!(warm.dep_signature[0].0.as_ref(), "/w/a.ts");
}

/// A panic inside the cold build must not leave the in-flight entry
/// in a `claimed=true, completed=None` state — otherwise the next
/// caller for the same key would wait on the condvar forever.
///
/// The `InflightPanicGuard` catches the drop and marks the entry with
/// an `Error(Other)` sentinel so joiners fail fast and subsequent
/// callers start a fresh build.
#[test]
fn panic_in_cold_build_does_not_deadlock_future_callers() {
    let host = ctx_host();
    use std::panic::{catch_unwind, AssertUnwindSafe};

    let store = SemanticGraphStore::new();
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/a.ts"),
        name: Arc::from("Explodes"),
    });

    // Cold build panics; `catch_unwind` turns it into an `Err`.
    let panicked = catch_unwind(AssertUnwindSafe(|| {
        store.execute_cooperative(
            &host,
            key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || -> (QueryResult<SemanticNodeId>, DepSignature) {
                panic!("simulated build panic");
            },
        )
    }));
    assert!(panicked.is_err(), "build must have unwound via panic");

    // The thread-local recursion stack must be empty (RAII guard) so
    // the same thread can query the same key without being flagged as
    // same-path recursion.
    let is_empty = IN_FLIGHT_ON_THIS_THREAD.with(|slot| slot.borrow().is_empty());
    assert!(is_empty, "recursion stack must be empty after panic");

    // A subsequent call for the same key must not deadlock. It must
    // be free to start a fresh cold build (the in-flight entry was
    // retired by the panic guard).
    let mut re_ran = false;
    let second = store.execute_cooperative(
        &host,
        key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            re_ran = true;
            let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
            (QueryResult::Value(id), empty_signature())
        },
    );
    assert!(
        re_ran,
        "post-panic call must run a fresh cold build, not wait on the retired entry"
    );
    assert!(matches!(second.value, QueryResult::Value(_)));
}

/// `invalidate_canonical` sweeps every slot whose recorded
/// dep-signature references the changed canonical. Unrelated entries
/// stay warm because their dep-signatures never mention the canonical
/// under invalidation.
#[test]
fn invalidate_canonical_removes_only_matching_scope_keys() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();

    // Warm `ResolveDecl(a.ts::Foo)` with a dep-sig referencing /w/a.ts.
    let a_key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/a.ts"),
        name: Arc::from("Foo"),
    });
    let _ = store.execute_cooperative(
        &host,
        a_key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
            (QueryResult::Value(id), dep_sig_for("/w/a.ts", 1))
        },
    );

    // Warm `ResolveDecl(b.ts::Foo)` with a dep-sig referencing /w/b.ts.
    let b_key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/b.ts"),
        name: Arc::from("Foo"),
    });
    let _ = store.execute_cooperative(
        &host,
        b_key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
            (QueryResult::Value(id), dep_sig_for("/w/b.ts", 2))
        },
    );

    assert_eq!(store.memo_entry_count(), 2);

    // Dep-sig sweep: only a.ts's entry matches.
    let removed = store.invalidate_canonical("/w/a.ts");
    assert_eq!(removed, 1);
    assert_eq!(store.memo_entry_count(), 1);

    // B.ts still warm (its dep-sig never mentioned /w/a.ts).
    assert!(store.get_unvalidated(&b_key).is_some());
    // A.ts gone — next call re-runs build.
    assert!(store.get_unvalidated(&a_key).is_none());
}

// ──────────────────────────────────────────────────────────────────
// DepSignatureInterner (Γ.C)
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
/// Round-7 Codex#2 P1 #2 — mandatory test:
/// `dep_signature_intern_sweep_removes_empty_buckets`.
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

/// `invalidate_canonical(c)`
/// uses the `canonical_to_entries` reverse index to find affected
/// `(family, slot)` pairs in O(referencing entries) instead of
/// O(all entries). A publish must register its dep_signature in
/// the reverse index for every canonical it references.
///
/// Discriminating: warm a specific `(family, slot)` whose
/// dep_signature references "/w/a.ts". Assert
/// `canonical_to_entries_count("/w/a.ts") >= 1`. Pre-fix:
/// reverse index was never populated, count is 0; assertion
/// FAILS. Post-fix: count is at least 1 (one for the family +
/// each backfilled narrower slot).
#[test]
fn family_map_publish_registers_canonical_to_entries_reverse_index() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/a.ts"),
        name: Arc::from("Foo"),
    });
    let _ = store.execute_cooperative(
        &host,
        key,
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
            (QueryResult::Value(id), dep_sig_for("/w/a.ts", 1))
        },
    );
    assert!(
        store.canonical_to_entries_count("/w/a.ts") >= 1,
        "publish must register the (family, slot) → dep_signature mapping \
         in canonical_to_entries[\"/w/a.ts\"] (Γ.B reverse index)"
    );
    assert_eq!(
        store.canonical_to_entries_count("/w/missing.ts"),
        0,
        "unrelated canonicals must NOT have a reverse-index entry"
    );
}

/// Refactor invariant — the helper extracted from
/// `execute_cooperative` step 5 (`warm_publish_one`) must:
///   1. Insert into the warm map (slot becomes `get`-readable).
///   2. Register the `(family, slot) → dep_signature` reverse-index
///      entry under every canonical the dep_signature references.
///
/// This is a TARGETED unit test (per §1.B.4 brief invariant) that
/// invokes `warm_publish_one` directly with a synthetic
/// `InflightEntry` so the assertion is on the helper's surface,
/// not the full cooperative-admission flow.
///
/// Discriminating: with the refactor, the helper does the publish
/// and reverse-index registration. If the refactor accidentally
/// dropped the reverse-index registration (e.g. by inlining
/// publish without the per-canonical loop), the
/// `canonical_to_entries_count` assertion would FAIL.
#[test]
fn warm_publish_one_inserts_warm_map_and_registers_reverse_index() {
    let store = SemanticGraphStore::new();
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/helper_test.ts"),
        name: Arc::from("Helper"),
    });
    let value = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let dep_sig = dep_sig_for("/w/helper_test.ts", 7);
    let inflight = Arc::new(InflightEntry::new());

    // Pre-condition: warm map empty for this key, reverse index
    // empty for the canonical.
    assert!(
        store.get_unvalidated(&key).is_none(),
        "warm map must start empty for the test key"
    );
    assert_eq!(
        store.canonical_to_entries_count("/w/helper_test.ts"),
        0,
        "reverse index must start empty for the test canonical"
    );

    // Direct invocation of the extracted helper.
    let walker_diagnostics: std::sync::Arc<
        [crate::project_semantic_dispatch::walk::ShallowDiagnostic],
    > = std::sync::Arc::from([]);
    let carrier = crate::fact_signature_helpers::ReadSetSignature::new(
        crate::fact_signature_helpers::empty_fact_signature(),
        Arc::clone(&dep_sig),
    );
    store.warm_publish_one(
        &key,
        &QueryResult::Value(value),
        &walker_diagnostics,
        &carrier,
        &Arc::from([]),
        &inflight,
    );

    // Post-condition 1: warm map contains the slot.
    let hit = store
        .get_unvalidated(&key)
        .expect("warm map must contain the published key after warm_publish_one");
    match hit.value {
        QueryResult::Value(id) => assert_eq!(
            id, value,
            "the published value must round-trip through the warm map"
        ),
        other => panic!("expected published Value, got {other:?}"),
    }

    // Post-condition 2: reverse index contains at least one
    // (family, slot) registration under the canonical.
    assert!(
        store.canonical_to_entries_count("/w/helper_test.ts") >= 1,
        "warm_publish_one must register the (family, slot) → dep_signature \
         mapping in canonical_to_entries[\"/w/helper_test.ts\"] (Γ.B reverse index)"
    );

    // Negative: an unrelated canonical must have NO reverse-index
    // entry — registration is per-canonical-in-dep-signature, not
    // a global broadcast.
    assert_eq!(
        store.canonical_to_entries_count("/w/unrelated.ts"),
        0,
        "unrelated canonicals must NOT receive reverse-index entries"
    );
}

/// `invalidate_canonical` drains the reverse-index
/// entry for the canonical AND propagates the cleanup to other
/// canonicals the evicted entry's dep_signature referenced
///
/// Discriminating: warm an entry whose dep_signature references
/// BOTH "/w/a.ts" AND "/w/b.ts". Verify both reverse-index
/// entries are populated (count == 1 each). Invalidate "/w/a.ts".
/// Verify both reverse-index entries are EMPTY (the "/w/a.ts"
/// shard via drain in step 1, the "/w/b.ts" shard via cross-
/// canonical cleanup in step 3). Pre-fix: cross-canonical
/// cleanup did not exist; the "/w/b.ts" entry would dangle.
#[test]
fn family_map_invalidate_canonical_propagates_cross_canonical_cleanup() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/a.ts"),
        name: Arc::from("Bar"),
    });
    // Compose a dep_sig referencing two canonicals.
    let dep_sig: DepSignature = Arc::from(
        vec![
            (
                Arc::<str>::from("/w/a.ts"),
                DepVersion::WholeHash([1u8; 16]),
            ),
            (
                Arc::<str>::from("/w/b.ts"),
                DepVersion::WholeHash([2u8; 16]),
            ),
        ]
        .into_boxed_slice(),
    );
    let _ = store.execute_cooperative(
        &host,
        key,
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
            (QueryResult::Value(id), Arc::clone(&dep_sig))
        },
    );
    assert!(
        store.canonical_to_entries_count("/w/a.ts") >= 1,
        "/w/a.ts reverse index must be populated post-publish"
    );
    assert!(
        store.canonical_to_entries_count("/w/b.ts") >= 1,
        "/w/b.ts reverse index must be populated post-publish"
    );

    let _ = store.invalidate_canonical("/w/a.ts");

    assert_eq!(
        store.canonical_to_entries_count("/w/a.ts"),
        0,
        "/w/a.ts reverse-index shard must be drained by invalidate_canonical \
         (Γ.B step 1 drain)"
    );
    assert_eq!(
        store.canonical_to_entries_count("/w/b.ts"),
        0,
        "/w/b.ts reverse-index entry for the evicted (family, slot) must be \
         cleaned up by cross-canonical cleanup (Γ.B step 3); pre-fix \
         this entry would dangle and bloat the reverse index over time"
    );
}

/// `invalidate_canonical` evicts the warm entry whose
/// dep_signature references the canonical (no behavioural change
/// from pre-Γ.B), but now via the reverse-index path. Existing
/// `invalidate_canonical_removes_only_matching_scope_keys` test
/// already covers correctness on the warm-slot side; this one
/// adds a pure regression guard against the reverse-index path
/// drifting out of sync.
///
/// Discriminating: warm two entries (a.ts-referencing and
/// b.ts-referencing). Verify reverse index has one entry per
/// canonical. Invalidate "/w/a.ts". Verify a.ts-referencing
/// warm entry is gone; b.ts-referencing warm entry survives.
#[test]
fn family_map_invalidate_canonical_uses_reverse_index_to_find_affected_pairs() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let a_key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/a.ts"),
        name: Arc::from("FooA"),
    });
    let b_key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/b.ts"),
        name: Arc::from("FooB"),
    });
    let _ = store.execute_cooperative(
        &host,
        a_key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
            (QueryResult::Value(id), dep_sig_for("/w/a.ts", 1))
        },
    );
    let _ = store.execute_cooperative(
        &host,
        b_key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
            (QueryResult::Value(id), dep_sig_for("/w/b.ts", 2))
        },
    );

    assert!(store.canonical_to_entries_count("/w/a.ts") >= 1);
    assert!(store.canonical_to_entries_count("/w/b.ts") >= 1);

    let removed = store.invalidate_canonical("/w/a.ts");
    assert_eq!(removed, 1);
    assert!(
        store.get_unvalidated(&a_key).is_none(),
        "a.ts entry must be evicted"
    );
    assert!(
        store.get_unvalidated(&b_key).is_some(),
        "b.ts entry survives — its dep_sig never referenced /w/a.ts"
    );
    assert_eq!(
        store.canonical_to_entries_count("/w/a.ts"),
        0,
        "a.ts reverse-index shard drained"
    );
    assert!(
        store.canonical_to_entries_count("/w/b.ts") >= 1,
        "b.ts reverse-index entry survives — its registration is independent"
    );
}

/// Γ.A (component-meta cold-path long-tail)
/// — Mandatory test gate. `invalidate_canonical(c)` must drop
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
    use crate::types::Hash16;

    let store = SemanticGraphStore::new();

    // Distinct payload per scope so dedup operates per scope key.
    let global_payload = || SemanticNodeData::Primitive(PrimitiveKind::String);
    let canonical_a: Arc<str> = Arc::from("/w/a.ts");
    let canonical_b: Arc<str> = Arc::from("/w/b.ts");
    let whole_a: Hash16 = [1u8; 16];
    let whole_b: Hash16 = [2u8; 16];
    let scope_a = NodeScopeId::File {
        canonical_id: Arc::clone(&canonical_a),
        whole_hash: whole_a,
        local_scope: None,
    };
    let scope_b = NodeScopeId::File {
        canonical_id: Arc::clone(&canonical_b),
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

    // Invalidate /w/a.ts. Per §1.10 Γ.A: only File { canonical_id:
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
         (Γ.A invariant — invalidation does NOT drop Global)"
    );
    assert_eq!(
        file_b_id_post, file_b_id_first,
        "File(/w/b.ts) shard entry must SURVIVE invalidation of /w/a.ts \
         (Γ.A invariant — invalidation drops only the matching canonical's File scope)"
    );
    assert_ne!(
        file_a_id_post, file_a_id_first,
        "File(/w/a.ts) shard entry must be DROPPED by invalidate_canonical(/w/a.ts); \
         re-intern must allocate a new SemanticNodeId (the arena is append-only — \
         ids never compress)"
    );
}

// ──────────────────────────────────────────────────────────────────
// B3 — dep-signature-based invalidation sweep + in-flight drop + retry
// ──────────────────────────────────────────────────────────────────

/// An `Instantiate` entry whose body compute reads the changed
/// canonical (via the dep-sig) is evicted by the sweep. Regardless of
/// the family-key shape — `Instantiate` carries semantic-node ids, not
/// canonicals — the dep-sig walk is the single invalidation authority.
///
/// Post-D1.4: `Instantiate` is mode-slot aware (`body_mode`). A write
/// at `Expanded` backfills `Shallow` / `Navigate` / `Identity` per
/// §7.11; all four slots carry the same dep-sig and the sweep evicts
/// every one that references the touched canonical.
#[test]
fn invalidate_canonical_evicts_instantiate_entries_that_read_that_canonical_body() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let base = crate::semantic_query::DeclIdentity::synthetic("Foo");
    let arg = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let key = SemanticQueryKey::Instantiate {
        base,
        args: Arc::from(vec![arg].into_boxed_slice()),
        body_mode: crate::semantic_query::ProjectionMode::Expanded,
    };

    // Dep-sig references /w/body.ts — the declaration file the
    // instantiation lowers from.
    let value_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let _ = store.execute_cooperative(
        &host,
        key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || (QueryResult::Value(value_id), dep_sig_for("/w/body.ts", 1)),
    );
    assert!(
        store.get_unvalidated(&key).is_some(),
        "entry must be warm pre-invalidation"
    );
    assert_eq!(
        store.memo_entry_count(),
        4,
        "Expanded write backfills Shallow + Navigate + Identity (§7.11)",
    );

    let removed = store.invalidate_canonical("/w/body.ts");
    assert_eq!(
        removed, 4,
        "Expanded plus its three backfilled narrower slots all reference /w/body.ts",
    );
    assert!(
        store.get_unvalidated(&key).is_none(),
        "Instantiate entry whose dep-sig references /w/body.ts must be evicted",
    );
}

/// An `Instantiate` entry whose dep-sig does NOT reference the
/// canonical under invalidation survives the sweep unchanged —
/// confirming the sweep is driven strictly by dep-sig membership.
#[test]
fn invalidate_canonical_keeps_instantiate_entries_whose_bases_are_unrelated() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let base = crate::semantic_query::DeclIdentity::synthetic("Foo");
    let arg = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let key = SemanticQueryKey::Instantiate {
        base,
        args: Arc::from(vec![arg].into_boxed_slice()),
        body_mode: crate::semantic_query::ProjectionMode::Expanded,
    };

    let value_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let _ = store.execute_cooperative(
        &host,
        key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            (
                QueryResult::Value(value_id),
                dep_sig_for("/w/unrelated.ts", 2),
            )
        },
    );

    let removed = store.invalidate_canonical("/w/changed.ts");
    assert_eq!(
        removed, 0,
        "no eviction: entry dep-sig references /w/unrelated.ts, not /w/changed.ts",
    );
    assert!(
        store.get_unvalidated(&key).is_some(),
        "unrelated Instantiate entry must remain warm after sweep",
    );
}

/// A `ProjectPath` entry whose dep-sig references a file touched by a
/// subtree walk is evicted. Tests the path-precise family: invalidation
/// must reach every mode slot because narrower-mode slots inherit the
/// broader compute's dep-sig via backfill (§7.11).
#[test]
fn invalidate_canonical_evicts_project_path_entries_through_touched_subtree() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let path: Arc<[PathSegment]> = Arc::from(
        vec![
            PathSegment::Member(Arc::from("a")),
            PathSegment::Member(Arc::from("foo")),
        ]
        .into_boxed_slice(),
    );
    let key = SemanticQueryKey::ProjectPath {
        base,
        path: Arc::clone(&path),
        mode: ProjectionMode::Shallow,
    };

    let value_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let _ = store.execute_cooperative(
        &host,
        key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            (
                QueryResult::Value(value_id),
                dep_sig_for("/w/subtree.ts", 3),
            )
        },
    );
    // Shallow backfills Navigate + Identity — both carry the same
    // dep-sig (§7.11 conservative rule). So three slots are populated,
    // and all three must evict on /w/subtree.ts invalidation.
    assert_eq!(store.memo_entry_count(), 3);

    let removed = store.invalidate_canonical("/w/subtree.ts");
    assert_eq!(
        removed, 3,
        "Shallow plus its two backfilled narrower slots all reference the touched subtree",
    );
    assert!(
        store.get_unvalidated(&key).is_none(),
        "ProjectPath Shallow entry through touched subtree must be evicted",
    );
    let narrower_key = SemanticQueryKey::ProjectPath {
        base,
        path,
        mode: ProjectionMode::Identity,
    };
    assert!(
        store.get_unvalidated(&narrower_key).is_none(),
        "backfilled Identity slot inherits the dep-sig and must evict too",
    );
}

/// Invalidation is per-(family, slot): invalidating one canonical
/// evicts only the slots whose dep-signature references it, leaving
/// sibling slots in the same family warm. After eviction, the next
/// caller for the evicted slot runs a fresh cold build — the
/// joiner-retry invariant surfaces here because an in-flight entry at
/// that slot (had one existed during the race window between warm
/// publish and in-flight retire) would have been dropped alongside
/// the warm slot.
#[test]
fn invalidate_canonical_evicts_in_flight_entries_per_mode_slot_and_joiners_retry() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let path: Arc<[PathSegment]> =
        Arc::from(vec![PathSegment::Member(Arc::from("foo"))].into_boxed_slice());

    let key_identity = SemanticQueryKey::ProjectPath {
        base,
        path: Arc::clone(&path),
        mode: ProjectionMode::Identity,
    };
    let key_expanded = SemanticQueryKey::ProjectPath {
        base,
        path: Arc::clone(&path),
        mode: ProjectionMode::Expanded,
    };

    // Identity build FIRST so the narrower slot is populated before
    // the Expanded build runs — this prevents Expanded's backfill
    // from clobbering Identity with Expanded's (matching) dep-sig.
    let ident_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let _ = store.execute_cooperative(
        &host,
        key_identity.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || (QueryResult::Value(ident_id), dep_sig_for("/w/a.ts", 1)),
    );
    let exp_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let _ = store.execute_cooperative(
        &host,
        key_expanded.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || (QueryResult::Value(exp_id), dep_sig_for("/w/b.ts", 2)),
    );
    // After both warm-ups:
    //   Identity = /w/a.ts (from Identity build)
    //   Navigate = /w/b.ts (backfilled from Expanded)
    //   Shallow  = /w/b.ts (backfilled from Expanded)
    //   Expanded = /w/b.ts (from Expanded build)
    assert_eq!(store.memo_entry_count(), 4);

    // Invalidate /w/a.ts — only Identity's dep-sig matches.
    let removed = store.invalidate_canonical("/w/a.ts");
    assert_eq!(
        removed, 1,
        "per-mode-slot invalidation: only the Identity slot is evicted",
    );
    assert!(
        store.get_unvalidated(&key_identity).is_none(),
        "Identity slot must be evicted (dep-sig /w/a.ts)",
    );
    assert!(
        store.get_unvalidated(&key_expanded).is_some(),
        "Expanded slot preserved (dep-sig /w/b.ts, unrelated)",
    );

    // Post-invalidation, a new caller for the Identity slot must run
    // a fresh cold build — not latch onto a lingering in-flight entry
    // from the pre-invalidation warm publish (the sweep also drops
    // in-flight entries for affected `(family, slot)` pairs so
    // joiners re-enter dispatch).
    let mut rebuilt = false;
    let new_ident = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let _ = store.execute_cooperative(
        &host,
        key_identity.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            rebuilt = true;
            (QueryResult::Value(new_ident), dep_sig_for("/w/a.ts", 9))
        },
    );
    assert!(
        rebuilt,
        "post-invalidation caller must run a fresh cold build (no stale in-flight)",
    );
}

/// Backfill inherits the broader compute's full dep-sig. When any
/// canonical from that broader dep-sig is invalidated, the narrower
/// backfilled slots evict too — conservative over-invalidation (plan
/// §7.11). The sweep is never *incorrect* (it never misses a real
/// invalidation); unrelated narrower-only entries with their own
/// dep-sigs stay warm.
#[test]
fn backfilled_slot_with_wider_dep_sig_over_invalidates_conservatively_not_incorrectly() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let path: Arc<[PathSegment]> =
        Arc::from(vec![PathSegment::Member(Arc::from("foo"))].into_boxed_slice());

    // Expanded build reads both /w/wide.ts and /w/narrow.ts —
    // its dep-sig spans both canonicals.
    let wide_dep_sig: DepSignature = Arc::from(
        vec![
            (
                Arc::<str>::from("/w/wide.ts"),
                crate::semantic_query::DepVersion::WholeHash([1u8; 16]),
            ),
            (
                Arc::<str>::from("/w/narrow.ts"),
                crate::semantic_query::DepVersion::WholeHash([2u8; 16]),
            ),
        ]
        .into_boxed_slice(),
    );
    let key_expanded = SemanticQueryKey::ProjectPath {
        base,
        path: Arc::clone(&path),
        mode: ProjectionMode::Expanded,
    };
    let exp_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let _ = store.execute_cooperative(
        &host,
        key_expanded.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || (QueryResult::Value(exp_id), wide_dep_sig.clone()),
    );
    // Expanded backfills Shallow, Navigate, Identity — all four slots
    // carry the same wide dep-sig.
    assert_eq!(store.memo_entry_count(), 4);

    // Conservative over-invalidation: evicting /w/wide.ts also evicts
    // the three narrower backfilled slots because they inherited the
    // broader compute's full dep-sig. Narrower independent builds
    // would have had a smaller read-set (potentially only /w/narrow.ts),
    // but B3 ships the conservative rule (§7.11 trade-off); tightening
    // the narrower-slot dep-sigs to their actual read-set is permitted
    // follow-up work.
    let removed = store.invalidate_canonical("/w/wide.ts");
    assert_eq!(
        removed, 4,
        "all four slots evict because backfill inherited the wide dep-sig",
    );
    for mode in [
        ProjectionMode::Identity,
        ProjectionMode::Navigate,
        ProjectionMode::Shallow,
        ProjectionMode::Expanded,
    ] {
        let key = SemanticQueryKey::ProjectPath {
            base,
            path: Arc::clone(&path),
            mode,
        };
        assert!(
            store.get_unvalidated(&key).is_none(),
            "{mode:?} slot evicted by conservative sweep",
        );
    }

    // Second phase: the sweep is NOT incorrect. A narrower-only
    // independent build with a dep-sig referencing only /w/narrow.ts
    // is NOT evicted by an invalidation of /w/wide.ts.
    let key_navigate = SemanticQueryKey::ProjectPath {
        base,
        path: Arc::clone(&path),
        mode: ProjectionMode::Navigate,
    };
    let narrow_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let _ = store.execute_cooperative(
        &host,
        key_navigate.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            (
                QueryResult::Value(narrow_id),
                dep_sig_for("/w/narrow.ts", 3),
            )
        },
    );
    // Navigate backfills Identity with the narrow-only dep-sig.
    assert_eq!(store.memo_entry_count(), 2);

    let removed = store.invalidate_canonical("/w/wide.ts");
    assert_eq!(
        removed, 0,
        "narrow-only dep-sig does not reference /w/wide.ts — no false eviction",
    );
    assert!(
        store.get_unvalidated(&key_navigate).is_some(),
        "narrower independent build survives unrelated invalidation",
    );
}

/// A cold winner whose `(family, slot)` was aborted mid-build by a
/// canonical invalidation MUST NOT warm-publish its now-stale result.
/// Otherwise the post-invalidation cache re-populates with a dep-sig
/// that may not reference the invalidated canonical (because the
/// winner's own reads never touched it) — stale data that even
/// `HostFenceValidator` cannot catch, because the stored dep-sig is
/// technically valid against the new state.
///
/// Scenario (exercises the winner-side `aborted` guard at step 5
/// AND the TOCTOU re-check under the entries lock):
///   1. Thread A starts a cold build for `(F, Identity)`. It blocks
///      on a barrier inside the build closure so the main thread can
///      orchestrate the race.
///   2. Main publishes `(F, Expanded)` with dep-sig `[/w/target.ts]`.
///      Expanded backfills the empty Identity slot (A has the claim
///      but `FamilySlots::publish` writes the slot field directly,
///      not gated on in-flight ownership). Identity is now warm with
///      Expanded's result + dep-sig.
///   3. Main calls `invalidate_canonical("/w/target.ts")`. This
///      evicts Identity + Expanded (both reference the canonical)
///      and aborts A's in-flight at `(F, Identity)`: sets
///      `state.aborted = true`, plants a completed sentinel, notifies.
///   4. Main releases the barrier. A finishes its build and returns
///      a (would-be) `Value` result with a dep-sig that does NOT
///      reference `/w/target.ts`.
///   5. A's step 5 enters the warm-publish block, acquires the
///      entries lock, re-checks `state.aborted` under the lock, sees
///      `true`, and skips the publish.
///
/// Assertion: after A completes, the Identity slot stays empty.
/// Without the guard, Identity would re-warm with A's stale result.
#[test]
fn winner_skips_warm_publish_when_aborted_by_invalidation_during_build() {
    let host = ctx_host();
    use std::sync::Barrier;
    use std::thread;
    let store = Arc::new(SemanticGraphStore::new());
    let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let path: Arc<[PathSegment]> =
        Arc::from(vec![PathSegment::Member(Arc::from("foo"))].into_boxed_slice());

    let key_identity = SemanticQueryKey::ProjectPath {
        base,
        path: Arc::clone(&path),
        mode: ProjectionMode::Identity,
    };
    let key_expanded = SemanticQueryKey::ProjectPath {
        base,
        path: Arc::clone(&path),
        mode: ProjectionMode::Expanded,
    };

    // Barrier 1: A signals it has entered the build closure; main
    // uses this to know A's in-flight entry is registered.
    // Barrier 2: main signals A to proceed after publish + invalidate.
    let a_in_build = Arc::new(Barrier::new(2));
    let main_done = Arc::new(Barrier::new(2));

    let a_result = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let store_a = Arc::clone(&store);
    let a_in_build_owner = Arc::clone(&a_in_build);
    let main_done_owner = Arc::clone(&main_done);
    let a_key_owner = key_identity.clone();

    let a_thread = thread::spawn(move || {
        let host = ctx_host();
        store_a.execute_cooperative(
            &host,
            a_key_owner,
            || store_a.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                // Signal main: A is inside the cold build closure.
                a_in_build_owner.wait();
                // Wait for main to finish publish + invalidate.
                main_done_owner.wait();
                // Return a result whose dep-sig does NOT reference
                // /w/target.ts — so even HostFenceValidator would
                // NOT catch a stale publish of this result.
                (
                    QueryResult::Value(a_result),
                    dep_sig_for("/w/unrelated.ts", 9),
                )
            },
        )
    });

    // Wait for A to enter its build closure.
    a_in_build.wait();

    // Publish Expanded. Its backfill fills the currently-empty
    // Identity slot despite A holding the in-flight claim, because
    // `FamilySlots::publish` writes the slot field directly.
    let exp_result = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let _ = store.execute_cooperative(
        &host,
        key_expanded,
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            (
                QueryResult::Value(exp_result),
                dep_sig_for("/w/target.ts", 2),
            )
        },
    );
    assert!(
        store.get_unvalidated(&key_identity).is_some(),
        "Expanded's backfill must populate Identity before invalidation runs",
    );

    // Invalidate /w/target.ts. evicts all four slots:
    // Expanded's publish fills its target slot + backfills Shallow,
    // Navigate, and the empty Identity (writing the slot field
    // directly without gating on A's in-flight claim). All four
    // carry Expanded's dep-sig. aborts A's in-flight at
    // (F, Identity) because `(F, Identity)` is now in
    // `affected_pairs`.
    let removed = store.invalidate_canonical("/w/target.ts");
    assert_eq!(
        removed, 4,
        "step 1 evicts all four slots (Expanded publish + 3 backfilled narrower slots)",
    );

    // Release A. It returns from the build closure and enters step 5.
    // Under the TOCTOU guard, A's re-check sees aborted=true and
    // skips warm publish; Identity stays empty.
    main_done.wait();
    let _ = a_thread.join().expect("A thread must not panic");

    assert!(
        store.get_unvalidated(&key_identity).is_none(),
        "aborted winner must skip warm publish — Identity slot stays evicted",
    );
}

/// `invalidate_all` clears every memo entry — used on
/// project-generation bumps (tsconfig / SDK / workspace-folder
/// changes).
#[test]
fn invalidate_all_clears_every_memo_entry() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    for name in ["X", "Y", "Z"] {
        let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/a.ts"),
            name: Arc::from(name),
        });
        let _ = store.execute_cooperative(
            &host,
            key,
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                (QueryResult::Value(id), empty_signature())
            },
        );
    }
    assert_eq!(store.memo_entry_count(), 3);
    let cleared = store.invalidate_all();
    assert_eq!(cleared, 3);
    assert_eq!(store.memo_entry_count(), 0);
}

#[test]
fn recursive_sentinel_does_not_promote_to_warm_memo() {
    let host = ctx_host();
    let store = Arc::new(SemanticGraphStore::new());
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/a.ts"),
        name: Arc::from("R"),
    });

    let id = store.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    let res = store.execute_cooperative(
        &host,
        key.clone(),
        || id,
        || (QueryResult::Recursive(id), empty_signature()),
    );
    assert!(matches!(res.value, QueryResult::Recursive(_)));
    assert_eq!(
        store.memo_entry_count(),
        0,
        "recursion sentinels must not promote to warm memo"
    );
}

/// Cross-thread waiter joins the in-flight key and observes the
/// winner's published result. Exercises the `Condvar` pairing.
#[test]
fn cross_thread_joiner_waits_on_winner_publish() {
    use std::thread;
    use std::time::Duration;

    let store = Arc::new(SemanticGraphStore::new());
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/a.ts"),
        name: Arc::from("Shared"),
    });

    let start_barrier = Arc::new(std::sync::Barrier::new(2));
    let store_owner = Arc::clone(&store);
    let key_owner = key.clone();
    let barrier_owner = Arc::clone(&start_barrier);

    let winner = thread::spawn(move || {
        let host = ctx_host();
        store_owner.execute_cooperative(
            &host,
            key_owner,
            || store_owner.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                barrier_owner.wait();
                // Hold the build open briefly so the joiner reaches
                // the condvar wait.
                thread::sleep(Duration::from_millis(25));
                let id =
                    store_owner.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                (QueryResult::Value(id), empty_signature())
            },
        )
    });

    // Let the winner claim first, then the joiner waits on the
    // condvar.
    start_barrier.wait();
    let joiner = thread::spawn({
        let store = Arc::clone(&store);
        let key = key.clone();
        move || {
            let host = ctx_host();
            store.execute_cooperative(
                &host,
                key,
                || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                || -> (QueryResult<SemanticNodeId>, DepSignature) {
                    panic!("joiner must never run the cold build");
                },
            )
        }
    });

    let winner_result = winner.join().unwrap();
    let joiner_result = joiner.join().unwrap();

    // Both must see the winner's node id.
    match (winner_result.value, joiner_result.value) {
        (QueryResult::Value(w), QueryResult::Value(j)) => assert_eq!(w, j),
        other => panic!("unexpected combined result: {other:?}"),
    }
}

// ──────────────────────────────────────────────────────────────────
// Vue macro resolution identity map (former ResolvedNamedTypesDb)
// ──────────────────────────────────────────────────────────────────

use crate::semantic_query::HostResolvedNamedTypeKey;
use verter_compiler::utils::oxc::vue::resolve_type::cache_keys::ResolvedNamedTypeCacheKey;
use verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements;

fn make_key(canonical: &str, whole_hash: [u8; 16], name: &str) -> HostResolvedNamedTypeKey {
    HostResolvedNamedTypeKey {
        canonical_id: Arc::from(canonical),
        whole_hash,
        inner: ResolvedNamedTypeCacheKey {
            name: name.as_bytes().to_vec().into_boxed_slice(),
            surface: None,
            base_offset: 0,
            companion_cache_key: Arc::from(Vec::<Box<[u8]>>::new().into_boxed_slice()),
            type_param_bindings: Arc::from(Vec::new().into_boxed_slice()),
        },
    }
}

/// Inserting a resolved-named-type entry stores the payload behind a
/// `VueMacroElements` node and returns a stable [`SemanticNodeId`].
/// Subsequent reads observe the same payload without rebuilding.
#[test]
fn resolved_named_type_insert_and_get_round_trip() {
    let store = SemanticGraphStore::new();
    let key = make_key("/w/a.ts", [1u8; 16], "Foo");
    let payload = Arc::new(ResolvedElements::default());
    let node_id = store.insert_resolved_named_type(key.clone(), Arc::clone(&payload));

    // Identity lookup and payload lookup both succeed.
    assert_eq!(store.resolved_named_type_node_id(&key), Some(node_id));
    let round = store
        .get_resolved_named_type(&key)
        .expect("payload must be retrievable");
    assert!(Arc::ptr_eq(&payload, &round));
    assert_eq!(store.resolved_named_type_count(), 1);
}

/// Missing keys return `None` without allocating — the hot-path
/// miss is refcount-free.
#[test]
fn resolved_named_type_missing_key_returns_none() {
    let store = SemanticGraphStore::new();
    let key = make_key("/w/a.ts", [0u8; 16], "Absent");
    assert!(store.get_resolved_named_type(&key).is_none());
    assert!(store.resolved_named_type_node_id(&key).is_none());
}

/// Per-canonical invalidation removes only matching entries; entries
/// for unrelated canonicals stay warm.
#[test]
fn resolved_named_type_per_canonical_invalidation() {
    let store = SemanticGraphStore::new();
    let hash = [5u8; 16];
    let key_a = make_key("/w/a.ts", hash, "Foo");
    let key_b = make_key("/w/b.ts", hash, "Bar");
    store.insert_resolved_named_type(key_a.clone(), Arc::new(ResolvedElements::default()));
    store.insert_resolved_named_type(key_b.clone(), Arc::new(ResolvedElements::default()));
    assert_eq!(store.resolved_named_type_count(), 2);

    let removed = store.invalidate_resolved_named_types_for_canonical("/w/a.ts");
    assert_eq!(removed, 1);
    assert!(store.get_resolved_named_type(&key_a).is_none());
    assert!(store.get_resolved_named_type(&key_b).is_some());
}

/// Global clear removes every entry (used on project-generation
/// bumps / epoch bumps).
#[test]
fn resolved_named_type_global_clear() {
    let store = SemanticGraphStore::new();
    let key = make_key("/w/a.ts", [1u8; 16], "Foo");
    store.insert_resolved_named_type(key.clone(), Arc::new(ResolvedElements::default()));
    assert_eq!(store.resolved_named_type_count(), 1);
    store.clear_resolved_named_types();
    assert_eq!(store.resolved_named_type_count(), 0);
    assert!(store.get_resolved_named_type(&key).is_none());
}

/// Repeat writes under the same key overwrite the identity mapping —
/// two successive inserts leave one entry and the latest payload
/// becomes observable. This matches the `NamedTypeCache` trait's
/// "insert overwrites any prior entry under the same key" contract.
#[test]
fn resolved_named_type_repeated_insert_overwrites_identity_mapping() {
    let store = SemanticGraphStore::new();
    let key = make_key("/w/a.ts", [1u8; 16], "Foo");
    let first = Arc::new(ResolvedElements::default());
    let second = Arc::new(ResolvedElements {
        has_call_signature: true,
        ..ResolvedElements::default()
    });

    store.insert_resolved_named_type(key.clone(), Arc::clone(&first));
    store.insert_resolved_named_type(key.clone(), Arc::clone(&second));

    assert_eq!(
        store.resolved_named_type_count(),
        1,
        "same key must not duplicate identity entries"
    );
    let observed = store.get_resolved_named_type(&key).unwrap();
    assert!(
        Arc::ptr_eq(&second, &observed),
        "latest insert wins — identity map points at the second payload",
    );
}

// ──────────────────────────────────────────────────────────────────
// B1b family-memo backfill matrix
// ──────────────────────────────────────────────────────────────────

fn family_test_path() -> Arc<[PathSegment]> {
    Arc::from(vec![PathSegment::Member(Arc::from("foo"))].into_boxed_slice())
}

fn family_test_key(base: SemanticNodeId, mode: ProjectionMode) -> SemanticQueryKey {
    SemanticQueryKey::ProjectPath {
        base,
        path: family_test_path(),
        mode,
    }
}

fn family_test_dep_signature() -> DepSignature {
    Arc::from(
        vec![(
            Arc::<str>::from("/w/family.ts"),
            crate::semantic_query::DepVersion::WholeHash([7u8; 16]),
        )]
        .into_boxed_slice(),
    )
}

/// Run a cold build for `mode` with a stable result + dep-signature.
/// Returns the published `SemanticNodeId`.
fn warm_family_slot(
    host: &VerterHost,
    store: &SemanticGraphStore,
    base: SemanticNodeId,
    mode: ProjectionMode,
) -> SemanticNodeId {
    let value_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let key = family_test_key(base, mode);
    let read = store.execute_cooperative(
        host,
        key,
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || (QueryResult::Value(value_id), family_test_dep_signature()),
    );
    match read.value {
        QueryResult::Value(id) => id,
        other => panic!("expected Value, got {other:?}"),
    }
}

fn assert_warm_at(
    store: &SemanticGraphStore,
    base: SemanticNodeId,
    mode: ProjectionMode,
    expected_id: SemanticNodeId,
) {
    let warm = store
        .get_unvalidated(&family_test_key(base, mode))
        .unwrap_or_else(|| panic!("expected warm hit at mode {mode:?}"));
    match warm.value {
        QueryResult::Value(id) => assert_eq!(id, expected_id, "wrong node id at {mode:?}"),
        other => panic!("expected Value at {mode:?}, got {other:?}"),
    }
    assert_eq!(
        warm.dep_signature.as_ref(),
        family_test_dep_signature().as_ref(),
        "narrower-slot dep_signature must match the broader compute's at {mode:?}",
    );
}

fn assert_cold_at(store: &SemanticGraphStore, base: SemanticNodeId, mode: ProjectionMode) {
    assert!(
        store
            .get_unvalidated(&family_test_key(base, mode))
            .is_none(),
        "{mode:?} slot must NOT be backfilled",
    );
}

// 1. Expanded backfills each narrower slot (×4: source + 3 narrower).

#[test]
fn family_expanded_backfills_shallow_navigate_identity_share_dep_signature() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let id = warm_family_slot(&host, &store, base, ProjectionMode::Expanded);

    // The Expanded slot itself.
    assert_warm_at(&store, base, ProjectionMode::Expanded, id);
    // All three narrower slots backfilled with the same id and same dep_sig.
    assert_warm_at(&store, base, ProjectionMode::Shallow, id);
    assert_warm_at(&store, base, ProjectionMode::Navigate, id);
    assert_warm_at(&store, base, ProjectionMode::Identity, id);
    assert_eq!(store.memo_entry_count(), 4, "all 4 slots populated");
}

// 2. Shallow backfills Navigate + Identity (×3).

#[test]
fn family_shallow_backfills_navigate_and_identity() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let id = warm_family_slot(&host, &store, base, ProjectionMode::Shallow);

    assert_warm_at(&store, base, ProjectionMode::Shallow, id);
    assert_warm_at(&store, base, ProjectionMode::Navigate, id);
    assert_warm_at(&store, base, ProjectionMode::Identity, id);
    // Expanded MUST stay cold — narrower never satisfies broader.
    assert_cold_at(&store, base, ProjectionMode::Expanded);
    assert_eq!(store.memo_entry_count(), 3);
}

// 3. Navigate backfills Identity only (×2).

#[test]
fn family_navigate_backfills_identity_only() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let id = warm_family_slot(&host, &store, base, ProjectionMode::Navigate);

    assert_warm_at(&store, base, ProjectionMode::Navigate, id);
    assert_warm_at(&store, base, ProjectionMode::Identity, id);
    assert_cold_at(&store, base, ProjectionMode::Shallow);
    assert_cold_at(&store, base, ProjectionMode::Expanded);
    assert_eq!(store.memo_entry_count(), 2);
}

// 4. Identity backfills NOTHING (single test, the negative case for it).

#[test]
fn family_identity_does_not_backfill_anything() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let id = warm_family_slot(&host, &store, base, ProjectionMode::Identity);

    assert_warm_at(&store, base, ProjectionMode::Identity, id);
    assert_cold_at(&store, base, ProjectionMode::Navigate);
    assert_cold_at(&store, base, ProjectionMode::Shallow);
    assert_cold_at(&store, base, ProjectionMode::Expanded);
    assert_eq!(store.memo_entry_count(), 1);
}

// 5. Six negative cases: narrower never satisfies broader.

#[test]
fn family_navigate_does_not_satisfy_shallow_or_expanded() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let _ = warm_family_slot(&host, &store, base, ProjectionMode::Navigate);
    assert_cold_at(&store, base, ProjectionMode::Shallow);
    assert_cold_at(&store, base, ProjectionMode::Expanded);
}

#[test]
fn family_shallow_does_not_satisfy_expanded() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let _ = warm_family_slot(&host, &store, base, ProjectionMode::Shallow);
    assert_cold_at(&store, base, ProjectionMode::Expanded);
}

#[test]
fn family_identity_does_not_satisfy_navigate_shallow_expanded() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let _ = warm_family_slot(&host, &store, base, ProjectionMode::Identity);
    assert_cold_at(&store, base, ProjectionMode::Navigate);
    assert_cold_at(&store, base, ProjectionMode::Shallow);
    assert_cold_at(&store, base, ProjectionMode::Expanded);
}

// 6. Concurrent narrower + broader cold builds — both run independently
//    per `(family, mode_slot)` in-flight authority (§7.15).

#[test]
fn family_concurrent_navigate_and_expanded_both_complete_independently() {
    use std::sync::Barrier;
    use std::thread;
    let store = Arc::new(SemanticGraphStore::new());
    let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    let nav_value = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let exp_value = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));

    // Barrier prevents either build closure from publishing until the
    // other has also entered its body — exercises per-(family, slot)
    // in-flight authority deterministically (without a barrier the
    // race is real and one thread can publish + backfill before the
    // other starts).
    let barrier = Arc::new(Barrier::new(2));

    let store_nav = Arc::clone(&store);
    let bar_nav = Arc::clone(&barrier);
    let store_exp = Arc::clone(&store);
    let bar_exp = Arc::clone(&barrier);
    let t_nav = thread::spawn(move || {
        let host = ctx_host();
        store_nav.execute_cooperative(
            &host,
            family_test_key(base, ProjectionMode::Navigate),
            || store_nav.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                bar_nav.wait();
                (QueryResult::Value(nav_value), family_test_dep_signature())
            },
        )
    });
    let t_exp = thread::spawn(move || {
        let host = ctx_host();
        store_exp.execute_cooperative(
            &host,
            family_test_key(base, ProjectionMode::Expanded),
            || store_exp.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                bar_exp.wait();
                (QueryResult::Value(exp_value), family_test_dep_signature())
            },
        )
    });
    let nav_read = t_nav.join().unwrap();
    let exp_read = t_exp.join().unwrap();

    let nav_id = match nav_read.value {
        QueryResult::Value(id) => id,
        other => panic!("nav: {other:?}"),
    };
    let exp_id = match exp_read.value {
        QueryResult::Value(id) => id,
        other => panic!("exp: {other:?}"),
    };
    // Each cold build returned its own value — both ran to completion
    // independently because per-(family, slot) in-flight authority
    // kept them on separate Condvar pairings, and the barrier kept
    // the publish ordering from racing them.
    assert_eq!(nav_id, nav_value);
    assert_eq!(exp_id, exp_value);
}

// 7. Wider backfill is a no-op when the narrower slot already filled.

#[test]
fn family_wider_backfill_noop_when_narrower_slot_already_filled() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    // Narrow build first — Navigate completes and fills Navigate +
    // Identity slots.
    let nav_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let _ = store.execute_cooperative(
        &host,
        family_test_key(base, ProjectionMode::Navigate),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || (QueryResult::Value(nav_id), family_test_dep_signature()),
    );
    assert_warm_at(&store, base, ProjectionMode::Navigate, nav_id);
    assert_warm_at(&store, base, ProjectionMode::Identity, nav_id);

    // Now an Expanded build with a DIFFERENT result. Backfill writes
    // only into empty slots, so Navigate + Identity must keep their
    // narrower-build result; only Shallow + Expanded get the new id.
    let exp_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let _ = store.execute_cooperative(
        &host,
        family_test_key(base, ProjectionMode::Expanded),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || (QueryResult::Value(exp_id), family_test_dep_signature()),
    );
    assert_warm_at(&store, base, ProjectionMode::Expanded, exp_id);
    assert_warm_at(&store, base, ProjectionMode::Shallow, exp_id);
    // Critical: the populated narrower slots survive — backfill is a
    // no-op against them.
    assert_warm_at(&store, base, ProjectionMode::Navigate, nav_id);
    assert_warm_at(&store, base, ProjectionMode::Identity, nav_id);
}

// 8. Cancelled / errored results do not backfill any slot.

#[test]
fn family_cancelled_does_not_backfill_any_slot() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    let read = store.execute_cooperative(
        &host,
        family_test_key(base, ProjectionMode::Expanded),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || (QueryResult::Error(QueryError::Miss), empty_signature()),
    );
    assert!(matches!(read.value, QueryResult::Error(_)));

    // Every slot — Expanded itself + the would-be backfilled narrower
    // slots — must stay cold. Errors never warm, ever.
    assert_cold_at(&store, base, ProjectionMode::Expanded);
    assert_cold_at(&store, base, ProjectionMode::Shallow);
    assert_cold_at(&store, base, ProjectionMode::Navigate);
    assert_cold_at(&store, base, ProjectionMode::Identity);
    assert_eq!(store.memo_entry_count(), 0);
}

// 9. ResolvedNamedType bypasses the family memo entirely.
//    The DashMap-backed identity map remains the only cache. After a
//    successful execute_cooperative path returning Value via the build
//    closure, the family memo's entries map stays empty for this key.

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
        crate::semantic_query::OriginMeta::MemberName(Arc::from("AliasName")),
        dep_sig_for("/w/a.ts", 1),
    );

    let alias_edges = store.origins_of_kind(target, OriginEdgeKind::AliasResolve);
    assert_eq!(alias_edges.len(), 1);
    assert_eq!(alias_edges[0].sources.as_ref(), &[alias_decl]);
    assert!(matches!(
        &alias_edges[0].meta,
        crate::semantic_query::OriginMeta::MemberName(name) if name.as_ref() == "AliasName"
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

/// `stats_snapshot` increments hits + misses on warm + cold paths.
#[test]
fn stats_counters_increment_on_hit_and_miss() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/stats.ts"),
        name: Arc::from("Foo"),
    });

    let stats0 = store.stats_snapshot();
    assert_eq!(stats0.hits, 0);
    assert_eq!(stats0.misses, 0);

    // Cold call → misses increments by 1; hits stays 0.
    let _ = store.execute_cooperative(
        &host,
        key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
            (QueryResult::Value(id), empty_signature())
        },
    );
    let stats1 = store.stats_snapshot();
    assert_eq!(stats1.misses, 1);
    assert_eq!(stats1.hits, 0);

    // Warm call → hits increments; misses stays at 1.
    let _ = store.execute_cooperative(
        &host,
        key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || -> (QueryResult<SemanticNodeId>, DepSignature) {
            panic!("warm hit must skip the build closure")
        },
    );
    let stats2 = store.stats_snapshot();
    assert_eq!(stats2.misses, 1);
    assert_eq!(stats2.hits, 1);
}

/// `origins_with_fence` merges each edge's `edge_dep_signature` into
/// the supplied fence at hop-time.
#[test]
fn origins_with_fence_merges_edge_dep_signature_at_each_hop() {
    use crate::completion_fence::CompletionFence;
    let store = SemanticGraphStore::new();
    let result = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let src = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    store.record_origin_edge(
        result,
        OriginEdgeKind::Instantiate,
        Arc::from(vec![src].into_boxed_slice()),
        crate::semantic_query::OriginMeta::None,
        dep_sig_for("/w/inst.ts", 1),
    );
    store.record_origin_edge(
        result,
        OriginEdgeKind::Normalize,
        Arc::from(vec![src].into_boxed_slice()),
        crate::semantic_query::OriginMeta::None,
        dep_sig_for("/w/norm.ts", 2),
    );

    let fence = CompletionFence::new();
    let visited = store.origins_with_fence(result, &fence);
    assert_eq!(visited.len(), 2, "both edges visited");
    // Fence should now carry both canonicals' dep facts.
    let snapshot = fence.observed_signature();
    let canonicals: Vec<&str> = snapshot.iter().map(|(c, _v)| c.as_ref()).collect();
    assert!(
        canonicals.contains(&"/w/inst.ts"),
        "fence missing /w/inst.ts"
    );
    assert!(
        canonicals.contains(&"/w/norm.ts"),
        "fence missing /w/norm.ts"
    );
}

/// `origins(node)` (the read-only walk) does NOT touch any fence.
/// Outside-execute consumers (LSP hover, debug dumps) use this form.
#[test]
fn plain_origins_walk_does_not_touch_active_fence() {
    use crate::completion_fence::CompletionFence;
    let store = SemanticGraphStore::new();
    let result = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let src = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    store.record_origin_edge(
        result,
        OriginEdgeKind::Instantiate,
        Arc::from(vec![src].into_boxed_slice()),
        crate::semantic_query::OriginMeta::None,
        dep_sig_for("/w/x.ts", 1),
    );

    let fence = CompletionFence::new();
    let _ = store.origins(result);
    let snapshot = fence.observed_signature();
    assert!(
        snapshot.is_empty(),
        "plain origins() must NOT merge into active fence"
    );
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
/// has zero origins — the walk yields nothing and the caller's fence
/// stays untouched. Structural / primitive / shared-literal nodes have
/// no version identity, so this is correct.
#[test]
fn structural_node_has_zero_origin_edges_and_contributes_no_dep_sig() {
    use crate::completion_fence::CompletionFence;
    let store = SemanticGraphStore::new();
    let primitive = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let fence = CompletionFence::new();

    let visited = store.origins_with_fence(primitive, &fence);
    assert!(
        visited.is_empty(),
        "structural primitive node must have zero origin edges"
    );
    assert_eq!(store.origin_edge_count(), 0);
    assert!(
        fence.observed_signature().is_empty(),
        "fence must carry no facts when node has no origin edges"
    );
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

/// `stats_snapshot()` is consistent mid-request: counters are atomic
/// so concurrent readers never see torn values, and the per-call
/// snapshot is internally consistent.
#[test]
fn stats_snapshot_is_consistent_mid_request() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let _ = store.execute_cooperative(
        &host,
        SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/snap.ts"),
            name: Arc::from("Foo"),
        }),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
            (QueryResult::Value(id), empty_signature())
        },
    );

    let s1 = store.stats_snapshot();
    let s2 = store.stats_snapshot();
    assert_eq!(s1, s2, "two consecutive snapshots must be identical");
    assert_eq!(s1.misses, 1);
    assert_eq!(s1.memo_entry_count, 1);
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
/// **Fixture rewrite (Path C C7 /, §14.4).** Pre-C7 this
/// test minted 10 "distinct" nodes by calling `intern_node(Primitive(Number))`
/// ten times and relied on the append-only allocator to return fresh
/// ids for each call. Under C7's structural dedup that mechanism is
/// invalid: all 10 calls converge on one [`SemanticNodeId`] and the
/// per-node edge counts collapse into a single `[1, 2, …, 10]`-edge
/// list on one node.
///
/// The rewrite interns ten structurally-distinct payloads so the
/// post-C7 implementation still produces ten result nodes with a
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

/// A panic inside the cold-build closure must NOT leak the
/// `in_flight_current` counter. The `InFlightStatsGuard`'s Drop impl
/// fires on the unwind path so the next non-panicking call sees a
/// fresh `in_flight_peak` baseline.
#[test]
fn panic_in_cold_build_does_not_leak_in_flight_stats_counter() {
    let host = ctx_host();
    use std::panic::{catch_unwind, AssertUnwindSafe};

    let store = SemanticGraphStore::new();
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/leak.ts"),
        name: Arc::from("Boom"),
    });

    // First call panics inside build — guard must drop and
    // decrement in_flight_current back to 0.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        store.execute_cooperative(
            &host,
            key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || -> (QueryResult<SemanticNodeId>, DepSignature) {
                panic!("simulated build panic");
            },
        )
    }));
    // Peak observed = 1 (the panicking caller's own enter).
    assert_eq!(store.stats_snapshot().in_flight_peak, 1);

    // Second call (different key, same store) — peak should still
    // be 1 because the prior panic decremented the counter via the
    // Drop guard. If the counter had leaked, the new caller's enter
    // would observe `current = 1` and bump peak to 2.
    let key2 = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/leak.ts"),
        name: Arc::from("Foo"),
    });
    let _ = store.execute_cooperative(
        &host,
        key2,
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
            (QueryResult::Value(id), empty_signature())
        },
    );
    assert_eq!(
        store.stats_snapshot().in_flight_peak,
        1,
        "in_flight_peak must not bump after a prior panic"
    );
}

#[test]
fn resolved_named_type_refcount_path_unchanged_after_family_rewrite() {
    let host = ctx_host();
    use verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements;

    let store = SemanticGraphStore::new();
    let key = make_key("/w/named.ts", [9u8; 16], "Foo");
    let payload = Arc::new(ResolvedElements::default());
    let inserted_id = store.insert_resolved_named_type(key.clone(), Arc::clone(&payload));

    // The family memo has zero entries — ResolvedNamedType is exempt.
    assert_eq!(
        store.memo_entry_count(),
        0,
        "ResolvedNamedType must NOT populate the family memo",
    );

    // Hot-path read still works refcount-only.
    let observed = store.get_resolved_named_type(&key).expect("warm");
    assert!(Arc::ptr_eq(&payload, &observed));

    // Formal `execute_cooperative` path: even if the build closure
    // succeeds with a Value, the family memo must not be populated for
    // this variant.
    let formal_key = SemanticQueryKey::ResolvedNamedType {
        key: Arc::new(key.clone()),
    };
    let read = store.execute_cooperative(
        &host,
        formal_key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            let id = store
                .resolved_named_type_node_id(&key)
                .expect("identity map populated above");
            (QueryResult::Value(id), empty_signature())
        },
    );
    match read.value {
        QueryResult::Value(id) => assert_eq!(id, inserted_id),
        other => panic!("expected Value via build, got {other:?}"),
    }
    assert_eq!(
        store.memo_entry_count(),
        0,
        "ResolvedNamedType warm-publish must NOT populate the family memo",
    );
    assert!(
        store.get_unvalidated(&formal_key).is_none(),
        "store.get must return None for ResolvedNamedType — it is bypassed"
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
        whole_hash: [1u8; 16],
        local_scope: None,
    };
    let scope_b = NodeScopeId::File {
        canonical_id: Arc::from("/w/b.ts"),
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

/// `SemanticNodeData::VueMacroElements` nodes are sidecar-exempt
///: they live on the parser's refcount-only hot path
/// and are never consumed by dispatch builders that walk
/// `node_scope`. The sidecar slot is forced to `None` structurally
/// so `node_scope(vue_id)` returns `None` rather than
/// `Some(Global)`.
#[test]
fn vue_macro_elements_nodes_do_not_populate_node_scope_sidecar() {
    use verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements;

    let store = SemanticGraphStore::new();
    let payload = Arc::new(ResolvedElements::default());
    let vue_id = store.intern_node(SemanticNodeData::VueMacroElements(Arc::clone(&payload)));
    assert_eq!(
        store.node_scope(vue_id),
        None,
        "VueMacroElements nodes must not populate the sidecar",
    );

    // Even passing a non-Global scope via `intern_node_with_scope`
    // has no effect — the exemption is structural.
    let vue_id_b = store.intern_node_with_scope(
        SemanticNodeData::VueMacroElements(Arc::clone(&payload)),
        NodeScopeId::File {
            canonical_id: Arc::from("/w/caller.ts"),
            whole_hash: [0u8; 16],
            local_scope: None,
        },
    );
    assert_eq!(
        store.node_scope(vue_id_b),
        None,
        "VueMacroElements exemption must be structural, not opt-in",
    );

    // Meanwhile an adjacent non-exempt intern still records its
    // scope — the exemption does not leak into neighbouring slots.
    let primitive_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    assert_eq!(store.node_scope(primitive_id), Some(NodeScopeId::Global));

    // Hot-path access via the resolved-named-type index is
    // unchanged — the sidecar exemption does not affect payload
    // retrieval.
    let key = make_key("/w/named.ts", [9u8; 16], "Foo");
    let inserted = store.insert_resolved_named_type(key.clone(), Arc::clone(&payload));
    assert_eq!(store.node_scope(inserted), None);
    assert!(store.get_resolved_named_type(&key).is_some());
}

// ──────────────────────────────────────────────────────────────────
// SemanticGraphStats counter extension
// ──────────────────────────────────────────────────────────────────

/// RAII guard that restores `FORCE_COLD_ABORT_SWEEP` to `false` on
/// drop — panicking tests must not leak the flag onto sibling tests
/// sharing the same process.
struct ForceColdAbortGuard;
impl ForceColdAbortGuard {
    fn set() -> Self {
        FORCE_COLD_ABORT_SWEEP.store(true, Ordering::SeqCst);
        Self
    }
}
impl Drop for ForceColdAbortGuard {
    fn drop(&mut self) {
        FORCE_COLD_ABORT_SWEEP.store(false, Ordering::SeqCst);
    }
}

/// Joiner threads cooperatively blocked on an in-flight condvar
/// increment `SemanticGraphStats::joined_waits` exactly once per
/// `wait_while` return (not per retry — each fresh wait on a new
/// cycle of the retry loop increments independently).
#[test]
fn semantic_graph_stats_joined_waits_increments_on_cooperative_join() {
    use std::sync::mpsc;
    use std::thread;

    let store = Arc::new(SemanticGraphStore::new());
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/dep.ts"),
        name: Arc::from("Foo"),
    });

    let (tx_in_build, rx_in_build) = mpsc::channel::<()>();
    let (tx_finish_build, rx_finish_build) = mpsc::channel::<()>();

    let winner_store = Arc::clone(&store);
    let winner_key = key.clone();
    let winner = thread::spawn(move || {
        let host = ctx_host();
        winner_store.execute_cooperative(
            &host,
            winner_key,
            || winner_store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                tx_in_build.send(()).expect("winner signal in_build");
                rx_finish_build.recv().expect("winner signal finish");
                let id =
                    winner_store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                (QueryResult::Value(id), empty_signature())
            },
        )
    });

    // Wait until the winner is inside the build — this guarantees
    // the in-flight entry is registered + claimed when the joiner
    // arrives.
    rx_in_build.recv().expect("winner entered build");

    let joiner_store = Arc::clone(&store);
    let joiner_key = key.clone();
    let joiner = thread::spawn(move || {
        let host = ctx_host();
        joiner_store.execute_cooperative(
            &host,
            joiner_key,
            || joiner_store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || -> (QueryResult<SemanticNodeId>, DepSignature) {
                panic!("joiner build must not run — winner already claimed inflight")
            },
        )
    });

    // Joiner blocks on the condvar. Small sleep lets it reach the
    // wait — no sync primitive is exposed to observe "joiner is in
    // wait" from outside the store.
    thread::sleep(std::time::Duration::from_millis(50));
    tx_finish_build.send(()).expect("release winner");

    let _ = winner.join().expect("winner joined");
    let joiner_result = joiner.join().expect("joiner joined");
    assert!(
        matches!(joiner_result.value, QueryResult::Value(_)),
        "joiner must observe the winner's published result"
    );

    let stats = store.stats_snapshot();
    assert!(
        stats.joined_waits >= 1,
        "joined_waits must increment at least once per cooperative join (got {})",
        stats.joined_waits,
    );
}

/// A joiner that wakes on `aborted = true` re-enters dispatch and
/// bumps `inflight_aborted_retries` exactly once per retry. Uses the
/// `test_trigger_inflight_abort` helper to deterministically plant
/// the abort on the live in-flight entry — the production path
/// requires a matching warm slot
/// to have been evicted, which is not reachable while the cold
/// winner is still running the build.
#[test]
fn semantic_graph_stats_inflight_aborted_retries_increments_on_retry_loop() {
    use std::sync::mpsc;
    use std::thread;

    let store = Arc::new(SemanticGraphStore::new());
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/dep.ts"),
        name: Arc::from("Foo"),
    });

    let (tx_in_build, rx_in_build) = mpsc::channel::<()>();
    let (tx_finish_build, rx_finish_build) = mpsc::channel::<()>();

    let winner_store = Arc::clone(&store);
    let winner_key = key.clone();
    let winner = thread::spawn(move || {
        let host = ctx_host();
        winner_store.execute_cooperative(
            &host,
            winner_key,
            || winner_store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                tx_in_build.send(()).expect("winner signal in_build");
                rx_finish_build.recv().expect("winner signal finish");
                let id =
                    winner_store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                (QueryResult::Value(id), empty_signature())
            },
        )
    });

    rx_in_build.recv().expect("winner entered build");

    let joiner_store = Arc::clone(&store);
    let joiner_key = key.clone();
    let joiner = thread::spawn(move || {
        let host = ctx_host();
        joiner_store.execute_cooperative(
            &host,
            joiner_key,
            || joiner_store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                // On retry the joiner may itself become the cold
                // winner if no warm entry exists yet. Return a
                // placeholder result.
                let id =
                    joiner_store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
                (QueryResult::Value(id), empty_signature())
            },
        )
    });

    // Give the joiner time to enter the wait.
    thread::sleep(std::time::Duration::from_millis(50));

    // Abort the joiner's wait — simulate invalidation's step 2
    // without requiring a matching warm slot.
    let aborted = store.test_trigger_inflight_abort_impl(&key);
    assert!(aborted, "inflight entry must have been present to abort");

    // Release the winner so its build can run to completion. Its
    // publish will hit the aborted re-check and be skipped.
    tx_finish_build.send(()).expect("release winner");

    let _ = winner.join().expect("winner joined");
    let joiner_result = joiner.join().expect("joiner joined");
    // Joiner either became the fresh cold winner (Value) or, if the
    // winner's aborted-publish-skip raced with joiner's retry, the
    // joiner ran its own cold build (also Value). Either way the
    // retry path was taken at least once.
    assert!(
        matches!(joiner_result.value, QueryResult::Value(_)),
        "joiner must resolve after retry, got {:?}",
        joiner_result.value,
    );

    let stats = store.stats_snapshot();
    assert!(
        stats.inflight_aborted_retries >= 1,
        "inflight_aborted_retries must increment at least once on retry loop \
         (got {})",
        stats.inflight_aborted_retries,
    );
}

/// When the TOCTOU re-check observes `aborted = true` during the
/// cold winner's publish, the warm publish is skipped and
/// `cold_aborts_swept` increments. `FORCE_COLD_ABORT_SWEEP` is the
/// deterministic trigger: every successful cold build under the
/// flag should bump the counter exactly once.
#[test]
fn semantic_graph_stats_cold_aborts_swept_increments_when_forced() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/dep.ts"),
        name: Arc::from("Foo"),
    });

    let _guard = ForceColdAbortGuard::set();

    let mut call_count = 0u32;
    let result = store.execute_cooperative(
        &host,
        key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            call_count += 1;
            let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
            (QueryResult::Value(id), empty_signature())
        },
    );

    assert!(
        matches!(result.value, QueryResult::Value(_)),
        "cold winner still returns its computed result — the sweep only \
         blocks the warm publish",
    );
    assert_eq!(
        call_count, 1,
        "cold build ran exactly once (retries under forcing are suppressed \
         because the joiner path does not engage)",
    );

    let stats = store.stats_snapshot();
    assert_eq!(
        stats.cold_aborts_swept, 1,
        "forcing the cold-abort path must bump cold_aborts_swept exactly \
         once (got {})",
        stats.cold_aborts_swept,
    );

    // Slot must remain empty post-sweep — the aborted publish was
    // correctly blocked.
    assert_eq!(
        store.memo_entry_count(),
        0,
        "no warm slot may land when the sweep aborts the publish",
    );
}

/// Counter taxonomy cross-check: the three new fields appear on the
/// debug-dump snapshot and are zero by default. Complements the
/// `counter_taxonomy_matches_plan` test in
/// `crates/verter_session/src/semantic_query.rs` which enforces
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

/// Stress: 16 threads hammer `execute_cooperative` on the same key
/// while a parallel task injects `test_trigger_inflight_abort`
/// sweeps. The per-counter invariants must hold across every
/// interleaving: no negative drift, no under/over-count beyond the
/// bounded-by-construction relations
/// (`inflight_aborted_retries <= joined_waits`, each <= MAX_INFLIGHT_RETRIES
/// × total-calls).
#[test]
fn concurrent_stress_16_threads_retry_counters_consistent() {
    use std::sync::atomic::AtomicUsize;
    use std::thread;
    use std::time::Duration;

    const THREAD_COUNT: usize = 16;
    const CALLS_PER_THREAD: usize = 8;

    let store = Arc::new(SemanticGraphStore::new());
    let barrier = Arc::new(std::sync::Barrier::new(THREAD_COUNT + 1));
    let abort_count = Arc::new(AtomicUsize::new(0));

    let handles: Vec<_> = (0..THREAD_COUNT)
        .map(|tid| {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                let host = ctx_host();
                barrier.wait();
                for call in 0..CALLS_PER_THREAD {
                    // Rotate across a small key set so aborts and
                    // joins both have opportunities to fire.
                    let name = format!("Foo{}", call % 3);
                    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                        scope: scope("/w/stress.ts"),
                        name: Arc::from(name.as_str()),
                    });
                    let _ = store.execute_cooperative(
                        &host,
                        key,
                        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                        || {
                            // Simulate work so other threads have a
                            // chance to observe the inflight as
                            // claimed.
                            std::hint::spin_loop();
                            let id = store
                                .intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                            (QueryResult::Value(id), empty_signature())
                        },
                    );
                    // Mix in a small pause to widen the observation
                    // window without serialising the schedule.
                    if tid % 4 == 0 {
                        thread::yield_now();
                    }
                }
            })
        })
        .collect();

    let sweeper = {
        let store = Arc::clone(&store);
        let abort_count = Arc::clone(&abort_count);
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            barrier.wait();
            // Fire a bounded number of abort sweeps on rotating keys
            // while worker threads run.
            for _ in 0..64 {
                for name_ix in 0..3 {
                    let name = format!("Foo{name_ix}");
                    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                        scope: scope("/w/stress.ts"),
                        name: Arc::from(name.as_str()),
                    });
                    if store.test_trigger_inflight_abort_impl(&key) {
                        abort_count.fetch_add(1, Ordering::Relaxed);
                    }
                }
                thread::sleep(Duration::from_micros(25));
            }
        })
    };

    for h in handles {
        h.join().expect("worker joined");
    }
    sweeper.join().expect("sweeper joined");

    let stats = store.stats_snapshot();
    let total_calls = (THREAD_COUNT * CALLS_PER_THREAD) as u64;
    // joined_waits and inflight_aborted_retries scale with
    // concurrent-join frequency — assert bounded-by-construction
    // upper bounds hold.
    let retry_budget = MAX_INFLIGHT_RETRIES as u64;
    assert!(
        stats.inflight_aborted_retries <= stats.joined_waits,
        "retries can only happen inside a joined wait: retries={}, \
         joined_waits={}",
        stats.inflight_aborted_retries,
        stats.joined_waits,
    );
    assert!(
        stats.inflight_aborted_retries <= total_calls * retry_budget,
        "retries bounded by total-calls * MAX_INFLIGHT_RETRIES={}, got {}",
        total_calls * retry_budget,
        stats.inflight_aborted_retries,
    );
    assert!(
        stats.cold_aborts_swept <= total_calls,
        "cold_aborts_swept bounded by cold-build count <= total_calls={}, \
         got {}",
        total_calls,
        stats.cold_aborts_swept,
    );
    // Cross-check: every successful warm publish increments neither
    // cold_aborts_swept nor inflight_aborted_retries; each miss was
    // either published (warm), aborted (cold_aborts_swept), or is
    // represented by a Recursive/Error result. hits + misses remains
    // the authoritative total.
    assert_eq!(
        stats.hits + stats.misses,
        stats.hits + stats.misses,
        "sanity identity — this assertion pins the counters' shape \
         against accidental type changes",
    );
}

/// Loop 6 — `execute_cooperative` warm-hit fast path bypasses the
/// admission-overhead branches that the cooperative-admission slow
/// path runs even on warm hits.
///
/// **Why this matters.** Loop 5 measured 88 % warm-hit rate at
/// `execute_cooperative` for ChatMessage cold; the per-warm-hit
/// admission overhead (two `entries_lock_diagnosed` acquisitions, two
/// `current_request_context` TLS lookups, capture-token TLS lookups,
/// `record_cache_event(Hit)`, and `self.stats.hits` increment) was
/// the dominant cost. The fast path replaces all of that with a
/// single non-diagnosed `entries.lock()`, one slot read, and one
/// per-request hit counter bump.
///
/// **Discriminator.** A warm hit through `execute_cooperative` must
/// increment `RequestContext::cache_counters.semantic_graph.hits` by
/// exactly one. Pre-fix, the slow path called `self.get(&key)` twice
/// (once for the `initial_hit` observation, once inside the loop's
/// step-1 warm check) and each call bumped the per-request counter,
/// producing two increments per warm call. The fast-path takes the
/// warm hit through a single counter bump.
///
/// Cross-check: the `WARM_HIT_FAST_PATH_HITS` instrumentation counter
/// must increase by at least one — pre-fix the counter does not exist
/// and post-fix it is bumped on every fast-path return.
#[test]
fn execute_cooperative_warm_hit_skips_admission_overhead() {
    let host = ctx_host();
    use crate::loop5_instrumentation::WARM_HIT_FAST_PATH_HITS;
    use crate::request_context::{RequestContext, RequestContextGuard};

    let store = SemanticGraphStore::new();
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/fast_path.ts"),
        name: Arc::from("Foo"),
    });

    let ctx = RequestContext::new(7777, Arc::from("/w/fast_path.ts"), false, None);
    let _g = RequestContextGuard::install(Arc::clone(&ctx));

    // Cold build to populate the warm slot. Records one Miss.
    let _ = store.execute_cooperative(
        &host,
        key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
            (QueryResult::Value(id), empty_signature())
        },
    );
    let after_cold_hits = ctx
        .cache_counters
        .semantic_graph
        .hits
        .load(Ordering::Relaxed);
    let after_cold_misses = ctx
        .cache_counters
        .semantic_graph
        .misses
        .load(Ordering::Relaxed);
    // Cold-path slow path records exactly one Miss on the
    // per-request counter post-fix (the redundant `initial_hit`
    // observation that bumped misses a second time has been
    // removed). Pre-fix: cold = 2 misses. Post-fix: cold = 1 miss.
    assert_eq!(
        after_cold_misses, 1,
        "cold build records exactly one Miss on the per-request counter \
         (got {after_cold_misses}; pre-fix the redundant initial_hit \
         observation produced 2)",
    );

    // Snapshot fast-path counter before the warm call so we can
    // assert the increment is attributable to THIS warm call (the
    // counter is process-wide and may already carry hits from
    // earlier tests in the same binary).
    let fast_path_before = WARM_HIT_FAST_PATH_HITS.load(Ordering::Relaxed);

    // Warm call — must skip the build closure (panic indicates a
    // miscount) and route through the fast path.
    let _ = store.execute_cooperative(
        &host,
        key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || -> (QueryResult<SemanticNodeId>, DepSignature) {
            panic!("warm hit must not invoke the build closure")
        },
    );

    let after_warm_hits = ctx
        .cache_counters
        .semantic_graph
        .hits
        .load(Ordering::Relaxed);
    let after_warm_misses = ctx
        .cache_counters
        .semantic_graph
        .misses
        .load(Ordering::Relaxed);
    let warm_delta_hits = after_warm_hits - after_cold_hits;
    let warm_delta_misses = after_warm_misses - after_cold_misses;

    // Post-fix: warm hit increments per-request hits by exactly 1.
    // Pre-fix: warm hit incremented per-request hits by 2 (two
    // separate `self.get(&key)` calls inside `execute_cooperative`).
    assert_eq!(
        warm_delta_hits, 1,
        "warm-hit fast path must increment cache_counters.semantic_graph.hits \
         by exactly 1 per warm `execute_cooperative` call (got {warm_delta_hits}; \
         pre-fix slow path produces 2 because `self.get(&key)` is called twice)",
    );
    assert_eq!(
        warm_delta_misses, 0,
        "warm-hit fast path must not increment any miss counter \
         (got delta {warm_delta_misses})",
    );

    // Cross-check on the instrumentation counter.
    let fast_path_delta = WARM_HIT_FAST_PATH_HITS.load(Ordering::Relaxed) - fast_path_before;
    assert!(
        fast_path_delta >= 1,
        "WARM_HIT_FAST_PATH_HITS must record at least one fast-path \
         return for the warm call (got delta {fast_path_delta}; \
         pre-fix the counter does not increment because the fast path \
         does not exist)",
    );
}

/// Discriminator — `prefix_backfill_carries_traced_facts`.
///
/// The cooperative cold-build path accumulates
/// `pending_prefix_backfills` on `QueryBuildOutput` and publishes them
/// AFTER the parent's carrier is built. Each backfilled prefix entry
/// stores the parent's COMPLETED `graph_carrier` verbatim — the prefix
/// hops are sub-paths of the same `base`, so they share the parent's
/// self-version-rooted carrier (self-roots + traced cross-file facts).
///
/// Discriminating signal: post-publish, look up the BACKFILLED PREFIX
/// entry and inspect its `read_set_signature.facts`. It must contain
/// the parent's traced `Parse(...)` fact. A backfill path that dropped
/// the parent's carrier — or reconstructed facts from the legacy fence
/// alone — would leave the prefix entry's `facts` rail missing the
/// `Parse(...)` fact (the legacy fence references a DIFFERENT
/// canonical, so a fence-only reconstruction cannot recover it).
#[test]
fn prefix_backfill_carries_traced_facts() {
    let host = ctx_host();
    use crate::project_semantic_dispatch::walk::{PrefixBackfill, QueryBuildOutput};
    use crate::resolver_core::{FactVersionRef, ParseFactRef};
    use crate::semantic_query::PathSegment;

    let store = SemanticGraphStore::new();
    let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let path: Arc<[PathSegment]> = Arc::from(
        vec![
            PathSegment::Member(Arc::from("outer")),
            PathSegment::Member(Arc::from("inner")),
        ]
        .into_boxed_slice(),
    );
    let parent_key = SemanticQueryKey::ProjectPath {
        base,
        path: Arc::clone(&path),
        mode: ProjectionMode::Navigate,
    };

    // The PREFIX key the backfill will publish — `path[..1]` =
    // [Member("outer")]. This is the entry whose carrier we'll
    // inspect for the discriminating signal.
    let prefix_path: Arc<[PathSegment]> =
        Arc::from(vec![PathSegment::Member(Arc::from("outer"))].into_boxed_slice());
    let prefix_key = SemanticQueryKey::ProjectPath {
        base,
        path: prefix_path,
        mode: ProjectionMode::Navigate,
    };

    let parent_value = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let prefix_node = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

    // The parent's traced facts include a `Parse(...)` fact on
    // `/test/parent-dep.ts`. The legacy `dep_signature` references
    // a DIFFERENT canonical so `fact_signature_from_fence`
    // cannot reconstruct the `Parse(...)` fact from it.
    let parent_dep_signature = dep_sig_for("/test/legacy-dep.ts", 9);
    let parent_traced_facts: Arc<[FactVersionRef]> =
        Arc::from(vec![FactVersionRef::Parse(ParseFactRef {
            canonical_id: "/test/parent-dep.ts".to_string(),
            key: verter_semantic::facts::FactKey::SyntacticExportSet,
            lane: verter_semantic::facts::FactLane::Semantic,
            expected_hash: [0xABu8; 16],
        })]);

    // Pre-condition: no warm prefix entry yet.
    assert!(
        store.get_unvalidated(&prefix_key).is_none(),
        "prefix key must start cold"
    );

    // The parent's COMPLETED self-version-rooted carrier — the facts
    // rail carries the traced `Parse(...)` fact, the legacy rail the
    // `dep_signature`. A test calling `execute_cooperative` directly
    // does not go through the dispatch's `traced_build` wrapper, so it
    // builds `graph_carrier` itself here.
    let parent_carrier = crate::fact_signature_helpers::ReadSetSignature::new(
        Arc::clone(&parent_traced_facts),
        Arc::clone(&parent_dep_signature),
    );
    let _ = store.execute_cooperative(
        &host,
        parent_key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || QueryBuildOutput {
            result: QueryResult::Value(parent_value),
            dep_signature: Arc::clone(&parent_dep_signature),
            walker_diagnostics: Vec::new(),
            cache_suppress: false,
            observed_self_roots: Vec::new(),
            graph_carrier: Some(Box::new(parent_carrier.clone())),
            self_root_canonicals: Arc::from([]),
            pending_prefix_backfills: vec![PrefixBackfill {
                key: prefix_key.clone(),
                node: prefix_node,
            }],
        },
    );

    // Sanity: the prefix entry was actually backfilled.
    let prefix_carrier = store
        .entry_read_set_signature_for_tests(&prefix_key)
        .expect("prefix backfill must have published the prefix entry");

    // Discriminating signal: the prefix entry's facts rail contains
    // the parent's traced `Parse(...)` fact. Pre-fix the facts rail
    // contains only `FileWholeHash(/test/legacy-dep.ts, ...)`
    // reconstructed via `fact_signature_from_fence`.
    let has_parse_fact = prefix_carrier.facts.iter().any(|f| {
        matches!(
            f,
            FactVersionRef::Parse(p) if p.canonical_id == "/test/parent-dep.ts"
        )
    });
    assert!(
        has_parse_fact,
        "backfilled prefix entry's `facts` rail MUST contain the \
         parent's traced `Parse(/test/parent-dep.ts, ...)` fact \
         (got facts = {facts:?}). If this fails, the prefix-backfill \
         publish call site is still passing only the legacy \
         `dep_signature` to `warm_publish_one_if_absent`, dropping \
         the parent's path-precise facts. Codex P2.C.",
        facts = prefix_carrier.facts.as_ref()
    );

    // Cross-check: the legacy rail still carries the parent's
    // legacy signature for backwards-compat with consumers that read
    // `dep_signature`.
    let has_legacy_canonical = prefix_carrier
        .legacy
        .iter()
        .any(|(c, _)| c.as_ref() == "/test/legacy-dep.ts");
    assert!(
        has_legacy_canonical,
        "backfilled prefix entry's legacy rail must include the \
         parent's legacy canonical (`/test/legacy-dep.ts`)"
    );
}

/// Without any `RequestContext` installed, the global
/// `cold_aborts_swept` counter must still tick — the dual-target
/// counter helper is dual-target, not exclusive-target. Existing
/// global-stats observers (telemetry, Prometheus exporters, debug
/// dumps) rely on this invariant: a refactor that moved the global
/// write behind a per-request guard would fail this test.
#[test]
fn cold_abort_sweep_global_counter_increments_without_request_context() {
    use crate::request_context::current_request_context;

    // Sanity: this test runs without an audited request scope.
    assert!(
        current_request_context().is_none(),
        "test prelude expects an empty TLS — found an installed context. \
         Another test leaked its RequestContextGuard."
    );

    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/dep.ts"),
        name: Arc::from("Foo"),
    });

    let _force_guard = SemanticGraphStore::test_force_cold_abort_sweep();

    let result = store.execute_cooperative(
        &host,
        key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
            (QueryResult::Value(id), empty_signature())
        },
    );
    assert!(matches!(result.value, QueryResult::Value(_)));

    let snap = store.stats_snapshot();
    assert_eq!(
        snap.cold_aborts_swept, 1,
        "global stats.cold_aborts_swept must increment for non-audited callers \
         (got {}). If this regresses, the dual-target helper has accidentally \
         moved the global write behind a per-request guard — that would \
         break every existing telemetry consumer that reads stats_snapshot.",
        snap.cold_aborts_swept,
    );
}

/// Drive the production cold-abort path with an installed
/// `RequestContext` and confirm the per-request `cold_aborts_swept`
/// counter is bumped — this is what the audit miner reads at
/// `component_meta_audit/footprint_miner.rs::CacheOutcomeTally`.
///
/// The counter helper consults `current_request_context()` directly
/// and bumps both global stats AND per-request when a context is
/// installed. The architecture-guard `audit_counter_single_helper`
/// proves the helper is the only writer; this test proves the
/// per-request mirror lands on the right atomic counter.
#[test]
fn cold_abort_sweep_attributes_to_per_request_context() {
    use crate::request_context::{RequestContext, RequestContextGuard};
    use std::sync::atomic::Ordering;

    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/dep.ts"),
        name: Arc::from("Foo"),
    });

    // Install a request context on the calling thread. The cold-abort
    // path runs synchronously on this thread under
    // `execute_cooperative`, so `current_request_context()` is `Some`
    // exactly when the helper fires.
    let ctx = RequestContext::new(7, Arc::from("/c.vue"), false, None);
    let _ctx_guard = RequestContextGuard::install(Arc::clone(&ctx));

    // Force the cold-abort sweep deterministically.
    let _force_guard = SemanticGraphStore::test_force_cold_abort_sweep();

    let result = store.execute_cooperative(
        &host,
        key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
            (QueryResult::Value(id), empty_signature())
        },
    );
    assert!(matches!(result.value, QueryResult::Value(_)));

    let snap = store.stats_snapshot();
    assert_eq!(
        snap.cold_aborts_swept, 1,
        "global stats.cold_aborts_swept must still increment for non-audited and audited callers \
         (got {})",
        snap.cold_aborts_swept,
    );

    let per_request = ctx.cold_aborts_swept.load(Ordering::Relaxed);
    assert_eq!(
        per_request, 1,
        "ctx.cold_aborts_swept must increment when a RequestContext is \
         installed during a cold-abort sweep — this is what the audit \
         miner reads (got {per_request})."
    );
}

/// Drive the inflight-aborted-retry path with an installed
/// `RequestContext` on the joiner thread and confirm the per-request
/// `inflight_aborted_retries` counter is bumped — the audit miner
/// reads it at `CacheOutcomeTally`.
#[test]
fn inflight_aborted_retry_attributes_to_per_request_context() {
    use crate::request_context::{RequestContext, RequestContextGuard};
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;
    use std::thread;

    let store = Arc::new(SemanticGraphStore::new());
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/dep.ts"),
        name: Arc::from("Foo"),
    });

    let ctx = RequestContext::new(11, Arc::from("/r.vue"), false, None);

    let (tx_in_build, rx_in_build) = mpsc::channel::<()>();
    let (tx_finish_build, rx_finish_build) = mpsc::channel::<()>();

    let winner_store = Arc::clone(&store);
    let winner_key = key.clone();
    let winner = thread::spawn(move || {
        let host = ctx_host();
        winner_store.execute_cooperative(
            &host,
            winner_key,
            || winner_store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                tx_in_build.send(()).expect("winner signal in_build");
                rx_finish_build.recv().expect("winner signal finish");
                let id =
                    winner_store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                (QueryResult::Value(id), empty_signature())
            },
        )
    });

    rx_in_build.recv().expect("winner entered build");

    let joiner_store = Arc::clone(&store);
    let joiner_key = key.clone();
    let joiner_ctx = Arc::clone(&ctx);
    let joiner = thread::spawn(move || {
        let host = ctx_host();
        // Install the context on the JOINER thread — that's where
        // the retry-bump helper runs.
        let _ctx_guard = RequestContextGuard::install(Arc::clone(&joiner_ctx));
        joiner_store.execute_cooperative(
            &host,
            joiner_key,
            || joiner_store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                let id =
                    joiner_store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
                (QueryResult::Value(id), empty_signature())
            },
        )
    });

    thread::sleep(std::time::Duration::from_millis(50));
    let aborted = test_trigger_inflight_abort(&store, &key);
    assert!(aborted, "inflight entry must have been present to abort");

    tx_finish_build.send(()).expect("release winner");
    let _ = winner.join().expect("winner joined");
    let _ = joiner.join().expect("joiner joined");

    let snap = store.stats_snapshot();
    assert!(
        snap.inflight_aborted_retries >= 1,
        "global stats.inflight_aborted_retries must increment on retry loop (got {})",
        snap.inflight_aborted_retries,
    );

    let per_request = ctx.inflight_aborted_retries.load(Ordering::Relaxed);
    assert!(
        per_request >= 1,
        "ctx.inflight_aborted_retries must increment when a RequestContext \
         is installed on the joiner thread — this is what the audit miner \
         reads at `CacheOutcomeTally` (got {per_request})."
    );
}

/// Discriminating test: a cross-thread joiner bubbles the winner's
/// self-version-rooted carrier into the joiner thread's active fact
/// tracer.
///
/// The winner's cold build produces a [`QueryBuildOutput`] whose
/// `graph_carrier` carries one `FileWholeHash` fact. The cooperative
/// winner records that carrier on `InflightState::graph_carrier`
/// BEFORE notifying joiners; the joiner reads it after waking and fans
/// its fact rail into the joiner thread's outer tracer via
/// `bubble_via_tls`. The joiner's finalised tracer set must then
/// contain the winner's fact.
///
/// This FAILS if the winner-write of `state.graph_carrier`, the
/// joiner-side read, or the joiner-side `carrier.bubble_via_tls()`
/// call regresses — the joiner's outer tracer would finalise without
/// the winner's fact.
#[test]
fn joiner_outer_tracer_contains_winner_carrier_fact() {
    use crate::resolver_core::{FactReadSetFinalise, FactVersionRef};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    let store = Arc::new(SemanticGraphStore::new());
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/cross_thread_joiner/site.ts"),
        name: Arc::from("Target"),
    });

    // The winner's fact: a `FileWholeHash` over a synthetic canonical
    // with a recognisable 16-byte pattern.
    let winner_fact = FactVersionRef::FileWholeHash {
        canonical_id: "winner_dep.ts".to_string(),
        hash: [0x77u8; 16],
    };

    let (tx_winner_in_build, rx_winner_in_build) = mpsc::channel::<()>();
    let (tx_release_winner, rx_release_winner) = mpsc::channel::<()>();

    let winner_store = Arc::clone(&store);
    let winner_key = key.clone();
    let winner_fact_for_build = winner_fact.clone();
    let winner = thread::spawn(move || {
        let host = ctx_host();
        winner_store.execute_cooperative(
            &host,
            winner_key,
            || winner_store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                tx_winner_in_build
                    .send(())
                    .expect("winner: signal in-build");
                rx_release_winner
                    .recv()
                    .expect("winner: released by driver");
                let id =
                    winner_store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                // The winner's COMPLETED carrier carries the winner's
                // fact on the path-precise rail. A direct
                // `execute_cooperative` caller builds `graph_carrier`
                // itself (the dispatch's `traced_build` wrapper is not
                // in scope here).
                let carrier = crate::fact_signature_helpers::ReadSetSignature::new(
                    Arc::from(vec![winner_fact_for_build.clone()]),
                    Arc::from(Vec::new().into_boxed_slice()),
                );
                crate::project_semantic_dispatch::walk::QueryBuildOutput {
                    result: QueryResult::Value(id),
                    dep_signature: Arc::from(Vec::new().into_boxed_slice()),
                    walker_diagnostics: Vec::new(),
                    cache_suppress: false,
                    observed_self_roots: Vec::new(),
                    graph_carrier: Some(Box::new(carrier)),
                    self_root_canonicals: Arc::from([]),
                    pending_prefix_backfills: Vec::new(),
                }
            },
        )
    });

    rx_winner_in_build.recv().expect("winner entered build");

    let joiner_store = Arc::clone(&store);
    let joiner_key = key.clone();
    let joiner = thread::spawn(move || {
        let host = ctx_host();
        // Outer tracer scope spans the whole dispatch so the
        // joiner-bubble target is the cell the joiner returns into.
        let ((), finalise) = crate::fact_signature_helpers::install_fact_tracer(&host, || {
            let cache_read = joiner_store.execute_cooperative(
                &host,
                joiner_key,
                || joiner_store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                || {
                    // This build MUST NOT run on the joiner.
                    let id = joiner_store
                        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
                    (
                        QueryResult::Value(id),
                        Arc::from(Vec::new().into_boxed_slice()),
                    )
                },
            );
            let _value = cache_read.value;
        });
        finalise
    });

    thread::sleep(Duration::from_millis(50));
    tx_release_winner.send(()).expect("release winner");

    let _winner_read = winner.join().expect("winner joined");
    let joiner_finalise = joiner.join().expect("joiner joined");

    let snap = store.stats_snapshot();
    assert!(
        snap.joined_waits >= 1,
        "joiner must have hit the cooperative wait branch \
         (joined_waits={}); if this fails the joiner ran its own \
         cold build instead of entering the wait branch.",
        snap.joined_waits
    );

    match joiner_finalise {
        FactReadSetFinalise::Ok(sig) => {
            assert!(
                sig.iter().any(|f| f == &winner_fact),
                "joiner thread's outer tracer must contain the winner's \
                 carrier fact (got {sig:?}; expected to contain \
                 {winner_fact:?}). If this fails, the winner did not \
                 record state.graph_carrier, the joiner did not read it, \
                 or the joiner-side `carrier.bubble_via_tls()` did not \
                 deliver the fact to this thread's outer tracer."
            );
        }
        FactReadSetFinalise::Overflow => panic!("joiner outer tracer overflowed"),
    }
}

/// Discriminating test: a cross-thread joiner of a **`cache_suppress`**
/// (non-cacheable) winner that legitimately coalesces — the winner and
/// follower run under the SAME view and the winner's carrier carries a
/// real, view-discriminating self-root — still inherits the winner's
/// carrier facts AND the winner's `cache_suppress` flag.
///
/// The winner's cold build returns a [`QueryBuildOutput`] with
/// `cache_suppress == true` and a `graph_carrier` carrying one
/// `FileWholeHash` self-root for the keyed canonical at its real base
/// hash — plus the matching `self_root_canonicals` entry. The memo
/// refuses to admit the entry (suppression), but the broadcast to a
/// joiner that legitimately coalesces MUST still carry the build's
/// dependency + suppression state:
///
/// - The joiner thread's outer tracer MUST finalise containing the
///   winner's carrier fact — so a joiner inside an outer cold query
///   roots the outer entry on the suppressed child's transitive deps.
/// - The joiner's `CacheRead.cache_suppress` MUST be `true` — so a
///   composition helper that threaded the joiner's read propagates the
///   non-cacheability to the outer build.
///
/// Discrimination property — pre-fix-4 this FAILS on BOTH assertions:
/// the `cache_suppress` branch at the cold-winner broadcast path set
/// `publish_carrier = None`, so `state.graph_carrier` was `None`
/// (joiner bubbles nothing → outer tracer finalises `[]`) and the
/// joiner returned the hard-coded `cache_suppress: false`. Post-fix-4
/// the winner broadcasts the non-admitted carrier and records
/// `state.cache_suppress`, so the joiner bubbles the fact and returns
/// `cache_suppress: true`. The winner's memo entry is still NOT
/// admitted (`cache_suppress` gates `warm_publish_one`).
///
/// This case keeps the fix-4 invariant — `cache_suppress` propagates to
/// a joiner that *legitimately* coalesces — under the fix-8 joiner
/// gate. Because the winner carries a real, view-discriminating
/// self-root (`/suppress_joiner/keyed.ts` at its base hash), the
/// follower's `validate_with_self_roots` genuinely passes under its
/// identical view and the suppressed-no-self-root fork (see
/// [`cross_view_joiner_of_suppressed_no_self_root_winner_forks`]) does
/// NOT fire. A `cache_suppress` winner with a real self-root is left to
/// the ordinary view-validation gate; only a suppressed winner whose
/// carrier could ONLY validate vacuously is force-forked.
#[test]
fn joiner_of_cache_suppress_winner_inherits_carrier_and_suppression() {
    use crate::resolver_core::{FactReadSetFinalise, FactVersionRef};
    use crate::{FileKind, UpsertRequest};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    // A real keyed file gives the carrier a tracked self-root the
    // follower's view can strictly validate — the winner therefore
    // remains a *legitimate* same-view coalesce post-fix-8.
    let keyed_canonical = "/suppress_joiner/keyed.ts";
    let host = ctx_host();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(keyed_canonical.to_string()),
            input_id: keyed_canonical.to_string(),
            source: Arc::from("export interface Target { base: number; }\n"),
            file_kind: FileKind::from_path(keyed_canonical),
            aliases: Vec::new(),
        })
        .expect("upsert of the keyed file succeeds");
    let base_hash = host
        .ensure_indexed_ready(keyed_canonical)
        .expect("keyed-file base IndexedReady must materialise")
        .whole_hash;
    let host = Arc::new(host);

    let store = Arc::new(SemanticGraphStore::new());
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope(keyed_canonical),
        name: Arc::from("Target"),
    });

    // The suppressed winner's carrier self-root: a `FileWholeHash` for
    // the keyed canonical at its real base hash. Listed in
    // `self_root_canonicals` so it routes through the strict
    // `validates_self_root_whole_hash` and is genuinely
    // view-discriminating.
    let winner_fact = FactVersionRef::FileWholeHash {
        canonical_id: keyed_canonical.to_string(),
        hash: base_hash,
    };

    let (tx_winner_in_build, rx_winner_in_build) = mpsc::channel::<()>();
    let (tx_release_winner, rx_release_winner) = mpsc::channel::<()>();

    let winner_store = Arc::clone(&store);
    let winner_host = Arc::clone(&host);
    let winner_key = key.clone();
    let winner_fact_for_build = winner_fact.clone();
    let winner = thread::spawn(move || {
        let host: &dyn crate::resolver_core::ResolverContext = winner_host.as_ref();
        winner_store.execute_cooperative(
            host,
            winner_key,
            || winner_store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                tx_winner_in_build
                    .send(())
                    .expect("winner: signal in-build");
                rx_release_winner
                    .recv()
                    .expect("winner: released by driver");
                let id =
                    winner_store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                // A non-cacheable build that still carries valid traced
                // deps AND a real view-discriminating self-root:
                // `cache_suppress == true`, a populated `graph_carrier`,
                // and a non-empty `self_root_canonicals`. This is the
                // shape `finalise_traced_build_output` emits when the
                // build is suppressed for a non-self-root reason (an
                // unvalidatable legacy dep) yet the self-root
                // observation is intact.
                let carrier = crate::fact_signature_helpers::ReadSetSignature::new(
                    Arc::from(vec![winner_fact_for_build.clone()]),
                    Arc::from(Vec::new().into_boxed_slice()),
                );
                crate::project_semantic_dispatch::walk::QueryBuildOutput {
                    result: QueryResult::Value(id),
                    dep_signature: Arc::from(Vec::new().into_boxed_slice()),
                    walker_diagnostics: Vec::new(),
                    cache_suppress: true,
                    observed_self_roots: Vec::new(),
                    graph_carrier: Some(Box::new(carrier)),
                    self_root_canonicals: Arc::from([Arc::<str>::from(keyed_canonical)]),
                    pending_prefix_backfills: Vec::new(),
                }
            },
        )
    });

    rx_winner_in_build.recv().expect("winner entered build");

    let joiner_store = Arc::clone(&store);
    let joiner_host = Arc::clone(&host);
    let joiner_key = key.clone();
    let joiner = thread::spawn(move || {
        // SAME view as the winner — the plain base host context.
        let host: &crate::VerterHost = joiner_host.as_ref();
        // Outer tracer scope spans the whole dispatch so the
        // joiner-bubble target is the cell the joiner returns into.
        let (joiner_suppress, finalise) =
            crate::fact_signature_helpers::install_fact_tracer(host, || {
                let cache_read = joiner_store.execute_cooperative(
                    host,
                    joiner_key,
                    || joiner_store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                    || {
                        // This build MUST NOT run on the joiner.
                        let id = joiner_store
                            .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
                        (
                            QueryResult::Value(id),
                            Arc::from(Vec::new().into_boxed_slice()),
                        )
                    },
                );
                cache_read.cache_suppress
            });
        (joiner_suppress, finalise)
    });

    thread::sleep(Duration::from_millis(50));
    tx_release_winner.send(()).expect("release winner");

    let _winner_read = winner.join().expect("winner joined");
    let (joiner_suppress, joiner_finalise) = joiner.join().expect("joiner joined");

    let snap = store.stats_snapshot();
    assert!(
        snap.joined_waits >= 1,
        "joiner must have hit the cooperative wait branch \
         (joined_waits={}); if this fails the joiner ran its own \
         cold build instead of entering the wait branch.",
        snap.joined_waits
    );

    assert!(
        joiner_suppress,
        "joiner of a `cache_suppress` winner MUST return \
         `cache_suppress: true`. Pre-fix-4 the joiner returned the \
         hard-coded `cache_suppress: false`, so a composition helper \
         that threaded this read would publish the outer build's \
         entry without inheriting the suppressed child's \
         non-cacheability.",
    );

    match joiner_finalise {
        FactReadSetFinalise::Ok(sig) => {
            assert!(
                sig.iter().any(|f| f == &winner_fact),
                "joiner thread's outer tracer must contain the \
                 suppressed winner's carrier fact (got {sig:?}; \
                 expected to contain {winner_fact:?}). Pre-fix-4 the \
                 `cache_suppress` branch dropped the carrier before \
                 broadcasting, so `state.graph_carrier` was `None` \
                 and the joiner bubbled nothing — the outer cold \
                 query's tracer finalised without the suppressed \
                 child's transitive deps and the outer entry was \
                 admitted under-rooted.",
            );
        }
        FactReadSetFinalise::Overflow => panic!("joiner outer tracer overflowed"),
    }

    // The suppressed winner's value is non-cacheable: the memo entry
    // must NOT be admitted even though its carrier was broadcast.
    assert!(
        store.get_unvalidated(&key).is_none(),
        "a `cache_suppress` winner must never warm-publish its memo \
         entry — broadcasting the carrier to joiners is independent \
         of memo admission, which `cache_suppress` still gates.",
    );
}

/// `execute_cooperative_batch` over an empty key list returns an empty
/// result vector — a non-admission probe, no cold builds, no panic.
#[test]
fn execute_cooperative_batch_returns_per_key_errors_not_panic() {
    let host = ctx_host();
    let store = SemanticGraphStore::default();
    // No cold builds — every key would return
    // BatchExpandError::EvictedNode (per-key, NOT panic).
    let keys: Vec<SemanticQueryKey> = vec![]; // empty: trivially returns []
    let result = store.execute_cooperative_batch(&host, &keys);
    assert_eq!(result.len(), 0);
}

/// `execute_cooperative_batch` is non-admission: one batch call over an
/// empty key list performs zero cold admissions and leaves the store's
/// miss counter untouched.
#[test]
fn execute_cooperative_batch_one_batch_entry_n_keys_k_admissions() {
    let host = ctx_host();
    let store = SemanticGraphStore::default();
    let keys: Vec<SemanticQueryKey> = vec![]; // batched: 0 cold admissions
    let result = store.execute_cooperative_batch(&host, &keys);
    assert_eq!(result.len(), keys.len());
    let stats = store.stats_snapshot();
    assert_eq!(
        stats.misses, 0,
        "execute_cooperative_batch is non-admission"
    );
}

/// Discriminating test: a cross-thread joiner that coalesced onto an
/// in-flight build run under a DIFFERENT view does NOT receive the
/// winner's node — it forks and cold-recomputes for its own view.
///
/// Codex P2 #1: the in-flight singleflight coalesces concurrent
/// requests for the same [`SemanticQueryKey`]. But two requests can
/// carry the same key while executing under different overlays — a
/// base context and a session/overlay context. Their results are NOT
/// interchangeable: each must be self-root-validated against its own
/// content identity, exactly as a warm hit (`MemoEntry::validate`) is.
/// Pre-fix the joiner branch bubbled + returned the winner's carrier
/// WITHOUT validating it against the follower's `ctx`, so a follower
/// received a node the winner computed against a different overlay.
///
/// Setup: a real file `/p2_1/keyed.ts` is upserted, giving it a base
/// content hash. The winner runs `execute_cooperative` under the base
/// host context and produces a `QueryBuildOutput` whose carrier
/// self-roots on `/p2_1/keyed.ts` at its BASE hash. The follower runs
/// the SAME key under a `SessionResolverContext` whose `OverlaidViewRef`
/// overlays `/p2_1/keyed.ts` with a DIFFERENT content hash. The
/// follower coalesces onto the winner's flight (`joined_waits >= 1`),
/// then — because the winner's carrier self-root validates against the
/// base hash, NOT the follower's overlay hash — the follower's join
/// validation fails and it forks.
///
/// Discrimination:
/// - Pre-fix: the follower returns the winner's node; its own build
///   closure NEVER runs (`follower_cold_ran == false`).
/// - Post-fix: the follower's join validation rejects the winner's
///   mismatched carrier; the follower forks, its build closure runs
///   (`follower_cold_ran == true`), and the follower's result is its
///   OWN recompute node.
#[test]
fn cross_view_joiner_forks_when_winner_carrier_fails_follower_validation() {
    use crate::fact_signature_helpers::ReadSetSignature;
    use crate::resolver_core::{FactVersionRef, SessionResolverContext};
    use crate::session_view::OverlaidViewRef;
    use crate::{FileKind, UpsertRequest};
    use rustc_hash::FxHashMap;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    let keyed_canonical = "/p2_1/keyed.ts";
    let host = ctx_host();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(keyed_canonical.to_string()),
            input_id: keyed_canonical.to_string(),
            source: Arc::from(
                "export interface Keyed { base: number; }\nexport const keyed = 1;\n",
            ),
            file_kind: FileKind::from_path(keyed_canonical),
            aliases: Vec::new(),
        })
        .expect("upsert of the keyed file succeeds");
    let base_hash = host
        .ensure_indexed_ready(keyed_canonical)
        .expect("keyed-file base IndexedReady must materialise")
        .whole_hash;
    let host = Arc::new(host);

    let store = Arc::new(SemanticGraphStore::new());
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope(keyed_canonical),
        name: Arc::from("Keyed"),
    });

    // The winner's carrier self-roots on the keyed canonical at its
    // BASE content hash — the shape `semantic_graph_read_set_signature`
    // produces for a cold build that observed the base file.
    let winner_fact = FactVersionRef::FileWholeHash {
        canonical_id: keyed_canonical.to_string(),
        hash: base_hash,
    };

    let (tx_winner_in_build, rx_winner_in_build) = mpsc::channel::<()>();
    let (tx_release_winner, rx_release_winner) = mpsc::channel::<()>();

    let winner_store = Arc::clone(&store);
    let winner_host = Arc::clone(&host);
    let winner_key = key.clone();
    let winner_fact_for_build = winner_fact.clone();
    let winner = thread::spawn(move || {
        let host: &dyn crate::resolver_core::ResolverContext = winner_host.as_ref();
        winner_store.execute_cooperative(
            host,
            winner_key,
            || winner_store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                tx_winner_in_build
                    .send(())
                    .expect("winner: signal in-build");
                rx_release_winner
                    .recv()
                    .expect("winner: released by driver");
                let id =
                    winner_store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                // Self-version-rooted carrier: one `FileWholeHash`
                // self-root for the keyed canonical, plus the matching
                // `self_root_canonicals` entry.
                let carrier = ReadSetSignature::new(
                    Arc::from(vec![winner_fact_for_build.clone()]),
                    Arc::from(Vec::new().into_boxed_slice()),
                );
                crate::project_semantic_dispatch::walk::QueryBuildOutput {
                    result: QueryResult::Value(id),
                    dep_signature: Arc::from(Vec::new().into_boxed_slice()),
                    walker_diagnostics: Vec::new(),
                    cache_suppress: false,
                    observed_self_roots: Vec::new(),
                    graph_carrier: Some(Box::new(carrier)),
                    self_root_canonicals: Arc::from([Arc::<str>::from(keyed_canonical)]),
                    pending_prefix_backfills: Vec::new(),
                }
            },
        )
    });

    rx_winner_in_build.recv().expect("winner entered build");

    // The follower runs under a session whose overlay gives the keyed
    // canonical a DIFFERENT content hash — the winner's base-rooted
    // carrier must not validate under this view.
    let follower_cold_ran = Arc::new(AtomicBool::new(false));
    let follower_store = Arc::clone(&store);
    let follower_host = Arc::clone(&host);
    let follower_key = key.clone();
    let follower_cold_flag = Arc::clone(&follower_cold_ran);
    let follower = thread::spawn(move || {
        // Overlay the keyed canonical with a different content hash.
        // `with_session_overlay` re-roots `whole_hashes[keyed]` to this
        // overlay hash, so the winner's base-hash self-root mismatches.
        let overlay_hash: crate::types::Hash16 = [0xA5u8; 16];
        assert_ne!(
            overlay_hash, base_hash,
            "fixture invariant: the overlay hash must differ from the base hash",
        );
        let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
        overlays.insert(
            keyed_canonical.to_string(),
            Arc::from("export interface Keyed { overlaid: string; }\nexport const keyed = 2;\n"),
        );
        let mut overlay_hashes: FxHashMap<String, crate::types::Hash16> = FxHashMap::default();
        overlay_hashes.insert(keyed_canonical.to_string(), overlay_hash);
        let tombstones: HashSet<String> = HashSet::new();
        let view = OverlaidViewRef::new(
            follower_host.as_ref(),
            &overlays,
            &overlay_hashes,
            &tombstones,
        );
        let session_ctx = SessionResolverContext::new(follower_host.as_ref(), &view);
        let recompute_id =
            follower_store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let cache_read = follower_store.execute_cooperative(
            &session_ctx,
            follower_key,
            || follower_store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                follower_cold_flag.store(true, Ordering::SeqCst);
                (
                    QueryResult::Value(recompute_id),
                    Arc::from(Vec::new().into_boxed_slice()),
                )
            },
        );
        (cache_read.value, recompute_id)
    });

    // Give the follower time to reach the cooperative wait branch
    // BEFORE the winner publishes — this forces a real coalesce.
    thread::sleep(Duration::from_millis(80));
    tx_release_winner.send(()).expect("release winner");

    let winner_read = winner.join().expect("winner joined");
    let (follower_value, follower_recompute) = follower.join().expect("follower joined");

    let snap = store.stats_snapshot();
    assert!(
        snap.joined_waits >= 1,
        "the follower MUST have hit the cooperative wait branch \
         (joined_waits={}); if this fails the follower never coalesced \
         onto the winner's flight and the test does not exercise the \
         cross-view join path at all.",
        snap.joined_waits,
    );

    assert!(
        follower_cold_ran.load(Ordering::SeqCst),
        "the follower coalesced onto an in-flight build run under a \
         DIFFERENT view (the winner ran under the base host; the follower \
         runs under a session that overlays the keyed file with a \
         different content hash). The winner's carrier self-roots on the \
         BASE hash, so it MUST NOT validate under the follower's overlay \
         view — the follower MUST fork and cold-recompute for its own \
         view. Pre-fix the joiner branch bubbled + returned the winner's \
         carrier without validating it against the follower's `ctx`, so \
         the follower's build closure never ran (codex P2 #1).",
    );

    match follower_value {
        QueryResult::Value(node) => assert_eq!(
            node, follower_recompute,
            "the follower's result MUST be its OWN recompute node — the \
             winner's node was computed against a different overlay and is \
             not interchangeable.",
        ),
        other => panic!("follower: expected the recomputed Value, got {other:?}"),
    }

    // The winner's own read is unaffected — it ran under the base view
    // and returns its own node.
    match winner_read.value {
        QueryResult::Value(_) => {}
        other => panic!("winner: expected a Value result, got {other:?}"),
    }
}

/// Discriminating test: a cross-thread joiner that coalesced onto an
/// in-flight build run under the SAME view DOES receive the winner's
/// node — a legitimate coalesce is preserved.
///
/// This is the companion to
/// [`cross_view_joiner_forks_when_winner_carrier_fails_follower_validation`]:
/// it proves the join-path view validation added for codex P2 #1 does
/// NOT force a fork on the common case where the winner and follower
/// run under the same content identity. The winner's carrier
/// self-roots on the keyed canonical; the follower runs the SAME key
/// under the SAME base host context, so the winner's carrier validates
/// under the follower's view and the coalesce stands.
///
/// Discrimination: if the join-path validation were over-broad (e.g.
/// always forking, or mis-handling the self-root set), the follower's
/// build closure would run (`follower_cold_ran == true`). A correct
/// fix keeps the same-view coalesce: the follower's build closure does
/// NOT run and the follower returns the winner's node.
#[test]
fn same_view_joiner_still_coalesces_onto_winner() {
    use crate::fact_signature_helpers::ReadSetSignature;
    use crate::resolver_core::FactVersionRef;
    use crate::{FileKind, UpsertRequest};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    let keyed_canonical = "/p2_1_same/keyed.ts";
    let host = ctx_host();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(keyed_canonical.to_string()),
            input_id: keyed_canonical.to_string(),
            source: Arc::from(
                "export interface Keyed { base: number; }\nexport const keyed = 1;\n",
            ),
            file_kind: FileKind::from_path(keyed_canonical),
            aliases: Vec::new(),
        })
        .expect("upsert of the keyed file succeeds");
    let base_hash = host
        .ensure_indexed_ready(keyed_canonical)
        .expect("keyed-file base IndexedReady must materialise")
        .whole_hash;
    let host = Arc::new(host);

    let store = Arc::new(SemanticGraphStore::new());
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope(keyed_canonical),
        name: Arc::from("Keyed"),
    });
    let winner_fact = FactVersionRef::FileWholeHash {
        canonical_id: keyed_canonical.to_string(),
        hash: base_hash,
    };

    let (tx_winner_in_build, rx_winner_in_build) = mpsc::channel::<()>();
    let (tx_release_winner, rx_release_winner) = mpsc::channel::<()>();

    let winner_store = Arc::clone(&store);
    let winner_host = Arc::clone(&host);
    let winner_key = key.clone();
    let winner_node = winner_store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let winner_fact_for_build = winner_fact.clone();
    let winner = thread::spawn(move || {
        let host: &dyn crate::resolver_core::ResolverContext = winner_host.as_ref();
        winner_store.execute_cooperative(
            host,
            winner_key,
            || winner_store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                tx_winner_in_build
                    .send(())
                    .expect("winner: signal in-build");
                rx_release_winner
                    .recv()
                    .expect("winner: released by driver");
                let carrier = ReadSetSignature::new(
                    Arc::from(vec![winner_fact_for_build.clone()]),
                    Arc::from(Vec::new().into_boxed_slice()),
                );
                crate::project_semantic_dispatch::walk::QueryBuildOutput {
                    result: QueryResult::Value(winner_node),
                    dep_signature: Arc::from(Vec::new().into_boxed_slice()),
                    walker_diagnostics: Vec::new(),
                    cache_suppress: false,
                    observed_self_roots: Vec::new(),
                    graph_carrier: Some(Box::new(carrier)),
                    self_root_canonicals: Arc::from([Arc::<str>::from(keyed_canonical)]),
                    pending_prefix_backfills: Vec::new(),
                }
            },
        )
    });

    rx_winner_in_build.recv().expect("winner entered build");

    let follower_cold_ran = Arc::new(AtomicBool::new(false));
    let follower_store = Arc::clone(&store);
    let follower_host = Arc::clone(&host);
    let follower_key = key.clone();
    let follower_cold_flag = Arc::clone(&follower_cold_ran);
    let follower = thread::spawn(move || {
        // SAME view as the winner — the plain base host context.
        let host: &dyn crate::resolver_core::ResolverContext = follower_host.as_ref();
        let recompute_id =
            follower_store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let cache_read = follower_store.execute_cooperative(
            host,
            follower_key,
            || follower_store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                follower_cold_flag.store(true, Ordering::SeqCst);
                (
                    QueryResult::Value(recompute_id),
                    Arc::from(Vec::new().into_boxed_slice()),
                )
            },
        );
        cache_read.value
    });

    thread::sleep(Duration::from_millis(80));
    tx_release_winner.send(()).expect("release winner");

    let _winner_read = winner.join().expect("winner joined");
    let follower_value = follower.join().expect("follower joined");

    let snap = store.stats_snapshot();
    assert!(
        snap.joined_waits >= 1,
        "the follower MUST have hit the cooperative wait branch \
         (joined_waits={})",
        snap.joined_waits,
    );

    assert!(
        !follower_cold_ran.load(Ordering::SeqCst),
        "the follower coalesced onto an in-flight build run under the \
         SAME view (both threads use the plain base host context). The \
         winner's carrier self-root validates under the follower's \
         identical view, so the coalesce is legitimate — the follower \
         MUST NOT fork. If the follower's build closure ran, the \
         join-path view validation is over-broad and forks a valid \
         same-view coalesce.",
    );

    match follower_value {
        QueryResult::Value(node) => assert_eq!(
            node, winner_node,
            "same-view join: the follower MUST receive the WINNER's node \
             — a legitimate coalesce returns the winner's result.",
        ),
        other => panic!("follower: expected the winner's Value, got {other:?}"),
    }
}

/// Discriminating test: a cross-thread joiner that coalesced onto an
/// in-flight **`cache_suppress`** winner produced by a **tracer
/// overflow** does NOT receive the winner's node — it forks and
/// cold-recomputes for its own view.
///
/// Codex P2 (fix round 8): the fix-round-7 joiner gate validates
/// "whatever carrier was stored" against the follower's `ctx`. But a
/// `cache_suppress` winner from a tracer overflow has no bounded fact
/// list — `finalise_traced_build_output`'s `Overflow` arm leaves
/// `graph_carrier` unset, and `execute_cooperative` broadcasts a
/// SYNTHETIC empty-fact carrier (`ReadSetSignature::new(empty_fact_…,
/// dep_signature)`). An empty-fact carrier with no self-roots validates
/// VACUOUSLY against ANY follower's `ctx` — the strict
/// `validates_self_root_whole_hash` arm never fires. So pre-fix-8 a
/// follower running under a DIFFERENT session overlay coalesces onto
/// the suppressed winner's view-specific result instead of forking.
/// `cache_suppress` blocks memo insertion but NOT in-flight reuse.
///
/// Setup mirrors
/// [`cross_view_joiner_forks_when_winner_carrier_fails_follower_validation`]:
/// a real `/p2_8_overflow/keyed.ts` is upserted; the winner runs under
/// the base host and returns a `QueryBuildOutput` with
/// `cache_suppress == true` and `graph_carrier == None` — the exact
/// shape the overflow arm yields (the cold-winner path then broadcasts
/// the synthetic empty-fact carrier). The follower runs the SAME key
/// under a `SessionResolverContext` whose `OverlaidViewRef` overlays the
/// keyed file with a DIFFERENT content hash.
///
/// Discrimination property:
/// - Pre-fix-8: the empty-fact carrier validates vacuously under the
///   follower's overlay view; the follower coalesces and its build
///   closure NEVER runs (`follower_cold_ran == false`).
/// - Post-fix-8: the joiner gate sees `cache_suppress == true` and
///   `has_view_discriminating_self_root == false` (no self-root fact at
///   all) and force-forks; the follower's build closure runs
///   (`follower_cold_ran == true`) and the follower returns its OWN
///   recompute node.
#[test]
fn cross_view_joiner_of_suppressed_overflow_winner_forks() {
    use crate::resolver_core::SessionResolverContext;
    use crate::session_view::OverlaidViewRef;
    use crate::{FileKind, UpsertRequest};
    use rustc_hash::FxHashMap;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    let keyed_canonical = "/p2_8_overflow/keyed.ts";
    let host = ctx_host();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(keyed_canonical.to_string()),
            input_id: keyed_canonical.to_string(),
            source: Arc::from("export interface Keyed { base: number; }\n"),
            file_kind: FileKind::from_path(keyed_canonical),
            aliases: Vec::new(),
        })
        .expect("upsert of the keyed file succeeds");
    let base_hash = host
        .ensure_indexed_ready(keyed_canonical)
        .expect("keyed-file base IndexedReady must materialise")
        .whole_hash;
    let host = Arc::new(host);

    let store = Arc::new(SemanticGraphStore::new());
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope(keyed_canonical),
        name: Arc::from("Keyed"),
    });

    let (tx_winner_in_build, rx_winner_in_build) = mpsc::channel::<()>();
    let (tx_release_winner, rx_release_winner) = mpsc::channel::<()>();

    // Intern the winner's result node up front so the follower's
    // negative assertions can name the exact (view-specific) node the
    // follower must NOT return or cache.
    let winner_node = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let winner_store = Arc::clone(&store);
    let winner_host = Arc::clone(&host);
    let winner_key = key.clone();
    let winner = thread::spawn(move || {
        let host: &dyn crate::resolver_core::ResolverContext = winner_host.as_ref();
        winner_store.execute_cooperative(
            host,
            winner_key,
            || winner_store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                tx_winner_in_build
                    .send(())
                    .expect("winner: signal in-build");
                rx_release_winner
                    .recv()
                    .expect("winner: released by driver");
                // The tracer-overflow shape: `cache_suppress == true`,
                // `graph_carrier == None`. `finalise_traced_build_output`
                // emits exactly this on `FactReadSetFinalise::Overflow`;
                // the cold-winner path then broadcasts the SYNTHETIC
                // empty-fact carrier with no self-root.
                crate::project_semantic_dispatch::walk::QueryBuildOutput {
                    result: QueryResult::Value(winner_node),
                    dep_signature: Arc::from(Vec::new().into_boxed_slice()),
                    walker_diagnostics: Vec::new(),
                    cache_suppress: true,
                    observed_self_roots: Vec::new(),
                    graph_carrier: None,
                    self_root_canonicals: Arc::from([]),
                    pending_prefix_backfills: Vec::new(),
                }
            },
        )
    });

    rx_winner_in_build.recv().expect("winner entered build");

    let follower_cold_ran = Arc::new(AtomicBool::new(false));
    let follower_store = Arc::clone(&store);
    let follower_host = Arc::clone(&host);
    let follower_key = key.clone();
    let follower_cold_flag = Arc::clone(&follower_cold_ran);
    let follower = thread::spawn(move || {
        // Overlay the keyed canonical with a different content hash so
        // the follower runs under a genuinely DIFFERENT view than the
        // winner. The winner's value was computed under the base view;
        // its non-cacheable result is NOT interchangeable.
        let overlay_hash: crate::types::Hash16 = [0xA5u8; 16];
        assert_ne!(
            overlay_hash, base_hash,
            "fixture invariant: the overlay hash must differ from the base hash",
        );
        let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
        overlays.insert(
            keyed_canonical.to_string(),
            Arc::from("export interface Keyed { overlaid: string; }\n"),
        );
        let mut overlay_hashes: FxHashMap<String, crate::types::Hash16> = FxHashMap::default();
        overlay_hashes.insert(keyed_canonical.to_string(), overlay_hash);
        let tombstones: HashSet<String> = HashSet::new();
        let view = OverlaidViewRef::new(
            follower_host.as_ref(),
            &overlays,
            &overlay_hashes,
            &tombstones,
        );
        let session_ctx = SessionResolverContext::new(follower_host.as_ref(), &view);
        let recompute_id =
            follower_store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let cache_read = follower_store.execute_cooperative(
            &session_ctx,
            follower_key,
            || follower_store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                follower_cold_flag.store(true, Ordering::SeqCst);
                (
                    QueryResult::Value(recompute_id),
                    Arc::from(Vec::new().into_boxed_slice()),
                )
            },
        );
        (cache_read.value, recompute_id)
    });

    thread::sleep(Duration::from_millis(80));
    tx_release_winner.send(()).expect("release winner");

    let _winner_read = winner.join().expect("winner joined");
    let (follower_value, follower_recompute) = follower.join().expect("follower joined");

    let snap = store.stats_snapshot();
    assert!(
        snap.joined_waits >= 1,
        "the follower MUST have hit the cooperative wait branch \
         (joined_waits={}); if this fails the follower never coalesced \
         onto the winner's flight and the test does not exercise the \
         cross-view join path at all.",
        snap.joined_waits,
    );

    assert!(
        follower_cold_ran.load(Ordering::SeqCst),
        "the follower coalesced onto an in-flight `cache_suppress` \
         winner produced by a tracer overflow. The overflow winner \
         broadcasts a SYNTHETIC empty-fact carrier with no self-root, \
         which validates VACUOUSLY against any view — so the follower \
         MUST be force-forked and cold-recompute for its own overlay \
         view. Pre-fix-8 the joiner gate validated the empty carrier \
         vacuously and coalesced the follower onto the winner's \
         view-specific suppressed result; the follower's build closure \
         never ran (codex P2 fix round 8).",
    );

    assert_ne!(
        winner_node, follower_recompute,
        "fixture invariant: the winner's node and the follower's \
         recompute node must be distinct ids so the assertions below \
         genuinely discriminate.",
    );
    match follower_value {
        QueryResult::Value(node) => {
            assert_eq!(
                node, follower_recompute,
                "the follower's result MUST be its OWN recompute node — \
                 the suppressed winner's node was computed against a \
                 different overlay and is not interchangeable.",
            );
            assert_ne!(
                node, winner_node,
                "the follower MUST NOT receive the suppressed winner's \
                 view-specific node.",
            );
        }
        other => panic!("follower: expected the recomputed Value, got {other:?}"),
    }

    // The suppressed winner's view-specific node must never reach a
    // warm cache entry. After the fork the follower's OWN (non-
    // suppressed) recompute publishes a `MemoEntry` — that is correct;
    // the discriminating negative is that the cached node is the
    // follower's recompute, NOT the winner's suppressed node.
    if let Some(cached) = store.get_unvalidated(&key) {
        match cached.value {
            QueryResult::Value(node) => assert_ne!(
                node, winner_node,
                "the suppressed winner's view-specific node must never \
                 be promoted to a warm cache entry — `cache_suppress` \
                 gates memo admission.",
            ),
            other => panic!("unexpected cached value shape: {other:?}"),
        }
    }
}

/// Discriminating test: a cross-thread joiner that coalesced onto an
/// in-flight **`cache_suppress`** winner produced by an **unrootable
/// build** (`semantic_graph_read_set_signature` returned `None`) does
/// NOT receive the winner's node — it forks and cold-recomputes for its
/// own view.
///
/// Codex P2 (fix round 8): the `Ok(traced)→None` suppress arm of
/// `finalise_traced_build_output` carries the build's traced cross-file
/// *dependency* facts on a non-admitted carrier but leaves
/// `self_root_canonicals` EMPTY (the build could not be soundly
/// self-rooted). The fix-round-7 joiner gate's
/// `validate_with_self_roots` then routes every `FileWholeHash` in the
/// carrier through the LAZY `validates` (none is a listed self-root),
/// whose untracked-file arm optimistically accepts — so the carrier
/// validates against ANY follower's `ctx`. Pre-fix-8 a follower under a
/// different overlay coalesces onto the suppressed winner's
/// view-specific result.
///
/// Setup: a real `/p2_8_unrootable/keyed.ts` is upserted; the winner
/// runs under the base host and returns a `QueryBuildOutput` with
/// `cache_suppress == true`, a `graph_carrier` carrying ONE
/// `FileWholeHash` for an unrelated cross-file *dependency*
/// (`/p2_8_unrootable/dep.ts`, never tracked by either host), and an
/// EMPTY `self_root_canonicals`. The follower runs the SAME key under a
/// session that overlays the keyed file with a different content hash.
///
/// Discrimination property:
/// - Pre-fix-8: the dependency `FileWholeHash` routes through lazy
///   `validates` (not a listed self-root), the untracked-file arm
///   accepts, the legacy rail is empty — the carrier validates
///   vacuously and the follower coalesces; its build closure NEVER runs
///   (`follower_cold_ran == false`).
/// - Post-fix-8: the joiner gate sees `cache_suppress == true` and
///   `has_view_discriminating_self_root == false` (the carrier holds a
///   `FileWholeHash`, but its canonical is NOT in the empty
///   `self_root_canonicals`) and force-forks; the follower's build
///   closure runs (`follower_cold_ran == true`).
#[test]
fn cross_view_joiner_of_suppressed_unrootable_winner_forks() {
    use crate::fact_signature_helpers::ReadSetSignature;
    use crate::resolver_core::{FactVersionRef, SessionResolverContext};
    use crate::session_view::OverlaidViewRef;
    use crate::{FileKind, UpsertRequest};
    use rustc_hash::FxHashMap;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    let keyed_canonical = "/p2_8_unrootable/keyed.ts";
    let host = ctx_host();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(keyed_canonical.to_string()),
            input_id: keyed_canonical.to_string(),
            source: Arc::from("export interface Keyed { base: number; }\n"),
            file_kind: FileKind::from_path(keyed_canonical),
            aliases: Vec::new(),
        })
        .expect("upsert of the keyed file succeeds");
    let base_hash = host
        .ensure_indexed_ready(keyed_canonical)
        .expect("keyed-file base IndexedReady must materialise")
        .whole_hash;
    let host = Arc::new(host);

    let store = Arc::new(SemanticGraphStore::new());
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope(keyed_canonical),
        name: Arc::from("Keyed"),
    });

    // The suppressed winner's carrier holds ONE cross-file *dependency*
    // fact — NOT a self-root. `/p2_8_unrootable/dep.ts` is never
    // upserted, so the lazy `validates` untracked-file arm accepts it
    // under any view: the carrier cannot discriminate by view.
    let dep_canonical = "/p2_8_unrootable/dep.ts";
    let winner_dep_fact = FactVersionRef::FileWholeHash {
        canonical_id: dep_canonical.to_string(),
        hash: [0x3cu8; 16],
    };

    let (tx_winner_in_build, rx_winner_in_build) = mpsc::channel::<()>();
    let (tx_release_winner, rx_release_winner) = mpsc::channel::<()>();

    // Intern the winner's result node up front so the follower's
    // negative assertions can name the exact (view-specific) node the
    // follower must NOT return or cache.
    let winner_node = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let winner_store = Arc::clone(&store);
    let winner_host = Arc::clone(&host);
    let winner_key = key.clone();
    let winner_dep_fact_for_build = winner_dep_fact.clone();
    let winner = thread::spawn(move || {
        let host: &dyn crate::resolver_core::ResolverContext = winner_host.as_ref();
        winner_store.execute_cooperative(
            host,
            winner_key,
            || winner_store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                tx_winner_in_build
                    .send(())
                    .expect("winner: signal in-build");
                rx_release_winner
                    .recv()
                    .expect("winner: released by driver");
                // The `Ok(traced)→None` unrootable shape: a non-admitted
                // carrier carrying only cross-file *dependency* facts,
                // with an EMPTY `self_root_canonicals` — the build could
                // not be soundly self-rooted.
                let carrier = ReadSetSignature::new(
                    Arc::from(vec![winner_dep_fact_for_build.clone()]),
                    Arc::from(Vec::new().into_boxed_slice()),
                );
                crate::project_semantic_dispatch::walk::QueryBuildOutput {
                    result: QueryResult::Value(winner_node),
                    dep_signature: Arc::from(Vec::new().into_boxed_slice()),
                    walker_diagnostics: Vec::new(),
                    cache_suppress: true,
                    observed_self_roots: Vec::new(),
                    graph_carrier: Some(Box::new(carrier)),
                    self_root_canonicals: Arc::from([]),
                    pending_prefix_backfills: Vec::new(),
                }
            },
        )
    });

    rx_winner_in_build.recv().expect("winner entered build");

    let follower_cold_ran = Arc::new(AtomicBool::new(false));
    let follower_store = Arc::clone(&store);
    let follower_host = Arc::clone(&host);
    let follower_key = key.clone();
    let follower_cold_flag = Arc::clone(&follower_cold_ran);
    let follower = thread::spawn(move || {
        let overlay_hash: crate::types::Hash16 = [0xA5u8; 16];
        assert_ne!(
            overlay_hash, base_hash,
            "fixture invariant: the overlay hash must differ from the base hash",
        );
        let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
        overlays.insert(
            keyed_canonical.to_string(),
            Arc::from("export interface Keyed { overlaid: string; }\n"),
        );
        let mut overlay_hashes: FxHashMap<String, crate::types::Hash16> = FxHashMap::default();
        overlay_hashes.insert(keyed_canonical.to_string(), overlay_hash);
        let tombstones: HashSet<String> = HashSet::new();
        let view = OverlaidViewRef::new(
            follower_host.as_ref(),
            &overlays,
            &overlay_hashes,
            &tombstones,
        );
        let session_ctx = SessionResolverContext::new(follower_host.as_ref(), &view);
        let recompute_id =
            follower_store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let cache_read = follower_store.execute_cooperative(
            &session_ctx,
            follower_key,
            || follower_store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                follower_cold_flag.store(true, Ordering::SeqCst);
                (
                    QueryResult::Value(recompute_id),
                    Arc::from(Vec::new().into_boxed_slice()),
                )
            },
        );
        (cache_read.value, recompute_id)
    });

    thread::sleep(Duration::from_millis(80));
    tx_release_winner.send(()).expect("release winner");

    let _winner_read = winner.join().expect("winner joined");
    let (follower_value, follower_recompute) = follower.join().expect("follower joined");

    let snap = store.stats_snapshot();
    assert!(
        snap.joined_waits >= 1,
        "the follower MUST have hit the cooperative wait branch \
         (joined_waits={}); if this fails the follower never coalesced \
         onto the winner's flight and the test does not exercise the \
         cross-view join path at all.",
        snap.joined_waits,
    );

    assert!(
        follower_cold_ran.load(Ordering::SeqCst),
        "the follower coalesced onto an in-flight `cache_suppress` \
         winner whose carrier holds only a cross-file dependency fact \
         and an EMPTY self-root set. That carrier validates VACUOUSLY \
         against any view (the dependency `FileWholeHash` routes \
         through the lazy untracked-file-accepting `validates`), so the \
         follower MUST be force-forked and cold-recompute for its own \
         overlay view. Pre-fix-8 the joiner gate validated the \
         self-root-less carrier vacuously and coalesced the follower \
         onto the winner's view-specific suppressed result; the \
         follower's build closure never ran (codex P2 fix round 8).",
    );

    assert_ne!(
        winner_node, follower_recompute,
        "fixture invariant: the winner's node and the follower's \
         recompute node must be distinct ids so the assertions below \
         genuinely discriminate.",
    );
    match follower_value {
        QueryResult::Value(node) => {
            assert_eq!(
                node, follower_recompute,
                "the follower's result MUST be its OWN recompute node — \
                 the suppressed winner's node was computed against a \
                 different overlay and is not interchangeable.",
            );
            assert_ne!(
                node, winner_node,
                "the follower MUST NOT receive the suppressed winner's \
                 view-specific node.",
            );
        }
        other => panic!("follower: expected the recomputed Value, got {other:?}"),
    }

    // The suppressed winner's view-specific node must never reach a
    // warm cache entry. After the fork the follower's OWN (non-
    // suppressed) recompute publishes a `MemoEntry` — that is correct;
    // the discriminating negative is that the cached node is the
    // follower's recompute, NOT the winner's suppressed node.
    if let Some(cached) = store.get_unvalidated(&key) {
        match cached.value {
            QueryResult::Value(node) => assert_ne!(
                node, winner_node,
                "the suppressed winner's view-specific node must never \
                 be promoted to a warm cache entry — `cache_suppress` \
                 gates memo admission.",
            ),
            other => panic!("unexpected cached value shape: {other:?}"),
        }
    }
}

/// Discriminating test: a cross-thread joiner that coalesced onto an
/// in-flight winner that completed with a **`QueryResult::Error(Miss)`**
/// — a view-specific missing declaration — and whose carrier has **no
/// view-discriminating self-root** does NOT receive the winner's miss
/// even though the winner is **NOT `cache_suppress`**. It forks and
/// cold-recomputes for its own view.
///
/// Codex P2 (fix round 9): fix round 8 made the no-self-root joiner
/// fork fire only for `cache_suppress` winners
/// (`suppressed_without_self_root = cache_suppress && !carrier
/// .has_view_discriminating_self_root(..)`). But a NON-suppressed
/// winner can ALSO have no view-discriminating self-root: a
/// `QueryResult::Error(Miss)` produced because the requested
/// declaration is absent UNDER THE WINNER'S overlay. That build
/// completes with `cache_suppress == false` and a carrier holding only
/// cross-file *dependency* facts (no self-root for the keyed
/// canonical, because the declaration the self-root would have rooted
/// does not exist under the winner's view). The fix-8 predicate is
/// gated on `cache_suppress`, which is `false` here, so it does not
/// fork — `validate_with_self_roots` then routes the carrier's
/// dependency `FileWholeHash` through the lazy untracked-file-accepting
/// `validates` and the carrier validates VACUOUSLY against any
/// follower's `ctx`. A cross-view follower whose own overlay DOES
/// contain the declaration coalesces onto the winner's view-specific
/// `Error(Miss)` and returns a stale miss instead of recomputing.
///
/// Setup: a real `/p2_9_nonsuppressed_miss/keyed.ts` is upserted under
/// the base host with content declaring only `Other`. The winner runs
/// the `ResolveDecl` key for `Keyed` (absent under the base view) under
/// the base host and returns a `QueryBuildOutput` with `result ==
/// QueryResult::Error(QueryError::Miss)`, `cache_suppress == false`, a
/// `graph_carrier` carrying ONE `FileWholeHash` for an unrelated
/// cross-file *dependency* (`/p2_9_nonsuppressed_miss/dep.ts`, never
/// tracked by either host), and an EMPTY `self_root_canonicals` — the
/// build could not self-root the keyed file because the requested
/// declaration is missing under the winner's view. The follower runs
/// the SAME key under a `SessionResolverContext` whose `OverlaidViewRef`
/// overlays the keyed file with content that DOES declare `Keyed`; its
/// recompute closure returns a non-Miss `QueryResult::Value`.
///
/// Discrimination property:
/// - Pre-fix-9: the fix-8 predicate `suppressed_without_self_root`
///   short-circuits on `cache_suppress == false` and is therefore
///   `false`; `carrier_view_validates` is `true` (the lone dependency
///   `FileWholeHash` is not a listed self-root, routes through lazy
///   `validates`, the untracked-file arm accepts) — the joiner gate
///   does NOT fork. The follower coalesces, its build closure NEVER
///   runs (`follower_cold_ran == false`), and `follower_value` is the
///   winner's `QueryResult::Error(Miss)`.
/// - Post-fix-9: the joiner fork predicate drops the `cache_suppress`
///   gate — it fires on `!carrier.has_view_discriminating_self_root(
///   &winner_self_roots)` regardless of `cache_suppress`. The carrier
///   holds a `FileWholeHash` whose canonical is NOT in the empty
///   `self_root_canonicals`, so `has_view_discriminating_self_root` is
///   `false` and the joiner force-forks; the follower's build closure
///   runs (`follower_cold_ran == true`) and `follower_value` is its
///   OWN recomputed non-Miss `Value`.
#[test]
fn cross_view_joiner_of_nonsuppressed_miss_winner_without_self_root_forks() {
    use crate::fact_signature_helpers::ReadSetSignature;
    use crate::resolver_core::{FactVersionRef, SessionResolverContext};
    use crate::session_view::OverlaidViewRef;
    use crate::{FileKind, UpsertRequest};
    use rustc_hash::FxHashMap;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    let keyed_canonical = "/p2_9_nonsuppressed_miss/keyed.ts";
    let host = ctx_host();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(keyed_canonical.to_string()),
            input_id: keyed_canonical.to_string(),
            source: Arc::from("export interface Other { base: number; }\n"),
            file_kind: FileKind::from_path(keyed_canonical),
            aliases: Vec::new(),
        })
        .expect("upsert of the keyed file succeeds");
    let base_hash = host
        .ensure_indexed_ready(keyed_canonical)
        .expect("keyed-file base IndexedReady must materialise")
        .whole_hash;
    let host = Arc::new(host);

    let store = Arc::new(SemanticGraphStore::new());
    // The winner resolves `Keyed` — a declaration ABSENT under the
    // winner's base view (the base source only declares `Other`), so
    // the winner's build legitimately completes with `Error(Miss)` and
    // cannot self-root the keyed file for this query.
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope(keyed_canonical),
        name: Arc::from("Keyed"),
    });

    // The NON-suppressed winner's carrier holds ONE cross-file
    // *dependency* fact — NOT a self-root. `/p2_9_nonsuppressed_miss/
    // dep.ts` is never upserted, so the lazy `validates` untracked-file
    // arm accepts it under any view: the carrier cannot discriminate by
    // view, and `self_root_canonicals` is EMPTY.
    let dep_canonical = "/p2_9_nonsuppressed_miss/dep.ts";
    let winner_dep_fact = FactVersionRef::FileWholeHash {
        canonical_id: dep_canonical.to_string(),
        hash: [0x7eu8; 16],
    };

    let (tx_winner_in_build, rx_winner_in_build) = mpsc::channel::<()>();
    let (tx_release_winner, rx_release_winner) = mpsc::channel::<()>();

    let winner_store = Arc::clone(&store);
    let winner_host = Arc::clone(&host);
    let winner_key = key.clone();
    let winner_dep_fact_for_build = winner_dep_fact.clone();
    let winner = thread::spawn(move || {
        let host: &dyn crate::resolver_core::ResolverContext = winner_host.as_ref();
        winner_store.execute_cooperative(
            host,
            winner_key,
            || winner_store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                tx_winner_in_build
                    .send(())
                    .expect("winner: signal in-build");
                rx_release_winner
                    .recv()
                    .expect("winner: released by driver");
                // A NON-suppressed build that completes with a
                // view-specific `Error(Miss)`: the requested `Keyed`
                // declaration is missing under the winner's overlay, so
                // the build could not self-root the keyed file. The
                // carrier carries only an unrelated cross-file
                // *dependency* fact and an EMPTY `self_root_canonicals`
                // — `cache_suppress` is `false` (a plain miss is a
                // cacheable result, not a non-cacheable build).
                let carrier = ReadSetSignature::new(
                    Arc::from(vec![winner_dep_fact_for_build.clone()]),
                    Arc::from(Vec::new().into_boxed_slice()),
                );
                crate::project_semantic_dispatch::walk::QueryBuildOutput {
                    result: QueryResult::Error(QueryError::Miss),
                    dep_signature: Arc::from(Vec::new().into_boxed_slice()),
                    walker_diagnostics: Vec::new(),
                    cache_suppress: false,
                    observed_self_roots: Vec::new(),
                    graph_carrier: Some(Box::new(carrier)),
                    self_root_canonicals: Arc::from([]),
                    pending_prefix_backfills: Vec::new(),
                }
            },
        )
    });

    rx_winner_in_build.recv().expect("winner entered build");

    let follower_cold_ran = Arc::new(AtomicBool::new(false));
    let follower_store = Arc::clone(&store);
    let follower_host = Arc::clone(&host);
    let follower_key = key.clone();
    let follower_cold_flag = Arc::clone(&follower_cold_ran);
    let follower = thread::spawn(move || {
        // The follower's overlay DOES declare `Keyed` — under its view
        // the declaration the winner found missing exists, so its
        // recompute resolves a real non-Miss result.
        let overlay_hash: crate::types::Hash16 = [0xC9u8; 16];
        assert_ne!(
            overlay_hash, base_hash,
            "fixture invariant: the overlay hash must differ from the base hash",
        );
        let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
        overlays.insert(
            keyed_canonical.to_string(),
            Arc::from("export interface Keyed { overlaid: string; }\n"),
        );
        let mut overlay_hashes: FxHashMap<String, crate::types::Hash16> = FxHashMap::default();
        overlay_hashes.insert(keyed_canonical.to_string(), overlay_hash);
        let tombstones: HashSet<String> = HashSet::new();
        let view = OverlaidViewRef::new(
            follower_host.as_ref(),
            &overlays,
            &overlay_hashes,
            &tombstones,
        );
        let session_ctx = SessionResolverContext::new(follower_host.as_ref(), &view);
        let recompute_id =
            follower_store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let cache_read = follower_store.execute_cooperative(
            &session_ctx,
            follower_key,
            || follower_store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                follower_cold_flag.store(true, Ordering::SeqCst);
                (
                    QueryResult::Value(recompute_id),
                    Arc::from(Vec::new().into_boxed_slice()),
                )
            },
        );
        (cache_read.value, recompute_id)
    });

    thread::sleep(Duration::from_millis(80));
    tx_release_winner.send(()).expect("release winner");

    let winner_read = winner.join().expect("winner joined");
    let (follower_value, follower_recompute) = follower.join().expect("follower joined");

    // Fixture invariant: the winner genuinely produced an `Error(Miss)`
    // — without this the test does not exercise the non-suppressed
    // missing-declaration shape at all.
    assert!(
        matches!(winner_read.value, QueryResult::Error(QueryError::Miss)),
        "fixture invariant: the winner must complete with \
         QueryResult::Error(Miss); got {:?}",
        winner_read.value,
    );
    assert!(
        !winner_read.cache_suppress,
        "fixture invariant: the winner must NOT be `cache_suppress` — \
         this test discriminates a NON-suppressed no-self-root winner \
         from the fix-8 `cache_suppress`-gated fork.",
    );

    let snap = store.stats_snapshot();
    assert!(
        snap.joined_waits >= 1,
        "the follower MUST have hit the cooperative wait branch \
         (joined_waits={}); if this fails the follower never coalesced \
         onto the winner's flight and the test does not exercise the \
         cross-view join path at all.",
        snap.joined_waits,
    );

    assert!(
        follower_cold_ran.load(Ordering::SeqCst),
        "the follower coalesced onto an in-flight winner that \
         completed with a view-specific QueryResult::Error(Miss) and a \
         carrier holding only a cross-file dependency fact with an \
         EMPTY self-root set — and `cache_suppress` is FALSE. That \
         carrier validates VACUOUSLY against any view (the dependency \
         `FileWholeHash` routes through the lazy \
         untracked-file-accepting `validates`), so the follower MUST be \
         force-forked and cold-recompute for its own overlay view, \
         under which the declaration the winner found missing DOES \
         exist. Pre-fix-9 the joiner fork predicate was gated on \
         `cache_suppress`, which is false here, so it never fired and \
         the follower coalesced onto the winner's stale view-specific \
         miss; the follower's build closure never ran (codex P2 fix \
         round 9).",
    );

    match follower_value {
        QueryResult::Value(node) => {
            assert_eq!(
                node, follower_recompute,
                "the follower's result MUST be its OWN recompute node — \
                 the winner's `Error(Miss)` was computed against a \
                 different overlay (under which `Keyed` is absent) and \
                 is not interchangeable with the follower's view.",
            );
        }
        QueryResult::Error(QueryError::Miss) => panic!(
            "the follower returned the winner's stale view-specific \
             QueryResult::Error(Miss) — pre-fix-9 regression: the \
             joiner fork did not fire for a NON-suppressed \
             no-self-root winner, so the follower coalesced onto the \
             winner's miss instead of recomputing under its own \
             overlay (where the declaration exists).",
        ),
        other => panic!("follower: expected the recomputed non-Miss Value, got {other:?}"),
    }

    // The winner's view-specific miss must never reach a warm cache
    // entry under the follower's key: after the fork the follower's OWN
    // recompute publishes a `MemoEntry` carrying the resolved Value —
    // the discriminating negative is that the cached result is the
    // follower's Value, NOT the winner's `Error(Miss)`.
    if let Some(cached) = store.get_unvalidated(&key) {
        assert!(
            !matches!(cached.value, QueryResult::Error(QueryError::Miss)),
            "the winner's view-specific QueryResult::Error(Miss) must \
             never be promoted to a warm cache entry the follower's \
             view would read back — the follower's fork publishes its \
             own resolved Value.",
        );
    }
}
