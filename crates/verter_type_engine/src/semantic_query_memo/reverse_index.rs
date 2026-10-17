//! Direct-edge receipt reverse indexes.
//!
//! A cached candidate's validity rail is hierarchical: its carrier holds the
//! facts its own computation read and, for every completed result it
//! consumed, that result's [`ResultReceipt`], whose evidence holds that
//! result's own facts and the receipts it consumed in turn. A reverse index
//! that registered every candidate under every canonical its carrier
//! reaches would copy each dependency chain's canonical closure once per
//! candidate: a chain of `n` results each reading one file costs `n²`
//! registrations.
//!
//! [`ReceiptReverseIndex`] registers the graph's edges instead, each once:
//!
//! - **canonical → evidence**: a canonical names the evidence (a receipt, a
//!   page, or a candidate's carrier) whose OWN facts read it;
//! - **child → parent**: a consumed receipt names each evidence that
//!   consumed it, including receipts no cache holds (uncached
//!   intermediates);
//! - **evidence → candidate**: a carrier names the candidates it validates.
//!
//! A candidate also registers directly under its extra canonicals (dispatch
//! fences, strict self-roots). A chain or a diamond therefore costs
//! `O(F + E + C)` physical registrations — its files, its edges and its
//! candidates — however many candidates share it, and an evidence many
//! candidates share is registered once. [`ReceiptReverseIndex::reached_by`]
//! walks upward from a canonical, visiting each node once.
//!
//! The index never answers validity. It drives eager cleanup of an
//! explicitly invalidated canonical; every warm read still revalidates its
//! own carrier. A COMPACTED fact names no canonical, so the evidence holding
//! it registers under fewer canonicals than it stands in for: a later
//! eviction, never a stale serve, because the aggregate rejects at read time
//! on any movement in its domain.
//!
//! **Ownership.** A node is held only while a registered candidate reaches
//! it: unregistering a candidate drops every evidence node no other
//! candidate reaches, and every canonical no node reads any more, so the
//! index drains to empty with its last candidate. Every edge has one owner
//! — the node it names: an edge naming a candidate is that candidate's
//! (charged by its owner through [`ReceiptReverseIndex::candidate_bytes`]),
//! an edge naming an evidence node is the index's own storage. Every
//! allocation the index holds is charged to a retention account for as long
//! as it is held: by CAPACITY, not by live count, so a table or set that
//! keeps spare buckets after a partial drain stays charged until it shrinks
//! or is freed. The candidates' own share is the only part an owner may
//! charge instead ([`ReceiptReverseIndex::charged`]); an owner that charges
//! nothing of its candidates leaves all of it to the index
//! ([`ReceiptReverseIndex::charging_registrations`]). There is no
//! account-less index: [`Default`] charges the process-local account.
//!
//! **Locking.** One leaf mutex guards the whole index; no other lock is
//! taken while it is held, and evidence the index drops is released after
//! it unlocks. Callers register and unregister inside their own consistency
//! boundary, so a candidate's visibility and its registration change in one
//! step.

use std::hash::Hash;
use std::sync::Arc;

use parking_lot::Mutex;
use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;
use verter_session_query::facts::fact_cache::{FactVersionRef, ResultReceipt};
use verter_session_query::retention::allocation::arc_block_bytes;
use verter_session_query::retention::granular::GranularPin;
use verter_session_query::retention::SemanticRetentionAccount;

use super::family::{FamilyKey, FamilySlots, MemoEntry, ModeSlot};
use crate::semantic_query::DepSignature;

/// The address of an evidence's fact array: stable and unique while the
/// node holding the evidence lives.
type EvidenceAddr = usize;

/// The index's own number for a registered candidate.
type CandidateId = u64;

/// The node an edge names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Holder {
    Evidence(EvidenceAddr),
    Candidate(CandidateId),
}

/// The evidence an evidence node keeps alive while it is indexed.
enum EvidenceOwner {
    /// A consumed result's receipt, or a page of a wide signature.
    Receipt(ResultReceipt),
    /// A candidate's top-level carrier.
    Carrier(Arc<[FactVersionRef]>),
}

impl EvidenceOwner {
    fn facts(&self) -> &[FactVersionRef] {
        match self {
            Self::Receipt(receipt) => receipt.facts(),
            Self::Carrier(facts) => facts,
        }
    }

    fn addr(&self) -> EvidenceAddr {
        self.facts().as_ptr() as EvidenceAddr
    }
}

struct EvidenceNode {
    owner: EvidenceOwner,
    /// The evidence nodes that consumed this one and the candidates it
    /// validates.
    parents: FxHashSet<Holder>,
}

struct CandidateNode<K, P> {
    key: K,
    payload: P,
    /// The carrier evidence node, when the carrier holds any fact.
    carrier: Option<EvidenceAddr>,
    /// The extra canonicals the candidate registered under, sharing the
    /// index's own copy of each.
    extras: Box<[Arc<str>]>,
}

/// Bytes one canonical's bucket holds, besides its text.
const CANONICAL_BUCKET_BYTES: usize = std::mem::size_of::<(Arc<str>, FxHashSet<Holder>)>() + 1;

/// Bytes one evidence node's bucket holds.
const EVIDENCE_BUCKET_BYTES: usize = std::mem::size_of::<(EvidenceAddr, EvidenceNode)>() + 1;

/// Bytes one bucket of the candidate-id table holds.
const fn id_bucket_bytes<K>() -> usize {
    std::mem::size_of::<(K, CandidateId)>() + 1
}

/// Bytes one bucket of the candidate table holds.
const fn candidate_bucket_bytes<K, P>() -> usize {
    std::mem::size_of::<(CandidateId, CandidateNode<K, P>)>() + 1
}

/// Bytes one edge's bucket holds in the set naming it.
const EDGE_BYTES: usize = std::mem::size_of::<Holder>() + 1;

/// Granule the index's own storage is charged in.
const INDEX_STORAGE_GRANULE: usize = 4 * 1024;

/// What the index currently holds: a required occupancy count, read under
/// the index's lock.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReverseIndexOccupancy {
    /// Canonicals some node reads directly.
    pub canonicals: usize,
    /// Evidence nodes: carriers, receipts and pages, cached or not.
    pub evidence: usize,
    /// Registered candidates.
    pub candidates: usize,
    /// Physical edges: canonical → node, child → parent and carrier →
    /// candidate.
    pub edges: usize,
}

/// Edge operations the index has performed since it was built: optional
/// observation, compiled only into test and `semantic-observe` builds.
#[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReverseIndexEdgeOps {
    /// Edges inserted.
    pub linked: u64,
    /// Edges removed.
    pub unlinked: u64,
    /// Nodes visited by [`ReceiptReverseIndex::reached_by`] walks.
    pub walked: u64,
}

/// The index's charge and the running totals it is figured from.
struct IndexStorage {
    account: Arc<SemanticRetentionAccount>,
    /// Whether each candidate's own registration
    /// ([`ReceiptReverseIndex::candidate_bytes`]) is charged by its owner
    /// rather than here.
    candidates_charged_by_owner: bool,
    pin: Option<GranularPin>,
    /// Text blocks of the canonicals the index holds.
    text: usize,
    /// Buckets of every holder set — a canonical's holders, an evidence
    /// node's parents — live or spare.
    set_slots: usize,
    /// Edges naming a candidate.
    candidate_edges: usize,
    /// Extra canonicals the candidates' lists name.
    candidate_extras: usize,
}

impl IndexStorage {
    fn new(account: Arc<SemanticRetentionAccount>, candidates_charged_by_owner: bool) -> Self {
        Self {
            account,
            candidates_charged_by_owner,
            pin: None,
            text: 0,
            set_slots: 0,
            candidate_edges: 0,
            candidate_extras: 0,
        }
    }
}

struct IndexState<K, P> {
    canonicals: FxHashMap<Arc<str>, FxHashSet<Holder>>,
    evidence: FxHashMap<EvidenceAddr, EvidenceNode>,
    candidate_ids: FxHashMap<K, CandidateId>,
    candidates: FxHashMap<CandidateId, CandidateNode<K, P>>,
    next_candidate: CandidateId,
    edges: usize,
    storage: IndexStorage,
    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    ops: ReverseIndexEdgeOps,
}

impl<K, P> IndexState<K, P> {
    fn new(storage: IndexStorage) -> Self {
        Self {
            canonicals: FxHashMap::default(),
            evidence: FxHashMap::default(),
            candidate_ids: FxHashMap::default(),
            candidates: FxHashMap::default(),
            next_candidate: 0,
            edges: 0,
            storage,
            #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
            ops: ReverseIndexEdgeOps::default(),
        }
    }

    /// The bytes the index's charge covers: every table's buckets and
    /// every holder set's buckets by capacity, spare ones included, and the
    /// canonicals' text — less the share each candidate's owner charges
    /// when it does.
    fn storage_bytes(&self) -> usize {
        let held = self.canonicals.capacity() * CANONICAL_BUCKET_BYTES
            + self.evidence.capacity() * EVIDENCE_BUCKET_BYTES
            + self.candidate_ids.capacity() * id_bucket_bytes::<K>()
            + self.candidates.capacity() * candidate_bucket_bytes::<K, P>()
            + self.storage.text
            + self.storage.set_slots * EDGE_BYTES;
        if self.storage.candidates_charged_by_owner {
            held - (self.candidates.len()
                * (id_bucket_bytes::<K>() + candidate_bucket_bytes::<K, P>())
                + self.storage.candidate_edges * EDGE_BYTES)
        } else {
            held + self.storage.candidate_extras * std::mem::size_of::<Arc<str>>()
        }
    }

    /// Settle the charge with what the index holds now.
    fn settle(&mut self) {
        let bytes = self.storage_bytes();
        let storage = &mut self.storage;
        let pin = storage
            .pin
            .get_or_insert_with(|| GranularPin::new(&storage.account, INDEX_STORAGE_GRANULE));
        let used = pin.used();
        if bytes > used {
            pin.add(bytes - used);
        } else if used > bytes {
            pin.sub(used - bytes);
        }
    }
}

/// Shrink `table` once a quarter or less of its capacity is live, freeing
/// it outright when empty.
fn shrink_sparse<T: Hash + Eq, V>(table: &mut FxHashMap<T, V>) {
    if table.is_empty() {
        *table = FxHashMap::default();
    } else if table.len() * 4 <= table.capacity() {
        table.shrink_to(table.len() * 2);
    }
}

/// Shrink a non-empty holder set once a quarter or less of its capacity is
/// live, answering the buckets it gave back.
fn shrink_holders(set: &mut FxHashSet<Holder>) -> usize {
    let before = set.capacity();
    if set.len() * 4 <= before {
        set.shrink_to(set.len() * 2);
    }
    before - set.capacity()
}

impl<K: Eq + Hash + Clone, P> IndexState<K, P> {
    fn note_linked(&mut self, holder: Holder) {
        self.edges += 1;
        if matches!(holder, Holder::Candidate(_)) {
            self.storage.candidate_edges += 1;
        }
        #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
        {
            self.ops.linked += 1;
        }
    }

    fn note_unlinked(&mut self, holder: Holder) {
        self.edges -= 1;
        if matches!(holder, Holder::Candidate(_)) {
            self.storage.candidate_edges -= 1;
        }
        #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
        {
            self.ops.unlinked += 1;
        }
    }

    /// Register `holder` under `canonical`, answering the index's copy of
    /// the canonical's text.
    fn link_canonical(&mut self, canonical: &str, holder: Holder) -> Arc<str> {
        let key = match self.canonicals.get_key_value(canonical) {
            Some((key, _)) => Arc::clone(key),
            None => {
                let key: Arc<str> = Arc::from(canonical);
                self.storage.text += arc_block_bytes(canonical.len());
                self.canonicals
                    .insert(Arc::clone(&key), FxHashSet::default());
                key
            }
        };
        let set = self.canonicals.get_mut(canonical).expect("present above");
        let before = set.capacity();
        let inserted = set.insert(holder);
        self.storage.set_slots += set.capacity() - before;
        if inserted {
            self.note_linked(holder);
        }
        key
    }

    /// Remove `holder` from `canonical`, dropping the canonical with its
    /// last holder.
    fn unlink_canonical(&mut self, canonical: &str, holder: Holder) {
        let Some(set) = self.canonicals.get_mut(canonical) else {
            return;
        };
        if !set.remove(&holder) {
            return;
        }
        if set.is_empty() {
            self.storage.set_slots -= set.capacity();
            self.canonicals.remove(canonical);
            self.storage.text -= arc_block_bytes(canonical.len());
            shrink_sparse(&mut self.canonicals);
        } else {
            self.storage.set_slots -= shrink_holders(set);
        }
        self.note_unlinked(holder);
    }

    /// Link `owner` under `parent`, registering every evidence node it
    /// reaches that is not indexed yet — each once, from an explicit stack.
    /// Evidence with no fact reaches no canonical and is not indexed.
    fn link_evidence(&mut self, owner: EvidenceOwner, parent: Holder) -> Option<EvidenceAddr> {
        if owner.facts().is_empty() {
            return None;
        }
        let root = owner.addr();
        let mut pending: Vec<(EvidenceOwner, Holder)> = vec![(owner, parent)];
        while let Some((owner, parent)) = pending.pop() {
            let addr = owner.addr();
            if let Some(node) = self.evidence.get_mut(&addr) {
                let before = node.parents.capacity();
                let inserted = node.parents.insert(parent);
                self.storage.set_slots += node.parents.capacity() - before;
                if inserted {
                    self.note_linked(parent);
                }
                continue;
            }
            let holder = Holder::Evidence(addr);
            for fact in owner.facts() {
                match fact {
                    FactVersionRef::Receipt(child) => {
                        if !child.facts().is_empty() {
                            pending.push((EvidenceOwner::Receipt(child.clone()), holder));
                        }
                    }
                    leaf => {
                        if let Some(canonical) = leaf.canonical_id() {
                            self.link_canonical(canonical, holder);
                        }
                    }
                }
            }
            let mut parents = FxHashSet::default();
            parents.insert(parent);
            self.storage.set_slots += parents.capacity();
            self.note_linked(parent);
            self.evidence.insert(addr, EvidenceNode { owner, parents });
        }
        Some(root)
    }

    /// Remove `parent` from the evidence at `addr`, dropping every evidence
    /// node no parent reaches any more. Dropped evidence is moved to
    /// `released`, freed after the index unlocks.
    fn unlink_evidence(
        &mut self,
        addr: EvidenceAddr,
        parent: Holder,
        released: &mut Vec<EvidenceOwner>,
    ) {
        let mut pending: Vec<(EvidenceAddr, Holder)> = vec![(addr, parent)];
        while let Some((addr, parent)) = pending.pop() {
            let Some(node) = self.evidence.get_mut(&addr) else {
                continue;
            };
            if !node.parents.remove(&parent) {
                continue;
            }
            if !node.parents.is_empty() {
                self.storage.set_slots -= shrink_holders(&mut node.parents);
                self.note_unlinked(parent);
                continue;
            }
            self.note_unlinked(parent);
            let node = self.evidence.remove(&addr).expect("present above");
            self.storage.set_slots -= node.parents.capacity();
            let holder = Holder::Evidence(addr);
            for fact in node.owner.facts() {
                match fact {
                    FactVersionRef::Receipt(child) => {
                        if !child.facts().is_empty() {
                            pending.push((child.facts().as_ptr() as EvidenceAddr, holder));
                        }
                    }
                    leaf => {
                        if let Some(canonical) = leaf.canonical_id() {
                            self.unlink_canonical(canonical, holder);
                        }
                    }
                }
            }
            released.push(node.owner);
        }
        shrink_sparse(&mut self.evidence);
    }

    fn unregister(&mut self, key: &K, released: &mut Vec<EvidenceOwner>) -> Option<P> {
        let id = self.candidate_ids.remove(key)?;
        let node = self.candidates.remove(&id).expect("every id names a node");
        let holder = Holder::Candidate(id);
        self.storage.candidate_extras -= node.extras.len();
        for canonical in node.extras.iter() {
            self.unlink_canonical(canonical, holder);
        }
        if let Some(carrier) = node.carrier {
            self.unlink_evidence(carrier, holder, released);
        }
        shrink_sparse(&mut self.candidates);
        shrink_sparse(&mut self.candidate_ids);
        Some(node.payload)
    }
}

/// A direct-edge reverse index from canonicals to the candidates whose
/// receipts reach them, keyed by candidate `K` with payload `P`. See the
/// module docs.
pub struct ReceiptReverseIndex<K, P> {
    state: Mutex<IndexState<K, P>>,
}

impl<K, P> ReceiptReverseIndex<K, P> {
    fn with_storage(storage: IndexStorage) -> Self {
        Self {
            state: Mutex::new(IndexState::new(storage)),
        }
    }

    /// An empty index whose own storage is charged to `account`; each
    /// candidate's own registration ([`Self::candidate_bytes`]) is charged
    /// by the candidate's owner.
    pub fn charged(account: &Arc<SemanticRetentionAccount>) -> Self {
        Self::with_storage(IndexStorage::new(Arc::clone(account), true))
    }

    /// An empty index charging `account` for everything it holds, its
    /// candidates' registrations included: for an owner that charges
    /// nothing of its candidates itself.
    pub fn charging_registrations(account: &Arc<SemanticRetentionAccount>) -> Self {
        Self::with_storage(IndexStorage::new(Arc::clone(account), false))
    }
}

impl<K, P> Default for ReceiptReverseIndex<K, P> {
    /// [`Self::charged`] to the process-local account.
    fn default() -> Self {
        Self::charged(&SemanticRetentionAccount::process_local())
    }
}

impl<K: Eq + Hash + Clone, P: Clone> ReceiptReverseIndex<K, P> {
    /// The bytes a candidate's own registration keeps: its two table
    /// buckets and node, its extra-canonical list, and the edges naming it
    /// (one per extra canonical, one from its carrier). Its payload's heap
    /// is the caller's to add.
    pub fn candidate_bytes(extras: usize, has_carrier: bool) -> usize {
        id_bucket_bytes::<K>()
            + candidate_bucket_bytes::<K, P>()
            + extras * std::mem::size_of::<Arc<str>>()
            + (extras + usize::from(has_carrier)) * EDGE_BYTES
    }

    /// Register candidate `key` with `payload`: its carrier evidence, every
    /// receipt that evidence reaches that is not indexed yet, and its extra
    /// canonicals. A key already registered is replaced. A candidate whose
    /// carrier holds no fact and that names no extra canonical is not
    /// registered; one whose facts name no canonical still is — no walk
    /// reaches it, and it drains with its candidate.
    pub fn register<'a>(
        &self,
        key: K,
        payload: P,
        carrier: &Arc<[FactVersionRef]>,
        extras: impl IntoIterator<Item = &'a str>,
    ) {
        let extras: SmallVec<[&str; 8]> = dedup_canonicals(extras);
        let mut released = Vec::new();
        {
            let mut state = self.state.lock();
            state.unregister(&key, &mut released);
            if carrier.is_empty() && extras.is_empty() {
                state.settle();
                drop(state);
                drop(released);
                return;
            }
            let id = state.next_candidate;
            state.next_candidate += 1;
            let holder = Holder::Candidate(id);
            let extras: Box<[Arc<str>]> = extras
                .iter()
                .map(|canonical| state.link_canonical(canonical, holder))
                .collect();
            state.storage.candidate_extras += extras.len();
            let carrier = state.link_evidence(EvidenceOwner::Carrier(Arc::clone(carrier)), holder);
            state.candidate_ids.insert(key.clone(), id);
            state.candidates.insert(
                id,
                CandidateNode {
                    key,
                    payload,
                    carrier,
                    extras,
                },
            );
            state.settle();
        }
        drop(released);
    }

    /// Unregister candidate `key`, dropping every node and canonical only
    /// it reached. Answers its payload when it was registered.
    pub fn unregister(&self, key: &K) -> Option<P> {
        let mut released = Vec::new();
        let payload = {
            let mut state = self.state.lock();
            let payload = state.unregister(key, &mut released);
            state.settle();
            payload
        };
        drop(released);
        payload
    }

    /// Update candidate `key`'s payload; `retain` answering `false`
    /// unregisters it. Answers whether `key` was registered.
    pub fn update(&self, key: &K, retain: impl FnOnce(&mut P) -> bool) -> bool {
        let mut released = Vec::new();
        let found = {
            let mut state = self.state.lock();
            let Some(&id) = state.candidate_ids.get(key) else {
                return false;
            };
            let node = state
                .candidates
                .get_mut(&id)
                .expect("every id names a node");
            if !retain(&mut node.payload) {
                state.unregister(key, &mut released);
                state.settle();
            }
            true
        };
        drop(released);
        found
    }

    /// Every candidate whose receipts reach `canonical`, with its payload,
    /// each once: a walk up from the evidence that reads it, each node
    /// visited once.
    pub fn reached_by(&self, canonical: &str) -> Vec<(K, P)> {
        #[cfg_attr(
            not(any(test, feature = "test-support", feature = "semantic-observe")),
            allow(unused_mut)
        )]
        let mut state = self.state.lock();
        let Some(holders) = state.canonicals.get(canonical) else {
            return Vec::new();
        };
        let mut visited: FxHashSet<Holder> = FxHashSet::default();
        let mut stack: Vec<Holder> = holders.iter().copied().collect();
        let mut reached = Vec::new();
        while let Some(holder) = stack.pop() {
            if !visited.insert(holder) {
                continue;
            }
            match holder {
                Holder::Candidate(id) => {
                    let node = &state.candidates[&id];
                    reached.push((node.key.clone(), node.payload.clone()));
                }
                Holder::Evidence(addr) => {
                    stack.extend(state.evidence[&addr].parents.iter().copied());
                }
            }
        }
        #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
        {
            state.ops.walked += visited.len() as u64;
        }
        reached
    }

    /// Unregister every candidate, giving back all of the index's storage.
    pub fn clear(&self) {
        let released = {
            let mut state = self.state.lock();
            let released: Vec<EvidenceOwner> =
                state.evidence.drain().map(|(_, node)| node.owner).collect();
            state.canonicals = FxHashMap::default();
            state.evidence = FxHashMap::default();
            state.candidate_ids = FxHashMap::default();
            state.candidates = FxHashMap::default();
            state.edges = 0;
            state.storage.text = 0;
            state.storage.set_slots = 0;
            state.storage.candidate_edges = 0;
            state.storage.candidate_extras = 0;
            state.settle();
            released
        };
        drop(released);
    }

    /// What the index holds now.
    pub fn occupancy(&self) -> ReverseIndexOccupancy {
        let state = self.state.lock();
        ReverseIndexOccupancy {
            canonicals: state.canonicals.len(),
            evidence: state.evidence.len(),
            candidates: state.candidates.len(),
            edges: state.edges,
        }
    }

    /// Bytes the index's own storage is charged.
    pub fn storage_charge_bytes(&self) -> usize {
        self.state
            .lock()
            .storage
            .pin
            .as_ref()
            .map_or(0, GranularPin::charged)
    }

    /// The edge operations performed so far.
    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    pub fn edge_ops(&self) -> ReverseIndexEdgeOps {
        self.state.lock().ops
    }
}

/// `canonicals`, each once, in first-observation order.
fn dedup_canonicals<'a>(canonicals: impl IntoIterator<Item = &'a str>) -> SmallVec<[&'a str; 8]> {
    const LINEAR_SCAN_MAX: usize = 32;
    let mut ordered: SmallVec<[&str; 8]> = SmallVec::new();
    let mut wide: Option<FxHashSet<&str>> = None;
    for canonical in canonicals {
        let fresh = match &mut wide {
            Some(index) => index.insert(canonical),
            None => !ordered.contains(&canonical),
        };
        if fresh {
            ordered.push(canonical);
            if wide.is_none() && ordered.len() > LINEAR_SCAN_MAX {
                wide = Some(ordered.iter().copied().collect());
            }
        }
    }
    ordered
}

// ── The family memo's index ───────────────────────────────────────────

/// What one family-memo candidate registers: its family and the slots its
/// publication populated. The store hands out each `admission_seq` once,
/// so the seq keys the registration and the candidate's slots share it.
#[derive(Clone)]
pub(super) struct MemoRegistration {
    family: Arc<FamilyKey>,
    slots: SmallVec<[ModeSlot; 4]>,
}

impl MemoRegistration {
    pub(super) fn family(&self) -> &FamilyKey {
        &self.family
    }

    pub(super) fn slots(&self) -> &[ModeSlot] {
        &self.slots
    }
}

/// The family memo's reverse index, keyed by candidate `admission_seq`.
///
/// It is the third member of the family memo's consistency cluster —
/// `entries`, `memo_budget` and this index — and every mutation runs under
/// the `entries` lock, so a candidate's slot membership and its
/// registration change in one step.
pub(super) type CanonicalToEntries = ReceiptReverseIndex<u64, MemoRegistration>;

/// One publication's registration inputs, reused by every member of a
/// publication batch: the carrier and the dispatch-fence canonicals.
pub(super) struct ReverseIndexInputs<'a> {
    carrier: &'a Arc<[FactVersionRef]>,
    extras: SmallVec<[&'a str; 8]>,
}

impl<'a> ReverseIndexInputs<'a> {
    pub(super) fn new(
        read_set_signature: &'a verter_session_query::facts::fact_cache::ReadSetSignature,
        dispatch_dep_signature: &'a DepSignature,
    ) -> Self {
        Self {
            carrier: &read_set_signature.facts,
            extras: dedup_canonicals(
                dispatch_dep_signature
                    .iter()
                    .map(|crate::semantic_query::DepEntry(canonical, _)| canonical.as_ref()),
            ),
        }
    }

    /// The edges a candidate registered from these inputs names: one per
    /// dispatch-fence canonical and one from its carrier. Zero when it
    /// registers nothing.
    pub(super) fn direct_edge_count(&self) -> usize {
        self.extras.len() + usize::from(!self.carrier.is_empty())
    }
}

/// What one family-memo candidate's registration keeps, by the edges naming
/// it ([`ReverseIndexInputs::direct_edge_count`]): its node, its edges, and
/// the one `Arc<FamilyKey>` clone it names its family by — its block and the
/// boxes the clone copies, plus the owned names the clone shares with
/// `family`, handed back for the candidate to hold. The evidence it shares
/// with other candidates is the index's own storage.
pub(super) fn registration_footprint(
    family: &FamilyKey,
    direct_edges: usize,
) -> verter_session_query::retention::Footprint {
    if direct_edges == 0 {
        return verter_session_query::retention::Footprint {
            bytes: 0,
            handles: Default::default(),
        };
    }
    let clone = super::family_footprint::family_key_clone_heap_bytes(family);
    // The carrier edge is counted with the extras; the node's extra list
    // holds at most every edge.
    verter_session_query::retention::Footprint {
        bytes: CanonicalToEntries::candidate_bytes(direct_edges, false)
            + arc_block_bytes(std::mem::size_of::<FamilyKey>())
            + clone.bytes,
        handles: clone.handles,
    }
}

/// Register the just-published candidate `admission_seq` for every slot
/// its publication populated. Caller holds the `entries` lock.
pub(super) fn register_reverse_index(
    canonical_to_entries: &CanonicalToEntries,
    family: &FamilyKey,
    populated_slots: &[ModeSlot],
    inputs: &ReverseIndexInputs<'_>,
    admission_seq: u64,
) {
    if populated_slots.is_empty() || inputs.direct_edge_count() == 0 {
        return;
    }
    // The index's lock acquisition, as request audit contention.
    #[cfg(feature = "semantic-observe")]
    crate::request_observers::record_family_map_lock_acquisition(std::time::Duration::ZERO);
    canonical_to_entries.register(
        admission_seq,
        MemoRegistration {
            family: Arc::new(family.clone()),
            slots: populated_slots.iter().copied().collect(),
        },
        inputs.carrier,
        inputs.extras.iter().copied(),
    );
}

/// Drop `entry`'s registration for `slot`; a candidate whose last slot
/// leaves is unregistered. Its sibling candidates, and its other slots,
/// keep theirs. Caller holds the `entries` lock.
pub(super) fn drain_candidate_reverse_index_registrations(
    canonical_to_entries: &CanonicalToEntries,
    slot: ModeSlot,
    entry: &MemoEntry,
) {
    canonical_to_entries.update(&entry.admission_seq, |registration| {
        registration.slots.retain(|registered| *registered != slot);
        !registration.slots.is_empty()
    });
}

/// Drop the registrations of every candidate a REMOVED family's slots hold.
/// Answers whether any candidate was registered. Caller holds the `entries`
/// lock.
pub(super) fn drain_family_slots_registrations(
    canonical_to_entries: &CanonicalToEntries,
    slots: &FamilySlots,
) -> bool {
    let mut pruned_any = false;
    for (slot, entry) in slots.iter_populated_slots_all() {
        pruned_any |= canonical_to_entries.update(&entry.admission_seq, |registration| {
            registration.slots.retain(|registered| *registered != slot);
            !registration.slots.is_empty()
        });
    }
    pruned_any
}

#[cfg(test)]
#[path = "reverse_index_tests.rs"]
mod tests;
