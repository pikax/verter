//! Final component-meta result cache.
//!
//! [`ComponentMetaResultDb`] is the authoritative final-payload cache for
//! component-meta results. Identical repeated requests on an unchanged
//! owner return from this cache with near-zero resolver work; concurrent
//! cold requests for the same owner/query coalesce onto one build.
//!
//! ## Contract
//!
//! - **Slot key:** [`ComponentMetaResultKey`] =
//!   `(owner_canonical, options_fingerprint, project_identity,
//!   parse_env_hash, resolve_env_hash, type_env_hash, lib_env_hash)` —
//!   content-free with the full R21 split env axes, per the
//!   query-identity-cache model. The owner's content version
//!   (`owner_whole_hash`) is NOT part of the slot key; it is the
//!   candidate discriminant, carried by the candidate and validated
//!   strictly on read.
//! - **Slot value:** a bounded candidate list. Concurrent overlay
//!   variants of the same owner coexist as candidates in one slot,
//!   capped at [`ComponentMetaResultDb::PER_SLOT_CANDIDATE_CAP`]; a
//!   global insertion-ordered budget
//!   ([`ComponentMetaResultDb::GLOBAL_BUDGET`]) caps the total candidate
//!   count across all slots. Eviction is FIFO (oldest insertion first);
//!   evicting a still-valid candidate only forces a recompute. The
//!   bounded substrate is the routine memory-reclamation path — old
//!   per-version entries do not accumulate unbounded in a long-lived
//!   session.
//! - **Candidate payload:** an immutable `Arc` payload — the native
//!   component-meta result and any strictly projected derivatives — plus
//!   the exact [`verter_session_query::facts::fact_cache::ReadSetSignature`] the
//!   build observed. Lookups revalidate that signature against the live
//!   host.
//! - **`options_fingerprint` is a stable `Hash16`** produced from a
//!   manually-stable serialization of output-affecting fields only —
//!   never request ids, trace flags, or caller metadata.
//! - Successful candidates are complete facts only. A certified divergence,
//!   an operation-profile refusal, or an isolated-root ledger refusal is
//!   retained in the refusal ledger with its original status and recovery,
//!   never as a successful candidate. Any other approximation, a missing
//!   input, and an execution abort are not retained.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use rustc_hash::FxHashMap;
use verter_session_query::analysis::types::Hash16;
use verter_session_query::facts::fact_cache::ReadSetSignature;
use verter_session_query::retention::RetentionCharge;

use crate::bounded_query_retention::BoundedCandidateMap;
use crate::project_semantic_dispatch::cost_receipt::BudgetProfileSpec;
use crate::request_budget::RequestSpent;
use crate::semantic_query::{FactStatus, PartialReason, PartialReasonSet, ResultCompleteness};

/// How many final-component refusals the ledger keeps. The oldest is dropped
/// first. Successful candidates use the candidate budget and are not counted
/// here.
const REFUSAL_LEDGER_CAP: usize = 256;

/// Stable fingerprint over output-affecting options. Constructed by the
/// caller from an explicitly versioned serialization; the type alias
/// points at the workspace-wide [`Hash16`] so downstream tooling does not
/// invent a parallel hash.
pub type ComponentMetaOptionsFingerprint = Hash16;

/// Content-free slot key for the final component-meta result.
///
/// "Content-free" per R6: the owner's content version
/// (`owner_whole_hash`) is intentionally absent — concurrent content
/// versions of the same owner coexist as candidates inside the slot this
/// key addresses (the documented query-identity-cache model), discriminated
/// value-side by the candidate's owner whole-hash.
///
/// The key DOES carry the split env axes (R21): `project_identity` plus
/// the four `*_env_hash` dimensions. The final component-meta payload
/// depends on parse/resolve/type/lib env and the owning project, so
/// concurrent env or project variants of the same owner must occupy
/// DISTINCT slots — without these axes a lookup under project/env A could
/// alias onto a slot computed under project/env B. The axes are
/// view-independent (they key on the owning project, not file content),
/// so a single canonical builder
/// (the session key builder) yields identical
/// axes at both the lookup and publish sites.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ComponentMetaResultKey {
    pub owner_canonical: Arc<str>,
    pub options_fingerprint: ComponentMetaOptionsFingerprint,
    /// Owning-project identity (R21). Prevents cross-project aliasing of
    /// otherwise identical `(owner, options)` slots.
    pub project_identity: verter_session_query::resolution::ProjectIdentity,
    /// Split env axes (R21). The component-meta result depends on all
    /// four — parse (SFC/compiler flags), resolve (module resolution),
    /// type (semantic options), and lib (intrinsic corpus).
    pub parse_env_hash: Hash16,
    pub resolve_env_hash: Hash16,
    pub type_env_hash: Hash16,
    pub lib_env_hash: Hash16,
}

/// Cache entry — the payload plus the carrier holding the
/// path-precise fact signature.
///
/// `read_set_signature.facts` is the path-precise signature produced
/// by the engine-owned raw fact-tracer scope wrapping cold compute — the
/// primary cache-validity rail. Warm-hit reads gate on
/// the request-bound `FactValidation::validates_fact_signature`: a fact-version bump on any
/// cross-file dep invalidates the warm hit.
///
/// `validated_at_generation` is the project-generation snapshot the
/// producer captured before its cold compute dispatched any work. The
/// `read_set_signature` carrier validates only file-content
/// whole-hashes; a `ProjectGeneration` reset (tsconfig / path-alias /
/// SDK / workspace-folder change) bumps no file content, so without
/// this stamp a stale-by-project-generation entry that raced a
/// `bump_project_generation_and_evict` cold-publish window would
/// validate forever on file-content terms.
/// The engine read operation rejects entries whose generation or fact signature
/// no longer validates through the request's `FactValidation` port. The
/// passive test-only [`ComponentMetaResultDb::get`] checks only the candidate
/// version; production reads use `ProjectSemanticDispatch::read_component_meta_result`.
pub struct ComponentMetaResultEntry<P> {
    pub payload: Arc<P>,
    pub read_set_signature: verter_session_query::facts::fact_cache::ReadSetSignature,
    pub validated_at_generation: u64,
}

/// Per-record byte estimates for the component-meta footprint.
///
/// Each constant is the approximate resident cost of one record of that
/// family — the struct plus the short owned strings and typed IR it
/// keeps alive. These are ACCOUNTING estimates: they decide whether the
/// process may RETAIN an entry, never whether a retained entry is valid.
/// An imprecise constant therefore costs hit rate, never correctness.
pub mod footprint {
    /// A surface record with a resolved type descriptor: a prop, event,
    /// slot, model, exposed member, or accepted-surface entry.
    pub const SURFACE_RECORD_BYTES: usize = 512;
    /// A lighter structural record: an import, binding, template ref,
    /// component usage, API call, or style block.
    pub const STRUCTURAL_RECORD_BYTES: usize = 192;
    /// A resolved type-registry analysis, which carries an expanded
    /// member list and is the heaviest per-record family.
    pub const TYPE_RECORD_BYTES: usize = 1024;
}

impl<P> verter_session_query::retention::RetainedFootprint for ComponentMetaResultEntry<P>
where
    P: verter_session_query::retention::RetainedFootprint,
{
    /// The payload, the entry's validity rail — its fact slice with every
    /// fact's owned text — and the entry's slot.
    fn retained_footprint_bytes(&self) -> usize {
        self.payload.retained_footprint_bytes()
            + verter_session_query::retention::allocation::fact_signature_bytes(
                &self.read_set_signature,
                &mut verter_session_query::retention::allocation::AliasSet::new(),
            )
            + verter_session_query::retention::ENTRY_OVERHEAD_BYTES
    }
}

impl<P> Clone for ComponentMetaResultEntry<P> {
    fn clone(&self) -> Self {
        Self {
            payload: self.payload.clone(),
            read_set_signature: self.read_set_signature.clone(),
            validated_at_generation: self.validated_at_generation,
        }
    }
}

/// Which eligible refusal a final component demand retained.
///
/// These stay refusals. A retained recovery is never a successful candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ComponentMetaRefusalClass {
    /// The producer's deterministic divergence certificate.
    CertifiedDivergence,
    /// The demand stopped under this operation profile.
    OperationProfile,
    /// An isolated root stopped under this profile, allowance and context.
    IsolatedRoot,
}

/// The request axes a refusal was sealed under, already normalized for its
/// class. Certified divergence ignores profile. Every class keeps the entry
/// allowance of the demand that sealed it: a different allowance is a
/// different refusal. An isolated-root refusal also keeps profile.
///
/// `profile` is the full budget profile (work, bytes, depth, tail,
/// request operations, relation comparisons, cost-model revision).
/// `entry_allowance` is the projection operations already spent when the
/// demand entered, not the paid-set size and not the operations still
/// remaining. Remaining allowance is that pair: the cap lives in
/// `profile.request_operations`, the spent count lives here.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ComponentMetaRefusalIdentity {
    pub class: ComponentMetaRefusalClass,
    pub profile: BudgetProfileSpec,
    pub root: Arc<str>,
    pub entry_allowance: u64,
    pub context: u64,
}

/// The axes of the demand asking to reuse a refusal.
///
/// `entered_isolated` is the entry shape (no operations spent and an
/// empty paid set). It is not part of the identity. Depth isolation also
/// requires the compute's own admission proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentMetaRefusalQuery {
    pub root: Arc<str>,
    pub profile: BudgetProfileSpec,
    pub entry_allowance: u64,
    pub context: u64,
    pub entered_isolated: bool,
}

impl ComponentMetaRefusalQuery {
    /// The ledger identities this demand may reuse, in class order.
    #[must_use]
    pub fn reusable_identities(&self) -> [ComponentMetaRefusalIdentity; 3] {
        [
            refusal_identity_for(ComponentMetaRefusalClass::CertifiedDivergence, self),
            refusal_identity_for(ComponentMetaRefusalClass::OperationProfile, self),
            refusal_identity_for(ComponentMetaRefusalClass::IsolatedRoot, self),
        ]
    }
}

/// Normalize `class` onto `query`. A different root, context, or entry
/// allowance produces a different identity. Certified divergence ignores
/// profile. An operation-profile or isolated-root refusal also keeps profile.
/// A higher entry allowance does not reuse a refusal sealed at a lower one.
#[must_use]
pub fn refusal_identity_for(
    class: ComponentMetaRefusalClass,
    query: &ComponentMetaRefusalQuery,
) -> ComponentMetaRefusalIdentity {
    match class {
        ComponentMetaRefusalClass::CertifiedDivergence => ComponentMetaRefusalIdentity {
            class,
            profile: BudgetProfileSpec::ignored(),
            root: Arc::clone(&query.root),
            entry_allowance: query.entry_allowance,
            context: query.context,
        },
        ComponentMetaRefusalClass::OperationProfile | ComponentMetaRefusalClass::IsolatedRoot => {
            ComponentMetaRefusalIdentity {
                class,
                profile: query.profile,
                root: Arc::clone(&query.root),
                entry_allowance: query.entry_allowance,
                context: query.context,
            }
        }
    }
}

/// Whether a finished final-component compute may be published or retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalComponentRetention {
    /// The demanded facts are complete.
    Complete,
    /// Retain this refusal class. Not a successful value.
    Retain(ComponentMetaRefusalClass),
    /// Approximate, missing-input, abort, or any other non-eligible outcome.
    NotAdmitted,
}

/// Proof that a final-component outcome is one of the retained classes.
///
/// A same-path recursion bit is not a divergence certificate: the graph
/// also records a cross-task wait cycle under that bit. Certified
/// divergence requires `divergence_certificate`. A schedule cut is the
/// wait cycle's accounting signal and admits nothing. `entered_isolated`
/// is true only when the demand entered alone and its own spend was the
/// request's spend. `request_spend_owned` is that same ownership for an
/// operation-profile refusal: a sibling thread's spend is not this
/// compute's allowance, and the refusal is not retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FinalComponentAdmissionProof {
    pub divergence_certificate: bool,
    pub schedule_cut: bool,
    pub entered_isolated: bool,
    /// This compute's recorded request spend was its own, or there was no
    /// request to interleave with. An operation-profile refusal is not
    /// retained when this is false.
    pub request_spend_owned: bool,
}

impl FinalComponentAdmissionProof {
    /// Depth may be an isolated root. No divergence certificate.
    #[must_use]
    pub const fn isolated(entered_isolated: bool) -> Self {
        Self {
            divergence_certificate: false,
            schedule_cut: false,
            entered_isolated,
            request_spend_owned: true,
        }
    }

    /// A producer certificate with no schedule cut.
    #[must_use]
    pub const fn certified() -> Self {
        Self {
            divergence_certificate: true,
            schedule_cut: false,
            entered_isolated: false,
            request_spend_owned: true,
        }
    }
}

/// What a refused final-component compute did to the demand that ran it.
/// Stored beside the refusal, not in its identity. A replay applies this
/// before it delivers the recovery, and misses when the request is not at
/// the entered state.
#[derive(Debug, Clone)]
pub(crate) struct ComponentMetaRefusalEffects {
    pub(crate) request: Option<RequestSpent>,
    pub(crate) work: usize,
    pub(crate) bytes: u64,
    pub(crate) trip: PartialReasonSet,
}

impl Default for ComponentMetaRefusalEffects {
    fn default() -> Self {
        Self {
            request: None,
            work: 0,
            bytes: 0,
            trip: PartialReasonSet::empty(),
        }
    }
}

/// Classify `completeness` for the final component store.
///
/// [`PartialReason::Propagated`] is the boolean-bridge marker: a child
/// partiality folded without a class of its own. It is not a second cause.
/// A sole eligible class stays that class when the marker is also present.
/// A set that contains only the marker stays unadmitted, and any real other
/// reason (a missing input, an abort, a flow-return cause) still vetoes.
///
/// [`PartialReason::ConnectedMemoryLimit`] is context-dependent: it is not
/// an operation-profile reason, and it vetoes retention even when a real
/// operation reason is also present. [`PartialReason::SamePathRecursion`]
/// is certified divergence only when `proof.divergence_certificate` is set
/// and no schedule cut, operation, depth, or other reason is present.
/// Without that certificate the recursion bit vetoes, including when it is
/// the wait-cycle the graph records under the same bit.
#[must_use]
pub fn classify_final_component_retention(
    completeness: ResultCompleteness,
    proof: FinalComponentAdmissionProof,
) -> FinalComponentRetention {
    if !completeness.is_partial() {
        return FinalComponentRetention::Complete;
    }
    if proof.schedule_cut {
        return FinalComponentRetention::NotAdmitted;
    }
    let mut operation = false;
    let mut recursion = false;
    let mut depth = false;
    let mut other = false;
    for reason in completeness.reasons().iter() {
        match reason {
            PartialReason::BudgetExceeded
            | PartialReason::OperationBudget
            | PartialReason::ProjectionWorkLimit
            | PartialReason::DeferredEvaluationLimit
            | PartialReason::StructuralFactDemandLimit => operation = true,
            PartialReason::ConnectedMemoryLimit => other = true,
            PartialReason::SamePathRecursion => recursion = true,
            PartialReason::ConnectedQueryDepthLimit => depth = true,
            // Spread marker only. See the function doc.
            PartialReason::Propagated => {}
            PartialReason::Cancelled
            | PartialReason::SupersededGeneration
            | PartialReason::UnstableState
            | PartialReason::WalkerFatal
            | PartialReason::SemanticQueryFault
            | PartialReason::MissingSemanticNodeData
            | PartialReason::MissingDependency
            | PartialReason::FlowReturnUninferred
            | PartialReason::FlowReturnUnverified
            | PartialReason::FlowReturnNoSurface
            | PartialReason::UndecidedConditional => other = true,
        }
    }
    if proof.divergence_certificate && !other && !operation && !depth {
        return FinalComponentRetention::Retain(ComponentMetaRefusalClass::CertifiedDivergence);
    }
    if other || recursion || (!operation && !depth) {
        return FinalComponentRetention::NotAdmitted;
    }
    if depth && proof.entered_isolated && !operation {
        return FinalComponentRetention::Retain(ComponentMetaRefusalClass::IsolatedRoot);
    }
    if operation && !depth && proof.request_spend_owned {
        return FinalComponentRetention::Retain(ComponentMetaRefusalClass::OperationProfile);
    }
    FinalComponentRetention::NotAdmitted
}

/// The refusal status of a partial reason set. `None` when the set is empty.
#[must_use]
pub fn final_component_refusal_status(reasons: PartialReasonSet) -> Option<FactStatus> {
    crate::semantic_query::surface_resolution::NonEmptyReasons::new(reasons)
        .map(FactStatus::Unavailable)
}

type RefusalSlot = (ComponentMetaResultKey, Hash16, ComponentMetaRefusalIdentity);

/// Recovery, signature, and the one retention charge. Every handle a
/// caller extracts shares this allocation, so dropping the ledger entry
/// does not release the bytes while a recovery or a signature is still
/// held.
#[derive(Debug)]
struct SharedRefusal<P> {
    recovery: P,
    signature: ReadSetSignature,
    /// Drop releases the retention charge. The field is never read.
    #[allow(dead_code)]
    retention: Option<RetentionCharge>,
}

/// A recovery handle. Dereferences to the refused payload. Cloning keeps
/// the retention charge alive.
#[derive(Debug)]
pub struct Charged<P> {
    shared: Arc<SharedRefusal<P>>,
}

impl<P> Clone for Charged<P> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<P> std::ops::Deref for Charged<P> {
    type Target = P;

    fn deref(&self) -> &P {
        &self.shared.recovery
    }
}

/// A signature handle over the same allocation as [`Charged`]. Cloning
/// keeps the retention charge alive. Dereferences to the read-set signature.
#[derive(Debug)]
pub struct ChargedSignature<P> {
    shared: Arc<SharedRefusal<P>>,
}

impl<P> Clone for ChargedSignature<P> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<P> std::ops::Deref for ChargedSignature<P> {
    type Target = ReadSetSignature;

    fn deref(&self) -> &ReadSetSignature {
        &self.shared.signature
    }
}

struct SealedComponentMetaRefusal<P> {
    status: FactStatus,
    completeness: ResultCompleteness,
    shared: Arc<SharedRefusal<P>>,
    effects: ComponentMetaRefusalEffects,
    validated_at_generation: u64,
}

struct ComponentMetaRefusalLedger<P> {
    entries: FxHashMap<RefusalSlot, SealedComponentMetaRefusal<P>>,
    order: VecDeque<RefusalSlot>,
}

impl<P> Default for ComponentMetaRefusalLedger<P> {
    fn default() -> Self {
        Self {
            entries: FxHashMap::default(),
            order: VecDeque::new(),
        }
    }
}

/// A retained final-component refusal. Its recovery is the original refused
/// outcome, not a successful cache candidate.
#[derive(Clone)]
pub struct ComponentMetaRetainedRefusal<P> {
    pub identity: ComponentMetaRefusalIdentity,
    pub status: FactStatus,
    pub completeness: ResultCompleteness,
    pub recovery: Charged<P>,
    pub read_set_signature: ChargedSignature<P>,
    pub validated_at_generation: u64,
}

/// Host-owned final result cache. Generic over the payload type so native
/// and compat projections can share the same backing without double-caching
/// semantic meaning.
///
/// Backed by [`BoundedCandidateMap`]: the slot key is the content-free
/// [`ComponentMetaResultKey`], the per-candidate discriminant is the
/// owner whole-hash, and the candidate payload is a
/// [`ComponentMetaResultEntry`]. Per-slot and global caps reclaim old
/// per-version entries write-side so a long-lived session does not grow
/// the cache monotonically with the owner edit count.
pub struct ComponentMetaResultDb<P> {
    inner: BoundedCandidateMap<ComponentMetaResultKey, Hash16, ComponentMetaResultEntry<P>>,
    /// Tracks the substrate's live candidate count for the
    /// `ProjectTypeStore` counter snapshot. Maintained by net-delta
    /// accounting ([`Self::apply_live_delta`]): every mutation applies
    /// the exact `added - removed` delta via atomic `fetch_add` /
    /// `fetch_sub`, so concurrent mutations compose without an absolute
    /// snapshot clobbering a newer count.
    live_counter: Arc<AtomicU64>,
    /// Counts every eviction / removal — replaces the historical
    /// "stale sweep" counter.
    #[cfg(any(test, feature = "semantic-observe"))]
    stale_sweeps: Arc<AtomicU64>,
    /// Cache-cluster schema version this Db was constructed under. See
    /// [`crate::cache_schema`] for the contract.
    schema_version: u32,
    /// The aggregate retained-byte account this cache admits against.
    ///
    /// There is no account-less result cache: the field carries a
    /// [`StoreAccount`](verter_session_query::retention::StoreAccount),
    /// whose `Default` is the ONE process-local account. A Db built
    /// outside a host type store therefore
    /// admits against the same ceiling rather than retaining entries that
    /// consume no aggregate headroom.
    retention_account: verter_session_query::retention::StoreAccount,
    /// Eligible refusals. Absent from [`Self::len`] so a refusal is never
    /// counted as a successful candidate.
    refusals: Mutex<ComponentMetaRefusalLedger<P>>,
}

impl<P> ComponentMetaResultDb<P> {
    pub(crate) fn is_current_schema(&self) -> bool {
        self.schema_version == crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION
    }
    pub(crate) fn candidate(
        &self,
        key: &ComponentMetaResultKey,
        owner_whole_hash: Hash16,
    ) -> Option<
        Arc<
            crate::bounded_query_retention::RetentionCandidate<Hash16, ComponentMetaResultEntry<P>>,
        >,
    > {
        self.inner.get_candidate(key, &owner_whole_hash)
    }

    /// Per-slot candidate cap. One owner + one options fingerprint is one
    /// slot; concurrent content versions of that owner are candidates in
    /// the slot, capped here. A fifth version evicts the oldest. Four
    /// covers the `{current, previous, two concurrent overlay}` working
    /// set (architecture rule R20 multi-candidate model) — the shared
    /// substrate's [`crate::bounded_query_retention::DEFAULT_CANDIDATE_CAP`].
    pub const PER_SLOT_CANDIDATE_CAP: usize = crate::bounded_query_retention::DEFAULT_CANDIDATE_CAP;

    /// Global total-candidate budget across every slot. A long-lived
    /// editor session touching many distinct owners caps here before
    /// FIFO eviction reclaims the oldest candidates. Tuned against the
    /// plan's memory budget order-of-magnitude.
    pub const GLOBAL_BUDGET: usize = 512;

    #[must_use]
    pub fn new() -> Self {
        Self::with_counters(Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)))
    }

    pub fn with_counters(live_counter: Arc<AtomicU64>, stale_sweeps: Arc<AtomicU64>) -> Self {
        Self::with_counters_and_schema_version(
            live_counter,
            stale_sweeps,
            crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION,
            verter_session_query::retention::StoreAccount::default(),
        )
    }

    /// The production constructor: counters plus the project's aggregate
    /// retention account, so every admitted entry's bytes are charged
    /// against the one process-local ceiling.
    pub fn with_counters_and_account(
        live_counter: Arc<AtomicU64>,
        stale_sweeps: Arc<AtomicU64>,
        retention_account: verter_session_query::retention::StoreAccount,
    ) -> Self {
        Self::with_counters_and_schema_version(
            live_counter,
            stale_sweeps,
            crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION,
            retention_account,
        )
    }

    /// The aggregate retained-byte account this cache admits against.
    /// Always present; see the field docs.
    #[must_use]
    pub(crate) fn retention_account(
        &self,
    ) -> &Arc<verter_session_query::retention::SemanticRetentionAccount> {
        self.retention_account.get()
    }

    /// Inspect the assigned retention account in lifetime tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn retention_account_for_tests(
        &self,
    ) -> &Arc<verter_session_query::retention::SemanticRetentionAccount> {
        self.retention_account()
    }

    /// Test-only constructor binding a specific retention account, so a
    /// pressure fixture can exercise the aggregate refusal path without
    /// perturbing the process-local account every other test shares.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn with_account_for_test(
        retention_account: Arc<verter_session_query::retention::SemanticRetentionAccount>,
    ) -> Self {
        Self::with_counters_and_account(
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU64::new(0)),
            verter_session_query::retention::StoreAccount::new(retention_account),
        )
    }

    /// Test-only constructor that pins a specific schema version on the Db.
    /// Used by `cache_invariant_migration` fixtures.
    #[cfg(any(test, feature = "test-support"))]
    pub fn new_with_schema_version_for_test(schema_version: u32) -> Self {
        Self::with_counters_and_schema_version(
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU64::new(0)),
            schema_version,
            verter_session_query::retention::StoreAccount::default(),
        )
    }

    fn with_counters_and_schema_version(
        live_counter: Arc<AtomicU64>,
        stale_sweeps: Arc<AtomicU64>,
        schema_version: u32,
        retention_account: verter_session_query::retention::StoreAccount,
    ) -> Self {
        #[cfg(not(any(test, feature = "semantic-observe")))]
        let _ = stale_sweeps;
        Self {
            inner: BoundedCandidateMap::with_caps(
                Self::PER_SLOT_CANDIDATE_CAP,
                Self::GLOBAL_BUDGET,
            ),
            live_counter,
            #[cfg(any(test, feature = "semantic-observe"))]
            stale_sweeps,
            schema_version,
            retention_account,
            refusals: Mutex::new(ComponentMetaRefusalLedger::default()),
        }
    }

    #[inline(always)]
    fn observe_removed(&self, count: usize) {
        #[cfg(any(test, feature = "semantic-observe"))]
        self.stale_sweeps.fetch_add(count as u64, Ordering::Relaxed);
        #[cfg(not(any(test, feature = "semantic-observe")))]
        let _ = count;
    }

    /// Apply a net live-count delta to the external counter.
    ///
    /// `added` candidates entered the cache and `removed` candidates left
    /// it as part of one mutation. The counter is updated by atomic
    /// `fetch_add` / `fetch_sub` so concurrent mutations compose exactly
    /// — never by an absolute `store` of a re-derived snapshot, which a
    /// racing mutation could clobber. The subtract is saturated against
    /// the counter's current value because `live_counter` is shared via
    /// `Arc<AtomicU64>` across the `ProjectTypeStore` and an underflow
    /// would corrupt every sibling DB's contribution to the shared sum.
    fn apply_live_delta(&self, added: usize, removed: usize) {
        if added > removed {
            self.live_counter
                .fetch_add((added - removed) as u64, Ordering::Relaxed);
        } else if removed > added {
            let delta = (removed - added) as u64;
            self.live_counter.fetch_sub(
                delta.min(self.live_counter.load(Ordering::Relaxed)),
                Ordering::Relaxed,
            );
        }
    }

    /// Per-slot candidate cap currently configured on the substrate.
    #[must_use]
    pub fn per_slot_candidate_cap(&self) -> usize {
        self.inner.per_slot_cap()
    }

    /// Global total-candidate budget currently configured on the
    /// substrate.
    #[must_use]
    pub fn global_budget(&self) -> usize {
        self.inner.global_cap()
    }

    /// Strict lookup — returns the cached entry for the given owner
    /// content version when a matching candidate is present. The caller
    /// is responsible for revalidating the dep signature before
    /// publishing the result; this split keeps the cache decoupled from
    /// the live host.
    ///
    /// Lookups against a Db whose `schema_version` does not match the
    /// current [`crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION`]
    /// return `None`.
    #[must_use]
    #[cfg(any(test, feature = "test-support"))]
    pub fn get(
        &self,
        key: &ComponentMetaResultKey,
        owner_whole_hash: Hash16,
    ) -> Option<ComponentMetaResultEntry<P>> {
        if self.schema_version != crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION {
            return None;
        }
        let result = self
            .inner
            .get_candidate(key, &owner_whole_hash)
            .map(|c| c.value.clone());
        if let Some(ctx) = crate::request_context::current_request_context() {
            if result.is_some() {
                ctx.cache_counters
                    .component_meta
                    .hits
                    .fetch_add(1, Ordering::Relaxed);
            } else {
                ctx.cache_counters
                    .component_meta
                    .misses
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        result
    }

    /// Insert a final result entry for the given owner content version.
    /// Cancelled, budget-exceeded, or partial results must **not** be
    /// passed here — callers are responsible for filtering. The cache
    /// does not inspect the payload.
    ///
    /// The owner whole-hash is the candidate discriminant: re-inserting
    /// the same `(key, owner_whole_hash)` refreshes the candidate in
    /// place; a new owner content version appends a candidate to the
    /// slot, and the bounded substrate evicts the oldest candidate /
    /// global-oldest entry to stay within the per-slot and global caps.
    ///
    /// Returns `false` when the aggregate retention account refused the
    /// entry's bytes: the freshly computed value is COMPLETE and is
    /// returned to its caller, the cache simply does not keep it. No
    /// stale candidate is substituted and no partial is fabricated — the
    /// slot is left exactly as it was.
    pub(crate) fn publish_core(
        &self,
        key: ComponentMetaResultKey,
        owner_whole_hash: Hash16,
        entry: ComponentMetaResultEntry<P>,
    ) -> bool
    where
        P: verter_session_query::retention::RetainedFootprint,
    {
        let bytes = {
            use verter_session_query::retention::RetainedFootprint as _;
            entry.retained_footprint_bytes()
        };
        let charge = match verter_session_query::facts::receipt::reserve_retained_with_evidence(
            self.retention_account(),
            bytes,
            &[&entry.read_set_signature.facts],
        ) {
            verter_session_query::retention::RetentionAdmission::Admitted(charge) => charge,
            verter_session_query::retention::RetentionAdmission::Refused(refusal) => {
                crate::cache_runtime::admission::propagate_non_admission(
                    refusal.non_admission_reason(),
                );
                #[cfg(feature = "semantic-observe")]
                tracing::debug!(
                    target: "verter::audit::record",
                    file = %key.owner_canonical,
                    refusal = %refusal,
                    "skipping component-meta cache promotion: aggregate retention refusal",
                );
                return false;
            }
        };
        let outcome = self.inner.admit(key, owner_whole_hash, entry, charge);
        if outcome.evicted > 0 {
            self.observe_removed(outcome.evicted);
        }
        // Net-delta the live counter: a fresh admission adds one live
        // candidate, an in-place replace adds none, and any FIFO
        // evictions remove that many. `fetch_add`/`fetch_sub` compose
        // exactly under concurrent admissions.
        self.apply_live_delta(usize::from(outcome.fresh), outcome.evicted);
        true
    }

    /// Test-support seed seam. Production admission must route through
    /// the selected engine `MemoPublish`, which owns tracing, finalization,
    /// and the admission decision before passive storage accepts the record.
    #[cfg(any(test, feature = "test-support"))]
    pub fn insert(
        &self,
        key: ComponentMetaResultKey,
        owner_whole_hash: Hash16,
        entry: ComponentMetaResultEntry<P>,
    ) where
        P: verter_session_query::retention::RetainedFootprint,
    {
        self.publish_core(key, owner_whole_hash, entry);
    }

    fn lock_refusals(&self) -> std::sync::MutexGuard<'_, ComponentMetaRefusalLedger<P>> {
        self.refusals
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Seal an eligible refusal. A complete status is not a refusal. The
    /// success candidate list is left unchanged, including when retention
    /// refuses the bytes.
    pub(crate) fn seal_refusal(
        &self,
        key: ComponentMetaResultKey,
        owner_whole_hash: Hash16,
        identity: ComponentMetaRefusalIdentity,
        status: FactStatus,
        completeness: ResultCompleteness,
        recovery: Arc<P>,
        read_set_signature: ReadSetSignature,
        validated_at_generation: u64,
        effects: ComponentMetaRefusalEffects,
    ) -> Option<ComponentMetaRetainedRefusal<P>>
    where
        P: verter_session_query::retention::RetainedFootprint + Clone,
    {
        if status.is_complete() {
            return None;
        }
        let identity = refusal_identity_for(
            identity.class,
            &ComponentMetaRefusalQuery {
                root: Arc::clone(&identity.root),
                profile: identity.profile,
                entry_allowance: identity.entry_allowance,
                context: identity.context,
                entered_isolated: false,
            },
        );
        let recovery = match Arc::try_unwrap(recovery) {
            Ok(value) => value,
            Err(shared) => (*shared).clone(),
        };
        let mut aliases = verter_session_query::retention::allocation::AliasSet::new();
        let request_bytes = effects
            .request
            .as_ref()
            .map_or(0, |spent| spent.owned_heap_bytes(&mut aliases));
        let bytes =
            verter_session_query::retention::RetainedFootprint::retained_footprint_bytes(&recovery)
                + verter_session_query::retention::allocation::fact_signature_bytes(
                    &read_set_signature,
                    &mut aliases,
                )
                + verter_session_query::retention::ENTRY_OVERHEAD_BYTES
                + request_bytes;
        let charge = match verter_session_query::facts::receipt::reserve_retained_with_evidence(
            self.retention_account(),
            bytes,
            &[&read_set_signature.facts],
        ) {
            verter_session_query::retention::RetentionAdmission::Admitted(charge) => charge,
            verter_session_query::retention::RetentionAdmission::Refused(refusal) => {
                crate::cache_runtime::admission::propagate_non_admission(
                    refusal.non_admission_reason(),
                );
                return None;
            }
        };
        let shared = Arc::new(SharedRefusal {
            recovery,
            signature: read_set_signature,
            retention: Some(charge),
        });
        let retained = ComponentMetaRetainedRefusal {
            identity: identity.clone(),
            status,
            completeness,
            recovery: Charged {
                shared: Arc::clone(&shared),
            },
            read_set_signature: ChargedSignature {
                shared: Arc::clone(&shared),
            },
            validated_at_generation,
        };
        let slot = (key, owner_whole_hash, identity);
        let mut ledger = self.lock_refusals();
        if ledger.entries.contains_key(&slot) {
            ledger.order.retain(|existing| existing != &slot);
        } else {
            while ledger.entries.len() >= REFUSAL_LEDGER_CAP {
                let Some(oldest) = ledger.order.pop_front() else {
                    break;
                };
                ledger.entries.remove(&oldest);
            }
        }
        ledger.entries.insert(
            slot.clone(),
            SealedComponentMetaRefusal {
                status,
                completeness,
                shared,
                effects,
                validated_at_generation,
            },
        );
        ledger.order.push_back(slot);
        Some(retained)
    }

    /// The retained refusal for one of `query`'s identities, if sealed.
    /// Does not validate the signature: a failed validation keeps the entry
    /// so a reverted edit can restore it.
    pub(crate) fn refusal_for(
        &self,
        key: &ComponentMetaResultKey,
        owner_whole_hash: Hash16,
        query: &ComponentMetaRefusalQuery,
    ) -> Option<(ComponentMetaRetainedRefusal<P>, ComponentMetaRefusalEffects)> {
        let ledger = self.lock_refusals();
        for identity in query.reusable_identities() {
            let slot = (key.clone(), owner_whole_hash, identity.clone());
            let Some(sealed) = ledger.entries.get(&slot) else {
                continue;
            };
            return Some((
                ComponentMetaRetainedRefusal {
                    identity,
                    status: sealed.status,
                    completeness: sealed.completeness,
                    recovery: Charged {
                        shared: Arc::clone(&sealed.shared),
                    },
                    read_set_signature: ChargedSignature {
                        shared: Arc::clone(&sealed.shared),
                    },
                    validated_at_generation: sealed.validated_at_generation,
                },
                sealed.effects.clone(),
            ));
        }
        None
    }

    fn clear_refusals(&self) {
        let mut ledger = self.lock_refusals();
        ledger.entries.clear();
        ledger.order.clear();
    }

    fn drop_refusals_for_owner(&self, owner_canonical: &str) {
        let mut ledger = self.lock_refusals();
        ledger
            .order
            .retain(|slot| slot.0.owner_canonical.as_ref() != owner_canonical);
        ledger
            .entries
            .retain(|slot, _| slot.0.owner_canonical.as_ref() != owner_canonical);
    }

    /// Remove the candidate for one owner content version. Returns the
    /// removed entry when present.
    #[cfg(any(test, feature = "test-support"))]
    pub fn remove(
        &self,
        key: &ComponentMetaResultKey,
        owner_whole_hash: Hash16,
    ) -> Option<ComponentMetaResultEntry<P>> {
        let candidate = self.inner.get_candidate(key, &owner_whole_hash)?;
        let removed = self.inner.evict_candidate(key, candidate.seq);
        if removed {
            self.observe_removed(1);
            // One live candidate left the cache — net-subtract one.
            self.apply_live_delta(0, 1);
            Some(candidate.value.clone())
        } else {
            None
        }
    }

    /// Drop every cached entry. Called on project-generation bumps
    /// (tsconfig / SDK / workspace-folder changes) — final results
    /// depend on routes and intrinsic resolution, which project-shape
    /// changes may shift.
    pub fn invalidate_all(&self) {
        let removed = self.inner.clear();
        if removed > 0 {
            self.observe_removed(removed);
        }
        // `clear` runs under the substrate's `retention_gate` write
        // guard, so `removed` is the exact live count it dropped —
        // net-subtract it. (Net-delta rather than `store(0)` keeps the
        // accounting uniform with the other mutation sites and robust
        // should the `component_meta_live` counter ever be shared.)
        self.apply_live_delta(0, removed);
        self.clear_refusals();
    }

    /// Invalidate every cached entry whose owner canonical matches
    /// `owner_canonical`, across all owner whole-hashes and options
    /// fingerprints. Called on owner-file content changes. Returns the
    /// number of candidates evicted.
    pub fn invalidate_owner(&self, owner_canonical: &str) -> usize {
        let removed = self
            .inner
            .retain_slots(|key| key.owner_canonical.as_ref() != owner_canonical);
        if removed > 0 {
            self.observe_removed(removed);
        }
        // Net-subtract exactly the candidates this invalidation removed.
        self.apply_live_delta(0, removed);
        self.drop_refusals_for_owner(owner_canonical);
        removed
    }

    /// Total live candidate count across every slot.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.live_count()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.live_count() == 0
    }

    /// Test-only synthetic-entry inserter used exclusively by
    /// `cache_invariant_migration` fixtures to verify the cache-cluster
    /// schema-version eviction invariant. The caller supplies the slot
    /// `key` (so the zero-env placeholder lives in the test fixture, not
    /// in production source) and a payload, so generic-parameter Dbs
    /// (`ComponentMetaResultDb<P>`) can be exercised without binding the
    /// helper to a single payload type. The `read_set_signature` carrier
    /// is crate-private, so the entry is assembled here rather than in the
    /// integration-test fixture.
    #[cfg(any(test, feature = "test-support"))]
    pub fn insert_synthetic_for_schema_test_with_payload(
        &self,
        key: ComponentMetaResultKey,
        payload: P,
    ) where
        P: verter_session_query::retention::RetainedFootprint,
    {
        let entry = ComponentMetaResultEntry {
            payload: Arc::new(payload),
            read_set_signature: verter_session_query::facts::fact_cache::ReadSetSignature::empty(),
            validated_at_generation: 0,
        };
        self.insert(key, [0u8; 16], entry);
    }
}

impl<P> Default for ComponentMetaResultDb<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P> crate::cache_schema::CacheSchemaVersioned for ComponentMetaResultDb<P> {
    fn schema_version(&self) -> u32 {
        self.schema_version
    }

    fn evict_if_schema_mismatch(&self, current: u32) -> usize {
        if self.schema_version == current {
            return 0;
        }
        let count = self.inner.clear();
        if count > 0 {
            self.observe_removed(count);
        }
        // `clear` ran under the write guard — `count` is the exact live
        // count it dropped. Net-subtract it.
        self.apply_live_delta(0, count);
        self.clear_refusals();
        count
    }
}

impl<P> crate::invalidation_domain::ParticipatesInInvalidation for ComponentMetaResultDb<P>
where
    P: Send + Sync,
{
    fn domains(&self) -> &'static [crate::invalidation_domain::InvalidationDomain] {
        use crate::invalidation_domain::InvalidationDomain::*;
        &[FileContent, ComponentMeta, ProjectGeneration]
    }
    fn invalidate(&self, domain: crate::invalidation_domain::InvalidationDomain) {
        use crate::invalidation_domain::InvalidationDomain::*;
        if matches!(domain, ProjectGeneration) {
            self.invalidate_all();
        }
    }
}

impl<P> crate::invalidation_domain::InvalidationByCanonical for ComponentMetaResultDb<P>
where
    P: Send + Sync,
{
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        // A content edit on the owner canonical drops every cached
        // result for that owner across all whole-hashes and options.
        self.invalidate_owner(canonical_id)
    }
}

impl<P> std::fmt::Debug for ComponentMetaResultDb<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComponentMetaResultDb")
            .field("live_candidates", &self.len())
            .field("per_slot_cap", &self.per_slot_candidate_cap())
            .field("global_budget", &self.global_budget())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod refusal_ledger_tests {
    use std::sync::Arc;

    use super::*;
    use crate::project_semantic_dispatch::cost_receipt::BudgetProfileSpec;
    use crate::semantic_query::{PartialReasonSet, ResultCompleteness};
    use verter_session_query::retention::{RetentionLimits, SemanticRetentionAccount};

    fn key(owner: &str) -> ComponentMetaResultKey {
        ComponentMetaResultKey {
            owner_canonical: Arc::from(owner),
            options_fingerprint: [0u8; 16],
            project_identity: verter_session_query::resolution::ProjectIdentity([0u8; 16]),
            parse_env_hash: [0u8; 16],
            resolve_env_hash: [0u8; 16],
            type_env_hash: [0u8; 16],
            lib_env_hash: [0u8; 16],
        }
    }

    fn spec(request_operations: u64) -> BudgetProfileSpec {
        BudgetProfileSpec {
            request_operations: request_operations as usize,
            ..BudgetProfileSpec::ignored()
        }
    }

    fn query(root: &str, profile: u64, allowance: u64, context: u64) -> ComponentMetaRefusalQuery {
        ComponentMetaRefusalQuery {
            root: Arc::from(root),
            profile: spec(profile),
            entry_allowance: allowance,
            context,
            entered_isolated: allowance == 0 && context == 0,
        }
    }

    #[test]
    fn retention_classes_follow_the_reason_set() {
        let budget = ResultCompleteness::partial(PartialReasonSet::BUDGET_EXCEEDED);
        assert_eq!(
            classify_final_component_retention(
                budget,
                FinalComponentAdmissionProof::isolated(true)
            ),
            FinalComponentRetention::Retain(ComponentMetaRefusalClass::OperationProfile)
        );
        let mut unowned = FinalComponentAdmissionProof::isolated(true);
        unowned.request_spend_owned = false;
        assert_eq!(
            classify_final_component_retention(budget, unowned),
            FinalComponentRetention::NotAdmitted,
            "an operation refusal whose spend was a sibling's is not retained"
        );
        let mut certified = FinalComponentAdmissionProof::certified();
        certified.request_spend_owned = false;
        assert_eq!(
            classify_final_component_retention(
                ResultCompleteness::partial(PartialReasonSet::SAME_PATH_RECURSION),
                certified
            ),
            FinalComponentRetention::Retain(ComponentMetaRefusalClass::CertifiedDivergence),
            "certified divergence does not consult spend ownership"
        );
        let mixed = ResultCompleteness::partial(
            PartialReasonSet::BUDGET_EXCEEDED.union(PartialReasonSet::MISSING_DEPENDENCY),
        );
        assert_eq!(
            classify_final_component_retention(mixed, FinalComponentAdmissionProof::isolated(true)),
            FinalComponentRetention::NotAdmitted
        );
        // The boolean bridge rides along with the named work-limit class.
        // It must not turn that operation refusal into a non-admission.
        let bridged = ResultCompleteness::partial(
            PartialReasonSet::PROJECTION_WORK_LIMIT.union(PartialReasonSet::PROPAGATED),
        );
        assert_eq!(
            classify_final_component_retention(
                bridged,
                FinalComponentAdmissionProof::isolated(true)
            ),
            FinalComponentRetention::Retain(ComponentMetaRefusalClass::OperationProfile)
        );
        assert_eq!(
            classify_final_component_retention(
                ResultCompleteness::partial(PartialReasonSet::PROPAGATED),
                FinalComponentAdmissionProof::isolated(true)
            ),
            FinalComponentRetention::NotAdmitted
        );
        let bridged_missing = ResultCompleteness::partial(
            PartialReasonSet::BUDGET_EXCEEDED
                .union(PartialReasonSet::MISSING_DEPENDENCY)
                .union(PartialReasonSet::PROPAGATED),
        );
        assert_eq!(
            classify_final_component_retention(
                bridged_missing,
                FinalComponentAdmissionProof::isolated(true)
            ),
            FinalComponentRetention::NotAdmitted
        );
        let depth = ResultCompleteness::partial(PartialReasonSet::CONNECTED_QUERY_DEPTH_LIMIT);
        assert_eq!(
            classify_final_component_retention(
                depth,
                FinalComponentAdmissionProof::isolated(false)
            ),
            FinalComponentRetention::NotAdmitted
        );
        assert_eq!(
            classify_final_component_retention(depth, FinalComponentAdmissionProof::isolated(true)),
            FinalComponentRetention::Retain(ComponentMetaRefusalClass::IsolatedRoot)
        );
        assert_eq!(
            classify_final_component_retention(
                ResultCompleteness::Complete,
                FinalComponentAdmissionProof::isolated(true)
            ),
            FinalComponentRetention::Complete
        );
        let recursion = ResultCompleteness::partial(PartialReasonSet::SAME_PATH_RECURSION);
        assert_eq!(
            classify_final_component_retention(
                recursion,
                FinalComponentAdmissionProof::isolated(true)
            ),
            FinalComponentRetention::NotAdmitted,
            "a recursion bit without a certificate is not certified divergence"
        );
        assert_eq!(
            classify_final_component_retention(
                recursion,
                FinalComponentAdmissionProof::certified()
            ),
            FinalComponentRetention::Retain(ComponentMetaRefusalClass::CertifiedDivergence)
        );
        let mut cut = FinalComponentAdmissionProof::certified();
        cut.schedule_cut = true;
        assert_eq!(
            classify_final_component_retention(recursion, cut),
            FinalComponentRetention::NotAdmitted,
            "a schedule cut is not a certified divergence"
        );
        let mut operation_cut = FinalComponentAdmissionProof::isolated(true);
        operation_cut.schedule_cut = true;
        assert_eq!(
            classify_final_component_retention(budget, operation_cut),
            FinalComponentRetention::NotAdmitted,
            "a schedule cut is not an operation-profile refusal"
        );
        let memory = ResultCompleteness::partial(PartialReasonSet::CONNECTED_MEMORY_LIMIT);
        assert_eq!(
            classify_final_component_retention(
                memory,
                FinalComponentAdmissionProof::isolated(true)
            ),
            FinalComponentRetention::NotAdmitted
        );
        let memory_and_budget = ResultCompleteness::partial(
            PartialReasonSet::CONNECTED_MEMORY_LIMIT.union(PartialReasonSet::BUDGET_EXCEEDED),
        );
        assert_eq!(
            classify_final_component_retention(
                memory_and_budget,
                FinalComponentAdmissionProof::isolated(true)
            ),
            FinalComponentRetention::NotAdmitted,
            "a memory limit vetoes a mixed operation reason"
        );
    }

    #[test]
    fn the_ledger_caps_at_256_and_invalidation_drops_refusals() {
        let db: ComponentMetaResultDb<u32> = ComponentMetaResultDb::new();
        let slot = key("/w/owner.vue");
        let status = final_component_refusal_status(PartialReasonSet::OPERATION_BUDGET).unwrap();
        for index in 0..=super::REFUSAL_LEDGER_CAP {
            let identity = refusal_identity_for(
                ComponentMetaRefusalClass::OperationProfile,
                &query(&format!("root-{index}"), 4, 0, 0),
            );
            assert!(db
                .seal_refusal(
                    slot.clone(),
                    [1u8; 16],
                    identity,
                    status,
                    ResultCompleteness::partial(PartialReasonSet::OPERATION_BUDGET),
                    Arc::new(index as u32),
                    verter_session_query::facts::fact_cache::ReadSetSignature::empty(),
                    0,
                    ComponentMetaRefusalEffects::default(),
                )
                .is_some());
        }
        assert_eq!(db.len(), 0);
        assert!(db
            .refusal_for(&slot, [1u8; 16], &query("root-0", 4, 0, 0))
            .is_none());
        assert!(db
            .refusal_for(
                &slot,
                [1u8; 16],
                &query(&format!("root-{}", super::REFUSAL_LEDGER_CAP), 4, 0, 0)
            )
            .is_some());
        db.invalidate_owner("/w/owner.vue");
        assert!(db
            .refusal_for(
                &slot,
                [1u8; 16],
                &query(&format!("root-{}", super::REFUSAL_LEDGER_CAP), 4, 0, 0)
            )
            .is_none());
    }

    #[test]
    fn a_held_recovery_keeps_the_retention_charge_after_invalidation() {
        let account = SemanticRetentionAccount::new(RetentionLimits {
            aggregate_ceiling_bytes: 10_000_000,
            active_ceiling_bytes: 10_000_000,
            max_entry_bytes: 10_000_000,
            pin_threshold_bytes: 10_000_000,
        });
        let db: ComponentMetaResultDb<u32> =
            ComponentMetaResultDb::with_account_for_test(Arc::clone(&account));
        let status = final_component_refusal_status(PartialReasonSet::OPERATION_BUDGET).unwrap();
        let retained = db
            .seal_refusal(
                key("/w/charged.vue"),
                [1u8; 16],
                refusal_identity_for(
                    ComponentMetaRefusalClass::OperationProfile,
                    &query("root", 4, 0, 0),
                ),
                status,
                ResultCompleteness::partial(PartialReasonSet::OPERATION_BUDGET),
                Arc::new(7u32),
                verter_session_query::facts::fact_cache::ReadSetSignature::empty(),
                0,
                ComponentMetaRefusalEffects::default(),
            )
            .expect("the refusal is retained");
        let charged = account.snapshot().retained_bytes;
        assert!(charged > 0, "sealing reserves retained bytes");
        let recovery = retained.recovery.clone();
        let signature = retained.read_set_signature.clone();
        drop(retained);
        db.invalidate_all();
        assert_eq!(
            account.snapshot().retained_bytes,
            charged,
            "the ledger drop does not release bytes a handle still holds"
        );
        assert_eq!(*recovery, 7);
        assert!(signature.facts.is_empty());
        drop(signature);
        assert_eq!(account.snapshot().retained_bytes, charged);
        drop(recovery);
        assert_eq!(account.snapshot().retained_bytes, 0);
    }

    #[test]
    fn retained_request_identities_grow_the_reservation() {
        use crate::project_semantic_dispatch::cost_receipt::CostIdentity;
        use crate::request_budget::RequestSpent;

        let account = SemanticRetentionAccount::new(RetentionLimits {
            aggregate_ceiling_bytes: 10_000_000,
            active_ceiling_bytes: 10_000_000,
            max_entry_bytes: 2_048,
            pin_threshold_bytes: 10_000_000,
        });
        let db: ComponentMetaResultDb<u32> =
            ComponentMetaResultDb::with_account_for_test(Arc::clone(&account));
        let status = final_component_refusal_status(PartialReasonSet::OPERATION_BUDGET).unwrap();
        let completeness = ResultCompleteness::partial(PartialReasonSet::OPERATION_BUDGET);
        let signature = verter_session_query::facts::fact_cache::ReadSetSignature::empty();
        let seal = |root: &str, effects: ComponentMetaRefusalEffects| {
            db.seal_refusal(
                key("/w/charged.vue"),
                [1u8; 16],
                refusal_identity_for(
                    ComponentMetaRefusalClass::OperationProfile,
                    &query(root, 4, 0, 0),
                ),
                status,
                completeness,
                Arc::new(7u32),
                signature.clone(),
                0,
                effects,
            )
        };
        let keyed = |keys: Vec<String>| ComponentMetaRefusalEffects {
            request: Some(RequestSpent::from_identities_for_test(
                keys.into_iter()
                    .map(|key| CostIdentity::new(key.into_bytes()))
                    .collect(),
                Vec::new(),
            )),
            ..ComponentMetaRefusalEffects::default()
        };
        let identities = |n: usize| keyed((0..n).map(|index| format!("paid-{index}")).collect());

        assert!(seal("none", ComponentMetaRefusalEffects::default()).is_some());
        let empty_prefix = account.snapshot().retained_bytes;
        assert!(seal("one", identities(1)).is_some());
        let one_identity = account.snapshot().retained_bytes - 2 * empty_prefix;
        assert!(
            one_identity > 0,
            "a retained paid identity grows the reservation"
        );
        assert!(seal("long", keyed(vec!["k".repeat(600)])).is_some());
        let long_identity = account.snapshot().retained_bytes - 3 * empty_prefix - one_identity;
        assert!(
            long_identity >= one_identity + 590,
            "a paid identity is charged for the key it keeps reachable, not a fixed proxy: \
             {long_identity} vs {one_identity}"
        );
        let charged = account.snapshot().retained_bytes;
        assert!(
            seal("wide", identities(200)).is_none(),
            "a paid prefix past the entry ceiling is not admitted"
        );
        assert_eq!(account.snapshot().retained_bytes, charged);
    }
}
