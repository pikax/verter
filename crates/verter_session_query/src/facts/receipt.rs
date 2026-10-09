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
//! claims it ([`reserve_retained_with_evidence`]): the page's bytes join that
//! admission's refusable [`ChargeClass::Retained`] reservation — so a wide
//! candidate is refused for its whole footprint like any other entry — and
//! the page's pin is exchanged for its share of the granted reservation.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use super::version::{CompactionDomain, FactAttribution, FactVersionRef};
use crate::retention::{
    ChargeClass, RetentionAdmission, RetentionCharge, SemanticRetentionAccount,
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
    /// Whether this evidence is a page or holds one, at any depth: a
    /// retaining admission walks only evidence that can reach a page.
    reaches_pages: bool,
    /// Set once a retaining admission has claimed every page reachable from
    /// here. A claimed page never returns to a pin, so a later admission
    /// need not walk this evidence again.
    pages_claimed: std::sync::atomic::AtomicBool,
    /// A page's reservation against the retention account, held for
    /// exactly as long as the page lives (every signature, candidate and
    /// refusal summary sharing it shares this one charge): pinned until a
    /// cache admission claims it, retained from then on. `None` for a
    /// result's evidence.
    retention: Option<parking_lot::Mutex<RetentionCharge>>,
}

/// Every canonical the facts reachable from a receipt name, ordered.
///
/// A persistent set: a receipt's set starts as its largest consumed
/// receipt's set, shared, and gains only what that set lacks, so a chain
/// of `n` receipts each naming one new canonical holds `O(n log n)` set
/// structure rather than a copy of every prefix. Its `len()` is the
/// logical count of the canonicals it names; the storage is shared.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct CanonicalSet(imbl::OrdSet<Arc<str>>);

impl CanonicalSet {
    /// Whether `canonical` is one of the set's canonicals.
    #[must_use]
    pub fn contains(&self, canonical: &str) -> bool {
        self.0.contains(canonical)
    }

    /// The set's own copy of `canonical`, when it holds it.
    #[must_use]
    pub fn get(&self, canonical: &str) -> Option<&Arc<str>> {
        self.0.get(canonical)
    }

    /// The canonicals, in order.
    pub fn iter(&self) -> impl Iterator<Item = &Arc<str>> + '_ {
        self.0.iter()
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
    /// shared, plus what each other set holds that it lacks (found by a
    /// difference walk that skips the structure the two share), plus each
    /// own canonical it lacks.
    ///
    /// Also answers the estimated bytes of the storage the union owns
    /// beyond the set it shares: a slot per inserted canonical, and the
    /// string each newly allocated own canonical holds.
    fn union_of(consumed: &[&CanonicalSet], own: &[&str]) -> (Self, usize) {
        let mut owned_bytes = 0;
        let Some(largest) = consumed.iter().copied().max_by_key(|set| set.len()) else {
            let mut set = imbl::OrdSet::new();
            for canonical in own {
                if !set.contains(*canonical) {
                    owned_bytes += SET_SLOT_BYTES + ARC_STR_HEADER_BYTES + canonical.len();
                    set.insert(Arc::from(*canonical));
                }
            }
            return (Self(set), owned_bytes);
        };
        let mut set = largest.0.clone();
        for other in consumed {
            if std::ptr::eq(*other, largest) {
                continue;
            }
            let added: Vec<Arc<str>> = set
                .diff(&other.0)
                .filter_map(|item| match item {
                    imbl::ordset::DiffItem::Add(canonical) => Some(Arc::clone(canonical)),
                    imbl::ordset::DiffItem::Remove(_) => None,
                })
                .collect();
            owned_bytes += added.len() * SET_SLOT_BYTES;
            for canonical in added {
                set.insert(canonical);
            }
        }
        for canonical in own {
            if !set.contains(*canonical) {
                owned_bytes += SET_SLOT_BYTES + ARC_STR_HEADER_BYTES + canonical.len();
                set.insert(Arc::from(*canonical));
            }
        }
        (Self(set), owned_bytes)
    }
}

impl std::fmt::Debug for CanonicalSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_set().entries(self.0.iter()).finish()
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

/// Estimated resident bytes an evidence page owns: its shared header, its
/// entry array, the strings its own facts hold, its aggregated domains and
/// the canonical-set storage it does not share with the pages it holds.
fn page_storage_bytes(
    facts: &[FactVersionRef],
    aggregated: usize,
    canonical_set_bytes: usize,
) -> usize {
    let owned_strings: usize = facts
        .iter()
        .filter_map(FactVersionRef::canonical_id)
        .map(str::len)
        .sum();
    ARC_HEADER_BYTES
        + std::mem::size_of::<ResultEvidence>()
        + ARC_HEADER_BYTES
        + std::mem::size_of_val(facts)
        + owned_strings
        + aggregated * std::mem::size_of::<CompactionDomain>()
        + canonical_set_bytes
}

/// A receipt for a completed result: its evidence, shared.
///
/// Equality, order and hash read the evidence's digest first; two
/// receipts with equal digests compare their facts, so a digest collision
/// never makes two different evidences equal.
#[derive(Clone)]
pub struct ResultReceipt(Arc<ResultEvidence>);

impl ResultReceipt {
    /// The receipt for `facts`, a completed result's own facts and the
    /// receipts of what it consumed. Sorted and deduplicated here.
    #[must_use]
    pub fn new(mut facts: Vec<FactVersionRef>) -> Self {
        facts.sort_unstable();
        facts.dedup();
        drop_subsumed_receipts(&mut facts);
        Self::seal(facts, EvidenceKind::Result, None)
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
        Self::seal(facts, EvidenceKind::Page, Some(account))
    }

    fn seal(
        facts: Vec<FactVersionRef>,
        kind: EvidenceKind,
        page_account: Option<&Arc<SemanticRetentionAccount>>,
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
        let mut reaches_pages = kind == EvidenceKind::Page;
        for fact in &facts {
            match fact {
                FactVersionRef::Receipt(child) => {
                    reaches_pages |= child.0.reaches_pages;
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
        let retention = page_account.map(|account| {
            parking_lot::Mutex::new(account.pin(page_storage_bytes(
                &facts,
                aggregated.len(),
                canonical_set_bytes,
            )))
        });
        Self(Arc::new(ResultEvidence {
            facts: facts.into(),
            kind,
            digest,
            canonicals,
            aggregated: aggregated.into(),
            resolution_evidence,
            reaches_pages,
            pages_claimed: std::sync::atomic::AtomicBool::new(false),
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

    /// Bytes this evidence holds charged against the retention account: a
    /// page's charge, shared by every holder; `0` for a result's evidence.
    #[must_use]
    pub fn retained_charge_bytes(&self) -> usize {
        self.0
            .retention
            .as_ref()
            .map_or(0, |charge| charge.lock().bytes())
    }

    /// The class of a page's charge: [`ChargeClass::Pinned`] until a cache
    /// admission claims it, [`ChargeClass::Retained`] after. `None` for a
    /// result's evidence.
    #[must_use]
    pub fn retained_charge_class(&self) -> Option<ChargeClass> {
        self.0
            .retention
            .as_ref()
            .map(|charge| charge.lock().class())
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
/// uncharged during the exchange. On refusal nothing changes: every page
/// stays pinned by the live signatures holding it.
pub fn reserve_retained_with_evidence(
    account: &Arc<SemanticRetentionAccount>,
    own_bytes: usize,
    signatures: &[&[FactVersionRef]],
) -> RetentionAdmission {
    use std::sync::atomic::Ordering;
    let mut seen: rustc_hash::FxHashSet<*const ResultEvidence> = rustc_hash::FxHashSet::default();
    let mut walked: Vec<&ResultEvidence> = Vec::new();
    let mut claim: Vec<(&parking_lot::Mutex<RetentionCharge>, usize)> = Vec::new();
    let mut stack: Vec<&[FactVersionRef]> = signatures.to_vec();
    // Every distinct evidence that can reach a page is walked once, whatever
    // its kind: a consumed result's receipt owns no charge of its own but may
    // hold pages (a result recorded over a wide warm-hit signature), and the
    // candidate retains those pages through it just as it retains its own.
    while let Some(level) = stack.pop() {
        for fact in level {
            let FactVersionRef::Receipt(receipt) = fact else {
                continue;
            };
            if !receipt.0.reaches_pages
                || receipt.0.pages_claimed.load(Ordering::Acquire)
                || !seen.insert(Arc::as_ptr(&receipt.0))
            {
                continue;
            }
            walked.push(&receipt.0);
            if let Some(cell) = receipt.0.retention.as_ref() {
                let charge = cell.lock();
                if charge.class() != ChargeClass::Retained {
                    claim.push((cell, charge.bytes()));
                }
            }
            stack.push(&receipt.0.facts);
        }
    }
    let evidence_bytes: usize = claim.iter().map(|(_, bytes)| bytes).sum();
    let mut charge = match account.reserve(ChargeClass::Retained, own_bytes + evidence_bytes) {
        RetentionAdmission::Admitted(charge) => charge,
        refused @ RetentionAdmission::Refused(_) => return refused,
    };
    for (cell, bytes) in claim {
        let share = charge.split_off(bytes);
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
        evidence.pages_claimed.store(true, Ordering::Release);
    }
    RetentionAdmission::Admitted(charge)
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
