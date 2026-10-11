//! Query-node lookup and publish driven through a live session host: the
//! compute context reads its compat token through the resolver context, and
//! publish revalidates self roots against the host's live view.

use crate::types::HostConfig;
use crate::VerterHost;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use verter_session_query::facts::fact_cache::FactVersionRef;
use verter_session_query::facts::fact_cache::ReadSetSignature;
use verter_type_engine::cache_runtime::admission::CacheAdmission;
use verter_type_engine::cache_runtime::admission::CacheEntry;
use verter_type_engine::cache_runtime::admission::Candidate;
use verter_type_engine::cache_runtime::admission::DeferredVictims;
use verter_type_engine::cache_runtime::admission::FactCandidateDiscriminant;
use verter_type_engine::cache_runtime::admission::PublishCoreOutcome;
use verter_type_engine::cache_runtime::candidate_store::ReverseIndexedCandidateStore;
use verter_type_engine::cache_runtime::node::*;
use verter_type_engine::cache_runtime::singleflight::InflightTable;
use verter_type_engine::resolver_core::resolver_context::RequestFlags;
use verter_type_engine::resolver_core::ResolverContext;

/// A minimal `ArtifactNode` impl over a bare value type. The point of
/// this test is structural: `ArtifactNode` exposes ONLY `Key` and
/// `Value` associated types — there is NO `Entry` associated type (the
/// runtime owns the `CacheEntry<Value>` carrier internally). If a future
/// refactor reintroduced an `Entry` associated type, this impl would
/// fail to satisfy the trait (missing associated type) and the test
/// would fail to compile.
struct CountingArtifactNode {
    entries: dashmap::DashMap<u32, Arc<CacheEntry<String>>>,
    inflight: InflightTable<QueryFlightKey<u32>>,
    compute_count: Arc<AtomicUsize>,
}

impl ArtifactNode for CountingArtifactNode {
    type Key = u32;
    type Value = String;

    fn entries(&self) -> &dashmap::DashMap<Self::Key, Arc<CacheEntry<Self::Value>>> {
        &self.entries
    }

    fn inflight(&self) -> &InflightTable<QueryFlightKey<Self::Key>> {
        &self.inflight
    }

    fn compute(&self, key: &Self::Key, cx: &mut ComputeCtx<'_>) -> CacheAdmission<Self::Value> {
        self.compute_count.fetch_add(1, Ordering::SeqCst);
        CacheAdmission::Cacheable {
            value: format!("v{key}"),
            // Empty signature with no self-roots validates vacuously, so
            // the generation gate alone decides validity here.
            signature: ReadSetSignature::empty(),
            self_root_canonicals: Arc::from(Vec::<Arc<str>>::new()),
            validated_at_generation: cx.generation(),
        }
    }

    fn validate(
        &self,
        _key: &Self::Key,
        entry: &CacheEntry<Self::Value>,
        cx: &ComputeCtx<'_>,
    ) -> Option<Self::Value> {
        if entry.validated_at_generation == cx.generation() {
            Some(entry.value.clone())
        } else {
            None
        }
    }
}

/// `ArtifactNode` exposes ONLY `Key` and `Value` associated types — no
/// `Entry` associated type. The runtime owns the `CacheEntry<Value>`
/// carrier internally; the node never names it as an associated type.
///
/// This is enforced structurally: the function below names every
/// associated type of `ArtifactNode` in a where-clause that binds
/// `Key = u32` and `Value = String`. If a future refactor added an
/// `Entry` associated type, `CountingArtifactNode`'s impl (which
/// supplies only `Key` and `Value`) would no longer satisfy the trait
/// and this would fail to compile.
#[test]
fn artifact_node_has_no_entry_associated_type() {
    fn assert_only_key_and_value<N>()
    where
        N: ArtifactNode<Key = u32, Value = String>,
    {
        // Naming `N::Key` and `N::Value` exhausts the public associated
        // types. `CacheEntry<N::Value>` is referenced as a CONCRETE
        // runtime type parameterised by `Value`, NOT as `N::Entry`.
        fn _carrier_is_runtime_owned<V: Clone + Send + Sync + 'static>(
        ) -> std::marker::PhantomData<CacheEntry<V>> {
            std::marker::PhantomData
        }
        let _: std::marker::PhantomData<(N::Key, N::Value)> = std::marker::PhantomData;
        let _ = _carrier_is_runtime_owned::<N::Value>();
    }
    assert_only_key_and_value::<CountingArtifactNode>();
}

/// A test `QueryNode` whose `compute` stamps a SUPERSEDED generation on
/// the candidate, modelling a self-root edit / generation bump that
/// landed during the cold compute window. The publish gate in
/// `query::lookup` (`candidate.validated_at_generation == generation`)
/// must then reject the candidate: `publish_core` is never invoked, and
/// the caller receives `None`. Storage is the shared
/// [`ReverseIndexedCandidateStore`].
struct StaleGenerationQueryNode {
    inflight: InflightTable<QueryFlightKey<u32>>,
    publish_count: Arc<AtomicUsize>,
    store: ReverseIndexedCandidateStore<u32, String>,
}

impl QueryNode for StaleGenerationQueryNode {
    type Key = u32;
    type Discriminant = FactCandidateDiscriminant;
    type Value = String;

    fn inflight(&self) -> &InflightTable<QueryFlightKey<Self::Key>> {
        &self.inflight
    }

    fn lookup_candidate(&self, key: &Self::Key, cx: &ComputeCtx<'_>) -> Option<Self::Value> {
        // The store validates by generation; the stale candidate never
        // matches the live generation, so the lookup is always a miss.
        let generation = cx.generation();
        verter_type_engine::project_semantic_dispatch::memo::read_candidate(
            &self.store,
            key,
            |candidate| {
                if candidate.validated_at_generation == generation {
                    Some(candidate.value.clone())
                } else {
                    None
                }
            },
        )
    }

    fn compute(&self, key: &Self::Key, cx: &mut ComputeCtx<'_>) -> CacheAdmission<Self::Value> {
        CacheAdmission::Cacheable {
            value: format!("stale{key}"),
            signature: ReadSetSignature::empty(),
            self_root_canonicals: Arc::from(Vec::<Arc<str>>::new()),
            // SUPERSEDED: one generation behind the live generation. The
            // publish gate `validated_at_generation == generation` fails.
            validated_at_generation: cx.generation().wrapping_sub(1),
        }
    }

    fn discriminant(
        &self,
        _key: &Self::Key,
        _value: &Self::Value,
        signature: &ReadSetSignature,
        validated_at_generation: u64,
    ) -> Self::Discriminant {
        // The discriminant carries the candidate's OWN stamped generation
        // (passed straight from the `Cacheable` arm), so it matches the
        // superseded `validated_at_generation` this node stamps in
        // `compute` — the runtime never substitutes its lookup-entry
        // snapshot here.
        FactCandidateDiscriminant {
            validated_at_generation,
            facts: Arc::clone(&signature.facts),
        }
    }

    fn publish_core(
        &self,
        key: Self::Key,
        candidate: Candidate<Self::Discriminant, Self::Value>,
    ) -> PublishCoreOutcome<Self::Key> {
        self.publish_count.fetch_add(1, Ordering::SeqCst);
        self.store.publish_core(key, candidate)
    }

    fn evict_deferred(&self, victims: DeferredVictims<Self::Key>) {
        self.store.evict_deferred(victims);
    }
}

/// A `QueryNode` modelling the discriminant-generation-skew scenario.
///
/// `compute` bumps the host project generation on its FIRST cold build
/// (modelling a tsconfig / path-alias change that lands DURING the
/// runtime's cold window, AFTER `query::lookup` captured its lookup-entry
/// snapshot) and then stamps the candidate at the now-LIVE generation —
/// exactly as a real producer snapshots its generation inside its own
/// `install_fact_tracer` compute, distinct from the runtime's lookup-entry
/// snapshot. Subsequent cold builds re-read the (already-bumped) live
/// generation without bumping again, so every candidate carries the SAME
/// stamped generation with the SAME facts.
///
/// `lookup_candidate` always misses so each `query::lookup` call drives a
/// fresh cold publish (the slot keeps prior candidates; this node never
/// view-validates them).
struct SkewedDiscriminantQueryNode {
    generations: Arc<crate::project_type_store::ProjectTypeStore>,
    inflight: InflightTable<QueryFlightKey<u32>>,
    store: ReverseIndexedCandidateStore<u32, String>,
    /// Cold-compute call counter — the first call bumps the generation.
    compute_calls: Arc<AtomicUsize>,
}

impl SkewedDiscriminantQueryNode {
    /// One fixed fact set shared by every candidate, so the discriminant's
    /// fact dimension is constant and only its GENERATION dimension can
    /// differ between candidates.
    fn fixed_signature() -> ReadSetSignature {
        let facts: Arc<[FactVersionRef]> = Arc::from(vec![FactVersionRef::FileWholeHash {
            canonical_id: "/skew.ts".to_string(),
            hash: [7u8; 16],
        }]);
        ReadSetSignature::new(facts)
    }
}

impl QueryNode for SkewedDiscriminantQueryNode {
    type Key = u32;
    type Discriminant = FactCandidateDiscriminant;
    type Value = String;

    fn inflight(&self) -> &InflightTable<QueryFlightKey<Self::Key>> {
        &self.inflight
    }

    fn lookup_candidate(&self, _key: &Self::Key, _cx: &ComputeCtx<'_>) -> Option<Self::Value> {
        // Always miss: force the cold publish path on every call so the
        // second call re-publishes rather than serving a warm hit.
        None
    }

    fn compute(&self, key: &Self::Key, cx: &mut ComputeCtx<'_>) -> CacheAdmission<Self::Value> {
        // On the FIRST cold build, bump the project generation so the live
        // generation now LEADS the runtime's lookup-entry snapshot
        // (`cx.generation()` here is that snapshot). The candidate is then
        // stamped at the LIVE generation, so its stamp != the lookup-entry
        // snapshot — the exact skew the discriminant must NOT inherit.
        if self.compute_calls.fetch_add(1, Ordering::SeqCst) == 0 {
            self.generations.bump_project_generation();
        }
        let live = cx.resolver.request_flags().current_project_generation();
        CacheAdmission::Cacheable {
            value: format!("v{key}"),
            signature: Self::fixed_signature(),
            // Empty self-roots so `validate_with_self_roots` is vacuous and
            // the generation gate alone decides revalidation.
            self_root_canonicals: Arc::from(Vec::<Arc<str>>::new()),
            // Stamp at the LIVE generation (the producer's in-compute
            // snapshot), NOT `cx.generation()` (the runtime's lookup-entry
            // snapshot). On the first call these differ; the post-compute
            // revalidation (`stamp == live`) still passes because the bump
            // already landed.
            validated_at_generation: live,
        }
    }

    fn discriminant(
        &self,
        _key: &Self::Key,
        _value: &Self::Value,
        signature: &ReadSetSignature,
        validated_at_generation: u64,
    ) -> Self::Discriminant {
        // Build from the candidate's OWN stamp (threaded straight from the
        // `Cacheable` arm). This is the fix under test: a regression that
        // read the runtime's lookup-entry snapshot instead would skew the
        // first publish's discriminant generation away from its candidate
        // generation, so the second same-view publish would COEXIST as a
        // duplicate.
        FactCandidateDiscriminant {
            validated_at_generation,
            facts: Arc::clone(&signature.facts),
        }
    }

    fn publish_core(
        &self,
        key: Self::Key,
        candidate: Candidate<Self::Discriminant, Self::Value>,
    ) -> PublishCoreOutcome<Self::Key> {
        self.store.publish_core(key, candidate)
    }

    fn evict_deferred(&self, victims: DeferredVictims<Self::Key>) {
        self.store.evict_deferred(victims);
    }
}

/// `ComputeCtx` carries the store-view compat token (the flight-lane
/// dimension) alongside the generation. `from_resolver` reads both from
/// the resolver. This test reads `compat_token` so the field's role is
/// exercised — a regression dropping it from the context fails here.
#[test]
fn compute_ctx_from_resolver_carries_compat_token_and_generation() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let ctx: &dyn ResolverContext<crate::resolver_core::HostCapabilities> = &host;
    let cx = ComputeCtx::from_resolver(ctx, ctx.request_flags());
    // The compat token matches the resolver's store-view token, and the
    // generation matches the project-type-store generation.
    assert_eq!(
        cx.compat_token,
        verter_type_engine::resolver_core::fact_validation_port::FactValidationView::new(ctx)
            .compat_token()
    );
    assert_eq!(
        cx.generation(),
        ctx.request_flags().current_project_generation()
    );
}

/// `lookup` dedups the cold compute for repeated calls on the same key
/// under the same view: the first call computes and publishes, the
/// second is a warm map hit (no recompute).
#[test]
fn lookup_dedups_cold_compute_under_same_view() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let ctx: &dyn ResolverContext<crate::resolver_core::HostCapabilities> = &host;
    let flags: &RequestFlags = ctx.request_flags();
    let node = CountingArtifactNode {
        entries: dashmap::DashMap::new(),
        inflight: InflightTable::new(),
        compute_count: Arc::new(AtomicUsize::new(0)),
    };

    let first = lookup(&node, 7u32, ctx, flags);
    assert_eq!(first.as_deref(), Some("v7"));
    let second = lookup(&node, 7u32, ctx, flags);
    assert_eq!(second.as_deref(), Some("v7"));

    assert_eq!(
        node.compute_count.load(Ordering::SeqCst),
        1,
        "lookup must compute once and serve the second call from the warm map"
    );
    assert_eq!(
        node.entries.len(),
        1,
        "exactly one entry published under the key"
    );
}

/// `query::lookup` rejects a candidate whose self-root was superseded
/// mid-compute (modelled by a stale `validated_at_generation`): the
/// publish closure is NOT invoked and the caller receives `None`.
#[test]
fn publish_rejects_candidate_when_self_root_edited_mid_compute() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let ctx: &dyn ResolverContext<crate::resolver_core::HostCapabilities> = &host;
    let flags: &RequestFlags = ctx.request_flags();
    let node = StaleGenerationQueryNode {
        inflight: InflightTable::new(),
        publish_count: Arc::new(AtomicUsize::new(0)),
        store: ReverseIndexedCandidateStore::with_counter(Arc::new(AtomicU64::new(0))),
    };

    let result = query::lookup(&node, 1u32, ctx, flags);
    assert_eq!(
        result, None,
        "a generation-superseded candidate must be rejected by the publish gate"
    );
    assert_eq!(
        node.publish_count.load(Ordering::SeqCst),
        0,
        "publish_core must NOT be invoked when post-compute revalidation rejects the entry"
    );
    assert_eq!(
        node.store.live_count(),
        0,
        "no candidate must enter the store"
    );
}

/// The candidate's discriminant generation MUST equal the candidate's
/// stamped `validated_at_generation`, even when a project-generation bump
/// lands between the runtime's lookup-entry snapshot and the producer's
/// in-compute snapshot.
///
/// Scenario: the first `query::lookup` enters at generation `G`; its cold
/// compute bumps the generation to `G+1` and stamps the candidate at
/// `G+1`. The second `query::lookup` enters at `G+1` and stamps a second
/// candidate, also at `G+1`, with the SAME facts. Both candidates share
/// one view (`G+1`, same facts), so the second publish must REPLACE the
/// first — exactly ONE candidate in the slot.
///
/// DISCRIMINATES: pre-fix the discriminant was built from
/// `cx.generation()` (the lookup-entry snapshot), so the first publish's
/// discriminant carried `G` while its candidate carried `G+1`; the second
/// publish's discriminant carried `G+1`. The two discriminants differed,
/// so the candidates COEXISTED — `slot_len == 2`, and the cap-4 budget /
/// FIFO order would be consumed by a phantom duplicate. Post-fix both
/// discriminants are `G+1` (the candidate stamp), the second publish
/// replaces in place, and `slot_len == 1`.
#[test]
fn discriminant_generation_tracks_candidate_stamp_not_lookup_snapshot() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let ctx: &dyn ResolverContext<crate::resolver_core::HostCapabilities> = &host;
    let flags: &RequestFlags = ctx.request_flags();
    let gen_before = ctx.request_flags().current_project_generation();
    let node = SkewedDiscriminantQueryNode {
        generations: Arc::clone(host.project_type_store()),
        inflight: InflightTable::new(),
        store: ReverseIndexedCandidateStore::with_counter(Arc::new(AtomicU64::new(0))),
        compute_calls: Arc::new(AtomicUsize::new(0)),
    };

    // First publish: enters at `G`, bumps to `G+1` mid-compute, stamps the
    // candidate at `G+1`.
    let first = query::lookup(&node, 1u32, ctx, flags);
    assert_eq!(first.as_deref(), Some("v1"), "first cold build publishes");
    let gen_after_first = ctx.request_flags().current_project_generation();
    assert_eq!(
        gen_after_first,
        gen_before + 1,
        "the first cold compute bumped the project generation once"
    );
    assert_eq!(
        node.store.slot_len_for_test(&1),
        1,
        "the first publish admits exactly one candidate"
    );

    // Second publish: enters at `G+1` (no further bump), stamps a second
    // candidate at `G+1` with the same facts. Same view → must REPLACE.
    let second = query::lookup(&node, 1u32, ctx, flags);
    assert_eq!(second.as_deref(), Some("v1"), "second cold build publishes");
    assert_eq!(
        ctx.request_flags().current_project_generation(),
        gen_before + 1,
        "the second cold compute does NOT bump again"
    );

    // THE DISCRIMINATOR: both candidates carry the SAME stamped generation
    // (`G+1`) and the SAME facts, so they are one view and the second
    // publish replaced the first. A skewed discriminant (pre-fix) would
    // have produced two coexisting candidates here.
    assert_eq!(
        node.store.slot_len_for_test(&1),
        1,
        "the second same-view publish MUST replace the first candidate, not coexist as a \
         duplicate — the discriminant generation must equal the candidate's stamped generation, \
         not the runtime's lookup-entry snapshot"
    );
    assert_eq!(
        node.store.live_count(),
        1,
        "exactly one live candidate after the replace"
    );
    assert_eq!(
        node.compute_calls.load(Ordering::SeqCst),
        2,
        "both calls took the cold path (lookup_candidate always misses)"
    );
}

/// An `ArtifactNode` whose cold compute returns one shared signature wider
/// than a page, claimed into the node's own account.
struct WideArtifactNode {
    entries: dashmap::DashMap<u32, Arc<CacheEntry<String>>>,
    inflight: InflightTable<QueryFlightKey<u32>>,
    signature: ReadSetSignature,
    stale_generation: bool,
    strict_root: bool,
    account: Arc<verter_session_query::retention::SemanticRetentionAccount>,
}

impl WideArtifactNode {
    fn new(account: Arc<verter_session_query::retention::SemanticRetentionAccount>) -> Self {
        let facts = (0..2 * verter_session_query::facts::fact_read_set::FACT_PAGE_WIDTH + 3)
            .map(|index| FactVersionRef::FileWholeHash {
                canonical_id: format!("/wide/{index:05}.ts"),
                hash: [1u8; 16],
            })
            .collect();
        Self {
            entries: dashmap::DashMap::new(),
            inflight: InflightTable::new(),
            stale_generation: false,
            strict_root: false,
            signature: ReadSetSignature::new(
                verter_session_query::facts::fact_read_set::seal_canonical_signature(facts),
            ),
            account,
        }
    }

    fn with_valid_signature(mut self, host: &VerterHost) -> Self {
        let facts = (0..2 * verter_session_query::facts::fact_read_set::FACT_PAGE_WIDTH + 3)
            .map(|index| {
                let canonical_id = format!("/wide/{index:05}.ts");
                let _ = host
                    .upsert(crate::types::UpsertRequest {
                        canonical_id: Some(canonical_id.clone()),
                        input_id: canonical_id.clone(),
                        source: Arc::from("export const value = 1;"),
                        file_language: verter_language::FileLanguage::script_ts(),
                        aliases: Vec::new(),
                    })
                    .expect("wide dependency");
                let source = host
                    .scheduler
                    .try_get_source(&canonical_id)
                    .expect("source");
                let hash = source
                    .downcast_data::<crate::host_executor::HostSourceData>()
                    .expect("host source")
                    .parse
                    .whole_hash;
                FactVersionRef::FileWholeHash { canonical_id, hash }
            })
            .collect();
        self.signature = ReadSetSignature::new(
            verter_session_query::facts::fact_read_set::seal_canonical_signature(facts),
        );
        self
    }

    fn page_classes(&self) -> Vec<Option<verter_session_query::retention::ChargeClass>> {
        self.signature
            .facts
            .iter()
            .filter_map(|fact| match fact {
                FactVersionRef::Receipt(page) if page.is_page() => {
                    Some(page.retained_charge_class())
                }
                _ => None,
            })
            .collect()
    }
}

impl ArtifactNode for WideArtifactNode {
    type Key = u32;
    type Value = String;

    fn entries(&self) -> &dashmap::DashMap<Self::Key, Arc<CacheEntry<Self::Value>>> {
        &self.entries
    }

    fn inflight(&self) -> &InflightTable<QueryFlightKey<Self::Key>> {
        &self.inflight
    }

    fn compute(&self, key: &Self::Key, cx: &mut ComputeCtx<'_>) -> CacheAdmission<Self::Value> {
        CacheAdmission::Cacheable {
            value: format!("v{key}"),
            signature: self.signature.clone(),
            self_root_canonicals: if self.strict_root {
                Arc::from(vec![Arc::from("/wide/00000.ts")])
            } else {
                Arc::from(Vec::<Arc<str>>::new())
            },
            validated_at_generation: if self.stale_generation {
                cx.generation().wrapping_sub(1)
            } else {
                cx.generation()
            },
        }
    }

    fn validate(
        &self,
        _key: &Self::Key,
        entry: &CacheEntry<Self::Value>,
        _cx: &ComputeCtx<'_>,
    ) -> Option<Self::Value> {
        Some(entry.value.clone())
    }

    fn retention_account(&self) -> Arc<verter_session_query::retention::SemanticRetentionAccount> {
        Arc::clone(&self.account)
    }
}

/// A cold winner about to publish a wide signature claims its pages into a
/// refusable reservation; an account that refuses that footprint returns
/// the complete value uncached and leaves every page pinned.
#[test]
fn cold_publish_claims_wide_signature_pages_and_is_refused_for_them() {
    use verter_session_query::retention::{ChargeClass, RetentionLimits, SemanticRetentionAccount};
    let host = VerterHost::new_standalone(HostConfig::default());
    let ctx: &dyn ResolverContext<crate::resolver_core::HostCapabilities> = &host;
    let flags: &RequestFlags = ctx.request_flags();

    let account = SemanticRetentionAccount::new(RetentionLimits::defaults());
    let node = WideArtifactNode::new(Arc::clone(&account)).with_valid_signature(&host);
    assert_eq!(lookup(&node, 7u32, ctx, flags).as_deref(), Some("v7"));
    assert_eq!(node.entries.len(), 1);
    let classes = node.page_classes();
    assert!(!classes.is_empty(), "premise: the signature is paged");
    assert!(classes
        .iter()
        .all(|class| *class == Some(ChargeClass::Retained)));
    assert!(account.snapshot().retained_bytes > 0);
    drop(node);
    assert_eq!(
        account.snapshot().retained_bytes,
        0,
        "the last holder's drop drains the pages"
    );

    let tight = SemanticRetentionAccount::new(RetentionLimits {
        max_entry_bytes: 1,
        ..RetentionLimits::defaults()
    });
    let node = WideArtifactNode::new(Arc::clone(&tight)).with_valid_signature(&host);
    assert_eq!(
        lookup(&node, 7u32, ctx, flags).as_deref(),
        Some("v7"),
        "a refused claim still delivers the complete value"
    );
    assert!(node.entries.is_empty(), "a refused claim publishes nothing");
    assert!(node
        .page_classes()
        .iter()
        .all(|class| *class == Some(ChargeClass::Pinned)));
    assert_eq!(tight.snapshot().retained_bytes, 0);
}

#[test]
fn retention_refusal_preserves_artifact_post_compute_validation() {
    use verter_session_query::retention::{RetentionLimits, SemanticRetentionAccount};
    let host = VerterHost::new_standalone(HostConfig::default());
    let ctx: &dyn ResolverContext<crate::resolver_core::HostCapabilities> = &host;
    for max_entry_bytes in [usize::MAX, 1] {
        for strict_root in [false, true] {
            let mut node = WideArtifactNode::new(SemanticRetentionAccount::new(RetentionLimits {
                max_entry_bytes,
                ..RetentionLimits::defaults()
            }));
            node.strict_root = strict_root;
            node.stale_generation = !strict_root;
            assert_eq!(
                lookup(&node, 7, ctx, ctx.request_flags()),
                None,
                "limit={max_entry_bytes}, strict_root={strict_root}"
            );
            assert!(node.entries.is_empty());
        }
    }
}

struct WideQueryNode {
    artifact: WideArtifactNode,
    store: ReverseIndexedCandidateStore<u32, String>,
    stale: bool,
    lower: bool,
}

impl QueryNode for WideQueryNode {
    type Key = u32;
    type Value = String;
    type Discriminant = FactCandidateDiscriminant;
    fn inflight(&self) -> &InflightTable<QueryFlightKey<u32>> {
        &self.artifact.inflight
    }
    fn lookup_candidate(&self, _key: &u32, _cx: &ComputeCtx<'_>) -> Option<String> {
        None
    }
    fn compute(&self, key: &u32, cx: &mut ComputeCtx<'_>) -> CacheAdmission<String> {
        CacheAdmission::Cacheable {
            value: format!("v{key}"),
            signature: self.artifact.signature.clone(),
            self_root_canonicals: Arc::from(Vec::<Arc<str>>::new()),
            validated_at_generation: if self.stale {
                cx.generation().wrapping_sub(1)
            } else {
                cx.generation()
            },
        }
    }
    fn discriminant(
        &self,
        _key: &u32,
        _value: &String,
        signature: &ReadSetSignature,
        validated_at_generation: u64,
    ) -> FactCandidateDiscriminant {
        FactCandidateDiscriminant {
            validated_at_generation,
            facts: Arc::clone(&signature.facts),
        }
    }
    fn publish_core(
        &self,
        key: u32,
        candidate: Candidate<FactCandidateDiscriminant, String>,
    ) -> PublishCoreOutcome<u32> {
        self.store.publish_core(key, candidate)
    }
    fn evict_deferred(&self, victims: DeferredVictims<u32>) {
        self.store.evict_deferred(victims);
    }
    fn lower_unadmitted(&self, value: &String) -> Option<String> {
        self.lower.then(|| format!("lowered:{value}"))
    }
    fn retention_account(&self) -> Arc<verter_session_query::retention::SemanticRetentionAccount> {
        Arc::clone(&self.artifact.account)
    }
}

#[test]
fn query_retention_refusal_preserves_delivery_validation_and_lowering() {
    use verter_session_query::retention::{RetentionLimits, SemanticRetentionAccount};
    let host = VerterHost::new_standalone(HostConfig::default());
    let ctx: &dyn ResolverContext<crate::resolver_core::HostCapabilities> = &host;
    for (limit, stale, lower) in [
        (usize::MAX, false, false),
        (1, false, false),
        (usize::MAX, true, false),
        (1, true, false),
        (usize::MAX, true, true),
        (1, true, true),
    ] {
        let artifact = WideArtifactNode::new(SemanticRetentionAccount::new(RetentionLimits {
            max_entry_bytes: limit,
            ..RetentionLimits::defaults()
        }))
        .with_valid_signature(&host);
        let node = WideQueryNode {
            artifact,
            store: ReverseIndexedCandidateStore::with_counter(Arc::new(AtomicU64::new(0))),
            stale,
            lower,
        };
        let expected = if stale {
            lower.then_some("lowered:v7")
        } else {
            Some("v7")
        };
        assert_eq!(
            query::lookup(&node, 7, ctx, ctx.request_flags()).as_deref(),
            expected
        );
        assert_eq!(
            node.store.slot_len_for_test(&7),
            usize::from(!stale && limit != 1)
        );
    }
}
