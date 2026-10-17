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
//! The same carrier holds an EVIDENCE PAGE ([`ResultReceipt::page`]): one
//! fixed-width slice of a signature too wide for one level. A page is not a
//! consumed result — it names no computation and carries no cost identity —
//! but it validates, projects and is shared exactly as a receipt is, so a
//! wide signature stays complete without any consumer learning a second
//! evidence shape.
//!
//! A page's storage is charged to the retention account for exactly as long
//! as the page lives, once, however many signatures share it. It is born a
//! [`ChargeClass::Pinned`] obligation of the live signature that sealed it.
//! The first cache admission that retains a signature holding the page
//! claims it ([`reserve_retained_with_evidence`], or [`claim_evidence_pages`]
//! for a store that charges no bytes of its own): the page's bytes join that
//! admission's refusable [`ChargeClass::Retained`] reservation — so a wide
//! candidate is refused for its whole footprint like any other entry — and
//! the page's pin is exchanged for its share of the granted reservation.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use super::version::{CompactionDomain, FactAttribution, FactVersionRef};
use crate::retention::{
    ChargeClass, RetentionAdmission, RetentionCharge, RetentionRefusal, SemanticRetentionAccount,
};

/// What one shared evidence holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EvidenceKind {
    /// A completed result's own facts and the receipts of what it consumed.
    Result,
    /// One fixed-width page of a signature wider than one level (see
    /// [`crate::facts::fact_read_set::FACT_PAGE_WIDTH`]): a slice of that
    /// signature's canonical entries, never a computation's evidence.
    Page,
}

/// The evidence one completed result recorded: its own facts and the
/// receipts of the results it consumed, in canonical order, with the
/// summaries a consumer projecting a signature needs without walking it.
#[derive(Debug)]
pub struct ResultEvidence {
    facts: Arc<[FactVersionRef]>,
    kind: EvidenceKind,
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
    use crate::retention::allocation::{arc_block_bytes, fact_heap_bytes, AliasSet};
    let mut aliases = AliasSet::new();
    let bytes = arc_block_bytes(std::mem::size_of::<ResultEvidence>())
        + arc_block_bytes(std::mem::size_of_val(facts))
        + facts
            .iter()
            .map(|fact| fact_heap_bytes(fact, &mut aliases))
            .sum::<usize>()
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
    use crate::retention::allocation::{fact_heap_bytes, AliasSet};
    let mut aliases = AliasSet::new();
    for fact in facts {
        fact_heap_bytes(fact, &mut aliases);
    }
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
        facts: &[FactVersionRef],
        account: &Arc<SemanticRetentionAccount>,
    ) -> Self {
        Self::seal(facts.to_vec(), EvidenceKind::Result, account)
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
        Self::seal(facts, EvidenceKind::Result, account)
    }

    /// The receipt for `facts`, a completed result's own facts and the
    /// receipts of what it consumed. Sorted and deduplicated here.
    ///
    /// Like a page, its storage is pinned against the process retention
    /// account from birth and claimed into the first retaining admission's
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
        Self::seal(facts, EvidenceKind::Result, account)
    }

    /// One page of a wide signature: `facts`, a contiguous run of that
    /// signature's canonical (strictly increasing) entries, kept whole —
    /// nothing is re-sorted, deduplicated or dropped.
    ///
    /// The page's storage is pinned against the process retention account
    /// from birth: a page exists only because a live signature holds it.
    /// A cache admission retaining it later exchanges the pin for a share
    /// of its refusable reservation ([`reserve_retained_with_evidence`]);
    /// either way the charge is released by the page's last holder's drop,
    /// once, however many candidates, refusal summaries or enclosing
    /// signatures share it.
    #[must_use]
    pub fn page(facts: Vec<FactVersionRef>) -> Self {
        Self::page_on(facts, &SemanticRetentionAccount::process_local())
    }

    /// [`Self::page`], pinned against `account`.
    pub(crate) fn page_on(
        facts: Vec<FactVersionRef>,
        account: &Arc<SemanticRetentionAccount>,
    ) -> Self {
        Self::seal(facts, EvidenceKind::Page, account)
    }

    fn seal(
        facts: Vec<FactVersionRef>,
        kind: EvidenceKind,
        account: &Arc<SemanticRetentionAccount>,
    ) -> Self {
        let mut digester = xxhash_rust::xxh3::Xxh3::new();
        kind.hash(&mut digester);
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
            kind,
            digest,
            canonicals,
            aggregated: aggregated.into(),
            resolution_evidence,
            evidence_claimed: std::sync::atomic::AtomicBool::new(false),
            retention,
        }))
    }

    /// What this evidence holds: a result's, or one page of a wide
    /// signature.
    #[must_use]
    pub fn kind(&self) -> EvidenceKind {
        self.0.kind
    }

    /// Whether this is one page of a wide signature.
    #[must_use]
    pub fn is_page(&self) -> bool {
        self.0.kind == EvidenceKind::Page
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
        visit: impl FnMut(&FactVersionRef) -> bool,
    ) -> bool {
        self.leaves_unless(receipt, |_| false, visit)
    }

    /// [`Self::leaves`], except that a receipt `settled` answers `true` for
    /// is taken as already validated: neither its facts nor anything only
    /// it reaches are visited. `settled` is asked once per distinct receipt
    /// the walk meets, so a caller can also learn every receipt it walked.
    pub fn leaves_unless(
        &mut self,
        receipt: &ResultReceipt,
        mut settled: impl FnMut(&ResultReceipt) -> bool,
        mut visit: impl FnMut(&FactVersionRef) -> bool,
    ) -> bool {
        if !self.walked.insert(Arc::as_ptr(&receipt.0)) || settled(receipt) {
            return true;
        }
        let mut stack: Vec<&ResultEvidence> = vec![&receipt.0];
        while let Some(evidence) = stack.pop() {
            for fact in evidence.facts.iter() {
                super::fact_read_set::record_signature_work(1);
                match fact {
                    FactVersionRef::Receipt(child) => {
                        if self.walked.insert(Arc::as_ptr(&child.0)) && !settled(child) {
                            stack.push(&child.0);
                        }
                    }
                    leaf => {
                        if !visit(leaf) {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }
}

/// Reserve a cache candidate's retained bytes against `account`: its own
/// `own_bytes` together with every evidence page `signatures` reach — pages
/// of pages, and pages a consumed result's receipt holds, included — that no
/// earlier admission claimed, as ONE refusable
/// [`ChargeClass::Retained`] reservation — so a wide candidate is refused,
/// as oversized or under pressure, for everything it would newly retain.
///
/// On admission each claimed page's pin is exchanged for its share of the
/// reservation, which the page then holds for the rest of its life; the
/// returned charge holds `own_bytes`. A page another admission claimed
/// first, concurrently, keeps that claim and its share is released here.
/// The reservation is taken before any pin is released, so a page is never
/// uncharged during the exchange. The owned names a claimed evidence's
/// facts reach are held with its share, so they are retained as long as it
/// is. On refusal nothing changes: every page and name stays pinned by the
/// live signatures holding it.
pub fn reserve_retained_with_evidence(
    account: &Arc<SemanticRetentionAccount>,
    own_bytes: usize,
    signatures: &[&[FactVersionRef]],
) -> RetentionAdmission {
    use std::sync::atomic::Ordering;
    let mut seen: rustc_hash::FxHashSet<*const ResultEvidence> = rustc_hash::FxHashSet::default();
    let mut walked: Vec<&ResultEvidence> = Vec::new();
    let mut claim: Vec<(&ResultEvidence, usize)> = Vec::new();
    let mut stack: Vec<&[FactVersionRef]> = signatures.to_vec();
    // Every distinct evidence that can reach a page is walked once, whatever
    // its kind: a consumed result's receipt carries its own charge, claimed
    // here like any other evidence no earlier admission claimed, and may hold
    // pages (a result recorded over a wide warm-hit signature), which the
    // candidate retains through it just as it retains its own.
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
    let mut charge = match account.reserve(ChargeClass::Retained, own_bytes + evidence_bytes) {
        RetentionAdmission::Admitted(charge) => charge,
        refused @ RetentionAdmission::Refused(_) => return refused,
    };
    let ctx = crate::retention::owned::ChargeCtx::new(account);
    let mut names = Vec::with_capacity(claim.len());
    for (evidence, _) in &claim {
        match evidence_names(&evidence.facts).hold(ctx) {
            Ok(holds) => names.push(holds),
            Err(refusal) => return RetentionAdmission::Refused(refusal),
        }
    }
    for ((evidence, bytes), names) in claim.into_iter().zip(names) {
        let share = charge.split_off(bytes).holding(names);
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
    // Every page reachable from a walked evidence is retained now, by this
    // admission or by the one that claimed it first.
    for evidence in walked {
        evidence.evidence_claimed.store(true, Ordering::Release);
    }
    RetentionAdmission::Admitted(charge)
}

/// Claim every evidence page `signature` reaches that no earlier admission
/// claimed, for a cache admission that retains the signature but charges
/// none of its own bytes against `account`: the reservation
/// [`reserve_retained_with_evidence`] takes with no own bytes, so the
/// claimed pages are refused, as oversized or under pressure, exactly as a
/// participant's are.
///
/// A signature reaching no unclaimed page reserves nothing and is always
/// admitted. On refusal the caller must not retain the signature: it
/// delivers its value uncached, and every page stays pinned by the live
/// signatures holding it until their last holder drops.
pub fn claim_evidence_pages(
    account: &Arc<SemanticRetentionAccount>,
    signature: &[FactVersionRef],
) -> Result<(), RetentionRefusal> {
    if !has_unclaimed_evidence_pages(signature) {
        return Ok(());
    }
    match reserve_retained_with_evidence(account, 0, &[signature]) {
        RetentionAdmission::Admitted(_) => Ok(()),
        RetentionAdmission::Refused(refusal) => Err(refusal),
    }
}

/// Whether retaining this signature needs an evidence-page reservation.
#[must_use]
pub fn has_unclaimed_evidence_pages(signature: &[FactVersionRef]) -> bool {
    signature.iter().any(|fact| {
        matches!(fact, FactVersionRef::Receipt(receipt)
            if !receipt.0.evidence_claimed.load(std::sync::atomic::Ordering::Acquire))
    })
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
    let mut consumed: rustc_hash::FxHashSet<*const ResultEvidence> =
        rustc_hash::FxHashSet::default();
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
        FactVersionRef::Receipt(receipt) => !consumed.contains(&Arc::as_ptr(&receipt.0)),
        _ => true,
    });
}

impl std::fmt::Debug for ResultReceipt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResultReceipt")
            .field("kind", &self.0.kind)
            .field("digest", &format_args!("{:032x}", self.0.digest))
            .field("facts", &self.0.facts.len())
            .finish()
    }
}

impl PartialEq for ResultReceipt {
    fn eq(&self, other: &Self) -> bool {
        self.ptr_eq(other)
            || (self.0.digest == other.0.digest
                && self.0.kind == other.0.kind
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
            .then_with(|| self.0.kind.cmp(&other.0.kind))
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
                match x.0.digest.cmp(&y.0.digest).then(x.0.kind.cmp(&y.0.kind)) {
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
