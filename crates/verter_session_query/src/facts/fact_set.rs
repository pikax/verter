//! A sealed read set: ONE canonical (sorted, deduplicated) set of facts,
//! held so that a set built from another shares its storage.
//!
//! Every reader sees a [`FactSet`] as one flat, strictly increasing run of
//! facts. It iterates ([`FactSet::iter`], double-ended), counts, searches
//! ([`FactSet::contains`], [`FactSet::iter_from`]) and compares it, and
//! nothing it can ask depends on how the set is stored:
//!
//! * a set no wider set was built from — every narrow set, and every
//!   freshly sealed one — is one shared slice, charged inline by whoever
//!   retains it;
//! * a set wider than [`FACT_SET_INLINE_WIDTH`] built by extending another
//!   set is a persistent B+tree ([`super::fact_tree`]): it shares every
//!   node of the set it was built from except the paths its own insertions
//!   and removals copied, so a chain of nested results — each link its
//!   child's set plus a few facts of its own — holds one path per link,
//!   never a copy of every prefix.
//!
//! ## Laws
//!
//! These hold for every set, whatever its storage or history, and readers
//! may rely on them:
//!
//! * **Canonical content.** A set holds each fact at most once, iterated in
//!   the derived [`Ord`] of [`FactVersionRef`] — deterministic across runs
//!   and processes.
//! * **Content-defined identity.** Equality, ordering and hashing read the
//!   facts alone: two sets with the same facts are equal, order equal and
//!   hash equal, however they were built. [`Ord`] is the lexicographic order
//!   of the canonical runs, [`Hash`] hashes the length then each fact in
//!   order with the caller's hasher, exactly as a sorted slice does.
//! * **Order-independent digest.** [`FactSet::digest`] is the wrapping sum
//!   of [`fact_digest`] over the facts, so equal content gives an equal
//!   digest whatever the insertion history. It is maintained on every
//!   insertion and removal of a tree set; a digest match is never taken as
//!   equality without comparing content.
//! * **No hidden copies.** No reader can obtain a wide tree set as a slice:
//!   there is no slice view of a tree. [`FactSet::as_flat`] answers only for
//!   a set already held as one.
//!
//! ## Retention
//!
//! A slice set is charged inline by its holder: its block and its facts'
//! heap. A tree set's holder charges only the set's small header
//! ([`FactSet::header_bytes`]); every tree node charges exactly its own
//! allocation, from the seal of the edit that built it until its own drop
//! (see [`super::fact_tree`]), and a retaining admission claims the nodes a
//! set reaches that no earlier admission claimed
//! ([`crate::facts::receipt::reserve_retained_with_evidence`]).

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use super::fact_tree::{self, Node};
use super::version::FactVersionRef;

/// The widest set held as a plain shared slice when it is built by
/// extending another set. Wider extensions are trees. A layout width only:
/// no reader can tell the two apart.
pub const FACT_SET_INLINE_WIDTH: usize = 32;

/// A strong, process-stable hash of one fact: the unit of a set's digest.
#[inline]
#[must_use]
pub fn fact_digest(fact: &FactVersionRef) -> u128 {
    let mut digester = xxhash_rust::xxh3::Xxh3::new();
    fact.hash(&mut digester);
    digester.digest128()
}

fn digest_of<'f>(facts: impl IntoIterator<Item = &'f FactVersionRef>) -> u128 {
    facts
        .into_iter()
        .fold(0u128, |sum, fact| sum.wrapping_add(fact_digest(fact)))
}

/// One canonical set of facts. See the module docs.
#[derive(Clone)]
pub struct FactSet(Repr);

#[derive(Clone)]
enum Repr {
    /// Strictly increasing facts in one shared slice.
    Flat(Arc<[FactVersionRef]>),
    /// More than [`FACT_SET_INLINE_WIDTH`] facts, built by extending
    /// another set.
    Tree(Arc<TreeStorage>),
}

/// A tree set's header: its root and its digest. Its block is charged by
/// the set's holders ([`FactSet::header_bytes`]); its nodes charge
/// themselves.
pub struct TreeStorage {
    root: Arc<Node>,
    digest: u128,
}

impl Default for FactSet {
    fn default() -> Self {
        Self::empty()
    }
}

impl FactSet {
    /// The empty set.
    #[must_use]
    pub fn empty() -> Self {
        thread_local! {
            static EMPTY: Arc<[FactVersionRef]> = Arc::from(Vec::new());
        }
        Self(Repr::Flat(EMPTY.with(Arc::clone)))
    }

    /// The set of `facts`, in any order and with any repetition.
    #[must_use]
    pub fn from_facts(mut facts: Vec<FactVersionRef>) -> Self {
        if !facts.windows(2).all(|pair| pair[0] < pair[1]) {
            facts.sort_unstable_by(|a, b| {
                super::fact_read_set::record_signature_work(1);
                a.cmp(b)
            });
            facts.dedup();
        }
        Self::from_canonical_vec(facts)
    }

    /// The set of `facts`, already strictly increasing (checked in debug
    /// builds).
    ///
    /// A freshly sealed set is one shared slice whatever its width: a tree
    /// earns its nodes back only by being shared, so a set becomes a tree
    /// when a wider set is built from it ([`FactSet::edit`]).
    #[must_use]
    pub fn from_canonical_vec(facts: Vec<FactVersionRef>) -> Self {
        debug_assert!(facts.windows(2).all(|pair| pair[0] < pair[1]));
        Self(Repr::Flat(Arc::from(facts)))
    }

    /// The set of `facts`, already strictly increasing, copied into one
    /// shared slice.
    #[must_use]
    pub fn from_canonical_slice(facts: &[FactVersionRef]) -> Self {
        debug_assert!(facts.windows(2).all(|pair| pair[0] < pair[1]));
        Self(Repr::Flat(Arc::from(facts)))
    }

    /// How many facts the set holds.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        match &self.0 {
            Repr::Flat(facts) => facts.len(),
            Repr::Tree(tree) => tree.root.len(),
        }
    }

    /// Whether the set holds no fact.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The facts in canonical order.
    #[inline]
    pub fn iter(&self) -> FactSetIter<'_> {
        match &self.0 {
            Repr::Flat(facts) => FactSetIter(IterRepr::Flat(facts.iter())),
            Repr::Tree(tree) => FactSetIter(IterRepr::Tree(fact_tree::Iter::new(&tree.root))),
        }
    }

    /// Whether the set holds `fact`.
    #[must_use]
    pub fn contains(&self, fact: &FactVersionRef) -> bool {
        match &self.0 {
            Repr::Flat(facts) => facts.binary_search(fact).is_ok(),
            Repr::Tree(tree) => fact_tree::contains(&tree.root, fact),
        }
    }

    /// The facts of the set from `from` (inclusive) on, in order: the
    /// canonical order keeps one variant's facts together, and one file's
    /// whole-file facts together, so a reader after one kind of fact starts
    /// at that kind's least value.
    pub fn iter_from<'a>(&'a self, from: &FactVersionRef) -> FactSetIter<'a> {
        match &self.0 {
            Repr::Flat(facts) => {
                let start = facts.partition_point(|fact| fact < from);
                FactSetIter(IterRepr::Flat(facts[start..].iter()))
            }
            Repr::Tree(tree) => {
                FactSetIter(IterRepr::Tree(fact_tree::Iter::from(&tree.root, from)))
            }
        }
    }

    /// The least fact.
    #[must_use]
    pub fn first(&self) -> Option<&FactVersionRef> {
        match &self.0 {
            Repr::Flat(facts) => facts.first(),
            Repr::Tree(tree) => Some(fact_tree::first(&tree.root)),
        }
    }

    /// The greatest fact.
    #[must_use]
    pub fn last(&self) -> Option<&FactVersionRef> {
        match &self.0 {
            Repr::Flat(facts) => facts.last(),
            Repr::Tree(tree) => Some(fact_tree::last(&tree.root)),
        }
    }

    /// The facts as an owned vector, in canonical order. Test support only:
    /// a production reader iterates the set in place.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn to_vec(&self) -> Vec<FactVersionRef> {
        self.iter().cloned().collect()
    }

    /// The facts as a slice, when the set is held as one already. A tree set
    /// answers `None`: it has no slice, and none is ever made for it.
    #[must_use]
    pub fn as_flat(&self) -> Option<&[FactVersionRef]> {
        match &self.0 {
            Repr::Flat(facts) => Some(facts),
            Repr::Tree(_) => None,
        }
    }

    /// The order-independent content digest (see the module laws).
    #[must_use]
    pub fn digest(&self) -> u128 {
        match &self.0 {
            Repr::Flat(facts) => digest_of(facts.iter()),
            Repr::Tree(tree) => tree.digest,
        }
    }

    /// Whether two sets share one storage.
    #[must_use]
    pub fn ptr_eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Repr::Flat(a), Repr::Flat(b)) => Arc::ptr_eq(a, b),
            (Repr::Tree(a), Repr::Tree(b)) => Arc::ptr_eq(a, b) || Arc::ptr_eq(&a.root, &b.root),
            _ => false,
        }
    }

    /// The address of the set's storage: equal for every clone of one set,
    /// for a cache keyed by the signature it was given.
    #[must_use]
    pub fn address(&self) -> usize {
        match &self.0 {
            Repr::Flat(facts) => Arc::as_ptr(facts).cast::<FactVersionRef>() as usize,
            Repr::Tree(tree) => Arc::as_ptr(tree) as usize,
        }
    }

    /// Whether every fact of `other` is in this set.
    #[must_use]
    pub fn is_superset(&self, other: &Self) -> bool {
        if other.len() > self.len() {
            return false;
        }
        if self.ptr_eq(other) {
            return true;
        }
        match (&self.0, &other.0) {
            (Repr::Flat(whole), Repr::Flat(part)) => {
                let mut at = 0;
                'parts: for fact in part.iter() {
                    while let Some(held) = whole.get(at) {
                        super::fact_read_set::record_signature_work(1);
                        at += 1;
                        match held.cmp(fact) {
                            std::cmp::Ordering::Less => {}
                            std::cmp::Ordering::Equal => continue 'parts,
                            std::cmp::Ordering::Greater => return false,
                        }
                    }
                    return false;
                }
                true
            }
            _ => other.iter().all(|fact| {
                super::fact_read_set::record_signature_work(1);
                self.contains(fact)
            }),
        }
    }

    /// The shared slice block of a slice set, charged inline by its holder.
    #[must_use]
    pub fn flat_arc(&self) -> Option<&Arc<[FactVersionRef]>> {
        match &self.0 {
            Repr::Flat(facts) => Some(facts),
            Repr::Tree(_) => None,
        }
    }

    /// The header of a tree set, charged by its holders; its nodes charge
    /// themselves.
    #[must_use]
    pub fn tree_storage(&self) -> Option<&Arc<TreeStorage>> {
        match &self.0 {
            Repr::Flat(_) => None,
            Repr::Tree(tree) => Some(tree),
        }
    }

    /// Bytes a tree set's header block costs its holder.
    #[must_use]
    pub const fn header_bytes() -> usize {
        crate::retention::allocation::arc_block_bytes(std::mem::size_of::<TreeStorage>())
    }

    /// The root of a tree set.
    pub(crate) fn tree_root(&self) -> Option<&Arc<Node>> {
        self.tree_storage().map(|tree| &tree.root)
    }

    /// Bytes every node of a tree set charges now (test support: the
    /// node charges a set reaches, each counted once).
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn tree_node_charges(&self) -> usize {
        self.tree_node_list()
            .iter()
            .map(|node| node.charged_bytes())
            .sum()
    }

    /// The nodes of this tree set not reachable from `base`, and those it
    /// shares with `base` (test support: structural sharing, observed).
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn tree_nodes_against(&self, base: &FactSet) -> (usize, usize) {
        let base_nodes = base.tree_node_list();
        let own = self.tree_node_list();
        let shared = own
            .iter()
            .filter(|node| base_nodes.iter().any(|held| Arc::ptr_eq(held, node)))
            .count();
        (own.len() - shared, shared)
    }

    /// Bytes the nodes of this tree set not reachable from `base` charge.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn tree_charge_beyond(&self, base: &FactSet) -> usize {
        let base_nodes = base.tree_node_list();
        self.tree_node_list()
            .iter()
            .filter(|node| !base_nodes.iter().any(|held| Arc::ptr_eq(held, node)))
            .map(|node| node.charged_bytes())
            .sum()
    }

    #[cfg(any(test, feature = "test-support"))]
    fn tree_node_list(&self) -> Vec<Arc<Node>> {
        let mut nodes: Vec<Arc<Node>> = Vec::new();
        let Some(root) = self.tree_root() else {
            return nodes;
        };
        let mut stack = vec![Arc::clone(root)];
        while let Some(node) = stack.pop() {
            if nodes.iter().any(|seen| Arc::ptr_eq(seen, &node)) {
                continue;
            }
            stack.extend(node.children().iter().cloned());
            nodes.push(node);
        }
        nodes
    }

    /// What a tree set's nodes amount to, each node counted once: how many
    /// there are, how many heap blocks they own (a node and its vector),
    /// and the bytes their own charges hold now. A slice set has no nodes.
    #[must_use]
    pub fn tree_footprint(&self) -> TreeFootprint {
        Self::tree_footprint_of([self])
    }

    /// [`Self::tree_footprint`] over several sets, a node they share counted
    /// once.
    #[must_use]
    pub fn tree_footprint_of<'a>(sets: impl IntoIterator<Item = &'a FactSet>) -> TreeFootprint {
        let mut seen: rustc_hash::FxHashSet<*const Node> = rustc_hash::FxHashSet::default();
        let mut footprint = TreeFootprint::default();
        let mut stack: Vec<&Arc<Node>> = sets.into_iter().filter_map(FactSet::tree_root).collect();
        while let Some(node) = stack.pop() {
            if !seen.insert(Arc::as_ptr(node)) {
                continue;
            }
            footprint.nodes += 1;
            footprint.blocks += 2;
            let (bytes, retained) = node.charge_state();
            footprint.charged_bytes += bytes;
            if retained {
                footprint.retained_bytes += bytes;
            } else {
                footprint.pinned_bytes += bytes;
            }
            stack.extend(node.children().iter());
        }
        footprint
    }

    /// Whether retaining this set needs a reservation for tree nodes no
    /// earlier admission claimed.
    #[must_use]
    pub fn has_unclaimed_storage(&self) -> bool {
        self.tree_root().is_some_and(|root| !root.is_claimed())
    }

    /// Start a set from this one: the result shares this set's storage and
    /// pays only for what it changes.
    #[must_use]
    pub fn edit(&self) -> FactSetEdit {
        FactSetEdit {
            base: self.clone(),
            work: Work::Unchanged,
        }
    }

    /// The set of these facts and `more` (see [`FactSetEdit`]).
    #[must_use]
    pub fn union_with_facts<'f>(
        &self,
        more: impl IntoIterator<Item = &'f FactVersionRef>,
    ) -> FactSet {
        let mut edit = self.edit();
        for fact in more {
            if !edit.contains(fact) {
                edit.insert(fact.clone());
            }
        }
        edit.finish()
    }
}

/// What a tree set's nodes amount to ([`FactSet::tree_footprint`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TreeFootprint {
    /// Distinct nodes.
    pub nodes: usize,
    /// Heap blocks the nodes own: each node's block and its vector's.
    pub blocks: usize,
    /// Bytes the nodes' charges hold now.
    pub charged_bytes: usize,
    /// The part of [`Self::charged_bytes`] a retaining admission claimed
    /// into its refusable reservation.
    pub retained_bytes: usize,
    /// The part still pinned by the live sets holding the nodes.
    pub pinned_bytes: usize,
}

/// A set under construction from a base set: insertions and removals
/// applied to a copy that shares the base's storage.
pub struct FactSetEdit {
    base: FactSet,
    work: Work,
}

enum Work {
    Unchanged,
    Flat(Vec<FactVersionRef>),
    Tree {
        root: Option<Arc<Node>>,
        digest: u128,
        changed: bool,
    },
}

impl FactSetEdit {
    fn work_mut(&mut self) -> &mut Work {
        if matches!(self.work, Work::Unchanged) {
            self.work = match &self.base.0 {
                // Room for the insertions an edit usually makes, up to the
                // width at which the set becomes a tree.
                Repr::Flat(facts) if facts.len() <= FACT_SET_INLINE_WIDTH => {
                    let room = (facts.len() * 2).clamp(8, FACT_SET_INLINE_WIDTH + 1);
                    let mut copy = Vec::with_capacity(room);
                    copy.extend_from_slice(facts);
                    Work::Flat(copy)
                }
                // A wide slice extended for the first time: the new set is a
                // tree built from it; every set built from that tree shares
                // it.
                Repr::Flat(facts) => Work::Tree {
                    root: fact_tree::build(facts),
                    digest: digest_of(facts.iter()),
                    changed: true,
                },
                Repr::Tree(tree) => Work::Tree {
                    root: Some(Arc::clone(&tree.root)),
                    digest: tree.digest,
                    changed: false,
                },
            };
        }
        &mut self.work
    }

    /// How many facts the edited set holds now.
    #[must_use]
    pub fn len(&self) -> usize {
        match &self.work {
            Work::Unchanged => self.base.len(),
            Work::Flat(facts) => facts.len(),
            Work::Tree { root, .. } => root.as_ref().map_or(0, |root| root.len()),
        }
    }

    /// Whether the edited set holds no fact.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether the edited set holds `fact`.
    #[must_use]
    pub fn contains(&self, fact: &FactVersionRef) -> bool {
        match &self.work {
            Work::Unchanged => self.base.contains(fact),
            Work::Flat(facts) => facts.binary_search(fact).is_ok(),
            Work::Tree { root, .. } => root
                .as_ref()
                .is_some_and(|root| fact_tree::contains(root, fact)),
        }
    }

    /// The edited set's facts, in order.
    pub fn iter(&self) -> FactSetIter<'_> {
        match &self.work {
            Work::Unchanged => self.base.iter(),
            Work::Flat(facts) => FactSetIter(IterRepr::Flat(facts.iter())),
            Work::Tree { root, .. } => match root {
                Some(root) => FactSetIter(IterRepr::Tree(fact_tree::Iter::new(root))),
                None => FactSetIter(IterRepr::Flat([].iter())),
            },
        }
    }

    /// The edited set's facts from `from` on (see [`FactSet::iter_from`]).
    pub fn iter_from<'a>(&'a self, from: &FactVersionRef) -> FactSetIter<'a> {
        match &self.work {
            Work::Unchanged => self.base.iter_from(from),
            Work::Flat(facts) => {
                let start = facts.partition_point(|fact| fact < from);
                FactSetIter(IterRepr::Flat(facts[start..].iter()))
            }
            Work::Tree { root, .. } => match root {
                Some(root) => FactSetIter(IterRepr::Tree(fact_tree::Iter::from(root, from))),
                None => FactSetIter(IterRepr::Flat([].iter())),
            },
        }
    }

    /// Add `fact`; whether it was new.
    pub fn insert(&mut self, fact: FactVersionRef) -> bool {
        if self.contains(&fact) {
            return false;
        }
        match self.work_mut() {
            Work::Unchanged => unreachable!("work_mut never leaves the edit unchanged"),
            Work::Flat(facts) => {
                let at = facts.partition_point(|held| *held < fact);
                facts.insert(at, fact);
                if facts.len() > FACT_SET_INLINE_WIDTH {
                    let facts = std::mem::take(facts);
                    let digest = digest_of(facts.iter());
                    self.work = Work::Tree {
                        root: fact_tree::build(&facts),
                        digest,
                        changed: true,
                    };
                }
            }
            Work::Tree {
                root,
                digest,
                changed,
            } => {
                *digest = digest.wrapping_add(fact_digest(&fact));
                *changed = true;
                fact_tree::insert(root, fact);
            }
        }
        true
    }

    /// Remove `fact`; whether it was held.
    pub fn remove(&mut self, fact: &FactVersionRef) -> bool {
        if !self.contains(fact) {
            return false;
        }
        match self.work_mut() {
            Work::Unchanged => unreachable!("work_mut never leaves the edit unchanged"),
            Work::Flat(facts) => {
                if let Ok(at) = facts.binary_search(fact) {
                    facts.remove(at);
                }
            }
            Work::Tree {
                root,
                digest,
                changed,
            } => {
                *digest = digest.wrapping_sub(fact_digest(fact));
                *changed = true;
                fact_tree::remove(root, fact);
            }
        }
        true
    }

    /// Keep only the facts `keep` accepts.
    pub fn retain(&mut self, mut keep: impl FnMut(&FactVersionRef) -> bool) {
        let dropped: Vec<FactVersionRef> =
            self.iter().filter(|fact| !keep(fact)).cloned().collect();
        for fact in &dropped {
            self.remove(fact);
        }
    }

    /// The edited set. Unchanged, it is the base itself (shared); a narrow
    /// result is a plain slice; a wide one is a tree sharing every node of
    /// the base the edit did not touch, the nodes it built sealed and
    /// charged.
    #[must_use]
    pub fn finish(self) -> FactSet {
        match self.work {
            Work::Unchanged => self.base,
            Work::Flat(facts) => FactSet(Repr::Flat(Arc::from(facts))),
            Work::Tree {
                root,
                digest,
                changed,
            } => {
                if !changed {
                    return self.base;
                }
                let Some(mut root) = root else {
                    return FactSet::empty();
                };
                if root.len() <= FACT_SET_INLINE_WIDTH {
                    return FactSet(Repr::Flat(fact_tree::Iter::new(&root).cloned().collect()));
                }
                fact_tree::seal(&mut root);
                FactSet(Repr::Tree(Arc::new(TreeStorage { root, digest })))
            }
        }
    }
}

/// A borrowed run of facts a reader validates or reads: a sealed set, read
/// in place, or a plain list of facts (in its own order). Positions a
/// reader reports (a failing fact's, say) are positions in [`Self::iter`].
#[derive(Clone, Copy)]
pub enum FactsRef<'a> {
    Set(&'a FactSet),
    Slice(&'a [FactVersionRef]),
}

impl<'a> FactsRef<'a> {
    /// The facts, in order.
    #[must_use]
    pub fn iter(self) -> FactSetIter<'a> {
        match self {
            Self::Set(set) => set.iter(),
            Self::Slice(facts) => FactSetIter::from_slice(facts),
        }
    }

    /// How many facts.
    #[must_use]
    pub fn len(self) -> usize {
        match self {
            Self::Set(set) => set.len(),
            Self::Slice(facts) => facts.len(),
        }
    }

    /// Whether there is no fact.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    /// The fact at `position` in [`Self::iter`]: a walk to it, for a reader
    /// naming the fact a validation stopped at.
    #[must_use]
    pub fn get(self, position: usize) -> Option<&'a FactVersionRef> {
        match self {
            Self::Set(set) => set.iter().nth(position),
            Self::Slice(facts) => facts.get(position),
        }
    }

    /// The address of the borrowed storage, for a cache keyed by the run it
    /// was given.
    #[must_use]
    pub fn address(self) -> usize {
        match self {
            Self::Set(set) => set.address(),
            Self::Slice(facts) => facts.as_ptr() as usize,
        }
    }
}

impl<'a> From<&'a FactSet> for FactsRef<'a> {
    fn from(set: &'a FactSet) -> Self {
        Self::Set(set)
    }
}

impl<'a> From<&'a [FactVersionRef]> for FactsRef<'a> {
    fn from(facts: &'a [FactVersionRef]) -> Self {
        Self::Slice(facts)
    }
}

impl<'a> From<&'a Vec<FactVersionRef>> for FactsRef<'a> {
    fn from(facts: &'a Vec<FactVersionRef>) -> Self {
        Self::Slice(facts)
    }
}

impl<'a> From<&'a Arc<[FactVersionRef]>> for FactsRef<'a> {
    fn from(facts: &'a Arc<[FactVersionRef]>) -> Self {
        Self::Slice(facts)
    }
}

impl<'a> From<&'a crate::retention::owned::OwnedSlice<FactVersionRef>> for FactsRef<'a> {
    fn from(facts: &'a crate::retention::owned::OwnedSlice<FactVersionRef>) -> Self {
        Self::Slice(facts)
    }
}

impl<'a> From<&'a crate::retention::heap_size::OwnedValue<Box<[FactVersionRef]>>> for FactsRef<'a> {
    fn from(facts: &'a crate::retention::heap_size::OwnedValue<Box<[FactVersionRef]>>) -> Self {
        Self::Slice(facts)
    }
}

impl<'a> From<&'a Box<[FactVersionRef]>> for FactsRef<'a> {
    fn from(facts: &'a Box<[FactVersionRef]>) -> Self {
        Self::Slice(facts)
    }
}

impl<'a, const N: usize> From<&'a [FactVersionRef; N]> for FactsRef<'a> {
    fn from(facts: &'a [FactVersionRef; N]) -> Self {
        Self::Slice(facts)
    }
}

impl<'a> IntoIterator for FactsRef<'a> {
    type Item = &'a FactVersionRef;
    type IntoIter = FactSetIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Iterator over a [`FactSet`]'s facts in canonical order.
pub struct FactSetIter<'a>(IterRepr<'a>);

enum IterRepr<'a> {
    Flat(std::slice::Iter<'a, FactVersionRef>),
    Tree(fact_tree::Iter<'a>),
}

impl<'a> FactSetIter<'a> {
    /// The facts of a slice, in its own order: for a reader that takes
    /// either a set or a plain list of facts.
    #[must_use]
    pub fn from_slice(facts: &'a [FactVersionRef]) -> Self {
        Self(IterRepr::Flat(facts.iter()))
    }
}

impl<'a> Iterator for FactSetIter<'a> {
    type Item = &'a FactVersionRef;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.0 {
            IterRepr::Flat(iter) => iter.next(),
            IterRepr::Tree(iter) => iter.next(),
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        match &self.0 {
            IterRepr::Flat(iter) => iter.size_hint(),
            IterRepr::Tree(iter) => iter.size_hint(),
        }
    }
}

impl DoubleEndedIterator for FactSetIter<'_> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        match &mut self.0 {
            IterRepr::Flat(iter) => iter.next_back(),
            IterRepr::Tree(iter) => iter.next_back(),
        }
    }
}

impl ExactSizeIterator for FactSetIter<'_> {}

impl<'a> IntoIterator for &'a FactSet {
    type Item = &'a FactVersionRef;
    type IntoIter = FactSetIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl PartialEq for FactSet {
    fn eq(&self, other: &Self) -> bool {
        if self.ptr_eq(other) {
            return true;
        }
        if self.len() != other.len() {
            return false;
        }
        if let (Repr::Tree(a), Repr::Tree(b)) = (&self.0, &other.0) {
            if a.digest != b.digest {
                return false;
            }
        }
        self.iter().eq(other.iter())
    }
}

impl Eq for FactSet {}

impl PartialEq<[FactVersionRef]> for FactSet {
    fn eq(&self, other: &[FactVersionRef]) -> bool {
        self.len() == other.len() && self.iter().eq(other.iter())
    }
}

impl PartialEq<Vec<FactVersionRef>> for FactSet {
    fn eq(&self, other: &Vec<FactVersionRef>) -> bool {
        self == other.as_slice()
    }
}

impl<const N: usize> PartialEq<[FactVersionRef; N]> for FactSet {
    fn eq(&self, other: &[FactVersionRef; N]) -> bool {
        self == other.as_slice()
    }
}

impl PartialOrd for FactSet {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FactSet {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        if self.ptr_eq(other) {
            return std::cmp::Ordering::Equal;
        }
        self.iter().cmp(other.iter())
    }
}

/// Hashes the length then each fact in canonical order with the caller's
/// hasher, exactly as a sorted slice hashes (see the module laws).
impl Hash for FactSet {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.len().hash(state);
        for fact in self.iter() {
            fact.hash(state);
        }
    }
}

impl std::fmt::Debug for FactSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl From<Vec<FactVersionRef>> for FactSet {
    fn from(facts: Vec<FactVersionRef>) -> Self {
        Self::from_facts(facts)
    }
}

impl From<&[FactVersionRef]> for FactSet {
    fn from(facts: &[FactVersionRef]) -> Self {
        Self::from_facts(facts.to_vec())
    }
}

impl From<Box<[FactVersionRef]>> for FactSet {
    fn from(facts: Box<[FactVersionRef]>) -> Self {
        Self::from_facts(facts.into_vec())
    }
}

impl From<&Vec<FactVersionRef>> for FactSet {
    fn from(facts: &Vec<FactVersionRef>) -> Self {
        Self::from_facts(facts.clone())
    }
}

impl<const N: usize> From<[FactVersionRef; N]> for FactSet {
    fn from(facts: [FactVersionRef; N]) -> Self {
        Self::from_facts(facts.into())
    }
}

impl From<Arc<[FactVersionRef]>> for FactSet {
    fn from(facts: Arc<[FactVersionRef]>) -> Self {
        if facts.windows(2).all(|pair| pair[0] < pair[1]) {
            return Self(Repr::Flat(facts));
        }
        Self::from_facts(facts.to_vec())
    }
}

impl FromIterator<FactVersionRef> for FactSet {
    fn from_iter<I: IntoIterator<Item = FactVersionRef>>(iter: I) -> Self {
        Self::from_facts(iter.into_iter().collect())
    }
}

#[cfg(test)]
#[path = "fact_set_tests.rs"]
mod tests;
