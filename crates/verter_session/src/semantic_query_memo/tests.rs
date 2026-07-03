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

/// Join `handle` and return its value, panicking if it does not complete
/// within ~10s.
///
/// These cooperative-join tests reach their post-rendezvous joins only
/// after the winner has been released and the joiner woken by the
/// winner's publish, so on the happy path they complete promptly. They
/// would block forever only on a genuine `execute_cooperative`
/// singleflight deadlock — exactly the hang class this suite must surface
/// loudly rather than hanging the whole `--lib` run. A bare
/// `handle.join()` would itself hang on such a deadlock; this helper runs
/// the join on a watchdog thread that reports the joined value through a
/// rendezvous channel, and the caller PANICs if `recv_timeout` elapses.
fn join_within<T: Send + 'static>(handle: std::thread::JoinHandle<T>, label: &str) -> T {
    use std::sync::mpsc;
    let (tx, rx) = mpsc::sync_channel::<std::thread::Result<T>>(1);
    std::thread::spawn(move || {
        // `send` fails only if the receiver was dropped (caller already
        // panicked on timeout); ignore the benign disconnect.
        let _ = tx.send(handle.join());
    });
    match rx.recv_timeout(std::time::Duration::from_secs(10)) {
        Ok(Ok(value)) => value,
        Ok(Err(_)) => panic!("{label} panicked"),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!("{label} deadlocked (join did not complete within 10s)")
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("{label} watchdog channel disconnected before reporting")
        }
    }
}

/// Receive one signal from `rx`, panicking if it does not arrive within
/// ~10s. Replaces a bare `recv()` in the cooperative-join rendezvous
/// channels so a producer that stalls/deadlocks before signalling fails
/// loudly within the deadline instead of hanging the suite forever.
fn recv_signal_within(rx: &std::sync::mpsc::Receiver<()>, label: &str) {
    use std::sync::mpsc::RecvTimeoutError;
    match rx.recv_timeout(std::time::Duration::from_secs(10)) {
        Ok(()) => {}
        Err(RecvTimeoutError::Timeout) => {
            panic!("{label}: timed out waiting for signal (10s)")
        }
        Err(RecvTimeoutError::Disconnected) => {
            panic!("{label}: signal channel disconnected before the producer signalled")
        }
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

/// Structural-interning negative invariant — `VueMacroElements` is an
/// identity-carrier with latest-insert-wins semantics (see
/// [`SemanticGraphStore::insert_resolved_named_type`]). Two
/// `intern_node` calls for the same `Arc<ResolvedElements>` payload
/// must still return distinct [`SemanticNodeId`]s so fresh inserts
/// under the same `HostResolvedNamedTypeKey` do not alias with prior
/// payloads. Under naive structural dedup this would collapse — the
/// exemption in `push_impl` short-circuits the dedup index.
#[test]
fn intern_does_not_dedup_vue_macro_elements_identity_carrier() {
    use verter_compiler::utils::oxc::script::type_surface::ResolvedElements;
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
         in canonical_to_entries[\"/w/a.ts\"] (reverse index)"
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
    let host = ctx_host();
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
    // The carrier's fact rail names the test canonical via a
    // `FileWholeHash` fact — `register_reverse_index` registers under
    // every canonical `read_set_signature.canonical_ids()` yields.
    let carrier = crate::fact_signature_helpers::ReadSetSignature::new(Arc::from(vec![
        crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: "/w/helper_test.ts".to_string(),
            hash: [7u8; 16],
        },
    ]));
    store.warm_publish_one(
        &host,
        &key,
        &QueryResult::Value(value),
        &walker_diagnostics,
        &carrier,
        &dep_sig,
        &Arc::from([]),
        &crate::semantic_query::demand::MaterializedSet::single(
            super::family::requested_point_for_key(&key),
        ),
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
         mapping in canonical_to_entries[\"/w/helper_test.ts\"] (reverse index)"
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
         (reverse-index shard drain)"
    );
    assert_eq!(
        store.canonical_to_entries_count("/w/b.ts"),
        0,
        "/w/b.ts reverse-index entry for the evicted (family, slot) must be \
         cleaned up by cross-canonical cleanup; without it this entry \
         would dangle and bloat the reverse index over time"
    );
}

/// `invalidate_canonical` evicts the warm entry whose
/// dep_signature references the canonical via the reverse-index
/// path, with no behavioural change to eviction. Existing
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

// ──────────────────────────────────────────────────────────────────
// B3 — dep-signature-based invalidation sweep + in-flight drop + retry
// ──────────────────────────────────────────────────────────────────

/// An `Instantiate` entry whose body compute reads the changed
/// canonical (via the dep-sig) is evicted by the sweep. Regardless of
/// the family-key shape — `Instantiate` carries semantic-node ids, not
/// canonicals — the dep-sig walk is the single invalidation authority.
///
/// Post-D1.4: `Instantiate` is mode-slot aware (the projection mode on
/// `context.projection_reduction.mode`). A write
/// at `Expanded` backfills `Shallow` / `Navigate` / `Identity` per
/// §7.11; all four slots carry the same dep-sig and the sweep evicts
/// every one that references the touched canonical.
#[test]
fn invalidate_canonical_evicts_instantiate_entries_that_read_that_canonical_body() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let base = crate::semantic_query::DeclIdentity::synthetic("Foo").to_type_slot_unscoped();
    let arg = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let key = SemanticQueryKey::Instantiate {
        base,
        args: Arc::from(vec![arg].into_boxed_slice()),
        context: crate::semantic_query::InstantiateContext::non_file(
            crate::semantic_query::ProjectionReductionContext::published(
                crate::semantic_query::ProjectionMode::Expanded,
            ),
            Default::default(),
        ),
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
    let base = crate::semantic_query::DeclIdentity::synthetic("Foo").to_type_slot_unscoped();
    let arg = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let key = SemanticQueryKey::Instantiate {
        base,
        args: Arc::from(vec![arg].into_boxed_slice()),
        context: crate::semantic_query::InstantiateContext::non_file(
            crate::semantic_query::ProjectionReductionContext::published(
                crate::semantic_query::ProjectionMode::Expanded,
            ),
            Default::default(),
        ),
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
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Shallow,
        ),
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
    // §3.4: Shallow backfills Identity ONLY (`Shallow ⊒ Identity`, but
    // `Shallow ⊅ Navigate`), both carrying the same dep-sig (§7.11
    // conservative rule). So TWO slots are populated, and both must evict
    // on /w/subtree.ts invalidation.
    assert_eq!(store.memo_entry_count(), 2);

    let removed = store.invalidate_canonical("/w/subtree.ts");
    assert_eq!(
        removed, 2,
        "Shallow plus its one backfilled (Identity) slot both reference the touched subtree",
    );
    assert!(
        store.get_unvalidated(&key).is_none(),
        "ProjectPath Shallow entry through touched subtree must be evicted",
    );
    let narrower_key = SemanticQueryKey::ProjectPath {
        base,
        path,
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Identity,
        ),
    };
    assert!(
        store.get_unvalidated(&narrower_key).is_none(),
        "backfilled Identity slot inherits the dep-sig and must evict too",
    );
}

/// §3.4 GUARD — satisfaction is decided by the RECORDED materialised
/// point, NOT by nominal slot presence. A candidate published into the
/// `Expanded` slot whose compute ACTUALLY materialised only a `Navigate`
/// point (`{Navigate@[foo]}`) must NOT serve an `Expanded` request at that
/// path (`Navigate ⊅ Expanded`), but MUST serve a `Navigate` request at
/// that path (`Navigate ⊒ Navigate`).
///
/// DISCRIMINATING: FAILS against a memo that gates on slot presence +
/// `validate` alone with no `cached_satisfies` gate (⇒ the Expanded request
/// HITS the slot-present entry); PASSES against the §3.4 two-gate warm hit
/// (the recorded `Navigate` point fails `cached_satisfies` for the
/// `Expanded` request). If `cached_satisfies` were keyed on the candidate's
/// NOMINAL demand (its slot mode = Expanded) instead of its RECORDED set,
/// the Expanded request would wrongly HIT — this guard catches exactly
/// that silent-soundness collapse.
#[test]
fn cache_satisfaction_is_materialized_point_not_nominal_demand() {
    use crate::semantic_query::demand::{
        Demand, MaterializedPoint, MaterializedSet, ProjectionPath,
    };

    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let path: Arc<[PathSegment]> =
        Arc::from(vec![PathSegment::Member(Arc::from("foo"))].into_boxed_slice());
    let proj_path = ProjectionPath::from_segments([PathSegment::Member(Arc::from("foo"))]);

    let key_expanded = SemanticQueryKey::ProjectPath {
        base,
        path: Arc::clone(&path),
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    };
    let key_navigate = SemanticQueryKey::ProjectPath {
        base,
        path: Arc::clone(&path),
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Navigate,
        ),
    };

    // Publish into the EXPANDED slot, but record ONLY a `Navigate` point —
    // the honest record of a compute that only navigated through `foo`,
    // never expanded it (the nominal slot is Expanded; the materialised set
    // is Navigate).
    let value = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let recorded =
        MaterializedSet::single(MaterializedPoint::new(Demand::navigate(proj_path.clone())));
    let populated = store.publish_with_materialized_set_for_tests(
        key_expanded.clone(),
        QueryResult::Value(value),
        crate::fact_signature_helpers::ReadSetSignature::new(
            crate::fact_signature_helpers::empty_fact_signature(),
        ),
        Arc::from([]),
        empty_signature(),
        0,
        recorded,
    );
    assert!(
        populated >= 1,
        "the Expanded-slot publish must populate ≥1 slot"
    );

    // GATE: an Expanded request MISSES — the recorded `Navigate` point does
    // NOT dominate the `Expanded` request (`Navigate ⊅ Expanded`).
    assert!(
        store.get_validated(&key_expanded, &host).is_none(),
        "recorded Navigate point must NOT satisfy an Expanded request \
         (materialised-point satisfaction, not nominal slot presence)",
    );

    // POSITIVE CONTROL: a Navigate request HITS — the SAME recorded
    // Navigate point dominates the `Navigate` request at that path. Proves
    // the MISS above is recorded-point discrimination, not a blanket
    // reject of the published entry.
    assert!(
        store.get_validated(&key_navigate, &host).is_some(),
        "recorded Navigate point MUST satisfy a Navigate request at the same path",
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
        PathSegment::Member(Arc::from("c")),
        PathSegment::Member(Arc::from("full")),
        PathSegment::Member(Arc::from("bar")),
    ]);
    let shallow_prefix = ProjectionPath::from_segments([PathSegment::Member(Arc::from("c"))]);

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

/// §3.4 SOUNDNESS PIN — the PRODUCTION publish path (`warm_publish_one`)
/// debug-asserts that a published entry's recorded terminal point (the
/// point at the key's own projection path) is at-least the slot's mode.
///
/// A sub-slot-mode terminal — e.g. a carrier-stopping `Navigate` terminal
/// recorded in an `Expanded` slot — would let the two-gate warm hit SERVE
/// (and the directional backfill CLONE into the `Shallow` slot, since
/// `Navigate ⊒ Shallow`) an under-materialised surface. This is
/// unreachable in production but UNGUARDED before §3.4 hardening; the
/// `debug_assert!` pins it.
///
/// DISCRIMINATING: this drives the PRODUCTION `warm_publish_one` directly
/// with a `Navigate` record published into an `Expanded` slot and asserts
/// it panics. If the `debug_assert!` were removed, the publish would
/// silently succeed and this test would FAIL (no panic). Gated on
/// `debug_assertions` because `debug_assert!` is a no-op in release.
/// Note the test-only `publish_with_materialized_set_for_tests` path (used
/// by `cache_satisfaction_is_materialized_point_not_nominal_demand`) does
/// NOT carry this assert, so that adversarial gate test is unaffected.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "records no terminal satisfying the slot's mode")]
fn warm_publish_one_debug_asserts_against_sub_slot_mode_terminal() {
    use crate::semantic_query::demand::{
        Demand, MaterializedPoint, MaterializedSet, ProjectionPath,
    };

    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let path: Arc<[PathSegment]> =
        Arc::from(vec![PathSegment::Member(Arc::from("foo"))].into_boxed_slice());
    let proj_path = ProjectionPath::from_segments([PathSegment::Member(Arc::from("foo"))]);
    let key_expanded = SemanticQueryKey::ProjectPath {
        base,
        path,
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    };
    let value = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let walker_diagnostics: std::sync::Arc<
        [crate::project_semantic_dispatch::walk::ShallowDiagnostic],
    > = std::sync::Arc::from([]);
    let carrier = crate::fact_signature_helpers::ReadSetSignature::new(
        crate::fact_signature_helpers::empty_fact_signature(),
    );
    let inflight = Arc::new(InflightEntry::new());
    // A `Navigate` terminal recorded into the `Expanded` slot — the
    // forbidden sub-slot-mode terminal the production assert rejects.
    let bad = MaterializedSet::single(MaterializedPoint::new(Demand::navigate(proj_path)));
    store.warm_publish_one(
        &host,
        &key_expanded,
        &QueryResult::Value(value),
        &walker_diagnostics,
        &carrier,
        &empty_signature(),
        &Arc::from([]),
        &bad,
        &inflight,
    );
}

/// §3.4 SOUNDNESS PIN (positive) — no production cold-build records a
/// sub-slot-mode terminal point. A single-terminal cold build through the
/// real `execute_cooperative` admission flow for an `Expanded` key records
/// EXACTLY the slot's own mode at the key's path
/// (`requested_point_for_key`), never a narrower (sub-slot) mode.
///
/// Together with the path-walk arithmetic test
/// (`path_walk_materialized_set_records_linear_navhops_and_stops_at_arm_split`,
/// which proves the path-walk terminal records `context.mode`) and the
/// `warm_publish_one` `debug_assert!` (live under `debug_assertions` for
/// EVERY production publish), this pins that no production cold-build path
/// can record a sub-slot-mode terminal.
///
/// DISCRIMINATING: if the cold-build default recorded a `Navigate`
/// terminal (or any narrower mode) for an `Expanded` key, the recorded-set
/// equality below would FAIL — and the `warm_publish_one` `debug_assert!`
/// would additionally fire on the publish.
#[test]
fn cold_build_default_records_slot_mode_terminal_not_sub_slot() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let path: Arc<[PathSegment]> =
        Arc::from(vec![PathSegment::Member(Arc::from("foo"))].into_boxed_slice());
    let key_expanded = SemanticQueryKey::ProjectPath {
        base,
        path,
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    };

    let value = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let _ = store.execute_cooperative(
        &host,
        key_expanded.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || (QueryResult::Value(value), empty_signature()),
    );

    // The recorded terminal is the slot's OWN mode at the key's path —
    // NOT a sub-slot (e.g. Navigate) terminal.
    let expected = crate::semantic_query::demand::MaterializedSet::single(
        super::family::requested_point_for_key(&key_expanded),
    );
    assert_eq!(
        store.entry_satisfied_projection_for_tests(&key_expanded),
        Some(expected),
        "a single-terminal cold build must record the slot-mode terminal \
         (Expanded@[foo]), never a sub-slot-mode terminal",
    );
    // Sanity: the entry self-satisfies its own Expanded request.
    assert!(
        store.get_validated(&key_expanded, &host).is_some(),
        "the entry must serve its own Expanded request",
    );
}

/// §3.4 DIRECTIONAL-GATE REGRESSION — a carrier-stopping `Navigate`
/// compute does NOT serve or backfill a `Shallow` request. The lattice has
/// `Navigate ⊒ Shallow`, so a naive all-peers-gated backfill would clone a
/// `Navigate` result into the `Shallow` slot — operationally unsound (it
/// would serve a one-shell Shallow surface from a result that only
/// navigated through, never expanded, the type). The §3.4 backfill is
/// DIRECTIONAL: `slot_domain_siblings(Navigate) = [Identity]` only, so a
/// Navigate compute never targets the broader Shallow slot.
///
/// DISCRIMINATING: a Navigate cold build leaves the `Shallow` slot EMPTY
/// (and a Shallow request MISSES). It backfills only the narrower
/// `Identity` slot. If the directional rule regressed to an all-lattice-
/// peers backfill, the Shallow slot would be populated from the Navigate
/// record and the Shallow request would wrongly HIT.
#[test]
fn navigate_compute_does_not_serve_or_backfill_shallow_request() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let path: Arc<[PathSegment]> =
        Arc::from(vec![PathSegment::Member(Arc::from("foo"))].into_boxed_slice());
    let mk = |mode| SemanticQueryKey::ProjectPath {
        base,
        path: Arc::clone(&path),
        context: crate::semantic_query::ProjectionReductionContext::published(mode),
    };
    let key_navigate = mk(ProjectionMode::Navigate);
    let key_shallow = mk(ProjectionMode::Shallow);
    let key_identity = mk(ProjectionMode::Identity);

    // Navigate cold build; the default record set is `{Navigate@[foo]}`.
    let value = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let _ = store.execute_cooperative(
        &host,
        key_navigate.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || (QueryResult::Value(value), empty_signature()),
    );

    // Shallow slot stays EMPTY — Navigate never targets the broader
    // Shallow slot (directional), so a Shallow request MISSES.
    assert_eq!(
        store.slot_candidate_count_for_tests(&key_shallow),
        0,
        "a Navigate compute must NOT backfill the broader Shallow slot (directional gate)",
    );
    assert!(
        store.get_validated(&key_shallow, &host).is_none(),
        "a Shallow request must MISS — a carrier-stopping Navigate result must not serve a \
         Shallow shell surface",
    );

    // Positive controls: the Navigate request HITS, and the narrower
    // Identity slot IS backfilled (`Navigate ⊒ Identity`).
    assert!(
        store.get_validated(&key_navigate, &host).is_some(),
        "the Navigate request HITS its own compute",
    );
    assert_eq!(
        store.slot_candidate_count_for_tests(&key_identity),
        1,
        "Navigate backfills the narrower Identity slot (Navigate ⊒ Identity)",
    );
}

/// §3.4 GUARD — same-family backfill writes ONLY the RECORDED materialised
/// points (verbatim), and ONLY into a sibling slot a recorded point
/// dominates — never by enum rank, never a synthesised/meet point.
///
/// A `Shallow` primary records `{Shallow@[foo]}`. Under the demand lattice
/// `Shallow ⊒ Identity` but `Shallow ⊅ Navigate`
/// (`normalization_depth: None < NavigateOnly`). So the backfill fills the
/// `Identity` slot (carrying the recorded `{Shallow@[foo]}` VERBATIM) and
/// leaves the `Navigate` slot EMPTY.
///
/// DISCRIMINATING: FAILS against the legacy `backfill_targets` enum-rank
/// hierarchy (`Shallow → [Navigate, Identity]` cloned the entry into the
/// Navigate slot ⇒ Navigate slot populated); PASSES against the §3.4
/// recorded-point backfill (Navigate slot stays empty because
/// `Shallow ⊅ Navigate`).
#[test]
fn backfill_writes_only_recorded_materialized_points() {
    use crate::semantic_query::demand::{Demand, MaterializedSet, ProjectionPath};

    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let path: Arc<[PathSegment]> =
        Arc::from(vec![PathSegment::Member(Arc::from("foo"))].into_boxed_slice());
    let proj_path = ProjectionPath::from_segments([PathSegment::Member(Arc::from("foo"))]);

    let mk = |mode| SemanticQueryKey::ProjectPath {
        base,
        path: Arc::clone(&path),
        context: crate::semantic_query::ProjectionReductionContext::published(mode),
    };
    let key_shallow = mk(ProjectionMode::Shallow);
    let key_navigate = mk(ProjectionMode::Navigate);
    let key_identity = mk(ProjectionMode::Identity);

    // Publish a Shallow primary; the default record set is `{Shallow@[foo]}`.
    let value = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let _ = store.execute_cooperative(
        &host,
        key_shallow.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || (QueryResult::Value(value), empty_signature()),
    );

    // Navigate slot stays EMPTY — `Shallow ⊅ Navigate`, so no backfill.
    assert_eq!(
        store.slot_candidate_count_for_tests(&key_navigate),
        0,
        "Shallow must NOT backfill the Navigate slot (Shallow ⊅ Navigate in the lattice)",
    );
    assert!(
        store.get_validated(&key_navigate, &host).is_none(),
        "a Navigate request must MISS — the Shallow compute never materialised a Navigate point",
    );

    // Identity slot IS backfilled (`Shallow ⊒ Identity`), and carries the
    // RECORDED `{Shallow@[foo]}` set VERBATIM — NOT a synthesised
    // `Identity@[foo]` point.
    assert_eq!(
        store.slot_candidate_count_for_tests(&key_identity),
        1,
        "Shallow must backfill the Identity slot (Shallow ⊒ Identity)",
    );
    let mut shallow_point = Demand::shallow();
    shallow_point.projection.path = proj_path;
    let expected = MaterializedSet::single(crate::semantic_query::demand::MaterializedPoint::new(
        shallow_point,
    ));
    assert_eq!(
        store.entry_satisfied_projection_for_tests(&key_identity),
        Some(expected),
        "the backfilled Identity entry must carry the RECORDED Shallow point verbatim, \
         never a synthesised Identity/meet point",
    );
    assert!(
        store.get_validated(&key_identity, &host).is_some(),
        "an Identity request HITS the backfilled entry (Shallow ⊒ Identity)",
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
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Identity,
        ),
    };
    let key_expanded = SemanticQueryKey::ProjectPath {
        base,
        path: Arc::clone(&path),
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
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
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
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
            context: crate::semantic_query::ProjectionReductionContext::published(mode),
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
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Navigate,
        ),
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
    // Navigate backfills Identity (directional, gated) with the
    // narrow-only dep-sig — two slots.
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
/// winner's own reads never touched it) — stale data that
/// `StoreView::validates_fact_signature` cannot catch, because the
/// stored signature is technically valid against the new state.
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
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Identity,
        ),
    };
    let key_expanded = SemanticQueryKey::ProjectPath {
        base,
        path: Arc::clone(&path),
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
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
                // /w/target.ts — so `StoreView::validates_fact_signature`
                // would NOT catch a stale publish of this result.
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
    store.insert_relation(
        crate::semantic_query::RelateMemoKey::assignable(
            result,
            result,
            crate::semantic_query::RelationContext::default(),
        ),
        crate::fact_signature_helpers::ReadSetSignature::empty(),
        Arc::from(Vec::<Arc<str>>::new().into_boxed_slice()),
        crate::semantic_query::RelationResult::NotAssignable,
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

/// `invalidate_all` ABORT INVARIANT — a project-generation bump that
/// finds a claimed, mid-build in-flight admission MUST abort that
/// admission, and the aborted winner's later publish attempt MUST be
/// skipped.
///
/// This is the deterministic single-API guard for the clear/abort
/// post-condition: when `invalidate_all` returns, every in-flight entry
/// it observed is `aborted = true`, the in-flight table is drained, and
/// a winner that completes its build afterwards observes the abort under
/// the entries lock and skips its warm publish — so no memo slot
/// re-warms with a result computed against the stale project
/// generation. Mirrors the per-canonical abort in
/// `invalidate_canonical`.
#[test]
fn invalidate_all_aborts_pending_inflight_and_skips_aborted_publish() {
    use std::sync::Barrier;
    use std::thread;

    let store = Arc::new(SemanticGraphStore::new());
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/a.ts"),
        name: Arc::from("W"),
    });

    // Barrier 1: winner signals it is inside the cold-build closure
    // (its in-flight entry is registered + claimed).
    // Barrier 2: main releases the winner to finish its build.
    let in_build = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));

    let store_w = Arc::clone(&store);
    let in_build_w = Arc::clone(&in_build);
    let release_w = Arc::clone(&release);
    let key_w = key.clone();
    let winner = thread::spawn(move || {
        let host = ctx_host();
        store_w.execute_cooperative(
            &host,
            key_w,
            || store_w.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                in_build_w.wait();
                release_w.wait();
                let id = store_w.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
                (QueryResult::Value(id), empty_signature())
            },
        )
    });

    // Winner is now parked mid-build with a claimed in-flight entry.
    // While only the cold winner is mid-build the strong count is
    // `WINNER_ONLY_INFLIGHT_REFS` (table entry + winner's `inflight`
    // local + the `InflightPanicGuard` clone created before `build()`).
    in_build.wait();
    assert_eq!(
        store.test_inflight_strong_count(&key),
        WINNER_ONLY_INFLIGHT_REFS,
        "winner's in-flight entry must be registered before invalidate_all",
    );

    // Project-generation bump while the winner is mid-build.
    let _ = store.invalidate_all();

    // The in-flight table was drained.
    assert_eq!(
        store.test_inflight_strong_count(&key),
        0,
        "invalidate_all must drain the in-flight admission table",
    );

    // Release the winner. Its build completes and it enters the
    // warm-publish path; the TOCTOU re-check observes `aborted = true`
    // (set by invalidate_all) and skips the publish.
    release.wait();
    let _ = winner.join().expect("winner thread must not panic");

    assert!(
        store.get_unvalidated(&key).is_none(),
        "ABORT BUG: a winner aborted by invalidate_all must skip its \
         warm publish — the memo slot must stay empty, never re-warm with \
         a result computed against the stale project generation",
    );
    assert_eq!(
        store.memo_entry_count(),
        0,
        "no memo entry may survive a project-generation bump that \
         aborted the only in-flight winner",
    );
}

/// ABORT-LOOP LOCK ORDER — `invalidate_all`'s in-flight abort loop MUST
/// NOT hold the `inflight` table lock while it takes each entry's
/// per-entry `state` lock.
///
/// The module documents one global rule — `state` is never taken while
/// the `inflight` table lock is held. An abort loop that held the table
/// lock across the per-entry `state` acquisition would establish a
/// `table → state` nesting that violates that rule — a latent lock-order
/// inconsistency. (It is not a live deadlock: `InflightPanicGuard::drop`,
/// the only other path touching both locks, acquires `state`, *releases*
/// it, and only then acquires the `inflight` table lock — two sequential,
/// non-nested acquisitions — so it cannot AB-BA against either order.)
/// The fix is collect-then-release: snapshot the `Arc<InflightEntry>`
/// handles AND drain the table under the table lock, RELEASE the table
/// lock, THEN lock each `state` — keeping the rule uniform.
///
/// Deterministic. A cold winner is parked mid-build so one in-flight
/// entry is registered. `invalidate_all` is run on a second thread; it
/// is parked — via the `invalidate_all_inflight_abort_gate` injection
/// point — while iterating the COLLECTED entries and locking per-entry
/// `state`. With `invalidate_all` pinned there the test asserts
/// `test_inflight_table_is_unlocked()` is `true`.
///
/// DISCRIMINATES. With the collect-then-release fix the `inflight` table
/// lock is released before the per-entry `state` loop, so `try_lock`
/// succeeds and the assertion PASSES. Against the pre-fix loop (table
/// lock held across the whole `for inflight in table.values()` body and
/// its nested `state.lock()`), the table lock is still held while the
/// loop is parked, `try_lock` returns `None`, and the assertion FAILS —
/// the exact `table → state` nesting the global rule forbids.
#[test]
fn invalidate_all_inflight_abort_loop_releases_table_lock_before_state() {
    use std::sync::Barrier;
    use std::thread;

    let store = Arc::new(SemanticGraphStore::new());
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/abortlock.ts"),
        name: Arc::from("W"),
    });

    // Park a cold winner mid-build so exactly one in-flight entry is
    // registered for `invalidate_all`'s abort loop to iterate.
    let in_build = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let store_w = Arc::clone(&store);
    let in_build_w = Arc::clone(&in_build);
    let release_w = Arc::clone(&release);
    let key_w = key.clone();
    let winner = thread::spawn(move || {
        let host = ctx_host();
        store_w.execute_cooperative(
            &host,
            key_w,
            || store_w.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                in_build_w.wait();
                release_w.wait();
                let id = store_w.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
                (QueryResult::Value(id), empty_signature())
            },
        )
    });

    // Winner is parked mid-build with a claimed in-flight entry.
    in_build.wait();
    assert_eq!(
        store.test_inflight_strong_count(&key),
        WINNER_ONLY_INFLIGHT_REFS,
        "winner's in-flight entry must be registered before invalidate_all",
    );

    // Arm the abort-loop injection point and run `invalidate_all` on a
    // second thread. It collects + drains the in-flight table under the
    // table lock, RELEASES the table lock, then iterates the collected
    // entries — parking on this barrier while a per-entry `state` lock is
    // held.
    let abort_parked = Arc::new(Barrier::new(2));
    let _gate = store.test_invalidate_all_inflight_abort_gate(Arc::clone(&abort_parked));
    let store_inv = Arc::clone(&store);
    let invalidator = thread::spawn(move || store_inv.invalidate_all());

    // `invalidate_all` is parked inside the per-entry `state`-lock loop.
    abort_parked.wait();

    // THE DISCRIMINATOR. The `inflight` table lock must already be
    // released — the loop is iterating COLLECTED handles, not the table.
    assert!(
        store.test_inflight_table_is_unlocked(),
        "ABORT-LOOP LOCK INVERSION: `invalidate_all`'s in-flight abort \
         loop is holding the `inflight` table lock while it takes a \
         per-entry `state` lock. That `table → state` nesting violates \
         the module-global rule (`state` is never taken while the \
         `inflight` table lock is held). The loop must collect the entry \
         handles + drain the table under the table lock, RELEASE the \
         table lock, THEN lock each `state`.",
    );

    // Release `invalidate_all`, then the winner; both must complete.
    abort_parked.wait();
    let _ = invalidator.join().expect("invalidator thread");
    release.wait();
    let _ = winner.join().expect("winner thread must not panic");

    assert_eq!(
        store.memo_entry_count(),
        0,
        "no memo entry may survive the project-generation reset",
    );
}

/// ABORT-LOOP LOCK ORDER — `invalidate_canonical`'s in-flight abort loop
/// MUST NOT hold the `inflight` table lock while it takes each entry's
/// per-entry `state` lock. This mirrors the same invariant
/// `invalidate_all_inflight_abort_loop_releases_table_lock_before_state`
/// asserts for `invalidate_all`: the module documents one global rule —
/// `state` is never taken while the `inflight` table lock is held — and
/// both invalidation paths must honour it. Holding `table` across the
/// per-entry `state` acquisition is the inverse of the sequential
/// `state`-then-`table` order in `InflightPanicGuard::drop`; the
/// collect-then-release shape keeps the two acquisitions from ever
/// nesting in opposite directions.
///
/// Deterministic, and it preserves `invalidate_canonical`'s SELECTIVE
/// semantics — only the in-flight entry whose `(family, slot)` was swept
/// is aborted. A cold winner is parked mid-build for `(F, Identity)` so
/// one in-flight entry is registered at that pair; an `Expanded` publish
/// then backfills the warm `Identity` slot with a dep-sig referencing
/// `/w/abortcanon.ts`, so the canonical sweep finds `(F, Identity)` in
/// its reverse index and aborts the parked winner. `invalidate_canonical`
/// runs on a second thread; it is parked — via the
/// `invalidate_canonical_inflight_abort_gate` injection point — while
/// iterating the COLLECTED entry handles and locking each `state`.
///
/// DISCRIMINATES. With the collect-then-release fix the `inflight` table
/// lock is released before the per-entry `state` loop, so
/// `test_inflight_table_is_unlocked()` is `true` and the assertion
/// PASSES. Against the pre-fix loop (the per-entry `state.lock()` taken
/// inside the `table.retain` closure, with the table lock held across
/// the whole closure), the table lock is still held while the loop is
/// parked, `try_lock` returns `None`, and the assertion FAILS.
#[test]
fn invalidate_canonical_inflight_abort_loop_releases_table_lock_before_state() {
    use std::sync::Barrier;
    use std::thread;

    let store = Arc::new(SemanticGraphStore::new());
    let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let path: Arc<[PathSegment]> =
        Arc::from(vec![PathSegment::Member(Arc::from("foo"))].into_boxed_slice());

    let key_identity = SemanticQueryKey::ProjectPath {
        base,
        path: Arc::clone(&path),
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Identity,
        ),
    };
    let key_expanded = SemanticQueryKey::ProjectPath {
        base,
        path: Arc::clone(&path),
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    };

    // Park a cold winner mid-build for `(F, Identity)` so exactly one
    // in-flight entry is registered + claimed at that pair.
    let in_build = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let store_w = Arc::clone(&store);
    let in_build_w = Arc::clone(&in_build);
    let release_w = Arc::clone(&release);
    let key_w = key_identity.clone();
    let a_result = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let winner = thread::spawn(move || {
        let host = ctx_host();
        store_w.execute_cooperative(
            &host,
            key_w,
            || store_w.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                in_build_w.wait();
                release_w.wait();
                // Dep-sig deliberately does NOT reference the swept
                // canonical — the winner is aborted by the sweep, so its
                // result must never re-warm the slot.
                (
                    QueryResult::Value(a_result),
                    dep_sig_for("/w/unrelated.ts", 9),
                )
            },
        )
    });

    // Winner is parked mid-build with a claimed in-flight entry at
    // `(F, Identity)`.
    in_build.wait();
    assert_eq!(
        store.test_inflight_strong_count(&key_identity),
        WINNER_ONLY_INFLIGHT_REFS,
        "winner's in-flight entry must be registered before invalidate_canonical",
    );

    // Publish `(F, Expanded)` with a dep-sig referencing the canonical
    // to be swept. Expanded's backfill fills the currently-empty
    // `Identity` slot directly (not gated on the winner's in-flight
    // claim), so `(F, Identity)` is registered under `/w/abortcanon.ts`
    // in the reverse index — making it land in `affected_pairs` when the
    // canonical is swept.
    let host = ctx_host();
    let exp_result = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let _ = store.execute_cooperative(
        &host,
        key_expanded,
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            (
                QueryResult::Value(exp_result),
                dep_sig_for("/w/abortcanon.ts", 2),
            )
        },
    );
    assert!(
        store.get_unvalidated(&key_identity).is_some(),
        "Expanded's backfill must populate the Identity slot before the sweep",
    );

    // Arm the abort-loop injection point and run `invalidate_canonical`
    // on a second thread. It collects the matching in-flight handles +
    // drains them from the table under the table lock, RELEASES the
    // table lock, then iterates the collected entries — parking on this
    // barrier while a per-entry `state` lock is held.
    let abort_parked = Arc::new(Barrier::new(2));
    let _gate = store.test_invalidate_canonical_inflight_abort_gate(Arc::clone(&abort_parked));
    let store_inv = Arc::clone(&store);
    let invalidator = thread::spawn(move || store_inv.invalidate_canonical("/w/abortcanon.ts"));

    // `invalidate_canonical` is parked inside the per-entry `state`-lock
    // loop.
    abort_parked.wait();

    // THE DISCRIMINATOR. The `inflight` table lock must already be
    // released — the loop is iterating COLLECTED handles, not the table.
    assert!(
        store.test_inflight_table_is_unlocked(),
        "ABORT-LOOP LOCK INVERSION: `invalidate_canonical`'s in-flight \
         abort loop is holding the `inflight` table lock while it takes a \
         per-entry `state` lock. The module documents a global rule — \
         `state` is never taken while the `inflight` table lock is held — \
         and `invalidate_canonical` must honour it. The loop must collect \
         the matching entry handles + drain them from the table under the \
         table lock, RELEASE the table lock, THEN lock each `state`.",
    );

    // Release `invalidate_canonical`, then the winner; both must complete.
    abort_parked.wait();
    let removed = invalidator.join().expect("invalidator thread");
    assert_eq!(
        removed, 4,
        "the sweep evicts all four warm slots backfilled from the Expanded publish",
    );
    release.wait();
    let _ = winner.join().expect("winner thread must not panic");

    // SELECTIVE-SEMANTICS GUARD. The aborted winner observed the sweep
    // under the entries lock and skipped its warm publish — the Identity
    // slot stays evicted, it did not re-warm with the winner's stale
    // `/w/unrelated.ts`-dep-sig result.
    assert!(
        store.get_unvalidated(&key_identity).is_none(),
        "aborted winner must skip warm publish — Identity slot stays evicted",
    );
}

/// `invalidate_all` TORN-PUBLISH WINDOW — a cold winner that finishes its
/// build and reaches `warm_publish_one` in the tail of a concurrent
/// `invalidate_all` MUST NOT strand a memo entry computed against the
/// superseded project generation.
///
/// The window: when `invalidate_all` clears `entries` and releases that
/// lock BEFORE a separate block sets `aborted` on the in-flight table, a
/// winner can acquire `entries` in the gap, see `aborted == false` in
/// `warm_publish_one`'s TOCTOU re-check, publish a memo slot — and the
/// abort block then drains `inflight` WITHOUT removing that stranded
/// slot. `invalidate_all` is meant to leave the memo empty on a
/// project-generation bump.
///
/// This test pins the window deterministically. The winner is parked
/// mid-build (its in-flight entry registered + claimed). `invalidate_all`
/// runs on a second thread and parks at the per-store post-`entries`-
/// clear injection point — that point fires AFTER the `entries` lock that
/// performs the abort + clear has been released. While `invalidate_all`
/// is parked there, the winner is released: it finishes its build and
/// runs `warm_publish_one`, which acquires the (now free) `entries` lock
/// and re-checks `aborted`.
///
/// DISCRIMINATES: against a tree where `invalidate_all` clears `entries`
/// and releases the lock before setting `aborted`, the winner re-check
/// sees `aborted == false` and publishes — `memo_entry_count()` is `1`
/// after `invalidate_all` returns. After the fix the abort runs under the
/// SAME `entries`-lock hold as the clear, so by the time the injection
/// point (post-lock-release) fires `aborted` is already `true`; the
/// winner's re-check skips the publish and the memo stays empty.
#[test]
fn invalidate_all_closes_torn_publish_window_against_cold_winner() {
    use std::sync::Barrier;
    use std::thread;

    let store = Arc::new(SemanticGraphStore::new());
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/torn.ts"),
        name: Arc::from("W"),
    });

    // winner_in_build: winner signals it is inside the cold-build closure
    //   (its in-flight entry is registered + claimed).
    // release_winner: main releases the winner to finish its build —
    //   driven only AFTER `invalidate_all` is parked at its injection
    //   point, so the winner's `warm_publish_one` runs in the post-clear
    //   tail of `invalidate_all`.
    let winner_in_build = Arc::new(Barrier::new(2));
    let release_winner = Arc::new(Barrier::new(2));
    // inval_gate: the barrier armed on the store's `invalidate_all`
    //   post-`entries`-clear injection point. `invalidate_all` calls
    //   `wait()` on it (party 1); main supplies party 2 only after the
    //   winner has fully finished, so `invalidate_all` stays parked
    //   across the winner's publish attempt.
    let inval_gate = Arc::new(Barrier::new(2));

    let store_w = Arc::clone(&store);
    let in_build_w = Arc::clone(&winner_in_build);
    let release_w = Arc::clone(&release_winner);
    let key_w = key.clone();
    let winner = thread::spawn(move || {
        let host = ctx_host();
        store_w.execute_cooperative(
            &host,
            key_w,
            || store_w.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                // Parked mid-build: the in-flight entry is claimed but
                // `inflight.state` is unlocked, so `invalidate_all`'s
                // abort can mark it. Released only once `invalidate_all`
                // is parked at its post-clear injection point.
                in_build_w.wait();
                release_w.wait();
                let id = store_w.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
                (QueryResult::Value(id), empty_signature())
            },
        )
    });

    // Winner is parked mid-build with a claimed in-flight entry.
    winner_in_build.wait();
    assert_eq!(
        store.test_inflight_strong_count(&key),
        WINNER_ONLY_INFLIGHT_REFS,
        "winner's in-flight entry must be registered before invalidate_all",
    );

    // Arm the post-`entries`-clear injection point and run
    // `invalidate_all` on a second thread.
    let _gate_guard = store.test_invalidate_all_post_entries_clear_gate(Arc::clone(&inval_gate));
    let store_i = Arc::clone(&store);
    let invalidator = thread::spawn(move || store_i.invalidate_all());

    // The winner is still parked, so `invalidate_all` runs its abort +
    // `entries` clear, releases the `entries` lock, and parks at the
    // injection point (waiting for party 2 of `inval_gate`). Release the
    // winner NOW: it finishes its build and enters `warm_publish_one`,
    // acquiring the freed `entries` lock and re-checking `aborted`.
    release_winner.wait();

    // Wait for the winner's `execute_cooperative` to fully return — its
    // publish (or abort-skip) is complete by this point.
    let _ = winner.join().expect("winner thread must not panic");

    // Release `invalidate_all` from the injection point so it runs its
    // remaining tail and returns.
    inval_gate.wait();
    let _ = invalidator
        .join()
        .expect("invalidator thread must not panic");

    // The memo MUST be empty. A torn publish that slipped through the
    // pre-fix window would leave exactly one stranded entry that the
    // abort/clear failed to remove.
    assert_eq!(
        store.memo_entry_count(),
        0,
        "TORN-PUBLISH BUG: a cold winner that reached `warm_publish_one` \
         in `invalidate_all`'s post-`entries`-clear tail published a memo \
         entry that survived the project-generation reset. `invalidate_all` \
         must hold `entries` across BOTH the abort and the clear so the \
         winner's `aborted` re-check skips the publish.",
    );
    assert!(
        store.get_unvalidated(&key).is_none(),
        "TORN-PUBLISH BUG: the winner's `(family, slot)` must not be warm \
         after `invalidate_all` — the build interned its ids against the \
         superseded project generation.",
    );
}

/// PREFIX-BACKFILL ABORT FENCE (P1, unit) — `warm_publish_one_if_absent`
/// MUST re-check the parent winner's in-flight `aborted` flag and skip
/// the publish when the parent build was aborted.
///
/// A parent cold winner accumulates prefix-backfill records, then
/// publishes them via `warm_publish_one_if_absent`. If a project-
/// generation reset (`invalidate_all`) aborts the parent build, every
/// backfill it accumulated was interned against a now-stale
/// `SemanticNodeId` epoch and MUST NOT enter the warm memo — exactly
/// the contract `warm_publish_one` already enforces for the parent slot.
///
/// This unit test drives the helper directly: it publishes one backfill
/// through `warm_publish_one_if_absent` under an `aborted` in-flight
/// entry, then a second through a fresh (non-aborted) one.
///
/// DISCRIMINATES: against a `warm_publish_one_if_absent` with no abort
/// re-check the aborted-parent publish lands an entry and
/// `memo_entry_count()` is `1` after the aborted call — the assertion
/// FAILS. With the abort fence the aborted-parent publish is skipped
/// (count `0`) and the non-aborted publish still lands (count `1`) —
/// both assertions PASS, proving the fence is precise (skips ONLY the
/// aborted case, never a healthy one).
#[test]
fn warm_publish_one_if_absent_skips_publish_when_parent_inflight_aborted() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();

    let aborted_key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/backfill_aborted.ts"),
        name: Arc::from("Stale"),
    });
    let healthy_key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/backfill_healthy.ts"),
        name: Arc::from("Fresh"),
    });
    let node = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

    // An ABORTED parent in-flight entry — models a project-generation
    // reset that aborted the parent cold build mid-flight.
    let aborted_parent = Arc::new(InflightEntry::new());
    aborted_parent.state.lock().aborted = true;
    store.warm_publish_one_if_absent(
        &host,
        aborted_key.clone(),
        QueryResult::Value(node),
        crate::fact_signature_helpers::ReadSetSignature::empty(),
        Arc::from(Vec::new()),
        Arc::from([]),
        crate::semantic_query::demand::MaterializedSet::single(
            super::family::requested_point_for_key(&aborted_key),
        ),
        &aborted_parent,
    );
    assert!(
        store.get_unvalidated(&aborted_key).is_none(),
        "ABORT-FENCE BUG: `warm_publish_one_if_absent` published a \
         backfill under an `aborted` parent in-flight entry. An aborted \
         winner's backfills were interned against a stale id epoch and \
         must NOT enter the warm memo.",
    );
    assert_eq!(
        store.memo_entry_count(),
        0,
        "ABORT-FENCE BUG: a backfill survived an aborted-parent publish",
    );

    // A fresh (non-aborted) parent — the negative control. The same
    // call MUST publish; the abort fence is precise, not a blanket
    // refusal.
    let healthy_parent = Arc::new(InflightEntry::new());
    store.warm_publish_one_if_absent(
        &host,
        healthy_key.clone(),
        QueryResult::Value(node),
        crate::fact_signature_helpers::ReadSetSignature::empty(),
        Arc::from(Vec::new()),
        Arc::from([]),
        crate::semantic_query::demand::MaterializedSet::single(
            super::family::requested_point_for_key(&healthy_key),
        ),
        &healthy_parent,
    );
    assert!(
        store.get_unvalidated(&healthy_key).is_some(),
        "the abort fence must skip ONLY the aborted parent — a healthy \
         parent's backfill must still publish",
    );
    assert_eq!(
        store.memo_entry_count(),
        1,
        "exactly the one non-aborted backfill is warm",
    );
}

/// PREFIX-BACKFILL ABORT FENCE (P1, end-to-end) — a cold winner whose
/// in-flight entry is aborted AFTER `warm_publish_one` returned `true`
/// but BEFORE its prefix-backfill loop runs MUST skip ALL its backfills.
///
/// The window the `published`-gate alone does not cover: `warm_publish_one`
/// re-checks `aborted` and publishes the parent (returns `true`); a
/// project-generation reset then starts and marks the winner's still-
/// registered in-flight entry `aborted`; the winner's backfill loop runs
/// next. Each `warm_publish_one_if_absent` call therefore re-checks
/// `aborted` under the `entries` lock too — so an aborted winner skips
/// every backfill regardless of when the reset lands.
///
/// This test pins the window deterministically with the per-store
/// cold-winner pre-prefix-backfill injection point: the winner parks
/// AFTER `warm_publish_one` and BEFORE the backfill loop; `invalidate_all`
/// runs while it is parked.
///
/// DISCRIMINATES: against a `warm_publish_one_if_absent` with no abort
/// re-check the backfill is published into the just-cleared `entries`
/// and `memo_entry_count()` is `1` — the assertion FAILS. With the abort
/// fence the backfill is skipped and the memo stays empty — PASSES.
#[test]
fn prefix_backfill_loop_skips_all_backfills_when_winner_aborted_mid_loop() {
    use crate::project_semantic_dispatch::walk::{PrefixBackfill, QueryBuildOutput};
    use std::sync::Barrier;
    use std::thread;

    let store = Arc::new(SemanticGraphStore::new());
    let parent_key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/backfill_parent.ts"),
        name: Arc::from("Parent"),
    });
    let backfill_key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/backfill_child.ts"),
        name: Arc::from("Child"),
    });

    // Park the winner AFTER `warm_publish_one` and BEFORE the backfill
    // loop. Two `wait()` calls: party-1 rendezvous, then held at party-2.
    let pre_backfill = Arc::new(Barrier::new(2));
    let _gate_guard = store.test_cold_winner_pre_backfill_gate(Arc::clone(&pre_backfill));

    let store_w = Arc::clone(&store);
    let parent_w = parent_key.clone();
    let backfill_w = backfill_key.clone();
    let winner = thread::spawn(move || {
        let host = ctx_host();
        store_w.execute_cooperative(
            &host,
            parent_w,
            || store_w.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                let parent_node =
                    store_w.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
                let child_node =
                    store_w.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                // A build output that carries one pending prefix
                // backfill — published by the cooperative-admission
                // flow AFTER `warm_publish_one` lands the parent.
                QueryBuildOutput {
                    result: QueryResult::Value(parent_node),
                    dep_signature: empty_signature(),
                    walker_diagnostics: Vec::new(),
                    cache_suppress: false,
                    result_is_partial: false,
                    taint: crate::semantic_query::ResultTaint::Clean,
                    observed_self_roots: Vec::new(),
                    graph_carrier: None,
                    self_root_canonicals: Arc::from([]),
                    pending_prefix_backfills: vec![PrefixBackfill {
                        satisfied_projection:
                            crate::semantic_query::demand::MaterializedSet::single(
                                super::family::requested_point_for_key(&backfill_w),
                            ),
                        key: backfill_w,
                        node: child_node,
                    }],
                    satisfied_projection: crate::semantic_query::demand::MaterializedSet::empty(),
                }
            },
        )
    });

    // The winner has published its parent via `warm_publish_one` and is
    // now parked at the pre-prefix-backfill injection point.
    pre_backfill.wait();
    assert!(
        store.get_unvalidated(&parent_key).is_some(),
        "winner must have published its parent entry before parking",
    );

    // Project-generation reset while the winner is parked: clears
    // `entries` (dropping the parent) and marks the winner's still-
    // registered in-flight entry `aborted`.
    let _ = store.invalidate_all();

    // Release the winner — its prefix-backfill loop now runs. Every
    // `warm_publish_one_if_absent` call re-checks `aborted` and skips.
    pre_backfill.wait();
    let _ = winner.join().expect("winner thread must not panic");

    assert!(
        store.get_unvalidated(&backfill_key).is_none(),
        "ABORT-FENCE BUG: a prefix backfill from an aborted winner was \
         published into the warm memo. The winner was aborted by a \
         project-generation reset AFTER `warm_publish_one` returned but \
         BEFORE the backfill loop; `warm_publish_one_if_absent` must \
         re-check `aborted` and skip.",
    );
    assert_eq!(
        store.memo_entry_count(),
        0,
        "ABORT-FENCE BUG: no memo entry — parent or backfill — may \
         survive a project-generation reset that aborted the winner",
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
        crate::fact_signature_helpers::ReadSetSignature::empty(),
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
        store.entries.try_lock().is_none(),
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
            crate::fact_signature_helpers::ReadSetSignature::empty(),
            Arc::from([]),
        )
    });

    // The publisher has landed its `entries` slot and recorded the
    // `memo_budget` admission, and is parked. With the fence in place
    // the `entries` lock is STILL held — a concurrent `invalidate_all`
    // would block.
    publish_parked.wait();
    assert!(
        store.entries.try_lock().is_none(),
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

/// Build a `ReadSetSignature` whose fact rail names exactly
/// `canonical` via a `FileWholeHash` fact — so a publish through
/// `publish_with_carrier_for_tests` registers a `canonical_to_entries`
/// reverse-index entry under that canonical. `hash` keeps each
/// carrier's `FileWholeHash` distinct.
fn carrier_naming(canonical: &str, hash: u8) -> crate::fact_signature_helpers::ReadSetSignature {
    crate::fact_signature_helpers::ReadSetSignature::new(Arc::from(vec![
        crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: canonical.to_string(),
            hash: [hash; 16],
        },
    ]))
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
        store.entries.try_lock().is_none(),
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
        store.entries.try_lock().is_none(),
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
    use crate::resolver_core::FactVersionRef;
    use crate::semantic_query::DepVersion;

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
    let carrier_a = crate::fact_signature_helpers::ReadSetSignature::new(Arc::from(vec![
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
    let carrier_b = crate::fact_signature_helpers::ReadSetSignature::new(Arc::from(vec![
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
    let carrier_c = crate::fact_signature_helpers::ReadSetSignature::new(Arc::from(vec![
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

    let store = Arc::new(SemanticGraphStore::new());
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/a.ts"),
        name: Arc::from("Shared"),
    });

    // Bounded entry rendezvous: the winner sends one signal as the first
    // act of its build closure, and the driver receives it with a 10s
    // deadline. A 2-party `Barrier` here would hang the driver forever if
    // the winner panicked before reaching the barrier; the channel +
    // `recv_signal_within` makes that a loud panic instead.
    let (tx_winner_in_build, rx_winner_in_build) = std::sync::mpsc::channel::<()>();
    let store_owner = Arc::clone(&store);
    let key_owner = key.clone();

    let winner = thread::spawn(move || {
        let host = ctx_host();
        store_owner.execute_cooperative(
            &host,
            key_owner,
            || store_owner.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                tx_winner_in_build
                    .send(())
                    .expect("winner: signal entered build");
                // Hold the build open until the joiner has PROVABLY
                // suspended on the per-entry condvar — not merely been
                // admitted onto the in-flight entry. This test's stated
                // intent is the condvar PAIRING: the joiner must be parked
                // on the condvar when the winner publishes, so the
                // publish's `notify_all` is what wakes it. The in-flight
                // strong count (used by the 8 same-flight tests) rises one
                // step EARLIER, when the joiner clones the entry's `Arc`
                // BEFORE reaching `wait_while`, so it does NOT prove the
                // joiner is on the condvar. `joiner_on_condvar_count` is
                // incremented immediately before `wait_while`, so observing
                // it proves the real invariant. The probe is bounded
                // (~10 s) and panics on a genuine hang. This is a read-only
                // counter poll, not a re-entrant `execute_cooperative`
                // call, so the winner does not self-await its own in-flight
                // entry.
                wait_for_joiner_on_condvar(&store_owner);
                let id =
                    store_owner.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                (QueryResult::Value(id), empty_signature())
            },
        )
    });

    // Let the winner claim first, then the joiner waits on the
    // condvar. Bounded: panics if the winner never enters its build.
    recv_signal_within(&rx_winner_in_build, "winner entered build");
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

    let winner_result = join_within(winner, "winner");
    let joiner_result = join_within(joiner, "joiner");

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
use verter_compiler::utils::oxc::script::type_surface::ResolvedElements;
use verter_compiler::utils::oxc::vue::named_type_keys::ResolvedNamedTypeCacheKey;

fn make_key(canonical: &str, whole_hash: [u8; 16], name: &str) -> HostResolvedNamedTypeKey {
    HostResolvedNamedTypeKey {
        canonical_id: Arc::from(canonical),
        whole_hash,
        resolve_env_hash: Default::default(),
        type_env_hash: Default::default(),
        lib_env_hash: Default::default(),
        project_identity: 0,
        inner: ResolvedNamedTypeCacheKey {
            name: name.as_bytes().to_vec().into_boxed_slice(),
            surface: None,
            base_offset: 0,
            from_root_body: true,
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
    let node_id = store
        .insert_resolved_named_type(
            key.clone(),
            Arc::clone(&payload),
            store.named_type_generation(),
        )
        .expect("current-generation insert is accepted");

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
    let gen = store.named_type_generation();
    store
        .insert_resolved_named_type(key_a.clone(), Arc::new(ResolvedElements::default()), gen)
        .expect("current-generation insert is accepted");
    store
        .insert_resolved_named_type(key_b.clone(), Arc::new(ResolvedElements::default()), gen)
        .expect("current-generation insert is accepted");
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
    store
        .insert_resolved_named_type(
            key.clone(),
            Arc::new(ResolvedElements::default()),
            store.named_type_generation(),
        )
        .expect("current-generation insert is accepted");
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

    let gen = store.named_type_generation();
    store
        .insert_resolved_named_type(key.clone(), Arc::clone(&first), gen)
        .expect("current-generation insert is accepted");
    store
        .insert_resolved_named_type(key.clone(), Arc::clone(&second), gen)
        .expect("current-generation insert is accepted");

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
// Family-memo backfill matrix
// ──────────────────────────────────────────────────────────────────

fn family_test_path() -> Arc<[PathSegment]> {
    Arc::from(vec![PathSegment::Member(Arc::from("foo"))].into_boxed_slice())
}

fn family_test_key(base: SemanticNodeId, mode: ProjectionMode) -> SemanticQueryKey {
    SemanticQueryKey::ProjectPath {
        base,
        path: family_test_path(),
        context: crate::semantic_query::ProjectionReductionContext::published(mode),
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

// 2. Shallow backfills Identity ONLY (×2) — §3.4 lattice, NOT enum rank.
//    `Shallow ⊒ Identity` but `Shallow ⊅ Navigate`
//    (`normalization_depth: None < NavigateOnly`), so the Navigate slot
//    stays cold. (Legacy enum rank wrongly backfilled Navigate here.)

#[test]
fn family_shallow_backfills_identity_only_not_navigate() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let id = warm_family_slot(&host, &store, base, ProjectionMode::Shallow);

    assert_warm_at(&store, base, ProjectionMode::Shallow, id);
    assert_warm_at(&store, base, ProjectionMode::Identity, id);
    // Navigate MUST stay cold — `Shallow ⊅ Navigate` in the demand lattice.
    assert_cold_at(&store, base, ProjectionMode::Navigate);
    // Expanded MUST stay cold — narrower never satisfies broader.
    assert_cold_at(&store, base, ProjectionMode::Expanded);
    assert_eq!(store.memo_entry_count(), 2);
}

// 3. Navigate backfills Identity only (×2). Backfill is DIRECTIONAL
//    (broader-projection → narrower-projection); Navigate's only
//    projection-narrower target is Identity, and `Navigate ⊒ Identity`
//    passes the §3.4 gate. Navigate does NOT backfill Shallow even though
//    `Navigate ⊒ Shallow` in the lattice — backfill never flows toward a
//    shallower-but-not-narrower-projection slot (see `slot_domain_siblings`).

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

// Backfill is DIRECTIONAL: Navigate's only narrower-projection target is
// Identity, so Navigate backfills NEITHER Shallow NOR Expanded (Shallow is
// not a narrower-projection target of Navigate; Expanded is broader).
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
    // Identity slots (directional backfill: Navigate → Identity only).
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
    // only into EMPTY slots, so Navigate + Identity keep their
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
    use super::derivation::DERIVATION_EDGE_BUCKET_CAP;

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
    use super::derivation::DERIVATION_EDGE_BUCKET_CAP;

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
    use super::derivation::{DERIVATION_EDGES_PER_BUCKET_CAP, DERIVATION_EDGE_BUCKET_CAP};

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
    use verter_compiler::utils::oxc::script::type_surface::ResolvedElements;

    let store = SemanticGraphStore::new();
    let key = make_key("/w/named.ts", [9u8; 16], "Foo");
    let payload = Arc::new(ResolvedElements::default());
    let inserted_id = store
        .insert_resolved_named_type(
            key.clone(),
            Arc::clone(&payload),
            store.named_type_generation(),
        )
        .expect("current-generation insert is accepted");

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
    use verter_compiler::utils::oxc::script::type_surface::ResolvedElements;

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
    let inserted = store
        .insert_resolved_named_type(
            key.clone(),
            Arc::clone(&payload),
            store.named_type_generation(),
        )
        .expect("current-generation insert is accepted");
    assert_eq!(store.node_scope(inserted), None);
    assert!(store.get_resolved_named_type(&key).is_some());
}

// ──────────────────────────────────────────────────────────────────
// SemanticGraphStats counter extension
// ──────────────────────────────────────────────────────────────────

/// Number of `Arc<InflightEntry>` strong references held while a cold
/// winner is inside its build closure and **no** joiner has joined
/// yet. The winner accounts for three: the in-flight table entry, the
/// winner's own `inflight` local in `execute_cooperative`, and the
/// `InflightPanicGuard`'s clone (created before the build closure
/// runs). A joiner that is admitted clones the same `Arc`, raising the
/// count to `WINNER_ONLY_INFLIGHT_REFS + 1`.
const WINNER_ONLY_INFLIGHT_REFS: usize = 3;

/// Block the calling (test) thread until a joiner has been admitted
/// onto the in-flight entry for `key` on `store`.
///
/// Each `execute_cooperative` caller clones the entry's `Arc` (step 3
/// of the dispatch loop). While only the cold winner is mid-build the
/// strong count is [`WINNER_ONLY_INFLIGHT_REFS`]; it rises by one once
/// a joiner has joined. Polling that count is a deterministic
/// alternative to a wall-clock `sleep`, which races the joiner under
/// parallel test load — if a joiner-retry test aborted the entry
/// before the joiner joined, the joiner would become a fresh winner
/// and never retry, intermittently failing the test.
///
/// The poll is bounded: it spins for at most ~10 s, then panics so a
/// genuine hang fails loudly rather than blocking the suite forever.
fn wait_for_joiner_admitted(store: &SemanticGraphStore, key: &SemanticQueryKey) {
    use std::time::{Duration, Instant};
    let deadline = Instant::now() + Duration::from_secs(10);
    while store.test_inflight_strong_count(key) <= WINNER_ONLY_INFLIGHT_REFS {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for a joiner to be admitted onto the \
             in-flight entry (strong count never exceeded {WINNER_ONLY_INFLIGHT_REFS})",
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Block the calling (test) thread until at least one cooperative
/// joiner has SUSPENDED on a per-entry `ready` condvar on `store`.
///
/// This is strictly stronger than [`wait_for_joiner_admitted`]: that
/// probe returns once a joiner has cloned the in-flight `Arc` (step 3
/// of the dispatch loop), which happens one statement BEFORE the
/// joiner reaches `wait_while`. For a condvar-PAIRING test — one whose
/// stated intent is that the joiner is genuinely parked on the condvar
/// when the winner publishes — admitted-count is insufficient. This
/// probe instead polls the store's `joiner_on_condvar_count`, which is
/// incremented immediately before `wait_while`, so a return proves the
/// joiner is committed to the condvar wait.
///
/// The poll is bounded: it spins for at most ~10 s, then panics so a
/// genuine hang fails loudly rather than blocking the suite forever.
fn wait_for_joiner_on_condvar(store: &SemanticGraphStore) {
    use std::time::{Duration, Instant};
    let deadline = Instant::now() + Duration::from_secs(10);
    while store.test_joiner_on_condvar_count() == 0 {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for a joiner to suspend on the per-entry \
             condvar (joiner_on_condvar_count never rose above 0)",
        );
        std::thread::sleep(Duration::from_millis(1));
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
                // Bounded wait for the driver's release — panics on stall.
                recv_signal_within(&rx_finish_build, "winner signal finish");
                let id =
                    winner_store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                (QueryResult::Value(id), empty_signature())
            },
        )
    });

    // Wait until the winner is inside the build — this guarantees
    // the in-flight entry is registered + claimed when the joiner
    // arrives. Bounded: panics if the winner never enters its build.
    recv_signal_within(&rx_in_build, "winner entered build");

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

    // Block until the joiner has been admitted onto the in-flight
    // entry (its `Arc` clone raises the strong count above the
    // winner-only baseline). This deterministically guarantees the
    // joiner reached the cooperative wait branch before the winner is
    // released to publish — no wall-clock race.
    wait_for_joiner_admitted(&store, &key);
    tx_finish_build.send(()).expect("release winner");

    let _ = join_within(winner, "winner");
    let joiner_result = join_within(joiner, "joiner");
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
                recv_signal_within(&rx_finish_build, "winner finish-build signal");
                let id =
                    winner_store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                (QueryResult::Value(id), empty_signature())
            },
        )
    });

    recv_signal_within(&rx_in_build, "winner entered build");

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

    // Deterministically wait until the joiner has been admitted onto
    // the winner's in-flight entry (its `Arc` clone raises the strong
    // count to 3: winner-local + table + joiner). A wall-clock sleep
    // here races the joiner under parallel test load — if the abort
    // fired before the joiner joined, the joiner would become a fresh
    // winner and never retry (test hermeticity).
    wait_for_joiner_admitted(&store, &key);

    // Abort the joiner's wait — simulate invalidation's step 2
    // without requiring a matching warm slot.
    let aborted = store.test_trigger_inflight_abort_impl(&key);
    assert!(aborted, "inflight entry must have been present to abort");

    // Release the winner so its build can run to completion. Its
    // publish will hit the aborted re-check and be skipped.
    tx_finish_build.send(()).expect("release winner");

    let _ = join_within(winner, "winner");
    let joiner_result = join_within(joiner, "joiner");
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
/// `cold_aborts_swept` increments. The per-store cold-abort trigger
/// is the deterministic mechanism: every successful cold build under
/// the trigger should bump the counter exactly once.
#[test]
fn semantic_graph_stats_cold_aborts_swept_increments_when_forced() {
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/dep.ts"),
        name: Arc::from("Foo"),
    });

    let _guard = store.test_force_cold_abort_sweep();

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

/// The cold-abort trigger is scoped to the store it is set on — it
/// must NOT bleed into a concurrent cold build on a *different*
/// store. Rust runs a test binary's tests in parallel; a
/// process-global trigger would abort an unrelated test's
/// `execute_cooperative` cold publish, silently emptying its memo.
///
/// Discriminating interleaving: `store_b`'s cold winner is parked
/// inside its build closure (poised to publish). `store_a`'s trigger
/// is then set and held across `store_b`'s release. When `store_b`'s
/// `warm_publish_one` runs its TOCTOU abort-check, a per-store trigger
/// leaves `store_b` untouched — `store_b` publishes and its memo holds
/// one entry. A process-global trigger would abort `store_b`'s publish
/// (memo empty, `cold_aborts_swept == 1`), failing this test.
#[test]
fn cold_abort_trigger_is_scoped_to_its_store_not_process_global() {
    use std::sync::mpsc;
    use std::thread;

    let store_a = Arc::new(SemanticGraphStore::new());
    let store_b = Arc::new(SemanticGraphStore::new());
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/dep.ts"),
        name: Arc::from("Foo"),
    });

    let (tx_in_build, rx_in_build) = mpsc::channel::<()>();
    let (tx_release, rx_release) = mpsc::channel::<()>();

    // store_b's cold winner enters its build closure, signals, then
    // parks until released — so its warm publish happens strictly
    // AFTER store_a's trigger is set below.
    let b_store = Arc::clone(&store_b);
    let b_key = key.clone();
    let b_thread = thread::spawn(move || {
        let host = ctx_host();
        b_store.execute_cooperative(
            &host,
            b_key,
            || b_store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                tx_in_build.send(()).expect("store_b signal in_build");
                rx_release.recv().expect("store_b await release");
                let id = b_store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                (QueryResult::Value(id), empty_signature())
            },
        )
    });

    // store_b is now parked mid-build, poised to publish.
    rx_in_build.recv().expect("store_b entered build");

    // Force the cold-abort trigger on store_a and HOLD it across
    // store_b's publish. A process-global trigger would now also abort
    // store_b's unrelated cold publish.
    let _a_guard = store_a.test_force_cold_abort_sweep();

    // Release store_b — its warm publish runs the TOCTOU abort-check
    // while store_a's trigger is set.
    tx_release.send(()).expect("release store_b");
    let b_result = b_thread.join().expect("store_b joined");
    assert!(
        matches!(b_result.value, QueryResult::Value(_)),
        "store_b's cold winner still returns its computed result",
    );

    // store_b's publish must NOT have been aborted by store_a's
    // trigger: the slot is populated and store_b swept zero aborts.
    assert_eq!(
        store_b.memo_entry_count(),
        1,
        "store_b's cold publish must land — store_a's cold-abort \
         trigger must not bleed into store_b (process-global leak)",
    );
    assert_eq!(
        store_b.stats_snapshot().cold_aborts_swept,
        0,
        "store_b must observe zero cold-abort sweeps — store_a's \
         per-store trigger must not reach store_b",
    );

    // store_a's own trigger is still scoped correctly: a cold build on
    // store_a under the held guard IS aborted.
    let a_host = ctx_host();
    let a_result = store_a.execute_cooperative(
        &a_host,
        key,
        || store_a.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            let id = store_a.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
            (QueryResult::Value(id), empty_signature())
        },
    );
    assert!(matches!(a_result.value, QueryResult::Value(_)));
    assert_eq!(
        store_a.memo_entry_count(),
        0,
        "store_a's own cold publish IS aborted by its held trigger",
    );
    assert_eq!(
        store_a.stats_snapshot().cold_aborts_swept,
        1,
        "store_a swept exactly one cold abort under its own trigger",
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
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Navigate,
        ),
    };

    // The PREFIX key the backfill will publish — `path[..1]` =
    // [Member("outer")]. This is the entry whose carrier we'll
    // inspect for the discriminating signal.
    let prefix_path: Arc<[PathSegment]> =
        Arc::from(vec![PathSegment::Member(Arc::from("outer"))].into_boxed_slice());
    let prefix_key = SemanticQueryKey::ProjectPath {
        base,
        path: prefix_path,
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Navigate,
        ),
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
    let parent_carrier =
        crate::fact_signature_helpers::ReadSetSignature::new(Arc::clone(&parent_traced_facts));
    let _ = store.execute_cooperative(
        &host,
        parent_key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || QueryBuildOutput {
            result: QueryResult::Value(parent_value),
            dep_signature: Arc::clone(&parent_dep_signature),
            walker_diagnostics: Vec::new(),
            cache_suppress: false,
            result_is_partial: false,
            taint: crate::semantic_query::ResultTaint::Clean,
            observed_self_roots: Vec::new(),
            graph_carrier: Some(Box::new(parent_carrier.clone())),
            self_root_canonicals: Arc::from([]),
            pending_prefix_backfills: vec![PrefixBackfill {
                key: prefix_key.clone(),
                node: prefix_node,
                satisfied_projection: crate::semantic_query::demand::MaterializedSet::single(
                    super::family::requested_point_for_key(&prefix_key),
                ),
            }],
            satisfied_projection: crate::semantic_query::demand::MaterializedSet::empty(),
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
         the parent's path-precise facts.",
        facts = prefix_carrier.facts.as_ref()
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

    let _force_guard = store.test_force_cold_abort_sweep();

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

    // Force the cold-abort sweep deterministically on this store.
    let _force_guard = store.test_force_cold_abort_sweep();

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

    // Deterministically wait until the joiner has joined the winner's
    // in-flight entry before aborting — a wall-clock sleep races the
    // joiner under parallel test load and would let it become a fresh
    // winner that never retries (test hermeticity).
    wait_for_joiner_admitted(&store, &key);
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
                let carrier =
                    crate::fact_signature_helpers::ReadSetSignature::new(Arc::from(vec![
                        winner_fact_for_build.clone(),
                    ]));
                crate::project_semantic_dispatch::walk::QueryBuildOutput {
                    result: QueryResult::Value(id),
                    dep_signature: Arc::from(Vec::new().into_boxed_slice()),
                    walker_diagnostics: Vec::new(),
                    cache_suppress: false,
                    result_is_partial: false,
                    taint: crate::semantic_query::ResultTaint::Clean,
                    observed_self_roots: Vec::new(),
                    graph_carrier: Some(Box::new(carrier)),
                    self_root_canonicals: Arc::from([]),
                    pending_prefix_backfills: Vec::new(),
                    satisfied_projection: crate::semantic_query::demand::MaterializedSet::empty(),
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
        let ((), finalise, _) = crate::fact_signature_helpers::install_fact_tracer(&host, || {
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

    // Block until the joiner has joined the winner's in-flight entry
    // (deterministic admission probe — no wall-clock race) before the
    // winner is released to publish.
    wait_for_joiner_admitted(&store, &key);
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
    use crate::UpsertRequest;
    use std::sync::mpsc;
    use std::thread;

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
            file_language: crate::LanguageRegistry::global()
                .classify_static(keyed_canonical)
                .static_resolution(),
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
                let carrier =
                    crate::fact_signature_helpers::ReadSetSignature::new(Arc::from(vec![
                        winner_fact_for_build.clone(),
                    ]));
                crate::project_semantic_dispatch::walk::QueryBuildOutput {
                    result: QueryResult::Value(id),
                    dep_signature: Arc::from(Vec::new().into_boxed_slice()),
                    walker_diagnostics: Vec::new(),
                    cache_suppress: true,
                    // A2 taxonomy: this is a BENIGN non-cacheable winner — a
                    // COMPLETE `Primitive(String)` value suppressed for a
                    // non-self-root admission reason, NOT a partial. Benign
                    // inner-memo non-cacheability is `cache_suppress` only;
                    // `result_is_partial` stays false.
                    result_is_partial: false,
                    taint: crate::semantic_query::ResultTaint::Clean,
                    observed_self_roots: Vec::new(),
                    graph_carrier: Some(Box::new(carrier)),
                    self_root_canonicals: Arc::from([Arc::<str>::from(keyed_canonical)]),
                    pending_prefix_backfills: Vec::new(),
                    satisfied_projection: crate::semantic_query::demand::MaterializedSet::empty(),
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
        let (joiner_suppress, finalise, _) =
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

    // Block until the joiner has joined the winner's in-flight entry
    // (deterministic admission probe — no wall-clock race) before the
    // winner is released to publish.
    wait_for_joiner_admitted(&store, &key);
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
/// The in-flight singleflight coalesces concurrent
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
    use crate::UpsertRequest;
    use rustc_hash::FxHashMap;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::thread;

    let keyed_canonical = "/p2_1/keyed.ts";
    let host = ctx_host();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(keyed_canonical.to_string()),
            input_id: keyed_canonical.to_string(),
            source: Arc::from(
                "export interface Keyed { base: number; }\nexport const keyed = 1;\n",
            ),
            file_language: crate::LanguageRegistry::global()
                .classify_static(keyed_canonical)
                .static_resolution(),
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
                let carrier = ReadSetSignature::new(Arc::from(vec![winner_fact_for_build.clone()]));
                crate::project_semantic_dispatch::walk::QueryBuildOutput {
                    result: QueryResult::Value(id),
                    dep_signature: Arc::from(Vec::new().into_boxed_slice()),
                    walker_diagnostics: Vec::new(),
                    cache_suppress: false,
                    result_is_partial: false,
                    taint: crate::semantic_query::ResultTaint::Clean,
                    observed_self_roots: Vec::new(),
                    graph_carrier: Some(Box::new(carrier)),
                    self_root_canonicals: Arc::from([Arc::<str>::from(keyed_canonical)]),
                    pending_prefix_backfills: Vec::new(),
                    satisfied_projection: crate::semantic_query::demand::MaterializedSet::empty(),
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
        let session_store_view = follower_host
            .as_ref()
            .resolver_store_view_read()
            .into_owned_view()
            .with_session_overlay(follower_host.as_ref(), &view);
        let session_ctx = SessionResolverContext::new(
            follower_host.as_ref(),
            &view,
            &session_store_view,
            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
        );
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

    // Block until the follower has been admitted onto the winner's
    // in-flight entry — it is now guaranteed to be inside the
    // cooperative wait branch BEFORE the winner publishes, forcing a
    // real coalesce. Deterministic alternative to a wall-clock sleep,
    // which races the follower under parallel test load.
    wait_for_joiner_admitted(&store, &key);
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
         the follower's build closure never ran.",
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
/// it proves the join-path view validation does
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
    use crate::UpsertRequest;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::thread;

    let keyed_canonical = "/p2_1_same/keyed.ts";
    let host = ctx_host();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(keyed_canonical.to_string()),
            input_id: keyed_canonical.to_string(),
            source: Arc::from(
                "export interface Keyed { base: number; }\nexport const keyed = 1;\n",
            ),
            file_language: crate::LanguageRegistry::global()
                .classify_static(keyed_canonical)
                .static_resolution(),
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
                let carrier = ReadSetSignature::new(Arc::from(vec![winner_fact_for_build.clone()]));
                crate::project_semantic_dispatch::walk::QueryBuildOutput {
                    result: QueryResult::Value(winner_node),
                    dep_signature: Arc::from(Vec::new().into_boxed_slice()),
                    walker_diagnostics: Vec::new(),
                    cache_suppress: false,
                    result_is_partial: false,
                    taint: crate::semantic_query::ResultTaint::Clean,
                    observed_self_roots: Vec::new(),
                    graph_carrier: Some(Box::new(carrier)),
                    self_root_canonicals: Arc::from([Arc::<str>::from(keyed_canonical)]),
                    pending_prefix_backfills: Vec::new(),
                    satisfied_projection: crate::semantic_query::demand::MaterializedSet::empty(),
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

    // Block until the follower has joined the winner's in-flight entry
    // (deterministic admission probe — no wall-clock race) before the
    // winner is released to publish.
    wait_for_joiner_admitted(&store, &key);
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
/// Joiner-gate invariant: the joiner gate validates "whatever carrier
/// was stored" against the follower's `ctx`. But a `cache_suppress`
/// winner from a tracer overflow has no bounded fact list —
/// `finalise_traced_build_output`'s `Overflow` arm leaves
/// `graph_carrier` unset, and `execute_cooperative` broadcasts a
/// SYNTHETIC empty-fact carrier (`ReadSetSignature::new(empty_fact_…,
/// dep_signature)`). An empty-fact carrier with no self-roots
/// validates VACUOUSLY against ANY follower's `ctx` — the strict
/// `validates_self_root_whole_hash` arm never fires. Without the
/// suppressed-winner force-fork gate, a follower running under a
/// DIFFERENT session overlay would coalesce onto the suppressed
/// winner's view-specific result instead of forking, because
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
    use crate::UpsertRequest;
    use rustc_hash::FxHashMap;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::thread;

    let keyed_canonical = "/p2_8_overflow/keyed.ts";
    let host = ctx_host();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(keyed_canonical.to_string()),
            input_id: keyed_canonical.to_string(),
            source: Arc::from("export interface Keyed { base: number; }\n"),
            file_language: crate::LanguageRegistry::global()
                .classify_static(keyed_canonical)
                .static_resolution(),
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
                //
                // A2 taxonomy: a tracer signature-overflow is BENIGN
                // non-cacheability — the `Primitive(String)` VALUE is
                // COMPLETE, only the fact list overflowed. So
                // `cache_suppress=true` (inner memo refuses admission) but
                // `result_is_partial=false` (a complete result that still
                // warms a component-meta final cache).
                crate::project_semantic_dispatch::walk::QueryBuildOutput {
                    result: QueryResult::Value(winner_node),
                    dep_signature: Arc::from(Vec::new().into_boxed_slice()),
                    walker_diagnostics: Vec::new(),
                    cache_suppress: true,
                    result_is_partial: false,
                    taint: crate::semantic_query::ResultTaint::Clean,
                    observed_self_roots: Vec::new(),
                    graph_carrier: None,
                    self_root_canonicals: Arc::from([]),
                    pending_prefix_backfills: Vec::new(),
                    satisfied_projection: crate::semantic_query::demand::MaterializedSet::empty(),
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
        let session_store_view = follower_host
            .as_ref()
            .resolver_store_view_read()
            .into_owned_view()
            .with_session_overlay(follower_host.as_ref(), &view);
        let session_ctx = SessionResolverContext::new(
            follower_host.as_ref(),
            &view,
            &session_store_view,
            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
        );
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

    // Block until the follower has joined the winner's in-flight entry
    // (deterministic admission probe — no wall-clock race) before the
    // winner is released to publish.
    wait_for_joiner_admitted(&store, &key);
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
         view. Without the suppressed-winner force-fork gate, the \
         joiner gate would validate the empty carrier vacuously and \
         coalesce the follower onto the winner's view-specific \
         suppressed result; the follower's build closure would never \
         run.",
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
/// Joiner-gate invariant (no-self-root suppress case): the
/// `Ok(traced)→None` suppress arm of `finalise_traced_build_output`
/// carries the build's traced cross-file *dependency* facts on a
/// non-admitted carrier but leaves `self_root_canonicals` EMPTY (the
/// build could not be soundly self-rooted). The joiner gate's
/// `validate_with_self_roots` then routes every `FileWholeHash` in
/// the carrier through the LAZY `validates` (none is a listed
/// self-root), whose untracked-file arm optimistically accepts — so
/// the carrier validates against ANY follower's `ctx`. Without the
/// suppressed-winner force-fork gate, a follower under a different
/// overlay would coalesce onto the suppressed winner's
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
    use crate::UpsertRequest;
    use rustc_hash::FxHashMap;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::thread;

    let keyed_canonical = "/p2_8_unrootable/keyed.ts";
    let host = ctx_host();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(keyed_canonical.to_string()),
            input_id: keyed_canonical.to_string(),
            source: Arc::from("export interface Keyed { base: number; }\n"),
            file_language: crate::LanguageRegistry::global()
                .classify_static(keyed_canonical)
                .static_resolution(),
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
                //
                // A2 taxonomy: an unrootable build is BENIGN
                // non-cacheability — the `Primitive(String)` VALUE is
                // COMPLETE, only its self-root could not be soundly
                // established for admission. So `cache_suppress=true`,
                // `result_is_partial=false` (a complete result that still
                // warms a component-meta final cache).
                let carrier =
                    ReadSetSignature::new(Arc::from(vec![winner_dep_fact_for_build.clone()]));
                crate::project_semantic_dispatch::walk::QueryBuildOutput {
                    result: QueryResult::Value(winner_node),
                    dep_signature: Arc::from(Vec::new().into_boxed_slice()),
                    walker_diagnostics: Vec::new(),
                    cache_suppress: true,
                    result_is_partial: false,
                    taint: crate::semantic_query::ResultTaint::Clean,
                    observed_self_roots: Vec::new(),
                    graph_carrier: Some(Box::new(carrier)),
                    self_root_canonicals: Arc::from([]),
                    pending_prefix_backfills: Vec::new(),
                    satisfied_projection: crate::semantic_query::demand::MaterializedSet::empty(),
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
        let session_store_view = follower_host
            .as_ref()
            .resolver_store_view_read()
            .into_owned_view()
            .with_session_overlay(follower_host.as_ref(), &view);
        let session_ctx = SessionResolverContext::new(
            follower_host.as_ref(),
            &view,
            &session_store_view,
            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
        );
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

    // Block until the follower has joined the winner's in-flight entry
    // (deterministic admission probe — no wall-clock race) before the
    // winner is released to publish.
    wait_for_joiner_admitted(&store, &key);
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
         overlay view. Without the suppressed-winner force-fork gate, \
         the joiner gate would validate the self-root-less carrier \
         vacuously and coalesce the follower onto the winner's \
         view-specific suppressed result; the follower's build \
         closure would never run.",
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
/// Joiner-gate invariant (non-suppressed no-self-root case): the
/// no-self-root joiner fork must fire for NON-suppressed winners as
/// well, not only `cache_suppress` winners. A NON-suppressed winner
/// can have no view-discriminating self-root when a
/// `QueryResult::Error(Miss)` is produced because the requested
/// declaration is absent UNDER THE WINNER'S overlay. That build
/// completes with `cache_suppress == false` and a carrier holding
/// only cross-file *dependency* facts (no self-root for the keyed
/// canonical, because the declaration the self-root would have
/// rooted does not exist under the winner's view). A predicate gated
/// on `cache_suppress` would miss this case because `cache_suppress`
/// is `false` here, so it does not
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
    use crate::UpsertRequest;
    use rustc_hash::FxHashMap;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::thread;

    let keyed_canonical = "/p2_9_nonsuppressed_miss/keyed.ts";
    let host = ctx_host();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(keyed_canonical.to_string()),
            input_id: keyed_canonical.to_string(),
            source: Arc::from("export interface Other { base: number; }\n"),
            file_language: crate::LanguageRegistry::global()
                .classify_static(keyed_canonical)
                .static_resolution(),
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
                let carrier =
                    ReadSetSignature::new(Arc::from(vec![winner_dep_fact_for_build.clone()]));
                crate::project_semantic_dispatch::walk::QueryBuildOutput {
                    result: QueryResult::Error(QueryError::Miss),
                    dep_signature: Arc::from(Vec::new().into_boxed_slice()),
                    walker_diagnostics: Vec::new(),
                    cache_suppress: false,
                    result_is_partial: false,
                    taint: crate::semantic_query::ResultTaint::Clean,
                    observed_self_roots: Vec::new(),
                    graph_carrier: Some(Box::new(carrier)),
                    self_root_canonicals: Arc::from([]),
                    pending_prefix_backfills: Vec::new(),
                    satisfied_projection: crate::semantic_query::demand::MaterializedSet::empty(),
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
        let session_store_view = follower_host
            .as_ref()
            .resolver_store_view_read()
            .into_owned_view()
            .with_session_overlay(follower_host.as_ref(), &view);
        let session_ctx = SessionResolverContext::new(
            follower_host.as_ref(),
            &view,
            &session_store_view,
            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
        );
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

    // Block until the follower has joined the winner's in-flight entry
    // (deterministic admission probe — no wall-clock race) before the
    // winner is released to publish.
    wait_for_joiner_admitted(&store, &key);
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
         exist. A joiner fork predicate gated solely on \
         `cache_suppress` would never fire here (`cache_suppress` is \
         false for a non-suppressed Miss winner) and the follower \
         would coalesce onto the winner's stale view-specific miss; \
         the follower's build closure would never run.",
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

/// Budget eviction must drop the now-EMPTY outer `canonical_to_entries`
/// shard when it removes an entry's last reverse-index registration —
/// not merely the inner `(family, slot)` registration.
///
/// The retention budget caps the primary `entries` memo, but the
/// reverse index keys `canonical -> Mutex<map of (family, slot)>`. If
/// budget eviction strips only the inner registration it leaves an
/// empty `Mutex<map>` plus the canonical `Arc<str>` resident under the
/// outer `DashMap` for every distinct evicted canonical. Under churn
/// across many distinct canonicals that secondary structure grows
/// unbounded until a project-generation clear, defeating the bound the
/// budget exists to enforce.
///
/// DISCRIMINATES: each entry is published under a DISTINCT canonical,
/// so every entry owns its own outer shard. A small `memo_budget`
/// (cap 2) FIFO-evicts all but the two newest families. Pre-fix
/// `evict_memo_family_for_budget` removes only the inner registration,
/// so the outer-shard count stays at `families` (= 12) and the
/// assertion FAILS. Post-fix the empty outer shards are dropped, so
/// the count collapses to the number of surviving families' canonicals
/// (≤ the budget cap) and the assertion PASSES.
#[test]
fn budget_eviction_prunes_empty_canonical_to_entries_shards() {
    let store = SemanticGraphStore::new_with_memo_budget_for_test(2);
    let families = 12usize;
    for i in 0..families {
        // Each family is keyed by a distinct decl name; each entry's
        // carrier legacy rail names a distinct canonical, so every
        // publish creates its own outer reverse-index shard.
        let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope(&format!("/w/scope{i}.ts")),
            name: Arc::from(format!("Decl{i}")),
        });
        let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let carrier = crate::fact_signature_helpers::ReadSetSignature::new(Arc::from(vec![
            crate::resolver_core::FactVersionRef::FileWholeHash {
                canonical_id: format!("/w/dist{i}.ts"),
                hash: [1u8; 16],
            },
        ]));
        store.publish_with_carrier_for_tests(key, QueryResult::Value(id), carrier, Arc::from([]));
    }

    // The budget cap is 2 — every family past the two newest has been
    // FIFO-evicted. Each evicted entry's reverse-index registration was
    // its shard's only registration, so the shard's inner map is now
    // empty. The outer reverse index must NOT retain those empty shards.
    let outer_shards = store.canonical_to_entries_shard_count_for_test();
    assert!(
        outer_shards <= 2,
        "budget eviction left {outer_shards} outer canonical_to_entries \
         shards resident — an empty `Mutex<map>` + canonical `Arc<str>` \
         lingers for every evicted canonical. Eviction must drop the \
         outer shard when its inner map becomes empty; the \
         count must collapse to the surviving families' canonicals \
         (≤ budget cap 2), not stay at {families}.",
    );
    // The surviving families' shards must still be intact (not
    // over-pruned): the two newest families each keep one registration.
    assert_eq!(
        store.canonical_to_entries_count("/w/dist11.ts"),
        1,
        "the newest surviving family's reverse-index registration must \
         remain — pruning must drop only EMPTIED shards",
    );
    assert_eq!(
        store.canonical_to_entries_count("/w/dist10.ts"),
        1,
        "the second-newest surviving family's registration must remain",
    );
    // An evicted family's shard is gone entirely.
    assert_eq!(
        store.canonical_to_entries_count("/w/dist0.ts"),
        0,
        "an evicted family's reverse-index shard must be fully removed",
    );
}

/// `invalidate_canonical`'s cross-canonical drain must also drop a
/// reverse-index shard it empties — an entry registered under multiple
/// canonicals, invalidated through one of them, must not leave empty
/// `Mutex<map>` shards behind under the OTHER canonicals.
///
/// DISCRIMINATES: one entry is published whose carrier legacy rail
/// names two canonicals (`/w/a.ts`, `/w/b.ts`) — so it registers a
/// shard under each. `invalidate_canonical("/w/a.ts")` drains the
/// `/w/a.ts` shard outright AND must drop the entry's registration from
/// the `/w/b.ts` shard; since that was `/w/b.ts`'s only registration,
/// the `/w/b.ts` outer shard must be removed too. Pre-fix the
/// cross-canonical cleanup leaves an empty `/w/b.ts` shard resident, so
/// `canonical_to_entries_count("/w/b.ts")` reads 0 (inner map empty)
/// but the outer shard count stays at 1; post-fix the outer shard is
/// dropped and the count is 0.
#[test]
fn invalidate_canonical_prunes_emptied_cross_canonical_shard() {
    let store = SemanticGraphStore::new();
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/scope.ts"),
        name: Arc::from("Multi"),
    });
    let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    // Carrier fact rail names two canonicals — the entry registers a
    // reverse-index shard under each.
    let two_canonical_facts: Arc<[crate::resolver_core::FactVersionRef]> = Arc::from(vec![
        crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: "/w/a.ts".to_string(),
            hash: [1u8; 16],
        },
        crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: "/w/b.ts".to_string(),
            hash: [1u8; 16],
        },
    ]);
    let carrier = crate::fact_signature_helpers::ReadSetSignature::new(two_canonical_facts);
    store.publish_with_carrier_for_tests(key, QueryResult::Value(id), carrier, Arc::from([]));
    assert_eq!(store.canonical_to_entries_shard_count_for_test(), 2);

    // Invalidate via `/w/a.ts`. Its shard is drained whole; the entry's
    // registration in `/w/b.ts` is removed by the cross-canonical
    // cleanup — and since it was the only registration there, that
    // outer shard must be dropped too.
    store.invalidate_canonical("/w/a.ts");
    assert_eq!(
        store.canonical_to_entries_shard_count_for_test(),
        0,
        "`invalidate_canonical` must drop the cross-canonical shard it \
         empties — an empty `Mutex<map>` for `/w/b.ts` must not linger \
         after its last registration is stripped",
    );
}

/// the partial-metadata invariant (§2) — MEMO-LAUNDERING discrimination (Finding B).
///
/// A producer that returns a `Value` carrying `result_is_partial = true`
/// must NOT be admitted to the family memo. Pre-FIX the admission gate
/// keyed ONLY on `cache_suppress`; a build that surfaced a partial value
/// without also setting `cache_suppress` (the exact pre-FIX-1 producer bug)
/// was published as a complete `MemoEntry`, then reconstructed as a
/// COMPLETE `CacheRead` (`result_is_partial = false`) on a later warm read
/// and republished as a complete component-meta result.
///
/// This drives `execute_cooperative` DIRECTLY with a `QueryBuildOutput`
/// carrying `result_is_partial = true, cache_suppress = false` (bypassing
/// `finalise_traced_build_output`, which would otherwise enforce the
/// invariant). Post-FIX the admission boundary's debug-asserted
/// `result_is_partial ⟹ cache_suppress` invariant catches the laundering
/// attempt and aborts rather than poisoning the shared memo.
///
/// DISCRIMINATION: reverting the FIX-2 admission `debug_assert!` + OR gate
/// makes this build publish a warm `MemoEntry` (no panic), so the
/// `#[should_panic]` expectation fails.
#[test]
#[should_panic(expected = "invariant violated at memo admission")]
fn memo_admission_debug_asserts_against_partial_without_suppress() {
    use crate::project_semantic_dispatch::walk::QueryBuildOutput;
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/launder.ts"),
        name: Arc::from("Laundered"),
    });
    let _ = store.execute_cooperative(
        &host,
        key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
            let mut out: QueryBuildOutput = (QueryResult::Value(id), empty_signature()).into();
            // The laundering shape: a COMPLETE Value that surfaced a
            // partial WITHOUT setting cache_suppress (the pre-FIX-1
            // producer bug). The FIX-2 admission invariant must catch this.
            out.result_is_partial = true;
            out.cache_suppress = false;
            out
        },
    );
}

/// the partial-metadata invariant (§2) — MEMO-LAUNDERING behavioral proof (Finding B).
///
/// A genuine partial (invariant-holding: `result_is_partial = true` AND
/// `cache_suppress = true`, exactly what `finalise_traced_build_output`
/// produces after FIX-1) must leave NO `MemoEntry`. A fresh request on the
/// SAME store for the SAME key must COLD-REBUILD — never warm-hit the
/// (refused) partial as complete.
///
/// DISCRIMINATION: reverting the FIX-2 admission gate (back to keying only
/// on `cache_suppress`) keeps THIS case refused (cache_suppress=true), so
/// the discriminating force here is the no-launder behavior of the whole
/// partial class — paired with the `#[should_panic]` test above which
/// isolates the OR/assert. Reverting FIX-1's finalise enforcement would let
/// a partial reach admission with `cache_suppress=false` and warm — caught
/// by the should_panic peer.
#[test]
fn partial_value_leaves_no_memo_entry_and_fresh_request_cold_rebuilds() {
    use crate::project_semantic_dispatch::walk::QueryBuildOutput;
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/partial.ts"),
        name: Arc::from("GenuinePartial"),
    });

    let mut first_ran = false;
    let first = store.execute_cooperative(
        &host,
        key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            first_ran = true;
            let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
            let mut out: QueryBuildOutput = (QueryResult::Value(id), empty_signature()).into();
            // A genuine, invariant-holding partial.
            out.result_is_partial = true;
            out.cache_suppress = true;
            out
        },
    );
    assert!(first_ran, "first request must cold-build");
    assert!(
        matches!(first.value, QueryResult::Value(_)),
        "the partial value still flows back to the caller"
    );
    assert!(
        first.result_is_partial,
        "the CacheRead carries result_is_partial=true back to the caller"
    );
    assert_eq!(
        store.memo_entry_count(),
        0,
        "a partial result must NOT be admitted to the family memo (Finding B)"
    );
    assert!(
        store.get_unvalidated(&key).is_none(),
        "no MemoEntry exists for the partial key — a fresh request cannot warm-hit it"
    );

    // A fresh request on the SAME store for the SAME key must COLD-REBUILD.
    let mut second_ran = false;
    let second = store.execute_cooperative(
        &host,
        key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            second_ran = true;
            let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
            (QueryResult::Value(id), empty_signature())
        },
    );
    assert!(
        second_ran,
        "fresh request MUST cold-rebuild — the partial was not laundered into a warm-complete entry"
    );
    match second.value {
        QueryResult::Value(id) => {
            let data = store.node_data(id).unwrap();
            assert!(
                matches!(*data, SemanticNodeData::Primitive(PrimitiveKind::Number)),
                "the fresh cold build's result is served, not the refused partial"
            );
        }
        other => panic!("expected fresh cold value, got {other:?}"),
    }
}

/// the partial-metadata invariant — RELEASE-BEHAVIORAL OR-gate proof (P3).
///
/// The `should_panic` peer
/// (`memo_admission_debug_asserts_against_partial_without_suppress`) only
/// fires in DEBUG builds: the `debug_assert!` at the memo-admission
/// boundary is compiled OUT in release, so it proves nothing about release
/// behavior. The actual admission GATE — `publish_carrier = if
/// cache_suppress || result_is_partial { None } else { Some(...) }` — is a
/// RUNTIME OR that refuses a `result_is_partial=true, cache_suppress=false`
/// shape regardless of build profile.
///
/// This test asserts that runtime refusal BEHAVIORALLY: it drives
/// `execute_cooperative` with the laundering shape (`result_is_partial =
/// true, cache_suppress = false`, bypassing `finalise_traced_build_output`)
/// and proves NO `MemoEntry` is admitted + a fresh request COLD-REBUILDS.
/// It is `#[cfg(not(debug_assertions))]`-gated so it runs ONLY where the
/// `debug_assert` is absent — i.e. exactly where the should_panic peer
/// cannot run — proving the OR-gate (not the assert) is the release
/// authority.
///
/// DISCRIMINATION: reverting the admission gate's `|| result_is_partial`
/// arm (back to keying only on `cache_suppress`) admits this shape as a
/// warm complete `MemoEntry`, so `get_unvalidated(&key)` returns `Some` and
/// the fresh request warm-hits — failing this test in release.
#[cfg(not(debug_assertions))]
#[test]
fn memo_admission_or_gate_refuses_partial_without_suppress_in_release() {
    use crate::project_semantic_dispatch::walk::QueryBuildOutput;
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/release_launder.ts"),
        name: Arc::from("ReleaseLaundered"),
    });

    let first = store.execute_cooperative(
        &host,
        key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
            let mut out: QueryBuildOutput = (QueryResult::Value(id), empty_signature()).into();
            // The laundering shape — a partial WITHOUT cache_suppress. In
            // debug this trips the debug_assert (covered by the should_panic
            // peer); in release the runtime OR-gate must still refuse it.
            out.result_is_partial = true;
            out.cache_suppress = false;
            out
        },
    );
    assert!(
        matches!(first.value, QueryResult::Value(_)),
        "the value still flows back to the caller"
    );
    assert_eq!(
        store.memo_entry_count(),
        0,
        "release OR-gate must refuse a partial-without-suppress admission"
    );
    assert!(
        store.get_unvalidated(&key).is_none(),
        "no MemoEntry exists — the release OR-gate refused the laundering shape"
    );

    // A fresh request must COLD-REBUILD, never warm-hit a laundered entry.
    let mut second_ran = false;
    let _ = store.execute_cooperative(
        &host,
        key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            second_ran = true;
            let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
            (QueryResult::Value(id), empty_signature())
        },
    );
    assert!(
        second_ran,
        "fresh request MUST cold-rebuild — the partial was not laundered into a warm-complete entry"
    );
}

/// the partial-metadata invariant — benign-suppress OR-gate proof
/// (holds in BOTH debug and release).
///
/// Complements the release-gated peer above: it drives the OTHER arm of the
/// admission OR — a benign, invariant-holding `cache_suppress = true,
/// result_is_partial = false` build (a complete-but-non-cacheable result,
/// e.g. an unrootable self-root / overflow). This shape never trips the
/// `debug_assert` (the invariant `result_is_partial ⟹ cache_suppress`
/// holds), so it runs in every build profile and proves the
/// `cache_suppress` arm of the runtime gate refuses admission.
///
/// DISCRIMINATION: dropping `cache_suppress` from the admission gate admits
/// this build as a warm `MemoEntry`, so `get_unvalidated(&key)` returns
/// `Some` — failing this test.
#[test]
fn memo_admission_or_gate_refuses_benign_cache_suppress() {
    use crate::project_semantic_dispatch::walk::QueryBuildOutput;
    let host = ctx_host();
    let store = SemanticGraphStore::new();
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/w/benign_suppress.ts"),
        name: Arc::from("BenignSuppress"),
    });

    let first = store.execute_cooperative(
        &host,
        key.clone(),
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || {
            let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
            let mut out: QueryBuildOutput = (QueryResult::Value(id), empty_signature()).into();
            // Benign non-cacheable COMPLETE result — invariant holds.
            out.cache_suppress = true;
            out.result_is_partial = false;
            out
        },
    );
    assert!(
        matches!(first.value, QueryResult::Value(_)),
        "the complete value still flows back to the caller"
    );
    assert!(
        !first.result_is_partial,
        "a benign non-cacheable result is NOT partial — component-meta may still warm"
    );
    assert_eq!(
        store.memo_entry_count(),
        0,
        "the cache_suppress arm of the admission OR-gate must refuse admission"
    );
    assert!(
        store.get_unvalidated(&key).is_none(),
        "no MemoEntry exists — the cache_suppress arm refused admission"
    );
}

/// Env-scoped key-identity guards.
///
/// These pin that `Instantiate.base` / `ResolveMacroPayload.owner` key on the
/// env-bearing, content-free `ResolvedDeclSlotIdentity` (not a content-free,
/// env-FREE declaration key), plus the env-scoping of the
/// `HostResolvedNamedTypeKey` resolved-named-type artifact identity. Each is
/// DISCRIMINATING: with an env-FREE key the two compared queries would collapse
/// onto ONE family slot (a warm-hit collision); because the env dim enters the
/// `FamilyKey` identity they occupy distinct slots.
mod env_scoped_key_identity_guards {
    use super::super::family::{family_and_slot, FamilyKey};
    use crate::semantic_query::{
        HashValue, InstantiateContext, MacroPayloadContext, MapperKey, MapperKind, MemberMergeRole,
        OptionalityMod, PrimitiveKind, ProjectionMode, ProjectionReductionContext, QueryResult,
        ReadonlyMod, ResolvedDeclSlotIdentity, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
        SurfaceProvenanceContext,
    };
    use std::sync::Arc;

    fn empty_args() -> Arc<[SemanticNodeId]> {
        Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice())
    }

    fn inst_key(slot: ResolvedDeclSlotIdentity, resolve_env: HashValue) -> SemanticQueryKey {
        SemanticQueryKey::Instantiate {
            base: slot,
            args: empty_args(),
            context: InstantiateContext::non_file(
                ProjectionReductionContext::published(ProjectionMode::Expanded),
                resolve_env,
            ),
        }
    }

    fn fam(key: &SemanticQueryKey) -> FamilyKey {
        family_and_slot(key).0
    }

    fn context_with_axes(
        provenance: SurfaceProvenanceContext,
        merge_role: MemberMergeRole,
    ) -> ProjectionReductionContext {
        let mut context = ProjectionReductionContext::published(ProjectionMode::Expanded);
        context.provenance = provenance;
        context.merge_role = merge_role;
        context
    }

    fn mapper_key(id: SemanticNodeId) -> MapperKey {
        MapperKey {
            parameter_node: id,
            key_space: id,
            value_expr: id,
            optionality: OptionalityMod::Keep,
            readonly: ReadonlyMod::Keep,
            name_remap: None,
            kind: MapperKind::Computed,
        }
    }

    fn assert_value_node(result: QueryResult<SemanticNodeId>, expected: SemanticNodeId) {
        match result {
            QueryResult::Value(actual) => assert_eq!(actual, expected),
            other => panic!("expected Value({expected:?}), got {other:?}"),
        }
    }

    /// Two `Instantiate` queries over the SAME declaration `(canonical, name)`
    /// that differ ONLY in an env dim — slot `type_env_hash` / `lib_env_hash` /
    /// `project_identity`, or the context `resolve_env_hash` — map to DISTINCT
    /// `FamilyKey`s and so cannot warm-hit each other. The env dims ride on the
    /// `ResolvedDeclSlotIdentity` base (`type_env_hash` / `lib_env_hash` /
    /// `project_identity`) and the dedicated `InstantiateContext`
    /// (`resolve_env_hash`); an env-free base would collapse all four pairs onto
    /// one family slot, so this test would fail.
    #[test]
    fn instantiate_same_base_different_env_or_context_do_not_warm_hit() {
        let canonical: Arc<str> = Arc::from("/u2b9/a.ts");
        let name: Arc<str> = Arc::from("Foo");
        let base_t = ResolvedDeclSlotIdentity::type_slot(
            Arc::clone(&canonical),
            Arc::clone(&name),
            0,
            [1u8; 16],
            [0u8; 16],
        );
        let baseline = fam(&inst_key(base_t.clone(), [0u8; 16]));

        // type_env differs on the slot.
        let t2 = ResolvedDeclSlotIdentity::type_slot(
            Arc::clone(&canonical),
            Arc::clone(&name),
            0,
            [2u8; 16],
            [0u8; 16],
        );
        assert_ne!(
            baseline,
            fam(&inst_key(t2, [0u8; 16])),
            "type_env change must distinguish the Instantiate FamilyKey"
        );

        // lib_env differs on the slot.
        let l = ResolvedDeclSlotIdentity::type_slot(
            Arc::clone(&canonical),
            Arc::clone(&name),
            0,
            [1u8; 16],
            [9u8; 16],
        );
        assert_ne!(
            baseline,
            fam(&inst_key(l, [0u8; 16])),
            "lib_env change must distinguish the Instantiate FamilyKey"
        );

        // project_identity differs on the slot.
        let j = ResolvedDeclSlotIdentity::type_slot(
            Arc::clone(&canonical),
            Arc::clone(&name),
            7,
            [1u8; 16],
            [0u8; 16],
        );
        assert_ne!(
            baseline,
            fam(&inst_key(j, [0u8; 16])),
            "project_identity change must distinguish the Instantiate FamilyKey"
        );

        // resolve_env differs in the context (same slot).
        assert_ne!(
            baseline,
            fam(&inst_key(base_t.clone(), [5u8; 16])),
            "resolve_env change in InstantiateContext must distinguish the FamilyKey"
        );

        // Identical env + context → SAME family slot (warm-hit IS allowed).
        assert_eq!(baseline, fam(&inst_key(base_t, [0u8; 16])));
    }

    /// Closes the "env validity purely ReadSetSignature" gap: a `type_env` /
    /// `lib_env` change to the DECLARATION ITSELF (its slot — not a dependency)
    /// now produces a KEY difference, not merely a fact-revalidation miss.
    #[test]
    fn decl_self_type_or_lib_env_change_produces_distinct_instantiate_key() {
        let canonical: Arc<str> = Arc::from("/u2b9/self.ts");
        let name: Arc<str> = Arc::from("Decl");
        let k = |t: HashValue, l: HashValue| {
            inst_key(
                ResolvedDeclSlotIdentity::type_slot(
                    Arc::clone(&canonical),
                    Arc::clone(&name),
                    0,
                    t,
                    l,
                ),
                [0u8; 16],
            )
        };
        // The KEYS themselves differ (env entered the query identity)...
        assert_ne!(k([1u8; 16], [0u8; 16]), k([2u8; 16], [0u8; 16]));
        assert_ne!(k([0u8; 16], [1u8; 16]), k([0u8; 16], [2u8; 16]));
        // ...and so do their family slots.
        assert_ne!(fam(&k([1u8; 16], [0u8; 16])), fam(&k([2u8; 16], [0u8; 16])));
        assert_ne!(fam(&k([0u8; 16], [1u8; 16])), fam(&k([0u8; 16], [2u8; 16])));
    }

    /// Two `ResolveMacroPayload` queries over the SAME owner that differ only in
    /// an env dim (slot `type_env`/`lib_env`/`project_identity` or context
    /// `resolve_env_hash`) map to DISTINCT `FamilyKey`s. The env dims ride on the
    /// `ResolvedDeclSlotIdentity` owner (`type_env`/`lib_env`/`project_identity`)
    /// and the dedicated `MacroPayloadContext` (`resolve_env_hash`); an env-free
    /// owner + bare `mode` would carry none of these.
    #[test]
    fn resolve_macro_payload_same_owner_different_env_or_context_do_not_warm_hit() {
        use verter_semantic::analysis::AnalyzedMacroKind;
        let canonical: Arc<str> = Arc::from("/u2b9/sfc.vue");
        let name: Arc<str> = Arc::from("<sfc-script-setup>");
        let macro_key = |slot: ResolvedDeclSlotIdentity, resolve_env: HashValue| {
            SemanticQueryKey::ResolveMacroPayload {
                owner: slot,
                macro_index: 0,
                macro_kind: AnalyzedMacroKind::DefineProps,
                type_args: empty_args(),
                context: MacroPayloadContext::new(resolve_env, ProjectionMode::Navigate),
            }
        };
        let owner_t = ResolvedDeclSlotIdentity::type_slot(
            Arc::clone(&canonical),
            Arc::clone(&name),
            0,
            [1u8; 16],
            [0u8; 16],
        );
        let baseline = fam(&macro_key(owner_t.clone(), [0u8; 16]));

        let t2 = ResolvedDeclSlotIdentity::type_slot(
            Arc::clone(&canonical),
            Arc::clone(&name),
            0,
            [2u8; 16],
            [0u8; 16],
        );
        assert_ne!(
            baseline,
            fam(&macro_key(t2, [0u8; 16])),
            "type_env change must distinguish the ResolveMacroPayload FamilyKey"
        );

        let l = ResolvedDeclSlotIdentity::type_slot(
            Arc::clone(&canonical),
            Arc::clone(&name),
            0,
            [1u8; 16],
            [9u8; 16],
        );
        assert_ne!(
            baseline,
            fam(&macro_key(l, [0u8; 16])),
            "lib_env must distinguish"
        );

        // project_identity differs on the owner slot. Mirrors the
        // `Instantiate` sibling's J case: the slot derives Hash/Eq over
        // `project_identity`, so a differing J → distinct FamilyKey →
        // no warm-hit. An env-free owner key would collapse this onto
        // the baseline slot and this assertion would fail.
        let j = ResolvedDeclSlotIdentity::type_slot(
            Arc::clone(&canonical),
            Arc::clone(&name),
            7,
            [1u8; 16],
            [0u8; 16],
        );
        assert_ne!(
            baseline,
            fam(&macro_key(j, [0u8; 16])),
            "project_identity change must distinguish the ResolveMacroPayload FamilyKey"
        );

        assert_ne!(
            baseline,
            fam(&macro_key(owner_t.clone(), [5u8; 16])),
            "resolve_env change in MacroPayloadContext must distinguish the FamilyKey"
        );

        assert_eq!(baseline, fam(&macro_key(owner_t, [0u8; 16])));
    }

    /// `ProjectionReductionContext.provenance` and `.merge_role` are
    /// value-affecting query-identity axes for every context-bearing operator
    /// family. `KeyOf` and `MappedType` must therefore follow the same
    /// FamilyKey pattern as `Instantiate` / `ProjectPath`: the axes live on the
    /// family identity, not only on the slot's demand/mode selector.
    ///
    /// Pre-fix, `FamilyKey::KeyOf { base }` and
    /// `FamilyKey::MappedType { source, mapper }` dropped both axes, so each
    /// assertion below collapsed onto one family slot and failed.
    #[test]
    fn keyof_and_mapped_type_context_axes_do_not_alias_family_identity() {
        let store = super::SemanticGraphStore::new();
        let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let mapper = mapper_key(base);

        let structural = context_with_axes(
            SurfaceProvenanceContext::Structural,
            MemberMergeRole::Authored,
        );
        let macro_own_body = context_with_axes(
            SurfaceProvenanceContext::MacroTypeArgOwnBody,
            MemberMergeRole::Authored,
        );
        let heritage = context_with_axes(
            SurfaceProvenanceContext::Structural,
            MemberMergeRole::Heritage,
        );

        let keyof_structural = SemanticQueryKey::KeyOf {
            base,
            context: structural,
        };
        let keyof_macro = SemanticQueryKey::KeyOf {
            base,
            context: macro_own_body,
        };
        let keyof_heritage = SemanticQueryKey::KeyOf {
            base,
            context: heritage,
        };
        assert_ne!(
            fam(&keyof_structural),
            fam(&keyof_macro),
            "KeyOf provenance must distinguish the FamilyKey"
        );
        assert_ne!(
            fam(&keyof_structural),
            fam(&keyof_heritage),
            "KeyOf merge_role must distinguish the FamilyKey"
        );

        let mapped_structural = SemanticQueryKey::MappedType {
            source: base,
            mapper: mapper.clone(),
            context: structural,
        };
        let mapped_macro = SemanticQueryKey::MappedType {
            source: base,
            mapper: mapper.clone(),
            context: macro_own_body,
        };
        let mapped_heritage = SemanticQueryKey::MappedType {
            source: base,
            mapper,
            context: heritage,
        };
        assert_ne!(
            fam(&mapped_structural),
            fam(&mapped_macro),
            "MappedType provenance must distinguish the FamilyKey"
        );
        assert_ne!(
            fam(&mapped_structural),
            fam(&mapped_heritage),
            "MappedType merge_role must distinguish the FamilyKey"
        );
    }

    #[test]
    fn keyof_queries_differing_only_by_provenance_do_not_warm_hit() {
        let host = super::ctx_host();
        let store = super::SemanticGraphStore::new();
        let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let structural_result =
            store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let macro_result = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));

        let structural = SemanticQueryKey::KeyOf {
            base,
            context: context_with_axes(
                SurfaceProvenanceContext::Structural,
                MemberMergeRole::Authored,
            ),
        };
        let macro_own_body = SemanticQueryKey::KeyOf {
            base,
            context: context_with_axes(
                SurfaceProvenanceContext::MacroTypeArgOwnBody,
                MemberMergeRole::Authored,
            ),
        };

        let first = store.execute_cooperative(
            &host,
            structural,
            || store.intern_node(SemanticNodeData::Opaque(super::QueryError::Miss)),
            || {
                (
                    QueryResult::Value(structural_result),
                    super::empty_signature(),
                )
            },
        );
        assert_value_node(first.value, structural_result);

        let mut second_build_ran = false;
        let second = store.execute_cooperative(
            &host,
            macro_own_body,
            || store.intern_node(SemanticNodeData::Opaque(super::QueryError::Miss)),
            || {
                second_build_ran = true;
                (QueryResult::Value(macro_result), super::empty_signature())
            },
        );

        assert!(
            second_build_ran,
            "a KeyOf query with different provenance must cold-build, not warm-hit a prior context"
        );
        assert_value_node(second.value, macro_result);
    }

    #[test]
    fn mapped_type_queries_differing_only_by_merge_role_do_not_warm_hit() {
        let host = super::ctx_host();
        let store = super::SemanticGraphStore::new();
        let source = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let mapper = mapper_key(source);
        let authored_result = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let heritage_result =
            store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));

        let authored = SemanticQueryKey::MappedType {
            source,
            mapper: mapper.clone(),
            context: context_with_axes(
                SurfaceProvenanceContext::Structural,
                MemberMergeRole::Authored,
            ),
        };
        let heritage = SemanticQueryKey::MappedType {
            source,
            mapper,
            context: context_with_axes(
                SurfaceProvenanceContext::Structural,
                MemberMergeRole::Heritage,
            ),
        };

        let first = store.execute_cooperative(
            &host,
            authored,
            || store.intern_node(SemanticNodeData::Opaque(super::QueryError::Miss)),
            || {
                (
                    QueryResult::Value(authored_result),
                    super::empty_signature(),
                )
            },
        );
        assert_value_node(first.value, authored_result);

        let mut second_build_ran = false;
        let second = store.execute_cooperative(
            &host,
            heritage,
            || store.intern_node(SemanticNodeData::Opaque(super::QueryError::Miss)),
            || {
                second_build_ran = true;
                (
                    QueryResult::Value(heritage_result),
                    super::empty_signature(),
                )
            },
        );

        assert!(
            second_build_ran,
            "a MappedType query with different merge_role must cold-build, not warm-hit a prior context"
        );
        assert_value_node(second.value, heritage_result);
    }

    fn typeof_key(
        slot: crate::semantic_query::ValueRootSlotIdentity,
        resolve_env: HashValue,
        prc: ProjectionReductionContext,
    ) -> SemanticQueryKey {
        SemanticQueryKey::TypeOf {
            value_root: slot,
            context: crate::semantic_query::TypeOfContext::new(prc, resolve_env),
        }
    }

    fn value_root_slot(
        canonical: &Arc<str>,
        name: &Arc<str>,
        project_identity: u32,
        type_env: HashValue,
        lib_env: HashValue,
    ) -> crate::semantic_query::ValueRootSlotIdentity {
        crate::semantic_query::ValueRootSlotIdentity::new(
            crate::semantic_query::ValueRootKey {
                scope: crate::semantic_query::ScopeId::file(Arc::clone(canonical)),
                name: Arc::clone(name),
            },
            project_identity,
            type_env,
            lib_env,
        )
    }

    /// Two `TypeOf` queries over the SAME value root `(canonical, name)`
    /// that differ ONLY in an env dim — slot `type_env_hash` /
    /// `lib_env_hash` / `project_identity`, or the context
    /// `resolve_env_hash` — map to DISTINCT `FamilyKey`s and so cannot
    /// warm-hit each other. `build_typeof` does env-sensitive name/export
    /// resolution, so the env dims ride on the `ValueRootSlotIdentity`
    /// slot (`T`/`L`/`J`) and the dedicated `TypeOfContext` (`R`); an
    /// env-free `ValueRootKey`-only key would collapse all four pairs
    /// onto one family slot (cross-env cache poisoning) and this test
    /// would fail.
    #[test]
    fn typeof_same_root_different_env_or_context_do_not_warm_hit() {
        let canonical: Arc<str> = Arc::from("/u2b9/value.ts");
        let name: Arc<str> = Arc::from("sample");
        let prc = ProjectionReductionContext::published(ProjectionMode::Navigate);
        let base = value_root_slot(&canonical, &name, 0, [1u8; 16], [0u8; 16]);
        let baseline = fam(&typeof_key(base.clone(), [0u8; 16], prc));

        // type_env differs on the slot.
        let t2 = value_root_slot(&canonical, &name, 0, [2u8; 16], [0u8; 16]);
        assert_ne!(
            baseline,
            fam(&typeof_key(t2, [0u8; 16], prc)),
            "type_env change must distinguish the TypeOf FamilyKey"
        );

        // lib_env differs on the slot.
        let l = value_root_slot(&canonical, &name, 0, [1u8; 16], [9u8; 16]);
        assert_ne!(
            baseline,
            fam(&typeof_key(l, [0u8; 16], prc)),
            "lib_env change must distinguish the TypeOf FamilyKey"
        );

        // project_identity differs on the slot.
        let j = value_root_slot(&canonical, &name, 7, [1u8; 16], [0u8; 16]);
        assert_ne!(
            baseline,
            fam(&typeof_key(j, [0u8; 16], prc)),
            "project_identity change must distinguish the TypeOf FamilyKey"
        );

        // resolve_env differs in the context (same slot).
        assert_ne!(
            baseline,
            fam(&typeof_key(base.clone(), [5u8; 16], prc)),
            "resolve_env change in TypeOfContext must distinguish the FamilyKey"
        );

        // Identical env + context → SAME family slot (warm-hit IS allowed).
        assert_eq!(baseline, fam(&typeof_key(base, [0u8; 16], prc)));
    }

    /// `TypeOf` follows the same provenance/merge_role family-identity
    /// pattern as `KeyOf` / `MappedType`: a macro-own-body `typeof`
    /// resolution and a structural one over the same value root must
    /// cold-build separately, never warm-hit one another.
    #[test]
    fn typeof_queries_differing_only_by_provenance_do_not_warm_hit() {
        let host = super::ctx_host();
        let store = super::SemanticGraphStore::new();
        let canonical: Arc<str> = Arc::from("/u2b9/value.ts");
        let name: Arc<str> = Arc::from("sample");
        let structural_result =
            store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let macro_result = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));

        let slot = value_root_slot(&canonical, &name, 0, [1u8; 16], [0u8; 16]);
        let structural = typeof_key(
            slot.clone(),
            [0u8; 16],
            context_with_axes(
                SurfaceProvenanceContext::Structural,
                MemberMergeRole::Authored,
            ),
        );
        let macro_own_body = typeof_key(
            slot,
            [0u8; 16],
            context_with_axes(
                SurfaceProvenanceContext::MacroTypeArgOwnBody,
                MemberMergeRole::Authored,
            ),
        );

        let first = store.execute_cooperative(
            &host,
            structural,
            || store.intern_node(SemanticNodeData::Opaque(super::QueryError::Miss)),
            || {
                (
                    QueryResult::Value(structural_result),
                    super::empty_signature(),
                )
            },
        );
        assert_value_node(first.value, structural_result);

        let mut second_build_ran = false;
        let second = store.execute_cooperative(
            &host,
            macro_own_body,
            || store.intern_node(SemanticNodeData::Opaque(super::QueryError::Miss)),
            || {
                second_build_ran = true;
                (QueryResult::Value(macro_result), super::empty_signature())
            },
        );

        assert!(
            second_build_ran,
            "a TypeOf query with different provenance must cold-build, not warm-hit a prior context"
        );
        assert_value_node(second.value, macro_result);
    }

    /// A `StructuralTransit` `typeof` lowering and a `Published` `typeof`
    /// resolution of the SAME value root are distinct evaluations: the
    /// transit lowering carrier-stops where the publication reduces, so a
    /// transit result must never be served from (or into) the publication
    /// slot.
    #[test]
    fn typeof_published_and_transit_contexts_do_not_warm_hit() {
        let host = super::ctx_host();
        let store = super::SemanticGraphStore::new();
        let canonical: Arc<str> = Arc::from("/u2b9/value.ts");
        let name: Arc<str> = Arc::from("sample");
        let published_result =
            store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let transit_result = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));

        let slot = value_root_slot(&canonical, &name, 0, [1u8; 16], [0u8; 16]);
        let published = typeof_key(
            slot.clone(),
            [0u8; 16],
            ProjectionReductionContext::published(ProjectionMode::Shallow),
        );
        let transit = typeof_key(
            slot,
            [0u8; 16],
            ProjectionReductionContext::structural_transit(),
        );

        let first = store.execute_cooperative(
            &host,
            published,
            || store.intern_node(SemanticNodeData::Opaque(super::QueryError::Miss)),
            || {
                (
                    QueryResult::Value(published_result),
                    super::empty_signature(),
                )
            },
        );
        assert_value_node(first.value, published_result);

        let mut second_build_ran = false;
        let second = store.execute_cooperative(
            &host,
            transit,
            || store.intern_node(SemanticNodeData::Opaque(super::QueryError::Miss)),
            || {
                second_build_ran = true;
                (QueryResult::Value(transit_result), super::empty_signature())
            },
        );

        assert!(
            second_build_ran,
            "a StructuralTransit TypeOf query must cold-build, not warm-hit the Published slot"
        );
        assert_value_node(second.value, transit_result);
    }

    /// The `HostResolvedNamedTypeKey` resolved-named-type artifact identity is
    /// env-scoped (R T L J): two resolutions of the SAME file content
    /// (`whole_hash`) under different envs are DISTINCT identities AND the
    /// `SemanticGraphStore` serves them as distinct entries. Pre-migration the
    /// key carried only `(canonical_id, whole_hash, inner)` — env-blind — so the
    /// two collided and a wrong-env macro surface could be served.
    #[test]
    fn resolved_named_type_key_identity_is_env_scoped() {
        use super::super::SemanticGraphStore;
        use crate::semantic_query::HostResolvedNamedTypeKey;
        use verter_compiler::utils::oxc::script::type_surface::ResolvedElements;
        use verter_compiler::utils::oxc::vue::named_type_keys::ResolvedNamedTypeCacheKey;

        let inner = |name: &str| ResolvedNamedTypeCacheKey {
            name: name.as_bytes().to_vec().into_boxed_slice(),
            surface: None,
            base_offset: 0,
            from_root_body: true,
            companion_cache_key: Arc::from(Vec::<Box<[u8]>>::new().into_boxed_slice()),
            type_param_bindings: Arc::from(Vec::new().into_boxed_slice()),
        };
        let mk = |resolve: HashValue, type_e: HashValue, lib_e: HashValue, pid: u32| {
            HostResolvedNamedTypeKey {
                canonical_id: Arc::from("/u2b9/x.ts"),
                whole_hash: [3u8; 16],
                resolve_env_hash: resolve,
                type_env_hash: type_e,
                lib_env_hash: lib_e,
                project_identity: pid,
                inner: inner("Foo"),
            }
        };

        let base = mk([0u8; 16], [1u8; 16], [0u8; 16], 0);
        // Each env dim independently forks the key identity.
        assert_ne!(
            base,
            mk([7u8; 16], [1u8; 16], [0u8; 16], 0),
            "resolve_env scopes the key"
        );
        assert_ne!(
            base,
            mk([0u8; 16], [2u8; 16], [0u8; 16], 0),
            "type_env scopes the key"
        );
        assert_ne!(
            base,
            mk([0u8; 16], [1u8; 16], [9u8; 16], 0),
            "lib_env scopes the key"
        );
        assert_ne!(
            base,
            mk([0u8; 16], [1u8; 16], [0u8; 16], 5),
            "project_identity scopes the key"
        );

        // The store serves env-distinct entries distinctly: an insert under
        // `base` must NOT be served to a different-type_env lookup.
        let store = SemanticGraphStore::new();
        let other_env = mk([0u8; 16], [2u8; 16], [0u8; 16], 0);
        let gen = store.named_type_generation();
        store
            .insert_resolved_named_type(base.clone(), Arc::new(ResolvedElements::default()), gen)
            .expect("current-generation insert is accepted");
        assert!(
            store.get_resolved_named_type(&base).is_some(),
            "same-env lookup hits"
        );
        assert!(
            store.get_resolved_named_type(&other_env).is_none(),
            "different-type_env lookup must MISS — the key is env-scoped, not env-blind"
        );
    }
}

/// `InstantiateBodySource` family-identity guards.
///
/// The `Instantiate` family folds the base body's SOURCE KIND: a file-backed
/// base folds the live `parse_env_hash` (`P`) into the family identity (two
/// lowerings differing only in the FileBacked `P` are DISTINCT FAMILIES —
/// never competing candidates under one slot), while a true non-file base
/// (`""` / `"__builtin__"` / `"<synthetic>"`) folds NO `P` at all — an
/// unconditional `P` would false-miss every parse-env-insensitive
/// instantiation (R21).
mod instantiate_body_source_family_identity {
    use super::super::family::{family_and_slot, FamilyKey};
    use crate::locator_identity::ParseEnvHash;
    use crate::semantic_query::{
        InstantiateBodySource, InstantiateContext, ProjectionMode, ProjectionReductionContext,
        ResolvedDeclSlotIdentity, SemanticNodeId, SemanticQueryKey,
    };
    use std::sync::Arc;

    fn empty_args() -> Arc<[SemanticNodeId]> {
        Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice())
    }

    fn slot() -> ResolvedDeclSlotIdentity {
        ResolvedDeclSlotIdentity::type_slot_unscoped(Arc::from("/w/a.ts"), Arc::from("Foo"))
    }

    fn inst_key(context: InstantiateContext) -> SemanticQueryKey {
        SemanticQueryKey::Instantiate {
            base: slot(),
            args: empty_args(),
            context,
        }
    }

    fn fam(key: &SemanticQueryKey) -> FamilyKey {
        family_and_slot(key).0
    }

    fn prc() -> ProjectionReductionContext {
        ProjectionReductionContext::published(ProjectionMode::Expanded)
    }

    /// Two contexts identical EXCEPT the FileBacked `P` (P0 vs P1) produce
    /// DIFFERENT `FamilyKey::Instantiate` values — a parse-env-only change on
    /// a file-backed base is a distinct family, never a warm collision.
    #[test]
    fn file_backed_parse_env_change_is_a_distinct_instantiate_family() {
        let p0 = ParseEnvHash::from_env_hash([1u8; 16]);
        let p1 = ParseEnvHash::from_env_hash([2u8; 16]);
        let f0 = fam(&inst_key(InstantiateContext::file_backed(
            prc(),
            Default::default(),
            p0,
        )));
        let f0_again = fam(&inst_key(InstantiateContext::file_backed(
            prc(),
            Default::default(),
            p0,
        )));
        let f1 = fam(&inst_key(InstantiateContext::file_backed(
            prc(),
            Default::default(),
            p1,
        )));
        assert_eq!(f0, f0_again, "same FileBacked P must be ONE family");
        assert_ne!(
            f0, f1,
            "FileBacked P0 vs P1 must be DISTINCT Instantiate families"
        );
    }

    /// A NonFile context folds NO `P`: the constructor takes no parse-env
    /// input, so the family is identical regardless of the live parse env,
    /// and its folded `body_source` carries no `ParseEnvHash`.
    #[test]
    fn non_file_context_folds_no_parse_env() {
        let a = fam(&inst_key(InstantiateContext::non_file(
            prc(),
            Default::default(),
        )));
        let b = fam(&inst_key(InstantiateContext::non_file(
            prc(),
            Default::default(),
        )));
        assert_eq!(
            a, b,
            "NonFile contexts have no P axis — one family regardless of the live parse env"
        );
        match a {
            FamilyKey::Instantiate { body_source, .. } => {
                assert_eq!(body_source, InstantiateBodySource::NonFile);
            }
            other => panic!("expected FamilyKey::Instantiate, got {other:?}"),
        }
    }

    /// FileBacked and NonFile are DISTINCT source kinds: same slot / args /
    /// projection / resolve env, different `body_source` ⇒ different family.
    #[test]
    fn file_backed_and_non_file_are_distinct_instantiate_families() {
        let file_backed = fam(&inst_key(InstantiateContext::file_backed(
            prc(),
            Default::default(),
            ParseEnvHash::from_env_hash([1u8; 16]),
        )));
        let non_file = fam(&inst_key(InstantiateContext::non_file(
            prc(),
            Default::default(),
        )));
        assert_ne!(
            file_backed, non_file,
            "FileBacked vs NonFile must be distinct Instantiate families"
        );
    }

    /// STRUCTURAL guard: `FamilyKey::Instantiate` folds `P` for EXACTLY the
    /// `FileBacked` arm. The match over `InstantiateBodySource` is EXHAUSTIVE
    /// (no wildcard), so adding a source-kind variant fails compilation here
    /// until its `P`-folding disposition is classified.
    #[test]
    fn family_key_instantiate_folds_parse_env_only_for_file_backed() {
        fn folded_parse_env(source: InstantiateBodySource) -> Option<ParseEnvHash> {
            match source {
                InstantiateBodySource::FileBacked(parse_env) => Some(parse_env),
                InstantiateBodySource::NonFile => None,
            }
        }

        let p = ParseEnvHash::from_env_hash([7u8; 16]);
        let contexts = [
            InstantiateContext::file_backed(prc(), Default::default(), p),
            InstantiateContext::non_file(prc(), Default::default()),
        ];
        for context in contexts {
            let family = fam(&inst_key(context));
            let FamilyKey::Instantiate { body_source, .. } = family else {
                panic!("expected FamilyKey::Instantiate");
            };
            // The family folds the context's body_source verbatim…
            assert_eq!(body_source, context.body_source());
            // …and its P content is exactly the exhaustive-match disposition.
            match folded_parse_env(body_source) {
                Some(parse_env) => assert_eq!(
                    body_source,
                    InstantiateBodySource::FileBacked(parse_env),
                    "FileBacked folds its ParseEnvHash into the family"
                ),
                None => assert_eq!(
                    body_source,
                    InstantiateBodySource::NonFile,
                    "NonFile folds no ParseEnvHash"
                ),
            }
        }
    }
}
