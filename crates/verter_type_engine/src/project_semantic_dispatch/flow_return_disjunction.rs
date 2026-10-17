//! The union-of-facts reading of a disjunction over SHARED environments.
//!
//! `a || b || c` is true through `a`, through `b` once `a` is false, or
//! through `c` once both are: alternative `i` is a PREFIX — every earlier
//! disjunct read the other way — followed by its own DELTA — its disjunct
//! read the guard's way. The prefixes nest, so one prefix environment
//! serves every alternative, extended in place by one disjunct per
//! alternative; each delta is indexed on its own; and a reference lands a
//! narrow only when EVERY alternative narrowed it, so the references the
//! first alternative narrowed are the only candidates, and each later
//! alternative is probed for them by subject index. No alternative is
//! materialized as a copy of its prefix, and no subject list is rescanned,
//! so a disjunction over `n` independent references does work linear in
//! `n` rather than copying `n²` facts and comparing `n³` subjects.
//!
//! The frame's overlay is marked by a persistent copy of the product state
//! (sharing its tree) and restored by re-installing exactly the roots the
//! guard ledger names since the mark: every narrowing write a guard
//! performs is a ledger entry, so those are the only roots that can differ.

use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use verter_session_query::flow::slice::{SliceGuard, SliceNarrowSubject};

use super::super::flow_solve::FlowDomain;
use super::{
    FlowDemandDriver, FlowEvaluator, FlowProductStore, FlowProductSubject, NarrowingLedgerEntry,
};
use crate::semantic_query::SemanticNodeId;

#[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
thread_local! {
    /// Narrowing entries disjunction narrowing wrote into its environments
    /// on this thread: a standing fact indexed, a per-alternative narrow
    /// recorded, or a root's overlay re-installed by a restore.
    static ENVIRONMENT_ENTRIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Subject identities disjunction narrowing probed on this thread: one
    /// per subject-index lookup or insertion.
    static SUBJECT_PROBES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The disjunction-narrowing work this thread performed so far.
#[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisjunctionWork {
    /// Narrowing entries written into disjunction environments.
    pub environment_entries: usize,
    /// Subject identities probed.
    pub subject_probes: usize,
}

/// The disjunction-narrowing work this thread performed so far.
#[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
pub fn disjunction_work_observed() -> DisjunctionWork {
    DisjunctionWork {
        environment_entries: ENVIRONMENT_ENTRIES.with(std::cell::Cell::get),
        subject_probes: SUBJECT_PROBES.with(std::cell::Cell::get),
    }
}

#[inline]
fn note_entries(_entries: usize) {
    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    ENVIRONMENT_ENTRIES.with(|count| count.set(count.get() + _entries));
}

#[inline]
fn note_entry() {
    note_entries(1);
}

#[inline]
fn note_probe() {
    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    SUBJECT_PROBES.with(|count| count.set(count.get() + 1));
}

/// A mark in the narrowing overlay that shares the frame's persistent
/// product state instead of copying every subject's facts.
struct NarrowingWindowMark {
    products: FlowProductStore,
    writes: usize,
}

/// One standing fact: `subject`, filed under the resolved `root`, narrowed
/// to `node`.
struct StandingFact {
    root: FlowProductSubject,
    subject: SliceNarrowSubject,
    node: SemanticNodeId,
}

/// A kill a ledger window performed: every fact under `root`, or only
/// those strictly below `path` under it.
struct StandingKill {
    root: FlowProductSubject,
    below: Option<Arc<[Arc<str>]>>,
}

impl StandingKill {
    fn covers(&self, root: &FlowProductSubject, subject: &SliceNarrowSubject) -> bool {
        self.root == *root
            && self.below.as_ref().is_none_or(|path| {
                subject.path.len() > path.len() && subject.path.starts_with(path)
            })
    }
}

/// The facts a sequence of ledger windows leaves standing, one per subject
/// and indexed by it: a fact about a subject already standing replaces it
/// where it stands, a kill drops what it names, and a subject established
/// again after a kill stands again at the end. Per subject, the newest
/// fact and the place the subject first stood are those of
/// [`super::standing_narrowings`] over the same entries.
#[derive(Default)]
struct StandingIndex {
    slots: Vec<Option<StandingFact>>,
    by_subject: FxHashMap<SliceNarrowSubject, usize>,
    /// The kills folded, in order: what a delta retracts from the prefix
    /// it extends.
    kills: Vec<StandingKill>,
}

impl StandingIndex {
    fn fold(&mut self, window: &[NarrowingLedgerEntry]) {
        for entry in window {
            match entry {
                NarrowingLedgerEntry::Established {
                    root,
                    subject,
                    node,
                } => {
                    note_probe();
                    note_entry();
                    let fact = StandingFact {
                        root: root.clone(),
                        subject: subject.clone(),
                        node: *node,
                    };
                    match self.by_subject.get(subject) {
                        Some(&slot) => self.slots[slot] = Some(fact),
                        None => {
                            self.by_subject.insert(subject.clone(), self.slots.len());
                            self.slots.push(Some(fact));
                        }
                    }
                }
                NarrowingLedgerEntry::Cleared { root } => self.kill(StandingKill {
                    root: root.clone(),
                    below: None,
                }),
                NarrowingLedgerEntry::ClearedBelow { root, path } => self.kill(StandingKill {
                    root: root.clone(),
                    below: Some(Arc::clone(path)),
                }),
            }
        }
    }

    fn kill(&mut self, kill: StandingKill) {
        for slot in &mut self.slots {
            if slot
                .as_ref()
                .is_some_and(|fact| kill.covers(&fact.root, &fact.subject))
            {
                let fact = slot.take().expect("a standing fact");
                note_probe();
                self.by_subject.remove(&fact.subject);
            }
        }
        self.kills.push(kill);
    }

    fn get(&self, subject: &SliceNarrowSubject) -> Option<&StandingFact> {
        note_probe();
        let slot = *self.by_subject.get(subject)?;
        self.slots[slot].as_ref()
    }

    /// The standing facts, in the order their subjects first stood.
    fn standing(&self) -> impl Iterator<Item = &StandingFact> {
        self.slots.iter().flatten()
    }

    /// The node `subject` (filed under `root`) stands at once this delta
    /// has extended `prefix`: the delta's own newest fact, or else the
    /// prefix's fact unless a kill in the delta retracted it.
    fn read_over(
        &self,
        prefix: &StandingIndex,
        root: &FlowProductSubject,
        subject: &SliceNarrowSubject,
    ) -> Option<SemanticNodeId> {
        if let Some(fact) = self.get(subject) {
            return Some(fact.node);
        }
        if self.kills.iter().any(|kill| kill.covers(root, subject)) {
            return None;
        }
        prefix.get(subject).map(|fact| fact.node)
    }
}

/// One reference the first alternative narrowed, with its narrow in every
/// alternative met so far, in alternative order.
struct UnionCandidate {
    root: Option<FlowProductSubject>,
    subject: SliceNarrowSubject,
    nodes: Vec<SemanticNodeId>,
}

/// The per-reference union over a sequence of alternatives: a reference
/// every alternative narrowed, with the union of its per-alternative
/// narrows. A reference some alternative leaves unnarrowed has its
/// original type on that path, which the union would be, so it drops out
/// — and since it must stand in the FIRST alternative, the candidates are
/// that alternative's references, in the order they first stood there.
struct AlternativeUnion {
    candidates: Vec<UnionCandidate>,
}

impl AlternativeUnion {
    fn first(candidates: impl IntoIterator<Item = UnionCandidate>) -> Self {
        let candidates: Vec<UnionCandidate> = candidates.into_iter().collect();
        note_entries(candidates.len());
        Self { candidates }
    }

    /// Meet one more alternative, which reads each candidate's narrow
    /// through `narrow_of`.
    fn meet(&mut self, mut narrow_of: impl FnMut(&UnionCandidate) -> Option<SemanticNodeId>) {
        self.candidates
            .retain_mut(|candidate| match narrow_of(candidate) {
                Some(node) => {
                    note_entry();
                    candidate.nodes.push(node);
                    true
                }
                None => false,
            });
    }

    fn into_narrows(self) -> impl Iterator<Item = (SliceNarrowSubject, Vec<SemanticNodeId>)> {
        self.candidates
            .into_iter()
            .map(|candidate| (candidate.subject, candidate.nodes))
    }
}

impl<D: FlowDemandDriver> FlowEvaluator<'_, '_, D> {
    fn narrowing_window_mark(&self) -> NarrowingWindowMark {
        NarrowingWindowMark {
            products: self.products.clone(),
            writes: self.narrowing_writes.len(),
        }
    }

    /// Restore the overlay to `mark`: every root a guard write named since
    /// the mark regains exactly the facts it held there, and the ledger
    /// returns to the mark's length.
    fn restore_narrowing_window(&mut self, mark: &NarrowingWindowMark) {
        let start = mark.writes.min(self.narrowing_writes.len());
        let mut seen: FxHashSet<&FlowProductSubject> = FxHashSet::default();
        let roots: Vec<FlowProductSubject> = self.narrowing_writes[start..]
            .iter()
            .map(|entry| match entry {
                NarrowingLedgerEntry::Established { root, .. }
                | NarrowingLedgerEntry::Cleared { root }
                | NarrowingLedgerEntry::ClearedBelow { root, .. } => root,
            })
            .filter(|root| seen.insert(*root))
            .cloned()
            .collect();
        for root in roots {
            note_entry();
            match mark.products.narrowing(&root) {
                Some(product) => self.products.set_narrowing(&root, product.clone()),
                None => self.products.remove(FlowDomain::Narrowing, &root),
            }
        }
        self.narrowing_writes.truncate(mark.writes);
    }

    /// The union-of-facts reading of a disjunction: each disjunct's
    /// narrow is computed against the starting overlay as the operands
    /// before it LEFT it — `a || b` is true through `a`, or through `b`
    /// once `a` is false, exactly as the checker narrows the right operand
    /// by the left's other edge — and a subject narrowed by every disjunct
    /// lands the union of the per-disjunct narrows. A subject some
    /// disjunct leaves unnarrowed contributes its ORIGINAL type to that
    /// union — which is the original type, so no narrow is established
    /// for it.
    ///
    /// Each part is applied once each way: its own reading on top of the
    /// shared prefix, read off the ledger as the alternative's delta and
    /// rolled back, then — unless it is the last — the other reading,
    /// which stays applied and extends the prefix for the next part.
    pub(super) fn apply_guard_union(&mut self, parts: &[SliceGuard], positive: bool) {
        let base = self.narrowing_window_mark();
        let mut prefix = StandingIndex::default();
        let mut union: Option<AlternativeUnion> = None;
        for (index, part) in parts.iter().enumerate() {
            let mark = self.narrowing_window_mark();
            self.apply_guard_scoped(part, positive);
            let mut delta = StandingIndex::default();
            delta.fold(&self.narrowing_writes[mark.writes.min(self.narrowing_writes.len())..]);
            match &mut union {
                // The first alternative has no prefix: its delta is all
                // of it.
                None => {
                    union = Some(AlternativeUnion::first(delta.standing().map(|fact| {
                        UnionCandidate {
                            root: Some(fact.root.clone()),
                            subject: fact.subject.clone(),
                            nodes: vec![fact.node],
                        }
                    })));
                }
                Some(union) => union.meet(|candidate| {
                    let root = candidate.root.as_ref().expect("a guard candidate's root");
                    delta.read_over(&prefix, root, &candidate.subject)
                }),
            }
            self.restore_narrowing_window(&mark);
            if index + 1 < parts.len() {
                let extension = self.narrowing_writes.len();
                self.apply_guard_scoped(part, !positive);
                prefix.fold(&self.narrowing_writes[extension..]);
            }
        }
        self.restore_narrowing_window(&base);
        let Some(union) = union else {
            return;
        };
        for (subject, nodes) in union.into_narrows() {
            let node = self.union(&nodes);
            self.push_narrowing(&subject, node);
        }
    }

    /// The per-reference union over `alternatives`, each the facts one
    /// path established in write order (the newest fact about a reference
    /// is the one standing at its end): a reference every alternative
    /// narrowed, with the union of its per-alternative narrows, in the
    /// order it first stood in the first alternative. A reference some
    /// alternative leaves unnarrowed has its original type on that path,
    /// which the union would be, so it is left out.
    pub(super) fn union_of_alternatives(
        &mut self,
        alternatives: Vec<Vec<(SliceNarrowSubject, SemanticNodeId)>>,
    ) -> Vec<(SliceNarrowSubject, SemanticNodeId)> {
        let mut alternatives = alternatives.into_iter();
        let Some(first) = alternatives.next() else {
            return Vec::new();
        };
        let mut order: Vec<UnionCandidate> = Vec::with_capacity(first.len());
        let mut by_subject: FxHashMap<SliceNarrowSubject, usize> = FxHashMap::default();
        for (subject, node) in first {
            note_probe();
            match by_subject.get(&subject) {
                Some(&slot) => order[slot].nodes[0] = node,
                None => {
                    by_subject.insert(subject.clone(), order.len());
                    order.push(UnionCandidate {
                        root: None,
                        subject,
                        nodes: vec![node],
                    });
                }
            }
        }
        let mut union = AlternativeUnion::first(order);
        for alternative in alternatives {
            let mut newest: FxHashMap<SliceNarrowSubject, SemanticNodeId> =
                FxHashMap::with_capacity_and_hasher(alternative.len(), Default::default());
            for (subject, node) in alternative {
                note_probe();
                newest.insert(subject, node);
            }
            union.meet(|candidate| {
                note_probe();
                newest.get(&candidate.subject).copied()
            });
        }
        union
            .into_narrows()
            .map(|(subject, nodes)| {
                let node = self.union(&nodes);
                (subject, node)
            })
            .collect()
    }
}
