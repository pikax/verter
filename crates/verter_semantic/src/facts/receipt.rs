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

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use super::version::{CompactionDomain, FactAttribution, FactVersionRef};

/// The evidence one completed result recorded: its own facts and the
/// receipts of the results it consumed, in canonical order, with the
/// summaries a consumer projecting a signature needs without walking it.
#[derive(Debug)]
pub struct ResultEvidence {
    facts: Arc<[FactVersionRef]>,
    digest: u128,
    /// Every canonical a fact reachable from here names, sorted.
    canonicals: Arc<[Arc<str>]>,
    /// Every compaction domain a reachable fact carries as its terminal
    /// aggregate, in first-appearance order.
    aggregated: Arc<[CompactionDomain]>,
    /// Whether a reachable fact is resolution evidence.
    resolution_evidence: bool,
    /// How many distinct receipts are reachable from here, this one
    /// included.
    receipts: usize,
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
        let mut digester = xxhash_rust::xxh3::Xxh3::new();
        facts.len().hash(&mut digester);
        for fact in &facts {
            fact.hash(&mut digester);
        }
        let digest = digester.digest128();
        let mut canonicals: Vec<Arc<str>> = Vec::new();
        let mut aggregated: Vec<CompactionDomain> = Vec::new();
        let mut resolution_evidence = false;
        let mut receipts = 1usize;
        for fact in &facts {
            match fact {
                FactVersionRef::Receipt(child) => {
                    canonicals.extend(child.0.canonicals.iter().cloned());
                    for domain in child.0.aggregated.iter() {
                        if !aggregated.contains(domain) {
                            aggregated.push(*domain);
                        }
                    }
                    resolution_evidence |= child.0.resolution_evidence;
                    receipts = receipts.saturating_add(child.0.receipts);
                }
                other => {
                    match other.attribution() {
                        FactAttribution::Canonical(canonical) => {
                            canonicals.push(Arc::from(canonical));
                        }
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
        canonicals.sort_unstable();
        canonicals.dedup();
        Self(Arc::new(ResultEvidence {
            facts: facts.into(),
            digest,
            canonicals: canonicals.into(),
            aggregated: aggregated.into(),
            resolution_evidence,
            receipts,
        }))
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

    /// Every canonical a fact reachable from this receipt names, sorted.
    #[must_use]
    pub fn canonicals(&self) -> &[Arc<str>] {
        &self.0.canonicals
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
        if !self.walked.insert(Arc::as_ptr(&receipt.0)) {
            return true;
        }
        let mut stack: Vec<&ResultEvidence> = vec![&receipt.0];
        while let Some(evidence) = stack.pop() {
            for fact in evidence.facts.iter() {
                match fact {
                    FactVersionRef::Receipt(child) => {
                        if self.walked.insert(Arc::as_ptr(&child.0)) {
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
            .field("digest", &format_args!("{:032x}", self.0.digest))
            .field("facts", &self.0.facts.len())
            .field("receipts", &self.0.receipts)
            .finish()
    }
}

impl PartialEq for ResultReceipt {
    fn eq(&self, other: &Self) -> bool {
        self.ptr_eq(other) || (self.0.digest == other.0.digest && self.0.facts == other.0.facts)
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
            .then_with(|| self.0.facts.cmp(&other.0.facts))
    }
}

impl Hash for ResultReceipt {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.digest.hash(state);
    }
}

#[cfg(test)]
#[path = "receipt_tests.rs"]
mod tests;
