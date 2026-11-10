//! Private host-issued references to typeinfo facts.
//!
//! A typeinfo request that answers a fact registers, after the answer is
//! exported, an [`IssuedRefKey`] for every exported position it can replay
//! exactly. The key is the whole tuple a later request must present:
//!
//! - the fact's descriptor-specific [`QueryIdentity`], minted from the
//!   request's semantic material (selector subject, effective mode and
//!   reduction demand), never from the wire query echo;
//! - the committed [`NativeFactBasis`] the fact was observed under;
//! - the digest of the producer's declaration slot;
//! - for a nested position, the digest of its typed role (absent for a
//!   declaration root).
//!
//! The [`IssuedReferenceIndex`] retains, per fact, the typed query
//! identity, the committed basis and the exact key-to-locator entries,
//! never a graph, a node root or evaluated IR. It is bounded by record
//! count, entry count and retained bytes, and evicts whole records first
//! in, first out. Registration accepts only producer-held identities and
//! locators: digest bytes presented later build a lookup key, never an
//! identity, so substituting a basis or a slot can never retarget a
//! reference.
//!
//! A reference is consumed in three steps, each a production function:
//! [`lookup_issued`] matches the whole tuple, [`admit_issued_in_view`]
//! validates the committed basis against a current view, and
//! [`replay_issued_in_view`] replays the locator through the shared
//! engine under that view. The ExpandAround executor is their production
//! caller; until it lands they are exercised by their own tests only.

use std::collections::VecDeque;
use std::sync::Arc;

use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{QueryIdentity, ResultContractId};
use verter_session_query::facts::store_view::StoreView;
use verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch;
use verter_type_engine::semantic_query::{
    ProjectionMode, ReductionDemand, ResolvedDeclSlotIdentity, SemanticNodeId,
};
use verter_type_expr::TopLevelOwnerId;

use crate::host_resolve_type_audit::TypeResolutionRequestError;
use crate::resolver_store::HostStoreView;
use crate::typeinfo::native_basis::{
    commit_native_fact_basis, validate_native_fact_basis, NativeBasisRefusal, NativeBasisStale,
    NativeFactBasis, NativeTrace,
};
use crate::VerterHost;

/// Most fact records the index retains.
pub(crate) const MAX_ISSUED_RECORDS: usize = 64;

/// Most metadata bytes the index retains across its records.
pub(crate) const MAX_ISSUED_RETAINED_BYTES: usize = 1 << 20;

/// Most address entries the index retains across its records: the bounded
/// export's node cap, so one maximal export fits.
pub(crate) const MAX_ISSUED_ENTRIES: usize =
    crate::typeinfo::request_validation::MAX_EXPANSION_NODE_BUDGET as usize;

/// [`QueryIdentity`] marker of the resolve-symbol fact descriptor.
pub(crate) enum ResolveSymbolFact {}

/// The resolve-symbol descriptor's query-kind domain.
const RESOLVE_SYMBOL_QUERY_DOMAIN: &str = "verter.typeinfo.fact.resolve_symbol.v1";

/// The whole tuple a reference presents. Every component is a 32-byte
/// digest; `role` is absent exactly for a declaration root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct IssuedRefKey {
    /// Digest of the fact's typed query identity.
    pub(crate) query: [u8; 32],
    /// Digest of the committed native basis identity.
    pub(crate) basis: [u8; 32],
    /// Digest of the producer's declaration slot.
    pub(crate) slot: [u8; 32],
    /// Digest of the typed role of a nested position.
    pub(crate) role: Option<[u8; 32]>,
}

/// How to reach an issued position again through the shared engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NativeReplayLocator {
    /// A resolved declaration root.
    Declaration(DeclReplayLocator),
}

/// The exact declaration a named-symbol resolution reached: its entry
/// selection, the slot it instantiated, and the mode and demand it ran
/// with. Replaying it re-runs the ordinary memoized declaration query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeclReplayLocator {
    /// The canonical the request named.
    pub(crate) entry_canonical: Arc<str>,
    /// The authored owner the entry's name was resolved under.
    pub(crate) entry_owner: TopLevelOwnerId,
    /// The name the request named.
    pub(crate) requested_name: Arc<str>,
    /// The slot the resolution instantiated: every field, env included.
    pub(crate) resolved_slot: ResolvedDeclSlotIdentity,
    /// The projection mode the resolution ran with.
    pub(crate) effective_mode: ProjectionMode,
    /// The reduction demand the resolution ran with.
    pub(crate) demand: ReductionDemand,
}

impl DeclReplayLocator {
    fn retained_bytes(&self) -> usize {
        self.entry_canonical.len()
            + self.requested_name.len()
            + self.resolved_slot.defining_canonical.len()
            + self.resolved_slot.merged_symbol_name.len()
    }
}

impl NativeReplayLocator {
    fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + match self {
                Self::Declaration(locator) => locator.retained_bytes(),
            }
    }
}

/// The typed query identity a record was issued under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FactQuery {
    /// A resolve-symbol fact.
    ResolveSymbol(QueryIdentity<ResolveSymbolFact>),
}

impl FactQuery {
    fn digest(&self) -> [u8; 32] {
        match self {
            Self::ResolveSymbol(identity) => *identity.digest().as_bytes(),
        }
    }

    fn retained_bytes(&self) -> usize {
        match self {
            Self::ResolveSymbol(identity) => identity.canonical_bytes().len(),
        }
    }
}

/// The semantic material a resolve-symbol fact is asked by: the selector
/// subject, the effective mode and the reduction demand. The closure kind,
/// the export budgets, the display policy, the diagnostics and provenance
/// flags and the wire version shape only the exported answer, so they are
/// excluded: the same declaration fact is the same question under each.
struct ResolveSymbolQuery<'a>(&'a DeclReplayLocator);

impl CanonicalEncode for ResolveSymbolQuery<'_> {
    const DOMAIN_TAG: &'static str = "verter.typeinfo.fact.resolve_symbol.arguments.v1";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder
            .field_str(1, &self.0.entry_canonical)
            .field_str(2, &self.0.requested_name)
            .field_enum_discriminant(3, projection_mode_tag(self.0.effective_mode))
            .field_enum_discriminant(4, reduction_demand_tag(self.0.demand));
    }
}

/// The resolve-symbol descriptor's result contract: the bounded terminal
/// projection of one resolved declaration root.
struct ResolveSymbolResultContract;

impl CanonicalEncode for ResolveSymbolResultContract {
    const DOMAIN_TAG: &'static str = "verter.typeinfo.fact.resolve_symbol.result.v1";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_str(1, "declaration-root.bounded-terminal-projection");
    }
}

const fn projection_mode_tag(mode: ProjectionMode) -> u32 {
    match mode {
        ProjectionMode::Identity => 1,
        ProjectionMode::Navigate => 2,
        ProjectionMode::Shallow => 3,
        ProjectionMode::Expanded => 4,
        ProjectionMode::Skeleton => 5,
    }
}

const fn reduction_demand_tag(demand: ReductionDemand) -> u32 {
    match demand {
        ReductionDemand::Published => 1,
        ReductionDemand::StructuralTransit => 2,
        ReductionDemand::MacroObjectSurface => 3,
        ReductionDemand::VueRuntimeObjectSurface => 4,
    }
}

/// The resolve-symbol query identity of `locator`'s request. It observes
/// no framework profile.
fn resolve_symbol_query_identity(locator: &DeclReplayLocator) -> QueryIdentity<ResolveSymbolFact> {
    QueryIdentity::compose(
        RESOLVE_SYMBOL_QUERY_DOMAIN,
        ResolveSymbolQuery(locator).canonical_digest(),
        &[],
        &ResultContractId::from_canonical(&ResolveSymbolResultContract),
    )
}

/// One issued address and the locator that replays it.
#[derive(Debug)]
struct IssuedEntry {
    key: IssuedRefKey,
    locator: NativeReplayLocator,
}

/// One issued fact: its typed query identity, the basis it was observed
/// under, and its exact addresses.
#[derive(Debug)]
pub(crate) struct NativeFactRecord {
    query: FactQuery,
    basis: NativeFactBasis,
    entries: Box<[IssuedEntry]>,
    retained_bytes: usize,
}

impl NativeFactRecord {
    fn new(
        query: FactQuery,
        basis: NativeFactBasis,
        entries: Vec<(IssuedRefKey, NativeReplayLocator)>,
    ) -> Self {
        let entries: Box<[IssuedEntry]> = entries
            .into_iter()
            .map(|(key, locator)| IssuedEntry { key, locator })
            .collect();
        let retained_bytes = std::mem::size_of::<Self>()
            + query.retained_bytes()
            + basis.retained_bytes()
            + entries
                .iter()
                .map(|entry| std::mem::size_of::<IssuedRefKey>() + entry.locator.retained_bytes())
                .sum::<usize>();
        Self {
            query,
            basis,
            entries,
            retained_bytes,
        }
    }

    fn basis_digest(&self) -> [u8; 32] {
        *self.basis.basis().id().digest().as_bytes()
    }

    /// Whether `other` is the same fact observed under the same basis.
    fn is_same_fact(&self, other: &Self) -> bool {
        self.query == other.query && self.basis.basis().id() == other.basis.basis().id()
    }
}

/// Why a fact issued no reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IssueRefusal {
    /// The traced fact committed no native basis.
    Basis(NativeBasisRefusal),
    /// The committed basis no longer described the host's current inputs
    /// when the fact came to be registered.
    StaleAtRegistration,
    /// The record alone exceeds the index's entry or byte bound.
    Oversized,
}

/// Issuance outcomes, counted.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IssuanceCounters {
    /// Records registered.
    pub(crate) registered: u64,
    /// Registrations of a fact the index already held under the same basis.
    pub(crate) reissued: u64,
    /// Records evicted to admit a newer one.
    pub(crate) evicted: u64,
    /// Basis refusals, by [`NativeBasisRefusal`] arm.
    pub(crate) basis_refused: [u64; 6],
    /// Facts whose basis went stale before registration.
    pub(crate) stale_at_registration: u64,
    /// Records refused for exceeding a bound alone.
    pub(crate) oversized: u64,
}

const fn basis_refusal_slot(refusal: NativeBasisRefusal) -> usize {
    match refusal {
        NativeBasisRefusal::OwnerUnobserved => 0,
        NativeBasisRefusal::NotRepresentable => 1,
        NativeBasisRefusal::MutationUnstable => 2,
        NativeBasisRefusal::NotVouched => 3,
        NativeBasisRefusal::RequestLocal => 4,
        NativeBasisRefusal::Uncommittable => 5,
    }
}

/// The host's bounded index of issued fact records.
#[derive(Debug, Default)]
pub(crate) struct IssuedReferenceIndex {
    records: VecDeque<Arc<NativeFactRecord>>,
    entries: usize,
    retained_bytes: usize,
    counters: IssuanceCounters,
}

impl IssuedReferenceIndex {
    /// Register `record`, evicting the oldest records until every bound
    /// holds. A record that alone exceeds a bound is refused whole: none
    /// of its entries is issued.
    fn register(&mut self, record: NativeFactRecord) -> Result<(), IssueRefusal> {
        if record.entries.len() > MAX_ISSUED_ENTRIES
            || record.retained_bytes > MAX_ISSUED_RETAINED_BYTES
        {
            return Err(IssueRefusal::Oversized);
        }
        if self.records.iter().any(|held| held.is_same_fact(&record)) {
            self.counters.reissued = self.counters.reissued.saturating_add(1);
            return Ok(());
        }
        while self.records.len() >= MAX_ISSUED_RECORDS
            || self.entries + record.entries.len() > MAX_ISSUED_ENTRIES
            || self.retained_bytes + record.retained_bytes > MAX_ISSUED_RETAINED_BYTES
        {
            let Some(evicted) = self.records.pop_front() else {
                break;
            };
            self.entries -= evicted.entries.len();
            self.retained_bytes -= evicted.retained_bytes;
            self.counters.evicted = self.counters.evicted.saturating_add(1);
        }
        self.entries += record.entries.len();
        self.retained_bytes += record.retained_bytes;
        self.records.push_back(Arc::new(record));
        self.counters.registered = self.counters.registered.saturating_add(1);
        Ok(())
    }

    fn note_refusal(&mut self, refusal: IssueRefusal) {
        let counter = match refusal {
            IssueRefusal::Basis(refusal) => {
                &mut self.counters.basis_refused[basis_refusal_slot(refusal)]
            }
            IssueRefusal::StaleAtRegistration => &mut self.counters.stale_at_registration,
            IssueRefusal::Oversized => &mut self.counters.oversized,
        };
        *counter = counter.saturating_add(1);
    }

    /// The issued entry `key` names, matched on the whole tuple.
    fn lookup(&self, key: &IssuedRefKey) -> Option<IssuedRef> {
        self.records.iter().find_map(|record| {
            if record.query.digest() != key.query || record.basis_digest() != key.basis {
                return None;
            }
            let entry = record.entries.iter().position(|entry| entry.key == *key)?;
            Some(IssuedRef {
                record: Arc::clone(record),
                entry,
            })
        })
    }
}

/// One issued entry, held past the index lock.
#[derive(Debug, Clone)]
pub(crate) struct IssuedRef {
    record: Arc<NativeFactRecord>,
    entry: usize,
}

impl IssuedRef {
    fn locator(&self) -> &NativeReplayLocator {
        &self.record.entries[self.entry].locator
    }
}

/// Commit the basis of a traced resolve-symbol fact and register its
/// declaration-root reference.
///
/// `view` is the one captured view the fact was resolved and exported
/// under and `port` the request's validation port over it (see
/// [`commit_native_fact_basis`]). After commit, the basis is checked
/// against a fresh capture of the host: a fact whose inputs moved after
/// its capture registers nothing. Every refusal is counted and leaves the
/// answer the request exported unchanged.
pub(crate) fn issue_resolve_symbol(
    host: &VerterHost,
    view: &HostStoreView,
    port: &dyn StoreView,
    trace: NativeTrace,
    locator: DeclReplayLocator,
) -> Result<IssuedRefKey, IssueRefusal> {
    let issued = commit_and_register(host, view, port, trace, locator);
    if let Err(refusal) = issued {
        host.typeinfo_issued_refs.lock().note_refusal(refusal);
    }
    issued
}

fn commit_and_register(
    host: &VerterHost,
    view: &HostStoreView,
    port: &dyn StoreView,
    trace: NativeTrace,
    locator: DeclReplayLocator,
) -> Result<IssuedRefKey, IssueRefusal> {
    let basis =
        commit_native_fact_basis(host, view, port, trace, &[locator.entry_canonical.as_ref()])
            .map_err(IssueRefusal::Basis)?;
    let query = resolve_symbol_query_identity(&locator);
    let key = IssuedRefKey {
        query: *query.digest().as_bytes(),
        basis: *basis.basis().id().digest().as_bytes(),
        slot: *locator.resolved_slot.canonical_digest().as_bytes(),
        role: None,
    };
    if admit_in_fresh_view(host, &basis).is_err() {
        return Err(IssueRefusal::StaleAtRegistration);
    }
    let record = NativeFactRecord::new(
        FactQuery::ResolveSymbol(query),
        basis,
        vec![(key, NativeReplayLocator::Declaration(locator))],
    );
    host.typeinfo_issued_refs.lock().register(record)?;
    Ok(key)
}

/// Validate `basis` against a fresh current capture of `host`, through a
/// request port over it. A host that cannot settle on a coherent view
/// admits nothing.
fn admit_in_fresh_view(host: &VerterHost, basis: &NativeFactBasis) -> Result<(), NativeBasisStale> {
    let Some(current) = crate::typeinfo::current_store_view_for_query(host) else {
        return Err(NativeBasisStale::Observation);
    };
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    verter_audit::specialise!(crate::capture::installed_mode() => M, {
        let ctx = crate::resolver_core::HostResolverContext::<'_, M>::from_current_in(host, &current, overlay);
        validate_native_fact_basis(
            current.view(),
            &verter_type_engine::resolver_core::fact_validation_port::FactValidationView::new(&ctx),
            basis,
        )
    })
}

/// The issued entry `key` names. A tuple that was never issued, was
/// evicted, or differs from an issued one in any component names nothing.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn lookup_issued(host: &VerterHost, key: &IssuedRefKey) -> Option<IssuedRef> {
    host.typeinfo_issued_refs.lock().lookup(key)
}

/// Admit `issued` under the request's captured view: its committed basis
/// must still describe exactly what `view` observes, validated through
/// `port`, the request's validation port over `view`. Success certifies
/// the issued basis; it never adopts the view's newer inputs.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn admit_issued_in_view(
    view: &HostStoreView,
    port: &dyn StoreView,
    issued: &IssuedRef,
) -> Result<(), NativeBasisStale> {
    validate_native_fact_basis(view, port, &issued.record.basis)
}

/// Why an admitted reference could not be replayed.
#[derive(Debug)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum ReplayRefusal {
    /// The replay reached another declaration than the one issued.
    Diverged,
    /// The replayed resolution faulted. The ExpandAround executor answers
    /// it as the graph fault arm; nothing reads it before then.
    Fault(#[allow(dead_code)] TypeResolutionRequestError),
}

/// Replay an admitted reference through the shared engine under `ctx`'s
/// captured view (`dispatch` must be built over `ctx`). A declaration
/// root re-runs its ordinary memoized declaration query; the replay must
/// reach exactly the issued locator, or it is refused rather than answered
/// with whatever the current inputs resolve.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn replay_issued_in_view<M: verter_audit::observe::ObserveExec>(
    host: &VerterHost,
    ctx: &crate::resolver_core::HostResolverContext<'_, M>,
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities<M>>,
    issued: &IssuedRef,
) -> Result<Option<SemanticNodeId>, ReplayRefusal> {
    match issued.locator() {
        NativeReplayLocator::Declaration(locator) => {
            let replayed = crate::typeinfo::resolve_named_symbol::resolve_named_symbol_located(
                host,
                ctx,
                dispatch,
                &locator.entry_canonical,
                &locator.requested_name,
                &[],
                locator.effective_mode,
            );
            if replayed.locator.as_ref() != Some(locator) {
                return Err(ReplayRefusal::Diverged);
            }
            replayed.outcome.map_err(ReplayRefusal::Fault)
        }
    }
}

/// Every key the index holds, oldest record first.
#[cfg(test)]
pub(crate) fn issued_keys_for_tests(host: &VerterHost) -> Vec<IssuedRefKey> {
    host.typeinfo_issued_refs
        .lock()
        .records
        .iter()
        .flat_map(|record| record.entries.iter().map(|entry| entry.key))
        .collect()
}

/// The index's issuance counters.
#[cfg(test)]
pub(crate) fn issuance_counters_for_tests(host: &VerterHost) -> IssuanceCounters {
    host.typeinfo_issued_refs.lock().counters
}

#[cfg(test)]
#[path = "node_refs_tests.rs"]
mod tests;
