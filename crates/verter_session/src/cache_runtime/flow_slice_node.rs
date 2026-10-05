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
//! [`FunctionFlowGraph`]) ONCE per cold demand, hashes the selected
//! subgraph, and RETAINS the plan on the published outcome
//! ([`PlannedFlowSlice`]); the lowered node's compute lowers exactly that
//! retained plan — it never re-plans and never computes a slice hash.
//!
//! Both nodes are CONTENT-ADDRESSED memory-side [`crate::cache_runtime::node::ArtifactNode`]s: the
//! key pins the canonical, the five-axis function identity, the
//! body-sensitive / cosmetic-insensitive `flow_body_stable_hash`, the
//! EXACT per-function byte hash, the parse-env hash, the exact parse
//! identity, the runtime-authoritative file language row, and the
//! demand identity, so key identity IS validity and no fact rail
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
//! request-local engine driver, and the shared armed
//! [`FlowSliceBudget`] cell. The `FlowReturn` executor consumes the hash
//! node on its cold path (the budget outcome gates memo admission); the
//! lowered node serves slice-IR demand through the same store.
use verter_session_query::flow::bundle::{
    BoundFlowGraph, FlowGraphBundle, FlowSliceFunctionKey, KeyedFunctionStructure,
};

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;

use verter_session_query::flow::flow_ir::{FlowSliceIR, ReturnSlicePlan};
use verter_session_query::flow::hashing::FlowSliceHash;
use verter_session_query::flow::peeker::{FlowSliceBudget, FlowSliceBudgetExceeded};
use verter_session_query::flow::skeleton::{FunctionBodySkeleton, PreparedFunctionBodySkeleton};

use super::admission::CacheEntry;
use super::node::QueryFlightKey;
use super::singleflight::InflightTable;

#[cfg(test)]
#[path = "flow_slice_node_tests.rs"]
pub(crate) mod tests;

// ── Keys ──────────────────────────────────────────────────────────────

/// The demand identity of one slice: the demanded return-projection
/// path (empty = whole return). Further demand axes (the
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

/// The once-per-content-version graph store: `FunctionFlowGraph`, its
/// skeleton, and binding map are built once per `(canonical, function,
/// flow_body_stable_hash, flow_body_exact_hash, parse_env_hash,
/// parse_key, file_language, toolchain)` and every
/// subsequent demand only re-plans reachability over the memoized
/// graph. Memory-side; evicted per canonical through
/// [`Self::remove_canonical`].
pub(crate) struct FunctionFlowGraphStore {
    entries: DashMap<FlowSliceFunctionKey, Arc<FlowGraphBundle>>,
    builds: AtomicU64,
}

/// A storage claim; the producer retains the existing shard lock until it
/// publishes or abandons the lease. Storage never drives source work.
// Keep the inline publication guard: boxing it would add a cold-graph allocation.
#[allow(clippy::large_enum_variant)]
pub(crate) enum GraphClaim<'a> {
    Read(Arc<FlowGraphBundle>),
    Produce(GraphPublish<'a>),
}

pub(crate) struct GraphPublish<'a> {
    slot: dashmap::mapref::entry::VacantEntry<'a, FlowSliceFunctionKey, Arc<FlowGraphBundle>>,
    builds: &'a AtomicU64,
}

impl GraphPublish<'_> {
    /// Publish `bundle` under the claimed key. A bundle built for any other
    /// key is refused — `None`, nothing published, the claim abandoned —
    /// so a slot only ever holds the product of its own key.
    pub(crate) fn publish(self, bundle: FlowGraphBundle) -> Option<Arc<FlowGraphBundle>> {
        if bundle.key() != self.slot.key() {
            return None;
        }
        self.builds.fetch_add(1, Ordering::Relaxed);
        let bundle = Arc::new(bundle);
        self.slot.insert(Arc::clone(&bundle));
        Some(bundle)
    }
}

impl FunctionFlowGraphStore {
    /// An empty store.
    pub(crate) fn new() -> Self {
        Self {
            entries: DashMap::new(),
            builds: AtomicU64::new(0),
        }
    }

    /// Read the memoized bundle or claim its publication slot. Concurrent
    /// same-key producers serialize on the map entry. The engine driver
    /// requests owned lowering while holding the claim; a failed or missing
    /// source product abandons it without publishing an absence.
    pub(crate) fn claim(&self, key: &FlowSliceFunctionKey) -> GraphClaim<'_> {
        if let Some(hit) = self.entries.get(key) {
            return GraphClaim::Read(Arc::clone(hit.value()));
        }
        match self.entries.entry(key.clone()) {
            dashmap::mapref::entry::Entry::Occupied(occupied) => {
                GraphClaim::Read(Arc::clone(occupied.get()))
            }
            dashmap::mapref::entry::Entry::Vacant(slot) => GraphClaim::Produce(GraphPublish {
                slot,
                builds: &self.builds,
            }),
        }
    }

    /// Seal the ALREADY-MEMOIZED bundle for `key` into a bound graph —
    /// the production admission seam for the demand planner. `None` when
    /// no bundle is memoized for the key: the hash node's cold compute is
    /// the only builder, so a miss here is a torn view between the hash
    /// node and the graph store, never a reason to build.
    pub(crate) fn bound_graph(&self, key: &FlowSliceFunctionKey) -> Option<BoundFlowGraph> {
        self.peek(key).map(BoundFlowGraph::new)
    }

    /// Seal a fixture built through the production indexed structural owner,
    /// retaining its already-prepared binding map without rebuilding it.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn mint_bound_flow_graph(
        &self,
        key: FlowSliceFunctionKey,
        prepared: PreparedFunctionBodySkeleton,
    ) -> BoundFlowGraph {
        let structure = KeyedFunctionStructure::bind_fixture(key.clone(), prepared);
        let bundle = match self.entries.entry(key) {
            dashmap::mapref::entry::Entry::Occupied(occupied) => Arc::clone(occupied.get()),
            dashmap::mapref::entry::Entry::Vacant(vacant) => {
                let bundle = Arc::new(FlowGraphBundle::build(structure));
                self.builds.fetch_add(1, Ordering::Relaxed);
                vacant.insert(Arc::clone(&bundle));
                bundle
            }
        };
        BoundFlowGraph::new(bundle)
    }

    /// Non-blocking peek at the already-memoized bundle for `key` — the
    /// `ResolverObservation::function_body_skeleton` backing
    /// primitive. NEVER calls `source.build_bundle`/
    /// `resolver.ensure_indexed_ready_serve` (the blocking cold path
    /// the engine driver probes on a miss): a plain `DashMap::get`,
    /// same shape as `FileArtifactStore::get_augmenter_set`. `None` means
    /// "not yet built for this content version" — the caller drives
    /// the engine driver's blocking build to resolve it. Unavailable source
    /// versions and correspondence errors never enter this store.
    pub(crate) fn peek(&self, key: &FlowSliceFunctionKey) -> Option<Arc<FlowGraphBundle>> {
        self.entries.get(key).map(|hit| Arc::clone(hit.value()))
    }

    /// Number of retained graph bundles (retention observability).
    pub(crate) fn entry_count(&self) -> usize {
        self.entries.len()
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

/// The one retained structural plan of a cold demand: the minted slice
/// identity plus the [`ReturnSlicePlan`] it was minted from. Planning runs
/// exactly once per cold demand (the hash node's compute); the lowered
/// node and the demand-plan assembly both consume THIS retained plan
/// instead of re-planning. Fields are private — immutable views only.
///
/// `pub` only so the hermetic test surface (`crate::for_tests`) can name
/// the carrier; the module path stays crate-private, so production code
/// outside this module tree cannot reach it.
pub struct PlannedFlowSlice {
    /// The planner-produced slice identity (the lowered-lookup key input).
    hash: FlowSliceHash,
    /// The structural selection the hash covers.
    selection: ReturnSlicePlan,
}

impl PlannedFlowSlice {
    /// Seal a minted slice identity to the selection it was minted from.
    /// Production construction belongs to this cache owner, immediately
    /// after the peeker produces its sealed selection and the hasher mints
    /// the identity over that selection.
    #[must_use]
    pub(crate) fn new(hash: FlowSliceHash, selection: ReturnSlicePlan) -> Self {
        Self { hash, selection }
    }

    /// Deliberately mismatched fixture pairs exercise provenance rejection.
    #[cfg(any(test, feature = "test-support"))]
    pub fn for_test(hash: FlowSliceHash, selection: ReturnSlicePlan) -> Self {
        Self::new(hash, selection)
    }

    /// The minted slice identity.
    pub fn hash(&self) -> FlowSliceHash {
        self.hash
    }

    /// The retained structural selection (planned once).
    pub fn selection(&self) -> &ReturnSlicePlan {
        &self.selection
    }
}

/// The hash node's caller-visible value. Only [`Self::Planned`] is ever
/// admitted; a budget trip rides `ReturnOnly` and is never published.
/// The planned arm carries the minted slice identity AND the retained
/// plan it was minted from, so lowering and demand planning share the one
/// cold planning run (hash-then-lower without a re-plan).
#[derive(Clone)]
pub(crate) enum FlowSliceHashOutcome {
    /// The planned slice: minted identity plus the retained plan.
    Planned(Arc<PlannedFlowSlice>),
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
    pub(crate) entries: DashMap<FlowSliceHashKey, Arc<CacheEntry<FlowSliceHashOutcome>>>,
    pub(crate) inflight: InflightTable<QueryFlightKey<FlowSliceHashKey>>,
    pub(crate) budget: FlowSliceBudgetCell,
}

impl FlowSliceHashNode {
    pub(crate) fn new(budget: FlowSliceBudgetCell) -> Self {
        Self {
            entries: DashMap::new(),
            inflight: InflightTable::new(),
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

    /// Number of published entries (retention observability).
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

    /// The retained plan of the published `Planned` outcome for `key`,
    /// when its minted hash still matches `slice_hash`. `None` when the
    /// entry is absent, evicted (`remove_canonical`), or names another
    /// plan — the lowered node fails closed on each, never re-planning.
    pub(crate) fn retained_plan(
        &self,
        key: &FlowSliceHashKey,
        slice_hash: FlowSliceHash,
    ) -> Option<Arc<PlannedFlowSlice>> {
        let entry = self.entries.get(key)?;
        match &entry.value {
            FlowSliceHashOutcome::Planned(planned) if planned.hash == slice_hash => {
                Some(Arc::clone(planned))
            }
            _ => None,
        }
    }
}

// ── Lowered-body node ─────────────────────────────────────────────────

/// The lowered-slice node: lowers ONLY the planned slice into
/// [`FlowSliceIR`]. Keyed additionally on the opaque slice hash, so it
/// is unreachable until the hash node produced one; its compute lowers
/// the hash node's RETAINED plan (the one cold planning run — it never
/// re-plans) and NEVER computes a slice hash.
pub(crate) struct FlowSliceLoweredBodyNode {
    pub(crate) entries: DashMap<FlowSliceLoweredKey, Arc<CacheEntry<Arc<FlowSliceIR>>>>,
    pub(crate) inflight: InflightTable<QueryFlightKey<FlowSliceLoweredKey>>,
}

impl FlowSliceLoweredBodyNode {
    pub(crate) fn new() -> Self {
        Self {
            entries: DashMap::new(),
            inflight: InflightTable::new(),
        }
    }

    /// Evict every entry of `canonical_id` (the standard
    /// `remove_canonical` cascade).
    pub(crate) fn remove_canonical(&self, canonical_id: &str) {
        self.entries
            .retain(|key, _| key.hash_key.function.canonical_id.as_ref() != canonical_id);
    }

    /// Number of published entries (retention observability).
    pub(crate) fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

// ── Project-global home ───────────────────────────────────────────────

/// The flow-slice substrate's home on the single `ProjectTypeStore`:
/// ONE shared once-per-content-version graph store, both
/// content-addressed stores over it (one graph build serves both), and the
/// shared armed budget cell. Source acquisition and artifact computation
/// belong to the request-local engine driver. Memory-side only — persistent registration of the two
/// nodes is separately owed work and nothing here builds a persistence
/// tier.
pub(crate) struct FlowSliceStores {
    pub(crate) graphs: Arc<FunctionFlowGraphStore>,
    hash_node: Arc<FlowSliceHashNode>,
    lowered_node: FlowSliceLoweredBodyNode,
    /// Number of demand plans built against this store (observability;
    /// the once-per-cold-demand fixture asserts warm replay and non-flow
    /// dispatch add zero).
    demand_plans: AtomicU64,
    /// The shared budget cell's store-side handle — held so a
    /// constrained test host can re-arm the budget the nodes read.
    #[cfg(test)]
    budget: FlowSliceBudgetCell,
}

impl FlowSliceStores {
    /// Production storage: armed default budget and one shared graph store.
    pub(crate) fn new() -> Self {
        let graphs = Arc::new(FunctionFlowGraphStore::new());
        let budget: FlowSliceBudgetCell =
            Arc::new(parking_lot::RwLock::new(FlowSliceBudget::default()));
        let hash_node = Arc::new(FlowSliceHashNode::new(Arc::clone(&budget)));
        let lowered_node = FlowSliceLoweredBodyNode::new();
        #[cfg(not(test))]
        drop(budget);
        Self {
            graphs,
            hash_node,
            lowered_node,
            demand_plans: AtomicU64::new(0),
            #[cfg(test)]
            budget,
        }
    }

    /// Record one demand plan built against this store.
    pub(crate) fn note_demand_planned(&self) {
        self.demand_plans.fetch_add(1, Ordering::Relaxed);
    }

    /// Number of demand plans built (observability; the
    /// once-per-cold-demand fixture asserts on it).
    #[cfg(test)]
    pub(crate) fn demand_plan_count(&self) -> u64 {
        self.demand_plans.load(Ordering::Relaxed)
    }

    /// Non-blocking peek at the memoized [`FunctionBodySkeleton`] of one
    /// function content version — the backing primitive of
    /// `ResolverObservation::function_body_skeleton`. Unlike
    /// the engine driver, NEVER drives the blocking
    /// owned-lowering demand cold build (`ensure_indexed_ready_serve`
    /// and `DeclLoweringService::acquire_lease`'s worker-thread rendezvous):
    /// `None` means "not yet built for this content version," not a
    /// resolved absence — a caller that needs the resolved value falls
    /// back to `skeleton_for`'s blocking path.
    #[allow(dead_code)]
    pub(crate) fn peek_skeleton_for(
        &self,
        key: &FlowSliceFunctionKey,
    ) -> Option<Arc<FunctionBodySkeleton>> {
        self.graphs
            .peek(key)
            .map(|bundle| Arc::clone(bundle.skeleton()))
    }

    /// The store-minted bound graph of one function content version — the
    /// completeness-proof layer's graph handle, sealed to the memoized
    /// bundle. `None` when the bundle is not yet built (the caller's
    /// hash-node lookup is the builder; a miss is a torn view).
    pub(crate) fn bound_graph_for(&self, key: &FlowSliceFunctionKey) -> Option<BoundFlowGraph> {
        self.graphs.bound_graph(key)
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

    /// Number of retained graph bundles (retention observability).
    pub(crate) fn graphs_entry_count(&self) -> usize {
        self.graphs.entry_count()
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
