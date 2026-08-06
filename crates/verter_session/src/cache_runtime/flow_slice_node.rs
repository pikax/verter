//! The flow-slice cache-runtime nodes: [`FlowSliceHashNode`] (the slice
//! identity) and [`FlowSliceLoweredBodyNode`] (the lowered slice), plus
//! the shared once-per-content-version [`FunctionFlowGraphStore`].
//!
//! Hash-then-lower is STRUCTURAL here: the lowered-body key
//! ([`FlowSliceLoweredKey`]) embeds the opaque
//! [`FlowSliceHash`] — a type only the semantic slice hasher can mint —
//! so no caller can reach the lowered store without the slice hash
//! having been computed first. The hash node's compute runs the demand
//! planner (graph reachability over the shared per-function
//! [`FunctionFlowGraph`]) and hashes the selected subgraph; the lowered
//! node's compute re-plans (cheap, deterministic over the same pinned
//! content) and lowers ONLY the plan — it never computes a slice hash.
//!
//! Both nodes are CONTENT-ADDRESSED memory-side [`ArtifactNode`]s: the
//! key pins the canonical, the five-axis function identity, the
//! body-sensitive / cosmetic-insensitive `flow_body_stable_hash`, the
//! EXACT per-function byte hash, the parse-env hash, the parser version,
//! and the demand identity, so key identity IS validity and no fact rail
//! is required — the entries' signatures stay EMPTY, and no slice
//! identity ever enters `ReadSetSignature.facts` (slice hashes and
//! selected IDs are never a warm-validity oracle). Their PERSISTENT
//! registration is deferred work gated on U4; nothing here builds a
//! persistence tier.
//!
//! "Key identity IS validity" is a CLAIM about the artifacts, and it
//! holds only because two things are true together: the key carries the
//! exact per-function byte hash, and the artifacts carry no absolute
//! source position (every span in the skeleton, and therefore in the
//! lowered slice IR, is relative to the function's own start). Drop
//! either one and one key admits contents whose positions differ, at
//! which point the key stops being an oracle and reuse serves a plan
//! that no longer addresses its own code. The stable hash alone cannot
//! carry the claim: it alpha-normalizes identifiers and folds the AST
//! rather than the text, so it is blind to a local rename that shifts
//! every position inside the body.
//!
//! Budget non-admission: an over-budget plan returns the typed
//! [`FlowSliceBudgetExceeded`] through `CacheAdmission::ReturnOnly`
//! (reason `BudgetExceeded`) — returned to the winning flight, never
//! published, never backfilled; the lowered store cannot even be
//! addressed for it because no slice hash exists.
//!
//! The production home is [`FlowSliceStores`] on the single
//! `ProjectTypeStore`: one shared graph store, both nodes over it, the
//! production [`RetainedSnapshotSkeletonSource`], and the shared armed
//! [`FlowSliceBudget`] cell. The `FlowReturn` executor consumes the hash
//! node on its cold path (the budget outcome gates memo admission); the
//! lowered node serves slice-IR demand through the same store.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;

use verter_semantic::analysis::flow::flow_graph::{build_function_flow_graph, FunctionFlowGraph};
use verter_semantic::analysis::flow::flow_ir::FlowSliceIR;
use verter_semantic::analysis::flow::hashing::{compute_flow_slice_hash, FlowSliceHash};
use verter_semantic::analysis::flow::lower::lower_slice_plan;
use verter_semantic::analysis::flow::peeker::{
    FlowSliceBudget, FlowSliceBudgetExceeded, ReturnPathPeeker, SliceDemand,
};
use verter_semantic::analysis::flow::FunctionBodySkeleton;
use verter_semantic::analysis::function_program::FunctionProgramKey;

use super::admission::{CacheAdmission, CacheEntry, NonAdmissionReason};
use super::node::{ArtifactNode, ComputeCtx, QueryFlightKey};
use super::singleflight::InflightTable;
use crate::fact_signature_helpers::ReadSetSignature;
use crate::resolver_core::ResolverContext;
use crate::types::Hash16;

#[cfg(test)]
#[path = "flow_slice_node_tests.rs"]
pub(crate) mod tests;

// ── Keys ──────────────────────────────────────────────────────────────

/// The content-pinned function identity every flow-slice artifact keys
/// on: the canonical, the five-axis served-function identity, the
/// body-sensitive `flow_body_stable_hash` (NOT `parse_stable_hash` —
/// `return {{ b: 1 }}` vs `return {{ b: 2 }}` key distinct slices), the
/// EXACT per-function byte hash, the parse-env hash, and the parser
/// version.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct FlowSliceFunctionKey {
    /// Canonical id of the file serving the function.
    pub canonical_id: Arc<str>,
    /// The five-axis function program identity.
    pub function: FunctionProgramKey,
    /// The whole-function body-sensitive / cosmetic-insensitive hash.
    pub flow_body_stable_hash: Hash16,
    /// The EXACT byte hash of the function's own source text.
    ///
    /// The artifacts this key addresses carry SOURCE POSITIONS, and the
    /// stable hash above cannot address those: it alpha-normalizes
    /// binding / reference identifiers and folds the AST rather than the
    /// text, so `const aa = 1` and `const aaaa = 1` share it while
    /// placing every position inside the body differently. Reuse across
    /// that boundary hands a plan positions that no longer address the
    /// code they were computed from.
    ///
    /// This is NOT a file-offset axis, and deliberately so: it covers
    /// the function's OWN bytes only, so an edit anywhere else in the
    /// file — a leading blank line, a sibling function's body — leaves
    /// it (and every anchor-relative position in the artifacts) intact,
    /// and the untouched function stays warm. The two halves are what
    /// make these artifacts genuinely content-addressed; either alone
    /// leaves one direction unsound.
    pub flow_body_exact_hash: Hash16,
    /// Parse-domain env hash.
    pub parse_env_hash: Hash16,
    /// Parser version.
    pub parser_version: u32,
}

/// The demand identity of one slice: the demanded return-projection
/// path (empty = whole return). Further demand axes (the C1
/// `ReturnProjectionDemand` lattice point with its `EvalPolicy`) land
/// with the `FlowReturn` key axes and map onto this identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct FlowSliceDemandIdentity {
    /// The demanded projection path under the return value, in authored
    /// key text (empty = the whole return).
    pub projection_path: Arc<[Arc<str>]>,
}

/// [`FlowSliceHashNode`] cache key: function content identity plus the
/// demand identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct FlowSliceHashKey {
    /// The content-pinned function identity.
    pub function: FlowSliceFunctionKey,
    /// The demand identity.
    pub demand: FlowSliceDemandIdentity,
}

/// [`FlowSliceLoweredBodyNode`] cache key: the hash key PLUS the slice
/// hash. [`FlowSliceHash`] has no public constructor — only the
/// semantic slice hasher mints it — so this key is unconstructible
/// until the hash node's compute has run: the slice hash PRECEDES the
/// lowered lookup by type, not by convention.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct FlowSliceLoweredKey {
    /// The slice's hash-node key.
    pub hash_key: FlowSliceHashKey,
    /// The planner-produced slice identity.
    pub slice_hash: FlowSliceHash,
}

// ── Graph storage (once per function content version) ────────────────

/// The skeleton producer seam: builds one authored function-body
/// skeleton from the retained parse snapshot for exactly the content
/// version the key pins. The production implementation is
/// [`RetainedSnapshotSkeletonSource`] (resolver-backed, over the
/// scheduler-retained parse snapshot); the store below guarantees it is
/// consulted at most ONCE per function content version.
pub(crate) trait FlowBodySkeletonSource: Send + Sync {
    /// Build the skeleton for `key`'s function, or `None` when the
    /// position is not served at exactly the pinned content version.
    fn build_skeleton(
        &self,
        key: &FlowSliceFunctionKey,
        resolver: &dyn ResolverContext,
    ) -> Option<FunctionBodySkeleton>;
}

/// The PRODUCTION skeleton source: resolves the served function through
/// the caller's resolver (`ensure_indexed_ready_serve` → the shared
/// `DeclBodyMemo` lease-only retained-snapshot run) and builds the
/// skeleton for exactly the content version the key pins. A live entry
/// whose `flow_body_stable_hash` no longer matches the pinned key is a
/// typed miss — never a skeleton of a different content version.
pub(crate) struct RetainedSnapshotSkeletonSource;

impl FlowBodySkeletonSource for RetainedSnapshotSkeletonSource {
    fn build_skeleton(
        &self,
        key: &FlowSliceFunctionKey,
        resolver: &dyn ResolverContext,
    ) -> Option<FunctionBodySkeleton> {
        let serve = resolver.ensure_indexed_ready_serve(key.canonical_id.as_ref())?;
        let decl_bodies = serve.indexed.shallow_state.decl_bodies();
        let index = decl_bodies.function_program_index();
        let entry = index.get(&key.function)?;
        if entry.flow_body_stable_hash != key.flow_body_stable_hash
            || entry.flow_body_exact_hash != key.flow_body_exact_hash
        {
            // The live content version is not the pinned one: the
            // content-addressed key can only be served by its own
            // version.
            return None;
        }
        decl_bodies.function_body_skeleton(entry)
    }
}

/// One memoized per-function flow bundle: the skeleton and the graph
/// built from it, shared by every demand against the same content
/// version.
pub(crate) struct FlowGraphBundle {
    /// The arena-free body skeleton.
    pub skeleton: Arc<FunctionBodySkeleton>,
    /// The typed-edge dependence graph built once from the skeleton.
    pub graph: Arc<FunctionFlowGraph>,
}

/// The once-per-content-version graph store: `FunctionFlowGraph` (and
/// its skeleton) is built ONCE per `(canonical, function,
/// flow_body_stable_hash, parse_env_hash, parser_version)` and every
/// subsequent demand only re-plans reachability over the memoized
/// graph. Memory-side; evicted per canonical through
/// [`Self::remove_canonical`].
pub(crate) struct FunctionFlowGraphStore {
    entries: DashMap<FlowSliceFunctionKey, Arc<FlowGraphBundle>>,
    builds: AtomicU64,
}

impl FunctionFlowGraphStore {
    /// An empty store.
    pub(crate) fn new() -> Self {
        Self {
            entries: DashMap::new(),
            builds: AtomicU64::new(0),
        }
    }

    /// Get the memoized bundle for `key`, building it (skeleton via
    /// `source`, then the graph from the skeleton ALONE) exactly once
    /// per content version. Concurrent same-key builders serialize on
    /// the map entry, so one wins and the rest read its bundle.
    pub(crate) fn get_or_build(
        &self,
        key: &FlowSliceFunctionKey,
        source: &dyn FlowBodySkeletonSource,
        resolver: &dyn ResolverContext,
    ) -> Option<Arc<FlowGraphBundle>> {
        if let Some(hit) = self.entries.get(key) {
            return Some(Arc::clone(hit.value()));
        }
        match self.entries.entry(key.clone()) {
            dashmap::mapref::entry::Entry::Occupied(occupied) => Some(Arc::clone(occupied.get())),
            dashmap::mapref::entry::Entry::Vacant(vacant) => {
                let skeleton = source.build_skeleton(key, resolver)?;
                self.builds.fetch_add(1, Ordering::Relaxed);
                let graph = build_function_flow_graph(&skeleton);
                let bundle = Arc::new(FlowGraphBundle {
                    skeleton: Arc::new(skeleton),
                    graph: Arc::new(graph),
                });
                vacant.insert(Arc::clone(&bundle));
                Some(bundle)
            }
        }
    }

    /// Number of graph builds performed (observability; the
    /// once-per-content-version fixture asserts on it).
    #[cfg(test)]
    pub(crate) fn build_count(&self) -> u64 {
        self.builds.load(Ordering::Relaxed)
    }

    /// Evict every bundle of `canonical_id` (the standard
    /// `remove_canonical` cascade).
    pub(crate) fn remove_canonical(&self, canonical_id: &str) {
        self.entries
            .retain(|key, _| key.canonical_id.as_ref() != canonical_id);
    }
}

// ── Hash node ─────────────────────────────────────────────────────────

/// The hash node's caller-visible value. Only [`Self::Planned`] is ever
/// admitted; a budget trip rides `ReturnOnly` and is never published.
/// The planned arm carries EXACTLY the slice identity — the plan itself
/// is not retained on the artifact (the lowered node re-plans
/// deterministically over the same pinned content; hash-then-lower).
#[derive(Clone)]
pub(crate) enum FlowSliceHashOutcome {
    /// The planned slice's minted identity — the lowered-lookup key
    /// input.
    Planned(FlowSliceHash),
    /// The typed budget refusal — a genuine partial: returned, never
    /// admitted, and carrying NO slice hash, so the lowered store cannot
    /// even be addressed for it.
    BudgetExceeded(FlowSliceBudgetExceeded),
}

/// The shared demand-slice budget cell: ONE armed value both nodes and
/// the store share, so a constrained test host can trip the budget
/// through the FULL dispatch path while production stays at the armed
/// default. The budget is runtime configuration, never key identity.
pub(crate) type FlowSliceBudgetCell = Arc<parking_lot::RwLock<FlowSliceBudget>>;

/// The slice-identity node: plans the demand slice as graph
/// reachability over the memoized `FunctionFlowGraph` and hashes
/// exactly the selected subgraph. Content-addressed; the demand
/// identity is a key axis.
pub(crate) struct FlowSliceHashNode {
    entries: DashMap<FlowSliceHashKey, Arc<CacheEntry<FlowSliceHashOutcome>>>,
    inflight: InflightTable<QueryFlightKey<FlowSliceHashKey>>,
    graphs: Arc<FunctionFlowGraphStore>,
    skeletons: Arc<dyn FlowBodySkeletonSource>,
    budget: FlowSliceBudgetCell,
}

impl FlowSliceHashNode {
    /// A node over `graphs` + `skeletons` with the shared `budget` cell.
    pub(crate) fn new(
        graphs: Arc<FunctionFlowGraphStore>,
        skeletons: Arc<dyn FlowBodySkeletonSource>,
        budget: FlowSliceBudgetCell,
    ) -> Self {
        Self {
            entries: DashMap::new(),
            inflight: InflightTable::new(),
            graphs,
            skeletons,
            budget,
        }
    }

    /// Evict every entry of `canonical_id` (the standard
    /// `remove_canonical` cascade — memory hygiene; key identity is
    /// validity, so retained stale-canonical entries would only leak).
    pub(crate) fn remove_canonical(&self, canonical_id: &str) {
        self.entries
            .retain(|key, _| key.function.canonical_id.as_ref() != canonical_id);
    }

    /// Number of published entries (test observability).
    #[cfg(test)]
    pub(crate) fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// The published entry for `key`, when present (test observability
    /// for the non-admission assertions).
    #[cfg(test)]
    pub(crate) fn published_entry(
        &self,
        key: &FlowSliceHashKey,
    ) -> Option<Arc<CacheEntry<FlowSliceHashOutcome>>> {
        self.entries.get(key).map(|entry| Arc::clone(entry.value()))
    }
}

impl ArtifactNode for FlowSliceHashNode {
    type Key = FlowSliceHashKey;
    type Value = FlowSliceHashOutcome;

    fn entries(&self) -> &DashMap<Self::Key, Arc<CacheEntry<Self::Value>>> {
        &self.entries
    }

    fn inflight(&self) -> &InflightTable<QueryFlightKey<Self::Key>> {
        &self.inflight
    }

    fn compute(&self, key: &Self::Key, cx: &mut ComputeCtx<'_>) -> CacheAdmission<Self::Value> {
        let Some(bundle) =
            self.graphs
                .get_or_build(&key.function, self.skeletons.as_ref(), cx.resolver)
        else {
            return CacheAdmission::Failed {
                reason: NonAdmissionReason::ComputeFailed,
            };
        };
        let demand =
            SliceDemand::for_return_projection(&bundle.skeleton, &key.demand.projection_path);
        let peeker = ReturnPathPeeker::new(&bundle.graph);
        let budget = *self.budget.read();
        match peeker.plan(&demand, &budget) {
            Err(exceeded) => CacheAdmission::ReturnOnly {
                value: FlowSliceHashOutcome::BudgetExceeded(exceeded),
                reason: NonAdmissionReason::BudgetExceeded,
            },
            Ok(plan) => {
                let slice_hash = compute_flow_slice_hash(&plan, &bundle.graph, &bundle.skeleton);
                CacheAdmission::Cacheable {
                    value: FlowSliceHashOutcome::Planned(slice_hash),
                    // Content-addressed: the key pins every input, so the
                    // fact rail stays EMPTY — no slice identity ever
                    // enters `ReadSetSignature.facts`.
                    signature: ReadSetSignature::empty(),
                    self_root_canonicals: Arc::from(Vec::<Arc<str>>::new()),
                    validated_at_generation: cx.generation(),
                }
            }
        }
    }

    /// Content-addressed warm validity: the key pins the canonical, the
    /// function identity, the body content hash, the parse env, the
    /// parser version, and the demand — key identity IS validity, so a
    /// published entry serves across generations (like every
    /// content-addressed artifact family).
    fn validate(
        &self,
        _key: &Self::Key,
        entry: &CacheEntry<Self::Value>,
        _cx: &ComputeCtx<'_>,
    ) -> Option<Self::Value> {
        Some(entry.value.clone())
    }
}

// ── Lowered-body node ─────────────────────────────────────────────────

/// The lowered-slice node: lowers ONLY the planned slice into
/// [`FlowSliceIR`]. Keyed additionally on the opaque slice hash, so it
/// is unreachable until the hash node produced one; its compute
/// re-plans over the memoized graph (never a rebuild) and NEVER
/// computes a slice hash.
pub(crate) struct FlowSliceLoweredBodyNode {
    entries: DashMap<FlowSliceLoweredKey, Arc<CacheEntry<Arc<FlowSliceIR>>>>,
    inflight: InflightTable<QueryFlightKey<FlowSliceLoweredKey>>,
    graphs: Arc<FunctionFlowGraphStore>,
    skeletons: Arc<dyn FlowBodySkeletonSource>,
    budget: FlowSliceBudgetCell,
}

impl FlowSliceLoweredBodyNode {
    /// A node over the SAME `graphs` store as its hash sibling (one
    /// graph build serves both) and the same shared budget cell (the
    /// re-plan is deterministic over the pinned content).
    pub(crate) fn new(
        graphs: Arc<FunctionFlowGraphStore>,
        skeletons: Arc<dyn FlowBodySkeletonSource>,
        budget: FlowSliceBudgetCell,
    ) -> Self {
        Self {
            entries: DashMap::new(),
            inflight: InflightTable::new(),
            graphs,
            skeletons,
            budget,
        }
    }

    /// Evict every entry of `canonical_id` (the standard
    /// `remove_canonical` cascade).
    pub(crate) fn remove_canonical(&self, canonical_id: &str) {
        self.entries
            .retain(|key, _| key.hash_key.function.canonical_id.as_ref() != canonical_id);
    }

    /// Number of published entries (test observability).
    #[cfg(test)]
    pub(crate) fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

impl ArtifactNode for FlowSliceLoweredBodyNode {
    type Key = FlowSliceLoweredKey;
    type Value = Arc<FlowSliceIR>;

    fn entries(&self) -> &DashMap<Self::Key, Arc<CacheEntry<Self::Value>>> {
        &self.entries
    }

    fn inflight(&self) -> &InflightTable<QueryFlightKey<Self::Key>> {
        &self.inflight
    }

    fn compute(&self, key: &Self::Key, cx: &mut ComputeCtx<'_>) -> CacheAdmission<Self::Value> {
        let Some(bundle) =
            self.graphs
                .get_or_build(&key.hash_key.function, self.skeletons.as_ref(), cx.resolver)
        else {
            return CacheAdmission::Failed {
                reason: NonAdmissionReason::ComputeFailed,
            };
        };
        let demand = SliceDemand::for_return_projection(
            &bundle.skeleton,
            &key.hash_key.demand.projection_path,
        );
        // Re-plan reachability over the memoized graph. The key's
        // content pins (flow_body_stable_hash + parse env + parser
        // version) guarantee the same plan the hash covered; no slice
        // hash is computed here (hash-then-lower).
        let peeker = ReturnPathPeeker::new(&bundle.graph);
        let budget = *self.budget.read();
        match peeker.plan(&demand, &budget) {
            Err(_) => CacheAdmission::Failed {
                reason: NonAdmissionReason::BudgetExceeded,
            },
            Ok(plan) => CacheAdmission::Cacheable {
                value: Arc::new(lower_slice_plan(&plan, &bundle.graph, &bundle.skeleton)),
                signature: ReadSetSignature::empty(),
                self_root_canonicals: Arc::from(Vec::<Arc<str>>::new()),
                validated_at_generation: cx.generation(),
            },
        }
    }

    /// Content-addressed warm validity — see
    /// [`FlowSliceHashNode::validate`].
    fn validate(
        &self,
        _key: &Self::Key,
        entry: &CacheEntry<Self::Value>,
        _cx: &ComputeCtx<'_>,
    ) -> Option<Self::Value> {
        Some(entry.value.clone())
    }
}

// ── Project-global home ───────────────────────────────────────────────

/// The flow-slice substrate's home on the single `ProjectTypeStore`:
/// ONE shared once-per-content-version graph store, both
/// content-addressed nodes over it (one graph build serves both), the
/// production retained-snapshot skeleton source, and the shared armed
/// budget cell. Memory-side only — persistent registration of the two
/// nodes is separately owed work and nothing here builds a persistence
/// tier.
pub(crate) struct FlowSliceStores {
    graphs: Arc<FunctionFlowGraphStore>,
    /// The production skeleton producer — held so the content lowering
    /// can read the SAME memoized skeleton the plan resolved against
    /// (one lexical authority, one build per content version).
    skeletons: Arc<dyn FlowBodySkeletonSource>,
    hash_node: FlowSliceHashNode,
    lowered_node: FlowSliceLoweredBodyNode,
    /// The shared budget cell's store-side handle — held so a
    /// constrained test host can re-arm the budget the nodes read.
    #[cfg(test)]
    budget: FlowSliceBudgetCell,
}

impl FlowSliceStores {
    /// Production stores: armed default budget, retained-snapshot
    /// skeleton source, one shared graph store.
    pub(crate) fn new() -> Self {
        let graphs = Arc::new(FunctionFlowGraphStore::new());
        let skeletons: Arc<dyn FlowBodySkeletonSource> = Arc::new(RetainedSnapshotSkeletonSource);
        let budget: FlowSliceBudgetCell =
            Arc::new(parking_lot::RwLock::new(FlowSliceBudget::default()));
        let hash_node = FlowSliceHashNode::new(
            Arc::clone(&graphs),
            Arc::clone(&skeletons),
            Arc::clone(&budget),
        );
        let lowered_node =
            FlowSliceLoweredBodyNode::new(graphs.clone(), Arc::clone(&skeletons), budget.clone());
        #[cfg(not(test))]
        drop(budget);
        Self {
            graphs,
            skeletons,
            hash_node,
            lowered_node,
            #[cfg(test)]
            budget,
        }
    }

    /// The memoized [`FunctionBodySkeleton`] of one function content
    /// version — the SAME artifact the demand plan resolved its lexical
    /// edges against, so the content lowering and the plan share ONE
    /// binding authority. Built at most once per content version (the
    /// graph store owns the memoization); `None` when the position is
    /// not served at exactly the pinned version.
    pub(crate) fn skeleton_for(
        &self,
        key: &FlowSliceFunctionKey,
        resolver: &dyn ResolverContext,
    ) -> Option<Arc<FunctionBodySkeleton>> {
        self.graphs
            .get_or_build(key, self.skeletons.as_ref(), resolver)
            .map(|bundle| Arc::clone(&bundle.skeleton))
    }

    /// The slice-identity node (plan + hash; the fourth budget layer's
    /// outcome producer).
    pub(crate) fn hash_node(&self) -> &FlowSliceHashNode {
        &self.hash_node
    }

    /// The lowered-slice node (hash-keyed, unreachable without a minted
    /// slice hash).
    pub(crate) fn lowered_node(&self) -> &FlowSliceLoweredBodyNode {
        &self.lowered_node
    }

    /// The shared once-per-content-version graph store (test
    /// observability: the once-per-content-version fixtures assert on
    /// its build count).
    #[cfg(test)]
    pub(crate) fn graphs(&self) -> &Arc<FunctionFlowGraphStore> {
        &self.graphs
    }

    /// Evict every flow-slice artifact of `canonical_id` (the standard
    /// `remove_canonical` cascade: graph bundles, hash entries, lowered
    /// entries).
    pub(crate) fn remove_canonical(&self, canonical_id: &str) {
        self.graphs.remove_canonical(canonical_id);
        self.hash_node.remove_canonical(canonical_id);
        self.lowered_node.remove_canonical(canonical_id);
    }

    /// Replace the shared budget (test-support only): lets a constrained
    /// host trip the budget through the FULL dispatch path. Production
    /// never rewrites the armed default.
    #[cfg(test)]
    pub(crate) fn set_budget_for_test(&self, budget: FlowSliceBudget) {
        *self.budget.write() = budget;
    }
}

impl Default for FlowSliceStores {
    fn default() -> Self {
        Self::new()
    }
}
