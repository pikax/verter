//! Discriminators for the cache substrate: fact-tracer persistence on
//! cold builds, the ComputeAdmission cooperative singleflight contract,
//! and the ReadSetSignature carrier's canonical coverage. Each test is
//! DISCRIMINATING — counter-delta, fact-kind-specific, or
//! thread-coordinated.
//!
//! Static counter serialization: this file uses several pre-existing
//! process-global static counters (overflow refusal, fact-tracer
//! installs, etc.) which must be serialized between tests to prevent
//! cross-test interference under `cargo test --test-threads > 1`.
//! Each test carries `#[serial(fact_counter_provenance)]` so it owns
//! the shared counters exclusively across the whole test process.

#![cfg(test)]

use serial_test::serial;
use verter_session_query::facts::fact_cache::ReadSetSignature;

/// Discriminator — `execute_read_cold_build_persists_traced_facts`.
///
/// A tree where `ProjectSemanticDispatch::execute_read` does
/// NOT wrap its cold build with `install_fact_tracer`. So a memo
/// entry first warmed through `execute_read` would carry only
/// `fact_signature_from_fence(dep_signature)`'s `FileWholeHash`
/// subset; path-precise `Parse(...)`, `ResolveImports(...)`, and
/// `RouteSurface(...)` observations would be silently dropped.
///
/// Both `execute` and `execute_read` route through the same
/// tracer-wrapped helper. The discriminating signal: a memo entry
/// warmed through any cold-build path carries the path-precise fact
/// signature.
///
/// Discriminating assertion: the carrier helper exists with the
/// `arch-guard:single-execute-cooperative-call` marker AND
/// `execute_read` delegates to it.
#[test]
#[serial(fact_counter_provenance)]
fn execute_read_cold_build_persists_traced_facts() {
    // The architectural witness: the shared cold-build helper exists
    // and `execute_read` routes through it. Without the helper, a
    // cold build started via `execute_read` would publish a memo
    // entry with `read_set_signature` derived only from the legacy
    // fence — losing every `Parse(...)` / `ResolveImports(...)` /
    // `RouteSurface(...)` observation.
    let mod_src = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../verter_type_engine/src/project_semantic_dispatch/mod.rs"),
    )
    .expect("read mod.rs");
    let raise_src = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../verter_type_engine/src/project_semantic_dispatch/raise.rs"),
    )
    .expect("read raise.rs");

    assert!(
        mod_src.contains("fn execute_via_cold_build_helper("),
        "shared cold-build helper must exist on ProjectSemanticDispatch"
    );
    assert!(
        mod_src.contains("install_delivering_fact_tracer(&basis_source, || {"),
        "shared cold-build helper must wrap the cold-build closure with its fact tracer, \
         seeded from the request-bound basis source it captured before the closure — a \
         host-only or unbound tracer disarms the cold build's compaction basis"
    );
    let frames_src = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../verter_type_engine/src/project_semantic_dispatch/query_frames.rs"),
    )
    .expect("read query_frames.rs");
    assert!(
        frames_src.contains("FactTracerBasisSource::from_ctx(self.ctx)"),
        "a cold build run as a continuation frame traces its steps from the same \
         request-bound basis source"
    );
    assert!(
        mod_src.contains("let basis_source = crate::fact_signature_helpers::FactTracerBasisSource::from_ctx(self.ctx);"),
        "and that source must come from the dispatch's REQUEST-BOUND context"
    );
    assert!(
        raise_src.contains("self.execute_via_cold_build_helper(key)"),
        "execute_read must delegate to the shared cold-build helper so its cold builds \
         install the fact tracer"
    );
    assert!(
        !raise_src.contains("graph.acquire_query("),
        "execute_read must NOT call graph.acquire_query directly. A separate \
         `graph.acquire_query(...)` call inside execute_read \
         would bypass install_fact_tracer."
    );
}

/// Behavioural discriminator —
/// `semantic_memo_invalidate_drains_fact_only_canonical_entry`.
///
/// Publishes a memo entry whose carrier has:
///   - `legacy` rail referencing ONLY `/test/legacy-only.ts`
///   - `facts` rail containing a `Parse(...)` fact on
///     `/test/fact-only.ts` (and no `FileWholeHash` for it)
///
/// Then calls `invalidate_canonical("/test/fact-dep.ts")` and
/// asserts the entry was drained from the warm cache.
///
/// `register_reverse_index` iterates
/// `read_set_signature.canonical_ids()` — every canonical the fact
/// rail names. The reverse-index shard for `/test/fact-dep.ts`
/// contains the entry's (family, slot) registration.
/// `invalidate_canonical` drains the shard, finds the entry, and
/// (via the `carrier_facts_reference_canonical` helper from
/// `family.rs`) evicts it.
///
/// Discriminating signal: post-invalidation, `store.get_unvalidated(&key)` is
/// `None`. A `register_reverse_index` that failed to register the
/// entry under a `Parse`-fact canonical would leave it `Some(...)`.
#[test]
#[serial(fact_counter_provenance)]
fn semantic_memo_invalidate_drains_fact_canonical_entry() {
    use std::sync::Arc;
    use verter_session_query::facts::fact_cache::ReadSetSignature;
    use verter_session_query::facts::fact_cache::{FactVersionRef, ParseFactRef};
    use verter_type_engine::semantic_query::{
        PrimitiveKind, QueryResult, ResolveDeclKey, ScopeId, SemanticNodeData, SemanticQueryKey,
    };
    use verter_type_engine::semantic_query_memo::SemanticGraphStore;

    let store = SemanticGraphStore::new();

    // Construct the query key. Its scope canonical is unrelated to the
    // fact-rail canonical — the entry's canonical reachability comes
    // entirely from the carrier's fact rail.
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: ScopeId {
            canonical_id: verter_type_expr::OwnedStr::from("/test/scope.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            local_scope: None,
            binder_scope_id: verter_type_engine::semantic_query::BinderScopeId::file_scope(
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
            ),
        },
        name: verter_type_expr::OwnedStr::from("MemoTarget"),
    });

    // Intern a placeholder node so we have a `Value` to publish.
    let node = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    // Build the carrier: a fact rail with one `Parse` fact naming
    // `/test/fact-dep.ts`.
    let facts: Arc<[FactVersionRef]> = Arc::from(vec![FactVersionRef::Parse(ParseFactRef {
        canonical_id: "/test/fact-dep.ts".into(),
        key: verter_session_query::facts::FactKey::SyntacticExportSet,
        lane: verter_session_query::facts::FactLane::Semantic,
        expected_hash: [0x22u8; 16],
    })]);
    let carrier = ReadSetSignature::new(
        verter_session_query::facts::policy::FactPolicy::production().set_of_shared(facts),
    );

    // Direct publish via the test-only helper.
    let populated = store.publish_with_carrier_for_tests(
        key.clone(),
        QueryResult::Value(node),
        carrier,
        std::sync::Arc::from([]),
    );
    assert!(
        populated >= 1,
        "publish must populate at least one slot (got {populated})"
    );

    // Sanity: entry is warm; the reverse-index shard for the
    // fact-rail canonical is non-empty.
    assert!(
        store.get_unvalidated(&key).is_some(),
        "entry must be warm pre-invalidation"
    );
    assert!(
        store.canonical_to_entries_count("/test/fact-dep.ts") >= 1,
        "the fact-rail canonical's reverse-index shard MUST be populated. \
         If 0, `register_reverse_index` is dropping a `Parse`-fact \
         canonical."
    );

    // Invalidate the fact-rail canonical.
    let removed = store.invalidate_canonical("/test/fact-dep.ts");
    assert_eq!(
        removed, 1,
        "invalidate_canonical for the fact-rail canonical must drain \
         the memo entry (got {removed}). If 0, `register_reverse_index` \
         never registered the entry under `/test/fact-dep.ts` — \
         the entry is orphaned across invalidation."
    );

    // Discriminating post-condition: the warm entry is gone.
    assert!(
        store.get_unvalidated(&key).is_none(),
        "entry must be evicted after invalidate_canonical of the \
         fact-rail canonical. If still present, the reverse index is \
         not draining fact-rail deps."
    );
}

/// A carrier-less winner belongs only to its own view. A waiting caller
/// must receive its own producer and answer, and neither answer becomes warm.
#[test]
#[serial(fact_counter_provenance)]
fn cooperative_return_only_not_shared_to_joiners() {
    use dashmap::DashMap;
    use std::sync::Arc;
    use std::task::{Poll, Waker};
    use verter_audit::NonAdmissionReason;
    use verter_execution::cancellation::CancellationToken;
    use verter_execution::tasks::TaskRegistry;
    use verter_type_engine::cache_runtime::singleflight::{
        owned_admit_with_post_publish, ComputeAdmission, InflightTable,
    };
    use verter_type_engine::cache_runtime::OwnedAdmissionStep;

    let registry = TaskRegistry::default();
    let flights = InflightTable::new();
    let map = Arc::new(DashMap::new());
    let begin = |value| {
        owned_admit_with_post_publish(
            map.clone(),
            &flights,
            1u32,
            |entry: &u32| Some(*entry),
            move || ComputeAdmission::ReturnOnly {
                value,
                reason: NonAdmissionReason::ForcedTestRefusal,
            },
            |entry| *entry,
            |_| true,
            |_, _| {},
            |_, _| {},
            None,
            registry.register_task(),
            CancellationToken::new(),
        )
    };
    let mut winner = begin(7);
    let Poll::Ready(Ok(OwnedAdmissionStep::Produce(producer))) = winner.poll(Waker::noop()) else {
        panic!("the cold winner owns its producer");
    };
    let mut follower = begin(9);
    assert!(follower.poll(Waker::noop()).is_pending());
    producer.run();
    assert!(matches!(
        winner.poll(Waker::noop()),
        Poll::Ready(Ok(OwnedAdmissionStep::Value(Some(7))))
    ));
    assert!(map.is_empty(), "ReturnOnly publishes no reusable entry");
    let Poll::Ready(Ok(OwnedAdmissionStep::Produce(producer))) = follower.poll(Waker::noop())
    else {
        panic!("the cross-view follower must recompute through admission");
    };
    producer.run();
    assert!(matches!(
        follower.poll(Waker::noop()),
        Poll::Ready(Ok(OwnedAdmissionStep::Value(Some(9))))
    ));
    assert!(map.is_empty());
    assert_eq!(registry.active_task_count_for_tests(), 0);
    assert_eq!(registry.wait_count_for_tests(), 0);
}

/// Bonus discriminator — the ReadSetSignature carrier's `canonical_ids()`
/// MUST cover every canonical the fact rail names across all
/// `FactVersionRef` variants. Without this, the reverse-index
/// registration would skip entries whose canonicals are only
/// reachable through specific fact variants.
#[test]
#[serial(fact_counter_provenance)]
fn read_set_signature_carrier_canonical_ids_covers_fact_rail() {
    use std::sync::Arc;
    use verter_session_query::facts::fact_cache::{FactVersionRef, ParseFactRef};

    let facts: Arc<[FactVersionRef]> = Arc::from(vec![
        FactVersionRef::FileWholeHash {
            canonical_id: "/whole.ts".into(),
            hash: [1u8; 16],
        },
        FactVersionRef::Parse(ParseFactRef {
            canonical_id: "/parse.ts".into(),
            key: verter_session_query::facts::FactKey::SyntacticExportSet,
            lane: verter_session_query::facts::FactLane::Semantic,
            expected_hash: [2u8; 16],
        }),
    ]);
    let sig = ReadSetSignature::new(
        verter_session_query::facts::policy::FactPolicy::production().set_of_shared(facts),
    );
    let canons: Vec<String> = sig
        .canonical_ids()
        .iter()
        .map(|a| a.as_ref().to_string())
        .collect();
    assert!(
        canons.contains(&"/whole.ts".to_string()),
        "FileWholeHash canonical must surface"
    );
    assert!(
        canons.contains(&"/parse.ts".to_string()),
        "Parse canonical must surface"
    );
    assert_eq!(
        canons.len(),
        2,
        "canonical_ids must yield the deduplicated set of fact-rail canonicals"
    );
}
