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
//! - **canonical → evidence**: a canonical names the evidence (a receipt or
//!   a candidate's carrier) whose OWN facts read it;
//! - **child → parent**: a consumed receipt names each evidence that
//!   consumed it, including receipts no cache holds (uncached
//!   intermediates);
//! - **evidence → candidate**: a carrier names the candidates it validates.
//!
//! A candidate's dispatch fence is evidence too: one node per fence
//! allocation, naming every candidate that shares it, so candidates over
//! one interned fence register its canonicals once. A candidate also
//! registers directly under its extra canonicals (strict self-roots). A
//! chain or a diamond therefore costs
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
//! as it is held, by the allocation itself under the declared heap-block
//! convention ([`heap_block_bytes`]): a table or set by the layout its
//! backing was allocated with — which a removal, tombstoned or not, never
//! shrinks — from the moment it is allocated until it is rebuilt or freed;
//! a candidate's extra-canonical list and each canonical's text by their
//! own blocks. The candidates' own share is the only part an owner may
//! charge instead ([`ReceiptReverseIndex::charged`]); an owner that charges
//! nothing of its candidates leaves all of it to the index
//! ([`ReceiptReverseIndex::charging_registrations`]). There is no
//! account-less index: [`Default`] charges the process-local account.
//!
//! **Registration at publication (family memo).** A family-memo candidate is
//! registered by the publish that makes it visible, under the same `entries`
//! lock hold, before the candidates it displaces are unregistered: evidence
//! the newcomer shares with a displaced sibling stays indexed across the
//! replacement, and no read of the index ever owes registration work.
//!
//! **Locking.** One leaf mutex guards the whole index; no other lock is
//! taken while it is held, and the evidence and candidates (keys and
//! payloads) the index drops are released after it unlocks, since their
//! final owners may take other locks as they drop. Callers register and unregister inside their own consistency
//! boundary, so a candidate's visibility and its registration change in one
//! step.

use std::hash::Hash;
use std::sync::Arc;

use parking_lot::Mutex;
use rustc_hash::{FxBuildHasher, FxHashSet};
use smallvec::SmallVec;
use verter_session_query::facts::fact_cache::{FactVersionRef, ResultReceipt};
use verter_session_query::retention::allocation::{arc_block_bytes, heap_block_bytes};
use verter_session_query::retention::granular::GranularPin;
use verter_session_query::retention::SemanticRetentionAccount;

/// The index's hash maps: they report the layout of their backing
/// allocation, which the index's charge follows.
type Map<K, V> = hashbrown::HashMap<K, V, FxBuildHasher>;

/// The index's hash sets; see [`Map`].
type Set<T> = hashbrown::HashSet<T, FxBuildHasher>;

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
    /// A consumed result's receipt.
    Receipt(ResultReceipt),
    /// A candidate's carrier.
    Carrier(verter_session_query::facts::fact_set::FactSet),
    /// A candidate's dispatch fence, shared by every candidate holding the
    /// same allocation.
    Fence(DepSignature),
}

/// What one evidence node reads: a canonical directly, or a consumed
/// receipt.
enum EvidenceRead<'a> {
    Canonical(&'a str),
    Receipt(&'a ResultReceipt),
}

impl EvidenceOwner {
    fn is_empty(&self) -> bool {
        match self {
            Self::Receipt(receipt) => receipt.facts().is_empty(),
            Self::Carrier(facts) => facts.is_empty(),
            Self::Fence(fence) => fence.is_empty(),
        }
    }

    fn addr(&self) -> EvidenceAddr {
        match self {
            Self::Receipt(receipt) => receipt.facts().as_ptr() as EvidenceAddr,
            Self::Carrier(facts) => facts.address() as EvidenceAddr,
            Self::Fence(fence) => fence.as_ptr() as EvidenceAddr,
        }
    }

    /// Visit what the node reads, in order: each consumed receipt, and each
    /// canonical once. A read set names a file once per fact it read of
    /// it, but linking a holder under a file, and unlinking it, is one
    /// edge however many of the file's facts it read.
    fn for_each_read<'a>(&'a self, mut visit: impl FnMut(EvidenceRead<'a>)) {
        let mut visited = FirstSeen::default();
        match self {
            Self::Receipt(receipt) => {
                for fact in receipt.facts() {
                    if let Some(read) = read_fact(fact, &mut visited) {
                        visit(read);
                    }
                }
            }
            // A carrier set is read in place.
            Self::Carrier(facts) => {
                for fact in facts.iter() {
                    if let Some(read) = read_fact(fact, &mut visited) {
                        visit(read);
                    }
                }
            }
            Self::Fence(fence) => {
                for crate::semantic_query::DepEntry(canonical, _) in fence.iter() {
                    if visited.insert(canonical) {
                        visit(EvidenceRead::Canonical(canonical));
                    }
                }
            }
        }
    }
}

/// What reading `fact` visits: a consumed receipt, or its canonical the
/// first time any fact of the node names it.
fn read_fact<'a>(
    fact: &'a FactVersionRef,
    visited: &mut FirstSeen<'a>,
) -> Option<EvidenceRead<'a>> {
    match fact {
        FactVersionRef::Receipt(child) => Some(EvidenceRead::Receipt(child)),
        leaf => leaf
            .canonical_id()
            .filter(|canonical| visited.insert(canonical))
            .map(EvidenceRead::Canonical),
    }
}

struct EvidenceNode {
    owner: EvidenceOwner,
    /// The evidence nodes that consumed this one and the candidates it
    /// validates.
    parents: Tracked<Set<Holder>>,
}

/// A hash table the index can resize and rebuild.
trait Table: Default {
    fn len(&self) -> usize;
    fn capacity(&self) -> usize;
    /// The size of the table's backing allocation, zero when it has none.
    fn allocation_size(&self) -> usize;
    /// The same entries in a fresh allocation for at least `capacity`.
    fn rebuilt(self, capacity: usize) -> Self;
}

impl<T: Hash + Eq, V> Table for Map<T, V> {
    fn len(&self) -> usize {
        self.len()
    }

    fn capacity(&self) -> usize {
        self.capacity()
    }

    fn allocation_size(&self) -> usize {
        self.allocation_size()
    }

    fn rebuilt(self, capacity: usize) -> Self {
        let mut table = Map::with_capacity_and_hasher(capacity, FxBuildHasher);
        table.extend(self);
        table
    }
}

impl<T: Hash + Eq> Table for Set<T> {
    fn len(&self) -> usize {
        self.len()
    }

    fn capacity(&self) -> usize {
        self.capacity()
    }

    fn allocation_size(&self) -> usize {
        self.allocation_size()
    }

    fn rebuilt(self, capacity: usize) -> Self {
        let mut table = Set::with_capacity_and_hasher(capacity, FxBuildHasher);
        table.extend(self);
        table
    }
}

/// A hash table and the most entries its current allocation was sized for.
///
/// A removal may leave a tombstone that lowers the table's `capacity()`
/// without giving its bucket back. `slots` is the capacity the current
/// allocation was made with — the most `capacity()` has answered since — so
/// [`Tracked::release_sparse`] judges sparseness against the allocation,
/// not against what tombstones leave of it. The charge never reads `slots`:
/// it follows the allocation's own layout ([`Tracked::block_bytes`]).
#[derive(Default)]
struct Tracked<T> {
    table: T,
    slots: usize,
}

impl<T: Table> Tracked<T> {
    /// What the table's backing allocation costs under the declared
    /// heap-block convention.
    fn block_bytes(&self) -> usize {
        heap_block_bytes(self.table.allocation_size())
    }

    /// Take in the table's capacity after an insertion: a reallocation
    /// raises `slots` to it.
    fn note_inserted(&mut self) {
        self.slots = self.slots.max(self.table.capacity());
    }

    /// Free the table once empty, and rebuild it into an allocation twice
    /// its live count once a quarter or less of its slots is live.
    fn release_sparse(&mut self) {
        let len = self.table.len();
        if len == 0 {
            *self = Self::default();
        } else if len * 4 <= self.slots {
            self.table = std::mem::take(&mut self.table).rebuilt(len * 2);
            self.slots = self.table.capacity();
        }
    }
}

struct CandidateNode<K, P> {
    key: K,
    payload: P,
    /// The carrier evidence node, when the carrier holds any fact.
    carrier: Option<EvidenceAddr>,
    /// The dispatch-fence evidence node, when the candidate has a fence.
    fence: Option<EvidenceAddr>,
    /// The extra canonicals the candidate registered under, sharing the
    /// index's own copy of each.
    extras: Box<[Arc<str>]>,
}

/// Bytes one bucket of the candidate-id table holds: its entry and its
/// control byte, which a live entry occupies within the table's allocation.
const fn id_bucket_bytes<K>() -> usize {
    std::mem::size_of::<(K, CandidateId)>() + 1
}

/// Bytes one bucket of the candidate table holds; see [`id_bucket_bytes`].
const fn candidate_bucket_bytes<K, P>() -> usize {
    std::mem::size_of::<(CandidateId, CandidateNode<K, P>)>() + 1
}

/// Bytes one edge's bucket holds in the set naming it.
const EDGE_BYTES: usize = std::mem::size_of::<Holder>() + 1;

/// Bytes a candidate's list of `extras` extra canonicals costs: one block.
const fn extras_list_bytes(extras: usize) -> usize {
    heap_block_bytes(extras * std::mem::size_of::<Arc<str>>())
}

/// Granule the index's own storage is charged in.
const INDEX_STORAGE_GRANULE: usize = 4 * 1024;

/// What the index currently holds: a required occupancy count, read under
/// the index's lock.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReverseIndexOccupancy {
    /// Canonicals some node reads directly.
    pub canonicals: usize,
    /// Evidence nodes: carriers and receipts, cached or not.
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
    /// Backing blocks of every holder set — a canonical's holders, an
    /// evidence node's parents.
    set_blocks: usize,
    /// Blocks of the candidates' extra-canonical lists.
    extras_blocks: usize,
    /// The registrations the candidates' owners charge
    /// ([`ReceiptReverseIndex::candidate_bytes`] per live candidate).
    owner_share: usize,
}

impl IndexStorage {
    fn new(account: Arc<SemanticRetentionAccount>, candidates_charged_by_owner: bool) -> Self {
        Self {
            account,
            candidates_charged_by_owner,
            pin: None,
            text: 0,
            set_blocks: 0,
            extras_blocks: 0,
            owner_share: 0,
        }
    }
}

struct IndexState<K, P> {
    canonicals: Tracked<Map<Arc<str>, Tracked<Set<Holder>>>>,
    evidence: Tracked<Map<EvidenceAddr, EvidenceNode>>,
    candidate_ids: Tracked<Map<K, CandidateId>>,
    candidates: Tracked<Map<CandidateId, CandidateNode<K, P>>>,
    next_candidate: CandidateId,
    edges: usize,
    storage: IndexStorage,
    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    ops: ReverseIndexEdgeOps,
}

impl<K, P> IndexState<K, P> {
    fn new(storage: IndexStorage) -> Self {
        Self {
            canonicals: Tracked::default(),
            evidence: Tracked::default(),
            candidate_ids: Tracked::default(),
            candidates: Tracked::default(),
            next_candidate: 0,
            edges: 0,
            storage,
            #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
            ops: ReverseIndexEdgeOps::default(),
        }
    }
}

impl<K: Eq + Hash, P> IndexState<K, P> {
    /// The bytes the index's charge covers: every table's and every holder
    /// set's backing block, the candidates' extra-canonical lists and the
    /// canonicals' text — less the share each candidate's owner charges
    /// when it does. That share is a candidate's own buckets, list and
    /// edges, all inside blocks counted here, so it never exceeds them; a
    /// share that did would be broken accounting, which fails here rather
    /// than settling the index's charge at a plausible zero.
    fn storage_bytes(&self) -> usize {
        let held = self.canonicals.block_bytes()
            + self.evidence.block_bytes()
            + self.candidate_ids.block_bytes()
            + self.candidates.block_bytes()
            + self.storage.set_blocks
            + self.storage.extras_blocks
            + self.storage.text;
        held.checked_sub(self.storage.owner_share)
            .expect("the candidates' owner share lies inside the blocks the index holds")
    }

    /// What candidate `node`'s owner charges of its registration, when the
    /// owner charges it.
    fn owner_share(&self, node: &CandidateNode<K, P>) -> usize {
        if self.storage.candidates_charged_by_owner {
            ReceiptReverseIndex::<K, P>::candidate_bytes(
                node.extras.len(),
                usize::from(node.carrier.is_some()) + usize::from(node.fence.is_some()),
            )
        } else {
            0
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

/// Release a non-empty holder set's spare buckets, keeping `set_blocks` in
/// step with its backing block.
fn release_holders(set_blocks: &mut usize, set: &mut Tracked<Set<Holder>>) {
    let before = set.block_bytes();
    set.release_sparse();
    *set_blocks = *set_blocks + set.block_bytes() - before;
}

/// Insert `holder` into `set`, keeping `set_blocks` in step with its backing
/// block. Answers whether it was new.
fn insert_holder(set_blocks: &mut usize, set: &mut Tracked<Set<Holder>>, holder: Holder) -> bool {
    let before = set.block_bytes();
    let inserted = set.table.insert(holder);
    set.note_inserted();
    *set_blocks = *set_blocks + set.block_bytes() - before;
    inserted
}

impl<K: Eq + Hash + Clone, P> IndexState<K, P> {
    fn note_linked(&mut self) {
        self.edges += 1;
        #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
        {
            self.ops.linked += 1;
        }
    }

    fn note_unlinked(&mut self) {
        self.edges -= 1;
        #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
        {
            self.ops.unlinked += 1;
        }
    }

    /// Register `holder` under `canonical`, answering the index's copy of
    /// the canonical's text.
    fn link_canonical(&mut self, canonical: &str, holder: Holder) -> Arc<str> {
        let key = match self.canonicals.table.get_key_value(canonical) {
            Some((key, _)) => Arc::clone(key),
            None => {
                let key: Arc<str> = Arc::from(canonical);
                self.storage.text += arc_block_bytes(canonical.len());
                self.canonicals
                    .table
                    .insert(Arc::clone(&key), Tracked::default());
                self.canonicals.note_inserted();
                key
            }
        };
        let set = self
            .canonicals
            .table
            .get_mut(canonical)
            .expect("present above");
        if insert_holder(&mut self.storage.set_blocks, set, holder) {
            self.note_linked();
        }
        key
    }

    /// Remove `holder` from `canonical`, dropping the canonical with its
    /// last holder.
    fn unlink_canonical(&mut self, canonical: &str, holder: Holder) {
        let Some(set) = self.canonicals.table.get_mut(canonical) else {
            return;
        };
        if !set.table.remove(&holder) {
            return;
        }
        if set.table.is_empty() {
            self.storage.set_blocks -= set.block_bytes();
            self.canonicals.table.remove(canonical);
            self.storage.text -= arc_block_bytes(canonical.len());
            self.canonicals.release_sparse();
        } else {
            release_holders(&mut self.storage.set_blocks, set);
        }
        self.note_unlinked();
    }

    /// Link `owner` under `parent`, registering every evidence node it
    /// reaches that is not indexed yet — each once, from an explicit stack.
    /// Evidence with no fact reaches no canonical and is not indexed.
    fn link_evidence(&mut self, owner: EvidenceOwner, parent: Holder) -> Option<EvidenceAddr> {
        if owner.is_empty() {
            return None;
        }
        let root = owner.addr();
        let mut pending: Vec<(EvidenceOwner, Holder)> = vec![(owner, parent)];
        while let Some((owner, parent)) = pending.pop() {
            let addr = owner.addr();
            if let Some(node) = self.evidence.table.get_mut(&addr) {
                if insert_holder(&mut self.storage.set_blocks, &mut node.parents, parent) {
                    self.note_linked();
                }
                continue;
            }
            let holder = Holder::Evidence(addr);
            owner.for_each_read(|read| match read {
                EvidenceRead::Receipt(child) => {
                    if !child.facts().is_empty() {
                        pending.push((EvidenceOwner::Receipt(child.clone()), holder));
                    }
                }
                EvidenceRead::Canonical(canonical) => {
                    self.link_canonical(canonical, holder);
                }
            });
            let mut parents = Tracked::default();
            insert_holder(&mut self.storage.set_blocks, &mut parents, parent);
            self.note_linked();
            self.evidence
                .table
                .insert(addr, EvidenceNode { owner, parents });
            self.evidence.note_inserted();
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
            let Some(node) = self.evidence.table.get_mut(&addr) else {
                continue;
            };
            if !node.parents.table.remove(&parent) {
                continue;
            }
            if !node.parents.table.is_empty() {
                release_holders(&mut self.storage.set_blocks, &mut node.parents);
                self.note_unlinked();
                continue;
            }
            self.note_unlinked();
            let node = self.evidence.table.remove(&addr).expect("present above");
            self.storage.set_blocks -= node.parents.block_bytes();
            let holder = Holder::Evidence(addr);
            node.owner.for_each_read(|read| match read {
                EvidenceRead::Receipt(child) => {
                    if !child.facts().is_empty() {
                        pending.push((child.facts().as_ptr() as EvidenceAddr, holder));
                    }
                }
                EvidenceRead::Canonical(canonical) => {
                    self.unlink_canonical(canonical, holder);
                }
            });
            released.push(node.owner);
        }
        self.evidence.release_sparse();
    }

    /// Unregister candidate `key`, answering its id-table key and its node
    /// for the caller to drop after the index unlocks.
    fn unregister(
        &mut self,
        key: &K,
        released: &mut Vec<EvidenceOwner>,
    ) -> Option<(K, CandidateNode<K, P>)> {
        let (key, id) = self.candidate_ids.table.remove_entry(key)?;
        Some((key, self.unregister_id(id, released)))
    }

    /// Unregister the candidate numbered `id`, whose key no longer names it,
    /// answering its node for the caller to drop after the index unlocks.
    fn unregister_id(
        &mut self,
        id: CandidateId,
        released: &mut Vec<EvidenceOwner>,
    ) -> CandidateNode<K, P> {
        let node = self
            .candidates
            .table
            .remove(&id)
            .expect("every id names a node");
        let holder = Holder::Candidate(id);
        self.storage.owner_share -= self.owner_share(&node);
        self.storage.extras_blocks -= extras_list_bytes(node.extras.len());
        for canonical in node.extras.iter() {
            self.unlink_canonical(canonical, holder);
        }
        for evidence in [node.carrier, node.fence].into_iter().flatten() {
            self.unlink_evidence(evidence, holder, released);
        }
        self.candidates.release_sparse();
        self.candidate_ids.release_sparse();
        node
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

    /// The bytes a candidate's own registration keeps: its two table
    /// buckets and node, its extra-canonical list, and the edges naming it
    /// (one per extra canonical, one from each of its `evidence` nodes —
    /// its carrier and its fence). Its payload's heap is the caller's to
    /// add.
    pub fn candidate_bytes(extras: usize, evidence: usize) -> usize {
        id_bucket_bytes::<K>()
            + candidate_bucket_bytes::<K, P>()
            + extras_list_bytes(extras)
            + (extras + evidence) * EDGE_BYTES
    }
}

impl<K, P> Default for ReceiptReverseIndex<K, P> {
    /// [`Self::charged`] to the process-local account.
    fn default() -> Self {
        Self::charged(&SemanticRetentionAccount::process_local())
    }
}

impl<K: Eq + Hash + Clone, P: Clone> ReceiptReverseIndex<K, P> {
    /// Register candidate `key` with `payload`: its carrier evidence, every
    /// receipt that evidence reaches that is not indexed yet, and its extra
    /// canonicals. A key already registered is replaced: the newcomer is
    /// linked first, so evidence it shares with the replaced registration
    /// stays indexed. A candidate whose
    /// carrier holds no fact and that names no extra canonical is not
    /// registered; one whose facts name no canonical still is — no walk
    /// reaches it, and it drains with its candidate.
    pub fn register<'a>(
        &self,
        key: K,
        payload: P,
        carrier: &verter_session_query::facts::fact_set::FactSet,
        extras: impl IntoIterator<Item = &'a str>,
    ) {
        self.register_fenced(key, payload, carrier, &DepSignature::default(), extras);
    }

    /// [`Self::register`], with the candidate's dispatch `fence` registered
    /// as one evidence node that every candidate holding the same fence
    /// allocation shares, instead of as extra canonicals of each.
    pub fn register_fenced<'a>(
        &self,
        key: K,
        payload: P,
        carrier: &verter_session_query::facts::fact_set::FactSet,
        fence: &DepSignature,
        extras: impl IntoIterator<Item = &'a str>,
    ) {
        let extras: SmallVec<[&str; 8]> = dedup_canonicals(extras);
        let mut released = Vec::new();
        let replaced = {
            let mut state = self.state.lock();
            let replaced = state.candidate_ids.table.remove_entry(&key);
            if !(carrier.is_empty() && fence.is_empty() && extras.is_empty()) {
                let id = state.next_candidate;
                state.next_candidate += 1;
                let holder = Holder::Candidate(id);
                let extras: Box<[Arc<str>]> = extras
                    .iter()
                    .map(|canonical| state.link_canonical(canonical, holder))
                    .collect();
                let carrier = state.link_evidence(EvidenceOwner::Carrier(carrier.clone()), holder);
                let fence = state.link_evidence(EvidenceOwner::Fence(fence.clone()), holder);
                state.candidate_ids.table.insert(key.clone(), id);
                state.candidate_ids.note_inserted();
                let node = CandidateNode {
                    key,
                    payload,
                    carrier,
                    fence,
                    extras,
                };
                state.storage.extras_blocks += extras_list_bytes(node.extras.len());
                state.storage.owner_share += state.owner_share(&node);
                state.candidates.table.insert(id, node);
                state.candidates.note_inserted();
            }
            let replaced = replaced.map(|(key, id)| (key, state.unregister_id(id, &mut released)));
            state.settle();
            replaced
        };
        drop(released);
        drop(replaced);
    }

    /// Unregister candidate `key`, dropping every node and canonical only
    /// it reached. Answers its payload when it was registered.
    pub fn unregister(&self, key: &K) -> Option<P> {
        let mut released = Vec::new();
        let removed = {
            let mut state = self.state.lock();
            let removed = state.unregister(key, &mut released);
            state.settle();
            removed
        };
        drop(released);
        removed.map(|(_, node)| node.payload)
    }

    /// Update candidate `key`'s payload; `retain` answering `false`
    /// unregisters it. Answers whether `key` was registered.
    pub fn update(&self, key: &K, retain: impl FnOnce(&mut P) -> bool) -> bool {
        let mut released = Vec::new();
        let removed = {
            let mut state = self.state.lock();
            let Some(&id) = state.candidate_ids.table.get(key) else {
                return false;
            };
            let node = state
                .candidates
                .table
                .get_mut(&id)
                .expect("every id names a node");
            if retain(&mut node.payload) {
                None
            } else {
                let removed = state.unregister(key, &mut released);
                state.settle();
                removed
            }
        };
        drop(released);
        drop(removed);
        true
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
        let Some(holders) = state.canonicals.table.get(canonical) else {
            return Vec::new();
        };
        let mut visited: FxHashSet<Holder> = FxHashSet::default();
        let mut stack: Vec<Holder> = holders.table.iter().copied().collect();
        let mut reached = Vec::new();
        while let Some(holder) = stack.pop() {
            if !visited.insert(holder) {
                continue;
            }
            match holder {
                Holder::Candidate(id) => {
                    let node = &state.candidates.table[&id];
                    reached.push((node.key.clone(), node.payload.clone()));
                }
                Holder::Evidence(addr) => {
                    stack.extend(state.evidence.table[&addr].parents.table.iter().copied());
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
        let (released, candidate_ids, candidates) = {
            let mut state = self.state.lock();
            let released: Vec<EvidenceOwner> = std::mem::take(&mut state.evidence.table)
                .into_values()
                .map(|node| node.owner)
                .collect();
            state.canonicals = Tracked::default();
            state.evidence = Tracked::default();
            let candidate_ids = std::mem::take(&mut state.candidate_ids);
            let candidates = std::mem::take(&mut state.candidates);
            state.edges = 0;
            state.storage.text = 0;
            state.storage.set_blocks = 0;
            state.storage.extras_blocks = 0;
            state.storage.owner_share = 0;
            state.settle();
            (released, candidate_ids, candidates)
        };
        drop(released);
        drop(candidate_ids);
        drop(candidates);
    }

    /// What the index holds now.
    pub fn occupancy(&self) -> ReverseIndexOccupancy {
        let state = self.state.lock();
        ReverseIndexOccupancy {
            canonicals: state.canonicals.table.len(),
            evidence: state.evidence.table.len(),
            candidates: state.candidates.table.len(),
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

    /// Test-only: whether the index's lock is held right now.
    #[cfg(test)]
    pub(crate) fn is_locked_for_test(&self) -> bool {
        self.state.is_locked()
    }
}

/// `canonicals`, each once, in first-observation order.
fn dedup_canonicals<'a>(canonicals: impl IntoIterator<Item = &'a str>) -> SmallVec<[&'a str; 8]> {
    let mut seen = FirstSeen::default();
    for canonical in canonicals {
        seen.insert(canonical);
    }
    seen.ordered
}

/// The canonicals seen so far, in first-observation order. A short list is
/// scanned; past [`FirstSeen::LINEAR_SCAN_MAX`] entries a table answers
/// membership, so recording `k` distinct canonicals costs `O(k)` probes, not
/// `k²/2` comparisons.
#[derive(Default)]
struct FirstSeen<'a> {
    ordered: SmallVec<[&'a str; 8]>,
    wide: Option<FxHashSet<&'a str>>,
}

impl<'a> FirstSeen<'a> {
    const LINEAR_SCAN_MAX: usize = 32;

    /// Record `canonical`; `false` when it was already seen.
    fn insert(&mut self, canonical: &'a str) -> bool {
        let fresh = match &mut self.wide {
            Some(index) => {
                #[cfg(test)]
                CANONICAL_PROBES.with(|probes| probes.set(probes.get() + 1));
                index.insert(canonical)
            }
            None => {
                #[cfg(test)]
                CANONICAL_PROBES.with(|probes| probes.set(probes.get() + self.ordered.len()));
                !self.ordered.contains(&canonical)
            }
        };
        if fresh {
            self.ordered.push(canonical);
            if self.wide.is_none() && self.ordered.len() > Self::LINEAR_SCAN_MAX {
                self.wide = Some(self.ordered.iter().copied().collect());
            }
        }
        fresh
    }
}

#[cfg(test)]
thread_local! {
    /// Canonical membership probes this thread's [`FirstSeen`] sets made.
    static CANONICAL_PROBES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Canonical membership probes this thread has made so far.
#[cfg(test)]
pub(super) fn canonical_probes_for_tests() -> usize {
    CANONICAL_PROBES.with(std::cell::Cell::get)
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

/// The edges a family-memo candidate registers names: one from its carrier
/// when the carrier holds any fact and one from its dispatch fence when it
/// has one. Zero when it registers nothing.
pub(super) fn direct_edge_count(
    read_set_signature: &verter_session_query::facts::fact_cache::ReadSetSignature,
    dispatch_dep_signature: &DepSignature,
) -> usize {
    usize::from(!read_set_signature.facts.is_empty())
        + usize::from(!dispatch_dep_signature.is_empty())
}

/// What one family-memo candidate's registration keeps, by the evidence
/// edges naming it ([`direct_edge_count`]): its node, its edges, and
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
    verter_session_query::retention::Footprint {
        bytes: CanonicalToEntries::candidate_bytes(0, direct_edges)
            + arc_block_bytes(std::mem::size_of::<FamilyKey>())
            + clone.bytes,
        handles: clone.handles,
    }
}

/// Register the candidate `admission_seq` that a publish just made visible
/// in the `populated` slots of `family`: its carrier and its dispatch fence,
/// once for all of its slots. A candidate with no direct edge registers
/// nothing. Called under the `entries` lock by the publish itself, before
/// it unregisters the candidates it displaced, so evidence the newcomer
/// shares with them stays indexed. `_mode` is the publishing unit's capture
/// execution: only a captured one records the index lock acquisition.
pub(super) fn register_published<M: verter_audit::observe::ObserveExec>(
    _mode: M,
    observation: &crate::request_observers::CaptureTargets,
    canonical_to_entries: &CanonicalToEntries,
    family: &FamilyKey,
    populated: &[ModeSlot],
    admission_seq: u64,
    read_set_signature: &verter_session_query::facts::fact_cache::ReadSetSignature,
    dispatch_dep_signature: &DepSignature,
) {
    let _ = observation;
    if populated.is_empty() || direct_edge_count(read_set_signature, dispatch_dep_signature) == 0 {
        return;
    }
    // The index's lock acquisition, as request audit contention.
    #[cfg(feature = "semantic-observe")]
    crate::request_observers::record_family_map_lock_acquisition::<M>(
        observation,
        std::time::Duration::ZERO,
    );
    canonical_to_entries.register_fenced(
        admission_seq,
        MemoRegistration {
            family: Arc::new(family.clone()),
            slots: populated.iter().copied().collect(),
        },
        &read_set_signature.facts,
        dispatch_dep_signature,
        [],
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
