//! Hierarchical result evidence: a completed result's receipt.
//!
//! A result records the facts its own computation read and, for every
//! completed result it consumed, that result's [`ResultReceipt`] — never a
//! copy of the consumed result's facts. A chain `a2 -> a1 -> a0` therefore
//! stores `a2`'s own facts and one receipt for `a1`, whose evidence holds
//! `a1`'s own facts and one receipt for `a0`: the evidence of a whole
//! dependency graph is its facts plus its edges, shared, never every
//! prefix of it copied.
//!
//! A receipt is an ordinary [`FactVersionRef`] entry
//! ([`FactVersionRef::Receipt`]), so a signature holding one validates,
//! orders, deduplicates and travels like any other. Validity stays read-side:
//! a receipt validates exactly when every fact reachable through it
//! validates, each distinct receipt visited once
//! ([`ResultReceipt::all_leaves`]).
//!
//! A receipt's storage is charged to the retention account for exactly as
//! long as the receipt lives, once, however many signatures share it. It is
//! born a [`ChargeClass::Pinned`] obligation of the live value holding it.
//! The first cache admission that retains a signature reaching it claims it
//! ([`reserve_retained_with_evidence`], or [`claim_retained_storage`] for a
//! store that charges no bytes of its own): its bytes join that admission's
//! refusable [`ChargeClass::Retained`] reservation — so a wide candidate is
//! refused for its whole footprint like any other entry — and its pin is
//! exchanged for its share of the granted reservation. A sealed set's tree
//! nodes are claimed the same way ([`super::fact_tree`]).

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use super::version::{CompactionDomain, FactAttribution, FactVersionRef};
use crate::retention::{
    ChargeClass, RetentionAdmission, RetentionCharge, RetentionRefusal, SemanticRetentionAccount,
};

/// The evidence one completed result recorded: its own facts and the
/// receipts of the results it consumed, in canonical order, with the
/// summaries a consumer projecting a signature needs without walking it.
#[derive(Debug)]
pub struct ResultEvidence {
    facts: Arc<[FactVersionRef]>,
    digest: u128,
    /// Every canonical a fact reachable from here names: a persistent set
    /// sharing its structure with the sets of the receipts it consumed.
    canonicals: CanonicalSet,
    /// Every compaction domain a reachable fact carries as its terminal
    /// aggregate, in first-appearance order.
    aggregated: Arc<[CompactionDomain]>,
    /// Whether a reachable fact is resolution evidence.
    resolution_evidence: bool,
    /// Set once a retaining admission has claimed every allocation reachable
    /// from here. A claimed allocation never returns to a pin, so a later admission
    /// need not walk this evidence again.
    evidence_claimed: std::sync::atomic::AtomicBool,
    /// The evidence's own reservation, held for exactly as long as the
    /// evidence lives (every signature, candidate and refusal summary
    /// sharing it shares this one charge): pinned until a cache admission
    /// claims it, retained from then on.
    retention: parking_lot::Mutex<RetentionCharge>,
}

/// Every canonical the facts reachable from a receipt name, ordered.
///
/// A persistent set: a receipt's set starts as its largest consumed
/// receipt's set, shared, and gains only what that set lacks, so a chain
/// of `n` receipts each naming one new canonical holds bounded-depth HAMT
/// paths rather than a copy of every prefix. Its `len()` is the
/// logical count of the canonicals it names; the storage is shared.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct CanonicalSet(
    imbl::GenericHashMap<
        Arc<str>,
        (),
        rustc_hash::FxBuildHasher,
        imbl::shared_ptr::DefaultSharedPtr,
    >,
);

impl CanonicalSet {
    /// Whether `canonical` is one of the set's canonicals.
    #[must_use]
    pub fn contains(&self, canonical: &str) -> bool {
        self.0.contains_key(canonical)
    }

    /// The set's own copy of `canonical`, when it holds it.
    #[must_use]
    pub fn get(&self, canonical: &str) -> Option<&Arc<str>> {
        self.0.get_key_value(canonical).map(|(key, _)| key)
    }

    /// The canonicals, in order.
    pub fn iter(&self) -> impl Iterator<Item = &Arc<str>> + '_ {
        let mut names: Vec<_> = self.0.keys().collect();
        let work =
            crate::declarations::module_exports::sort_bytes(&mut names, |name| name.as_bytes());
        super::fact_read_set::record_signature_work(work);
        names.into_iter()
    }

    /// How many canonicals the set names.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the set names no canonical.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The union of `consumed` and `own`: the largest consumed set,
    /// shared, plus what each other set holds that it lacks, plus each
    /// own canonical it lacks.
    ///
    /// Also answers the estimated bytes of the storage the union owns
    /// beyond the set it shares: a slot per inserted canonical, and the
    /// string each newly allocated own canonical holds.
    fn union_of(consumed: &[&CanonicalSet], own: &[&str]) -> (Self, usize) {
        let mut owned_bytes = 0;
        let mut added_slots = 0;
        let Some(largest) = consumed.iter().copied().max_by_key(|set| set.len()) else {
            let mut set = imbl::GenericHashMap::with_hasher(rustc_hash::FxBuildHasher);
            for canonical in own {
                super::fact_read_set::record_signature_work(canonical.len() + 1);
                if !set.contains_key(*canonical) {
                    added_slots += 1;
                    owned_bytes += ARC_STR_HEADER_BYTES + canonical.len();
                    set.insert(Arc::from(*canonical), ());
                }
            }
            let slots = (set.len() * 2 + 32).min(added_slots * 13 * 32);
            return (Self(set), owned_bytes + slots * SET_SLOT_BYTES);
        };
        let mut set = largest.0.clone();
        for other in consumed {
            if std::ptr::eq(*other, largest) || other.0.ptr_eq(&largest.0) {
                continue;
            }
            for canonical in other.0.keys() {
                super::fact_read_set::record_signature_work(canonical.len() + 1);
                if !set.contains_key(canonical) {
                    added_slots += 1;
                    set.insert(Arc::clone(canonical), ());
                }
            }
        }
        for canonical in own {
            super::fact_read_set::record_signature_work(canonical.len() + 1);
            if !set.contains_key(*canonical) {
                added_slots += 1;
                owned_bytes += ARC_STR_HEADER_BYTES + canonical.len();
                set.insert(Arc::from(*canonical), ());
            }
        }
        let slots = (set.len() * 2 + 32).min(added_slots * 13 * 32);
        (Self(set), owned_bytes + slots * SET_SLOT_BYTES)
    }
}

impl std::fmt::Debug for CanonicalSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

/// Dropped from an explicit stack: a long chain of receipts, each the last
/// owner of the one it consumed, would otherwise drop one native frame per
/// level.
impl Drop for ResultEvidence {
    fn drop(&mut self) {
        let mut owned: Vec<Arc<ResultEvidence>> = Vec::new();
        take_consumed(&mut self.facts, &mut owned);
        while let Some(evidence) = owned.pop() {
            if let Ok(mut evidence) = Arc::try_unwrap(evidence) {
                take_consumed(&mut evidence.facts, &mut owned);
            }
        }
    }
}

/// Move the receipts `facts` consumed into `owned` when this is their last
/// holder, leaving a scalar in each place.
fn take_consumed(facts: &mut Arc<[FactVersionRef]>, owned: &mut Vec<Arc<ResultEvidence>>) {
    let Some(facts) = Arc::get_mut(facts) else {
        return;
    };
    for fact in facts.iter_mut() {
        if matches!(fact, FactVersionRef::Receipt(_)) {
            if let FactVersionRef::Receipt(receipt) =
                std::mem::replace(fact, FactVersionRef::ProjectGeneration { generation: 0 })
            {
                owned.push(receipt.0);
            }
        }
    }
}

/// Estimated bytes one canonical's slot in a [`CanonicalSet`] occupies:
/// the element and its share of the tree node holding it.
const SET_SLOT_BYTES: usize = 2 * std::mem::size_of::<Arc<str>>();

/// The reference counts heading a shared allocation.
const ARC_HEADER_BYTES: usize = 2 * std::mem::size_of::<usize>();

/// The header of one newly allocated `Arc<str>`.
const ARC_STR_HEADER_BYTES: usize = ARC_HEADER_BYTES;

/// Bytes one evidence owns: its shared block, its entry array, the heap its
/// own facts hold (a consumed receipt is its own charged allocation, so it
/// costs only its slot here), its aggregated domains and the canonical-set
/// storage it does not share with the evidence it consumed.
///
/// The owned names its facts reach are charged by their own cells: they
/// are pinned with the evidence (`pin_names_to`), never counted here.
fn evidence_storage_bytes(
    facts: &[FactVersionRef],
    aggregated: usize,
    canonical_set_bytes: usize,
    pin_names_to: &std::sync::Arc<crate::retention::SemanticRetentionAccount>,
) -> usize {
    use crate::retention::allocation::{arc_block_bytes, facts_heap_bytes, AliasSet};
    let mut aliases = AliasSet::new();
    let bytes = arc_block_bytes(std::mem::size_of::<ResultEvidence>())
        + arc_block_bytes(std::mem::size_of_val(facts))
        + facts_heap_bytes(facts, &mut aliases)
        + arc_block_bytes(aggregated * std::mem::size_of::<CompactionDomain>())
        + canonical_set_bytes;
    aliases
        .take_handles()
        .pin(crate::retention::owned::ChargeCtx::new(pin_names_to));
    bytes
}

/// The owned names `facts` reach, each once: charged by their own cells,
/// pinned with the evidence holding them and held with its retained share.
fn evidence_names(facts: &[FactVersionRef]) -> crate::retention::owned::OwnedHandles {
    use crate::retention::allocation::{facts_heap_bytes, AliasSet};
    let mut aliases = AliasSet::new();
    // A raw leaf is counted in the evidence's own charge.
    let _raw = facts_heap_bytes(facts, &mut aliases);
    aliases.take_handles()
}

/// A receipt for a completed result: its evidence, shared.
///
/// Equality, order and hash read the evidence's digest first; two
/// receipts with equal digests compare their facts, so a digest collision
/// never makes two different evidences equal.
#[derive(Clone)]
pub struct ResultReceipt(Arc<ResultEvidence>);

impl ResultReceipt {
    /// Already-canonical producer evidence, charged for the lifetime of this
    /// shared allocation. Admission claims its pin together with the payload.
    #[must_use]
    pub fn retained_signature(
        facts: super::fact_set::FactsRef<'_>,
        account: &Arc<SemanticRetentionAccount>,
    ) -> Self {
        Self::seal(facts.iter().cloned().collect(), account)
    }

    /// Join complete dependency receipts without expanding their evidence or
    /// comparison-sorting a transitive read set. Digest radix order preserves
    /// receipt Ord; equal digests still compare exact evidence.
    #[must_use]
    pub fn retained_join(receipts: Vec<Self>, account: &Arc<SemanticRetentionAccount>) -> Self {
        let mut keyed: Vec<_> = receipts
            .into_iter()
            .map(|r| (r.digest().to_be_bytes(), r))
            .collect();
        let work = crate::declarations::module_exports::sort_bytes(&mut keyed, |(key, _)| key);
        super::fact_read_set::record_signature_work(work);
        let mut start = 0;
        while start < keyed.len() {
            let mut end = start + 1;
            while end < keyed.len() && keyed[end].0 == keyed[start].0 {
                end += 1;
            }
            keyed[start..end].sort_unstable_by(|a, b| {
                super::fact_read_set::record_signature_work(1);
                a.1.cmp(&b.1)
            });
            start = end;
        }
        let mut facts: Vec<_> = keyed
            .into_iter()
            .map(|(_, r)| FactVersionRef::Receipt(r))
            .collect();
        facts.dedup();
        drop_subsumed_receipts(&mut facts);
        Self::seal(facts, account)
    }

    /// The receipt for `facts`, a completed result's own facts and the
    /// receipts of what it consumed. Sorted and deduplicated here.
    ///
    /// Its storage is pinned against the process retention account from
    /// birth and claimed into the first retaining admission's
    /// reservation, so every receipt is charged for exactly as long as it
    /// lives, however many signatures share it.
    #[must_use]
    pub fn new(facts: Vec<FactVersionRef>) -> Self {
        Self::new_on(facts, &SemanticRetentionAccount::process_local())
    }

    /// [`Self::new`], pinned against `account`.
    #[must_use]
    pub fn new_on(mut facts: Vec<FactVersionRef>, account: &Arc<SemanticRetentionAccount>) -> Self {
        facts.sort_unstable_by(|a, b| {
            super::fact_read_set::record_signature_work(1);
            a.cmp(b)
        });
        facts.dedup();
        drop_subsumed_receipts(&mut facts);
        Self::seal(facts, account)
    }

    fn seal(facts: Vec<FactVersionRef>, account: &Arc<SemanticRetentionAccount>) -> Self {
        let mut digester = xxhash_rust::xxh3::Xxh3::new();
        facts.len().hash(&mut digester);
        for fact in &facts {
            fact.hash(&mut digester);
        }
        let digest = digester.digest128();
        let mut consumed: Vec<&CanonicalSet> = Vec::new();
        let mut own: Vec<&str> = Vec::new();
        let mut aggregated: Vec<CompactionDomain> = Vec::new();
        let mut resolution_evidence = false;
        for fact in &facts {
            super::fact_read_set::record_signature_work(1);
            match fact {
                FactVersionRef::Receipt(child) => {
                    consumed.push(&child.0.canonicals);
                    for domain in child.0.aggregated.iter() {
                        if !aggregated.contains(domain) {
                            aggregated.push(*domain);
                        }
                    }
                    resolution_evidence |= child.0.resolution_evidence;
                }
                other => {
                    match other.attribution() {
                        FactAttribution::Canonical(canonical) => own.push(canonical),
                        FactAttribution::DomainAggregate(domain) => {
                            if !aggregated.contains(&domain) {
                                aggregated.push(domain);
                            }
                        }
                        FactAttribution::ProjectScalar
                        | FactAttribution::StrictSelfRootWorld
                        | FactAttribution::ResultReceipt => {}
                    }
                    resolution_evidence |= matches!(
                        other,
                        FactVersionRef::ResolveImports(
                            super::version::ResolveImportsFactRef::Resolution(_)
                        )
                    ) || matches!(
                        other.attribution(),
                        FactAttribution::DomainAggregate(CompactionDomain::Resolution)
                    );
                }
            }
        }
        let (canonicals, canonical_set_bytes) = CanonicalSet::union_of(&consumed, &own);
        let retention = parking_lot::Mutex::new(account.pin(evidence_storage_bytes(
            &facts,
            aggregated.len(),
            canonical_set_bytes,
            account,
        )));
        Self(Arc::new(ResultEvidence {
            facts: facts.into(),
            digest,
            canonicals,
            aggregated: aggregated.into(),
            resolution_evidence,
            evidence_claimed: std::sync::atomic::AtomicBool::new(false),
            retention,
        }))
    }

    /// Bytes this evidence holds charged against the retention account,
    /// shared by every holder.
    #[must_use]
    pub fn retained_charge_bytes(&self) -> usize {
        self.0.retention.lock().bytes()
    }

    /// The class of the evidence's charge: [`ChargeClass::Pinned`] until a
    /// cache admission claims it, [`ChargeClass::Retained`] after. Always
    /// `Some`: every evidence carries its charge.
    #[must_use]
    pub fn retained_charge_class(&self) -> Option<ChargeClass> {
        Some(self.0.retention.lock().class())
    }

    /// The result's own facts and the receipts of what it consumed.
    #[must_use]
    pub fn facts(&self) -> &[FactVersionRef] {
        &self.0.facts
    }

    /// The evidence's digest.
    #[must_use]
    pub fn digest(&self) -> u128 {
        self.0.digest
    }

    /// Every canonical a fact reachable from this receipt names.
    #[must_use]
    pub fn canonicals(&self) -> &CanonicalSet {
        &self.0.canonicals
    }

    /// Whether a fact reachable from this receipt names `canonical`.
    #[must_use]
    pub fn references_canonical(&self, canonical: &str) -> bool {
        self.0.canonicals.contains(canonical)
    }

    /// Every compaction domain a reachable fact carries as its terminal
    /// aggregate.
    #[must_use]
    pub fn aggregated_domains(&self) -> &[CompactionDomain] {
        &self.0.aggregated
    }

    /// Whether a reachable fact is resolution evidence.
    #[must_use]
    pub fn carries_resolution_evidence(&self) -> bool {
        self.0.resolution_evidence
    }

    /// Whether two receipts share one evidence.
    #[must_use]
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Visit every fact reachable from this receipt that is not itself a
    /// receipt, each distinct receipt walked once, from an explicit stack.
    /// Stops at the first fact `visit` answers `false` for, and answers
    /// `false`; `true` when every fact was visited.
    pub fn all_leaves(&self, visit: impl FnMut(&FactVersionRef) -> bool) -> bool {
        ReceiptWalk::default().leaves(self, visit)
    }
}

/// One walk over several receipts' evidence — every receipt of one
/// signature, say — visiting each distinct evidence once across all of
/// them, so validating a signature whose receipts share a dependency graph
/// costs that graph once, never once per receipt.
#[derive(Default)]
pub struct ReceiptWalk {
    walked: rustc_hash::FxHashSet<*const ResultEvidence>,
}

impl ReceiptWalk {
    /// Visit every fact reachable from `receipt` that is not itself a
    /// receipt and that no earlier call of this walk reached. Stops at the
    /// first fact `visit` answers `false` for, and answers `false`.
    ///
    /// The walk remembers evidence by address, so it must not outlive the
    /// receipts it was given (every caller walks one borrowed signature).
    pub fn leaves(
        &mut self,
        receipt: &ResultReceipt,
        mut visit: impl FnMut(&FactVersionRef) -> bool,
    ) -> bool {
        self.leaves_with_verdicts(receipt, |_| ReceiptVerdict::Unknown, |fact| visit(fact))
            .is_ok()
    }

    /// [`Self::leaves`], with what the caller already knows about each
    /// receipt the walk meets: a receipt `known` answers
    /// [`ReceiptVerdict::Valid`] for is taken as validated, so neither its
    /// facts nor anything only it reaches are visited; one it answers
    /// [`ReceiptVerdict::Stale`] for fails the walk without being read.
    /// `known` is asked once per distinct receipt the walk meets, so a
    /// caller can also learn every receipt it walked.
    ///
    /// On failure answers the receipts PROVEN to reach the failure,
    /// innermost first: the receipt whose own facts hold the failing fact
    /// or the known-stale receipt, then the receipt the walk first met that
    /// one through, and so on up to `receipt`. A known-stale receipt itself
    /// is not in the chain (its verdict is already known), and neither is
    /// anything else the walk met: a receipt it had not finished, or never
    /// reached, is proven neither way.
    pub fn leaves_with_verdicts<'r>(
        &mut self,
        receipt: &'r ResultReceipt,
        mut known: impl FnMut(&ResultReceipt) -> ReceiptVerdict,
        mut visit: impl FnMut(&FactVersionRef) -> bool,
    ) -> Result<(), Vec<&'r ResultReceipt>> {
        if !self.walked.insert(Arc::as_ptr(&receipt.0)) {
            return Ok(());
        }
        match known(receipt) {
            ReceiptVerdict::Valid => return Ok(()),
            ReceiptVerdict::Stale => return Err(Vec::new()),
            ReceiptVerdict::Unknown => {}
        }
        // Each receipt to read, in the order the walk met them, with the
        // index of the one it was first met through, so a failure names
        // its proven ancestors. Read front to back: the list is its own
        // queue.
        let mut met: Vec<(&'r ResultReceipt, usize)> = vec![(receipt, usize::MAX)];
        let mut next = 0;
        let chain = |met: &[(&'r ResultReceipt, usize)], mut at: usize| {
            let mut chain = Vec::new();
            while at != usize::MAX {
                let (receipt, parent) = met[at];
                chain.push(receipt);
                at = parent;
            }
            chain
        };
        while let Some(&(current, _)) = met.get(next) {
            let at = next;
            next += 1;
            for fact in current.0.facts.iter() {
                super::fact_read_set::record_signature_work(1);
                match fact {
                    FactVersionRef::Receipt(child) => {
                        if !self.walked.insert(Arc::as_ptr(&child.0)) {
                            continue;
                        }
                        match known(child) {
                            ReceiptVerdict::Valid => {}
                            ReceiptVerdict::Stale => return Err(chain(&met, at)),
                            ReceiptVerdict::Unknown => met.push((child, at)),
                        }
                    }
                    leaf => {
                        if !visit(leaf) {
                            return Err(chain(&met, at));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// What a walk's caller already knows about one receipt
/// ([`ReceiptWalk::leaves_with_verdicts`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptVerdict {
    /// Not known: the walk reads it.
    Unknown,
    /// Every fact it reaches validates.
    Valid,
    /// A fact it reaches does not validate.
    Stale,
}

/// Reserve a cache candidate's retained bytes against `account`: its own
/// `own_bytes` together with every receipt and sealed-set tree node
/// `signatures` reach — receipts a consumed result's receipt holds included —
/// that no earlier admission claimed, as ONE refusable
/// [`ChargeClass::Retained`] reservation — so a wide candidate is refused,
/// as oversized or under pressure, for everything it would newly retain.
///
/// On admission each claimed receipt's and node's pin is exchanged for its
/// share of the reservation, which it then holds for the rest of its life;
/// the returned charge holds `own_bytes`. A receipt or node another
/// admission claimed first, concurrently, keeps that claim and its share is
/// released here. The reservation is taken before any pin is released, so
/// nothing is uncharged during the exchange. The owned names a claimed
/// receipt's or node's facts reach are held with its share, so they are
/// retained as long as it is. On refusal nothing changes: every receipt,
/// node and name stays pinned by the live signatures holding it.
pub fn reserve_retained_with_evidence<S: SignatureFacts + ?Sized>(
    account: &Arc<SemanticRetentionAccount>,
    own_bytes: usize,
    signatures: &[&S],
) -> RetentionAdmission {
    match reserve_retained_with_evidence_then(account, own_bytes, signatures, Ok) {
        Ok(charge) => RetentionAdmission::Admitted(charge),
        Err(refusal) => RetentionAdmission::Refused(refusal),
    }
}

/// [`reserve_retained_with_evidence`] whose admission also needs the
/// candidate's own charge finished (its owned names held, say) by `finish`.
/// The claims are committed only once `finish` succeeds: a refusal from
/// `finish` releases the whole reservation and leaves every receipt and
/// tree node pinned and unclaimed, exactly as it found them, so a later
/// admission is still charged and refused for them.
pub fn reserve_retained_with_evidence_then<S: SignatureFacts + ?Sized, T>(
    account: &Arc<SemanticRetentionAccount>,
    own_bytes: usize,
    signatures: &[&S],
    finish: impl FnOnce(RetentionCharge) -> Result<T, RetentionRefusal>,
) -> Result<T, RetentionRefusal> {
    use std::sync::atomic::Ordering;
    let mut seen: rustc_hash::FxHashSet<*const ResultEvidence> = rustc_hash::FxHashSet::default();
    let mut walked: Vec<&ResultEvidence> = Vec::new();
    let mut claim: Vec<(&ResultEvidence, usize)> = Vec::new();
    // A tree set's nodes: every node it reaches that no earlier admission
    // claimed, each once (a claimed node's subtree is claimed).
    let mut nodes: Vec<&Arc<super::fact_tree::Node>> = Vec::new();
    let mut seen_nodes: rustc_hash::FxHashSet<*const super::fact_tree::Node> =
        rustc_hash::FxHashSet::default();
    let mut stack: Vec<&[FactVersionRef]> = Vec::new();
    for signature in signatures {
        if let Some(root) = signature.fact_set().and_then(|set| set.tree_root()) {
            super::fact_tree::unclaimed_nodes(root, &mut nodes, &mut seen_nodes);
        }
        signature.visit_receipt_candidates(&mut |fact| {
            let FactVersionRef::Receipt(receipt) = fact else {
                return;
            };
            if receipt.0.evidence_claimed.load(Ordering::Acquire)
                || !seen.insert(Arc::as_ptr(&receipt.0))
            {
                return;
            }
            walked.push(&receipt.0);
            let charge = receipt.0.retention.lock();
            if charge.class() != ChargeClass::Retained {
                claim.push((&receipt.0, charge.bytes()));
            }
            stack.push(&receipt.0.facts);
        });
    }
    let node_claims: Vec<(&Arc<super::fact_tree::Node>, usize)> = nodes
        .iter()
        .filter_map(|node| {
            let charge = node.charge().lock();
            charge
                .as_ref()
                .filter(|charge| charge.class() != ChargeClass::Retained)
                .map(|charge| (*node, charge.bytes()))
        })
        .collect();
    let node_bytes: usize = node_claims.iter().map(|(_, bytes)| bytes).sum();
    // Every distinct receipt reachable from a walked receipt is walked once:
    // a consumed result's receipt carries its own charge, claimed here like
    // any other receipt no earlier admission claimed, and may hold further
    // receipts (a result recorded over a warm-hit signature that consumed
    // results), which the candidate retains through it just as it retains
    // its own.
    while let Some(level) = stack.pop() {
        for fact in level {
            let FactVersionRef::Receipt(receipt) = fact else {
                continue;
            };
            if receipt.0.evidence_claimed.load(Ordering::Acquire)
                || !seen.insert(Arc::as_ptr(&receipt.0))
            {
                continue;
            }
            walked.push(&receipt.0);
            let charge = receipt.0.retention.lock();
            if charge.class() != ChargeClass::Retained {
                claim.push((&receipt.0, charge.bytes()));
            }
            stack.push(&receipt.0.facts);
        }
    }
    let evidence_bytes: usize = claim.iter().map(|(_, bytes)| bytes).sum();
    let mut charge = match account.reserve(
        ChargeClass::Retained,
        own_bytes + evidence_bytes + node_bytes,
    ) {
        RetentionAdmission::Admitted(charge) => charge,
        RetentionAdmission::Refused(refusal) => return Err(refusal),
    };
    let ctx = crate::retention::owned::ChargeCtx::new(account);
    let mut names = Vec::with_capacity(claim.len());
    for (evidence, _) in &claim {
        names.push(evidence_names(&evidence.facts).hold(ctx)?);
    }
    let mut node_names = Vec::with_capacity(node_claims.len());
    for (node, _) in &node_claims {
        node_names.push(super::fact_tree::node_names(node).hold(ctx)?);
    }
    // Every share is split off before anything is committed, so the
    // caller's own hold can still refuse with nothing claimed: dropping the
    // shares releases the reservation whole.
    let evidence_shares: Vec<_> = claim
        .into_iter()
        .zip(names)
        .map(|((evidence, bytes), names)| (evidence, charge.split_off(bytes).holding(names)))
        .collect();
    let node_shares: Vec<_> = node_claims
        .into_iter()
        .zip(node_names)
        .map(|((node, bytes), names)| (node, charge.split_off(bytes).holding(names)))
        .collect();
    let finished = finish(charge)?;
    for (evidence, share) in evidence_shares {
        let cell = &evidence.retention;
        let mut held = cell.lock();
        let released = if held.class() == ChargeClass::Retained {
            share
        } else {
            std::mem::replace(&mut *held, share)
        };
        drop(held);
        drop(released);
    }
    // Every receipt reachable from a walked receipt is retained now, by
    // this admission or by the one that claimed it first.
    for evidence in walked {
        evidence.evidence_claimed.store(true, Ordering::Release);
    }
    // Each unclaimed tree node exchanges its pin for its share, exactly as
    // an evidence does; a node another admission claimed meanwhile keeps
    // that claim and its share is released here.
    for (node, share) in node_shares {
        let mut held = node.charge().lock();
        let released = match held.as_mut() {
            Some(pinned) if pinned.class() != ChargeClass::Retained => {
                std::mem::replace(pinned, share)
            }
            _ => share,
        };
        drop(held);
        drop(released);
    }
    // Deepest first, so a node marked claimed always has its subtree
    // claimed: the walk discovered every node after its parent.
    for node in nodes.into_iter().rev() {
        node.mark_claimed();
    }
    Ok(finished)
}

/// A signature a retaining admission reserves for: its facts (a consumed
/// result's receipt, and every receipt it holds, claimed with it) and, for a
/// wide sealed set, its tree nodes' charges.
pub trait SignatureFacts {
    /// Visit the facts that may be receipts: for a canonical set, only its
    /// tail (receipts sort after every other fact); for a plain list, all of
    /// it.
    fn visit_receipt_candidates<'a>(&'a self, visit: &mut dyn FnMut(&'a FactVersionRef));
    /// The sealed set, when the signature is one: its tree nodes are
    /// claimed with the reservation.
    fn fact_set(&self) -> Option<&super::fact_set::FactSet> {
        None
    }
}

impl SignatureFacts for [FactVersionRef] {
    fn visit_receipt_candidates<'a>(&'a self, visit: &mut dyn FnMut(&'a FactVersionRef)) {
        self.iter().for_each(visit);
    }
}

impl SignatureFacts for Vec<FactVersionRef> {
    fn visit_receipt_candidates<'a>(&'a self, visit: &mut dyn FnMut(&'a FactVersionRef)) {
        self.iter().for_each(visit);
    }
}

impl SignatureFacts for Arc<[FactVersionRef]> {
    fn visit_receipt_candidates<'a>(&'a self, visit: &mut dyn FnMut(&'a FactVersionRef)) {
        self.iter().for_each(visit);
    }
}

impl SignatureFacts for super::fact_set::FactSet {
    fn visit_receipt_candidates<'a>(&'a self, visit: &mut dyn FnMut(&'a FactVersionRef)) {
        self.iter()
            .rev()
            .take_while(|fact| {
                matches!(
                    fact,
                    FactVersionRef::Receipt(_)
                        | FactVersionRef::DomainGeneration(_)
                        | FactVersionRef::StrictSelfRootWorld(_)
                )
            })
            .for_each(visit);
    }

    fn fact_set(&self) -> Option<&super::fact_set::FactSet> {
        Some(self)
    }
}

impl SignatureFacts for super::fact_set::FactsRef<'_> {
    fn visit_receipt_candidates<'a>(&'a self, visit: &mut dyn FnMut(&'a FactVersionRef)) {
        match self {
            Self::Set(set) => set.visit_receipt_candidates(visit),
            Self::Slice(facts) => facts.visit_receipt_candidates(visit),
        }
    }

    fn fact_set(&self) -> Option<&super::fact_set::FactSet> {
        match self {
            Self::Set(set) => Some(set),
            Self::Slice(_) => None,
        }
    }
}

impl SignatureFacts for super::fact_cache::ReadSetSignature {
    fn visit_receipt_candidates<'a>(&'a self, visit: &mut dyn FnMut(&'a FactVersionRef)) {
        self.facts.visit_receipt_candidates(visit);
    }

    fn fact_set(&self) -> Option<&super::fact_set::FactSet> {
        Some(&self.facts)
    }
}

/// Claim every receipt and tree node `signature` reaches that no earlier
/// admission claimed, for a cache admission that retains the signature but
/// charges none of its own bytes against `account`: the reservation
/// [`reserve_retained_with_evidence`] takes with no own bytes, so the
/// claimed storage is refused, as oversized or under pressure, exactly as a
/// participant's is.
///
/// A signature reaching no unclaimed storage reserves nothing and is always
/// admitted. On refusal the caller must not retain the signature: it
/// delivers its value uncached, and every receipt and node stays pinned by
/// the live signatures holding it until their last holder drops.
pub fn claim_retained_storage<S: SignatureFacts + ?Sized>(
    account: &Arc<SemanticRetentionAccount>,
    signature: &S,
) -> Result<(), RetentionRefusal> {
    if !has_unclaimed_retained_storage(signature) {
        return Ok(());
    }
    match reserve_retained_with_evidence(account, 0, &[signature]) {
        RetentionAdmission::Admitted(_) => Ok(()),
        RetentionAdmission::Refused(refusal) => Err(refusal),
    }
}

/// Whether retaining this signature needs a reservation for receipts or
/// sealed-set tree nodes no earlier admission claimed.
#[must_use]
pub fn has_unclaimed_retained_storage<S: SignatureFacts + ?Sized>(signature: &S) -> bool {
    if signature
        .fact_set()
        .is_some_and(super::fact_set::FactSet::has_unclaimed_storage)
    {
        return true;
    }
    let mut unclaimed = false;
    signature.visit_receipt_candidates(&mut |fact| {
        unclaimed |= matches!(fact, FactVersionRef::Receipt(receipt)
            if !receipt.0.evidence_claimed.load(std::sync::atomic::Ordering::Acquire));
    });
    unclaimed
}

/// Drop from `facts` every receipt another receipt in `facts` directly
/// consumed: the consumer's evidence already holds it, so keeping both only
/// lengthens the set. A chain's scopes collect one receipt per completed
/// level, each consuming the one below; this keeps the top one. Only
/// direct consumption is read, so the cost is the receipts' own entries.
pub fn drop_subsumed_receipts(facts: &mut Vec<FactVersionRef>) {
    let mut receipts = facts
        .iter()
        .filter(|fact| matches!(fact, FactVersionRef::Receipt(_)));
    if receipts.next().is_none() || receipts.next().is_none() {
        return;
    }
    let mut consumed = ConsumedReceipts::default();
    for fact in facts.iter() {
        if let FactVersionRef::Receipt(receipt) = fact {
            for inner in receipt.0.facts.iter() {
                if let FactVersionRef::Receipt(child) = inner {
                    consumed.insert(Arc::as_ptr(&child.0));
                }
            }
        }
    }
    if consumed.is_empty() {
        return;
    }
    facts.retain(|fact| match fact {
        FactVersionRef::Receipt(receipt) => !consumed.contains(Arc::as_ptr(&receipt.0)),
        _ => true,
    });
}

/// The receipts a set's receipts consumed: held inline, compared linearly,
/// while few — a set usually holds a handful of receipts — and hashed once
/// many, so dropping them allocates nothing in the common case.
#[derive(Default)]
struct ConsumedReceipts {
    few: smallvec::SmallVec<[*const ResultEvidence; 8]>,
    many: Option<rustc_hash::FxHashSet<*const ResultEvidence>>,
}

impl ConsumedReceipts {
    const LINEAR: usize = 16;

    fn insert(&mut self, evidence: *const ResultEvidence) {
        if let Some(many) = &mut self.many {
            many.insert(evidence);
            return;
        }
        if self.few.contains(&evidence) {
            return;
        }
        self.few.push(evidence);
        if self.few.len() > Self::LINEAR {
            self.many = Some(self.few.drain(..).collect());
        }
    }

    fn contains(&self, evidence: *const ResultEvidence) -> bool {
        match &self.many {
            Some(many) => many.contains(&evidence),
            None => self.few.contains(&evidence),
        }
    }

    fn is_empty(&self) -> bool {
        self.few.is_empty() && self.many.is_none()
    }
}

impl std::fmt::Debug for ResultReceipt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResultReceipt")
            .field("digest", &format_args!("{:032x}", self.0.digest))
            .field("facts", &self.0.facts.len())
            .finish()
    }
}

impl PartialEq for ResultReceipt {
    fn eq(&self, other: &Self) -> bool {
        self.ptr_eq(other)
            || (self.0.digest == other.0.digest
                && compare_evidence(&self.0, &other.0) == std::cmp::Ordering::Equal)
    }
}

impl Eq for ResultReceipt {}

impl PartialOrd for ResultReceipt {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ResultReceipt {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        if self.ptr_eq(other) {
            return std::cmp::Ordering::Equal;
        }
        self.0
            .digest
            .cmp(&other.0.digest)
            .then_with(|| compare_evidence(&self.0, &other.0))
    }
}

/// Order two evidences by their facts, as a slice comparison would, but
/// reading the receipts they consumed from an explicit stack: two equal
/// evidence graphs assembled apart (a result recomputed after its receipt
/// was released, say) compare in heap memory, never one native frame per
/// level. A pair of receipts found equal once is not compared again, so a
/// graph that shares its children costs its distinct pairs.
fn compare_evidence(a: &ResultEvidence, b: &ResultEvidence) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let mut stack: Vec<(&[FactVersionRef], &[FactVersionRef], usize)> =
        vec![(&a.facts, &b.facts, 0)];
    let mut compared: rustc_hash::FxHashSet<(*const ResultEvidence, *const ResultEvidence)> =
        rustc_hash::FxHashSet::default();
    while let Some(top) = stack.last_mut() {
        let (left, right, at) = *top;
        let (Some(x), Some(y)) = (left.get(at), right.get(at)) else {
            match left.len().cmp(&right.len()) {
                Ordering::Equal => {
                    stack.pop();
                    continue;
                }
                unequal => return unequal,
            }
        };
        top.2 += 1;
        match (x, y) {
            (FactVersionRef::Receipt(x), FactVersionRef::Receipt(y)) => {
                if x.ptr_eq(y) {
                    continue;
                }
                match x.0.digest.cmp(&y.0.digest) {
                    Ordering::Equal => {}
                    unequal => return unequal,
                }
                // A pair already on the stack or found equal: an evidence
                // graph is acyclic, so it was found equal.
                if compared.insert((Arc::as_ptr(&x.0), Arc::as_ptr(&y.0))) {
                    stack.push((&x.0.facts, &y.0.facts, 0));
                }
            }
            (x, y) => match x.cmp(y) {
                Ordering::Equal => {}
                unequal => return unequal,
            },
        }
    }
    Ordering::Equal
}

impl Hash for ResultReceipt {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.digest.hash(state);
    }
}

#[cfg(test)]
#[path = "receipt_tests.rs"]
mod tests;
