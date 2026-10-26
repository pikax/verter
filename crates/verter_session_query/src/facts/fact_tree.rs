//! The persistent B+tree a wide [`super::fact_set::FactSet`] is held in.
//!
//! Facts sit inline in the leaves, in canonical order; a branch holds only
//! its children. A set built from another copies the path its insertions
//! and removals touch and shares every other node, so a chain of sets each
//! one fact wider than the last holds one path per link, never a copy of
//! the chain.
//!
//! ## Retention: every node charges exactly its own allocation
//!
//! Each node carries its own [`RetentionCharge`], pinned on the process
//! retention account when the edit that built it seals
//! ([`seal`]), and released by the node's own drop: exactly when its
//! allocation is freed, however many sets shared it. The charge is the
//! node's block ([`node_bytes`]): its `Arc` block, its vector's backing by
//! capacity, and — for a leaf — the heap its facts own beyond their slots,
//! measured once per leaf with the alias-aware fact measurement (a text the
//! leaf's facts share is counted once; a text several leaves share is
//! counted by each, the declared per-carrier convention: a bounded
//! overcharge, never an undercharge). Nothing is estimated: the node's own
//! allocations are measured from the node itself, under the allocation
//! convention every charge uses ([`arc_block_bytes`], [`heap_block_bytes`]).
//!
//! A retaining admission claims the nodes a set reaches that no earlier
//! admission claimed ([`unclaimed_nodes`]): their pins are exchanged for
//! shares of its refusable reservation, together with the consumed-result
//! receipts it reaches. A claimed node's whole subtree is claimed (children are always
//! older than their parents, and a node is marked claimed only after its
//! subtree), so a claim walk stops at the first claimed node and costs only
//! what the set built.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::version::FactVersionRef;
use crate::retention::allocation::{arc_block_bytes, heap_block_bytes};
use crate::retention::{RetentionCharge, SemanticRetentionAccount};

/// Facts one leaf holds at most.
pub(crate) const LEAF_CAP: usize = 16;

/// Children one branch holds at most.
pub(crate) const BRANCH_CAP: usize = 16;

/// One node of a fact tree. See the module docs.
pub(crate) struct Node {
    /// Facts in this node's subtree.
    len: usize,
    body: Body,
    /// `None` while the edit building the node may still change it in
    /// place; `Some` once sealed, from then on immutable and shared.
    charge: parking_lot::Mutex<Option<RetentionCharge>>,
    /// Set once a retaining admission claimed this node and every node
    /// below it.
    claimed: AtomicBool,
}

enum Body {
    Leaf(Vec<FactVersionRef>),
    Branch(Vec<Arc<Node>>),
}

impl Node {
    fn leaf(facts: Vec<FactVersionRef>) -> Arc<Node> {
        Arc::new(Node {
            len: facts.len(),
            body: Body::Leaf(facts),
            charge: parking_lot::Mutex::new(None),
            claimed: AtomicBool::new(false),
        })
    }

    fn branch(children: Vec<Arc<Node>>) -> Arc<Node> {
        Arc::new(Node {
            len: children.iter().map(|child| child.len).sum(),
            body: Body::Branch(children),
            charge: parking_lot::Mutex::new(None),
            claimed: AtomicBool::new(false),
        })
    }

    /// How many facts this subtree holds.
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// The least fact of this subtree. Never empty: an emptied node is
    /// removed from its parent.
    fn first(&self) -> &FactVersionRef {
        let mut node = self;
        loop {
            match &node.body {
                Body::Leaf(facts) => return &facts[0],
                Body::Branch(children) => node = &children[0],
            }
        }
    }

    /// The greatest fact of this subtree.
    fn last(&self) -> &FactVersionRef {
        let mut node = self;
        loop {
            match &node.body {
                Body::Leaf(facts) => return &facts[facts.len() - 1],
                Body::Branch(children) => node = &children[children.len() - 1],
            }
        }
    }

    /// The same node, unsealed, sharing its children (or copying its facts).
    /// A leaf's copy has room for `extra` more facts, so an insertion's copy
    /// is allocated once, at the length it seals at, never grown and trimmed
    /// again; a branch grows only when a child splits.
    fn shallow_copy(&self, extra: usize) -> Node {
        fn copy<T: Clone>(items: &[T], extra: usize) -> Vec<T> {
            let mut copy = Vec::with_capacity(items.len() + extra);
            copy.extend_from_slice(items);
            copy
        }
        Node {
            len: self.len,
            body: match &self.body {
                Body::Leaf(facts) => Body::Leaf(copy(facts, extra)),
                Body::Branch(children) => Body::Branch(copy(children, 0)),
            },
            charge: parking_lot::Mutex::new(None),
            claimed: AtomicBool::new(false),
        }
    }

    /// Whether this node's charge was claimed into a retained reservation,
    /// with every node below it.
    pub(crate) fn is_claimed(&self) -> bool {
        self.claimed.load(Ordering::Acquire)
    }

    /// Bytes this node's charge holds, and whether a retaining admission
    /// claimed them (zero and unclaimed while unsealed).
    pub(crate) fn charge_state(&self) -> (usize, bool) {
        self.charge.lock().as_ref().map_or((0, false), |charge| {
            (
                charge.bytes(),
                charge.class() == crate::retention::ChargeClass::Retained,
            )
        })
    }

    /// Bytes this node's charge holds (zero while unsealed).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn charged_bytes(&self) -> usize {
        self.charge
            .lock()
            .as_ref()
            .map_or(0, RetentionCharge::bytes)
    }

    /// The children of a branch; empty for a leaf.
    pub(crate) fn children(&self) -> &[Arc<Node>] {
        match &self.body {
            Body::Leaf(_) => &[],
            Body::Branch(children) => children,
        }
    }

    pub(crate) fn charge(&self) -> &parking_lot::Mutex<Option<RetentionCharge>> {
        &self.charge
    }

    pub(crate) fn mark_claimed(&self) {
        self.claimed.store(true, Ordering::Release);
    }
}

/// The index of the child of `children` whose range holds `fact`: the last
/// child whose least fact is at most `fact` (the first when none is).
fn child_index(children: &[Arc<Node>], fact: &FactVersionRef) -> usize {
    let mut lo = 0;
    let mut hi = children.len();
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        super::fact_read_set::record_signature_work(1);
        if children[mid].first() <= fact {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

/// Cut `items` into `ceil(len / cap)` runs whose lengths differ by at most
/// one, so every node of a built level is as full as the level allows. Each
/// run is allocated once, at its length.
fn even_runs<T: Clone>(items: &[T], cap: usize) -> impl Iterator<Item = Vec<T>> + '_ {
    let count = items.len().div_ceil(cap);
    let mut rest = items;
    (0..count).map(move |index| {
        let (run, tail) = rest.split_at(rest.len().div_ceil(count - index));
        rest = tail;
        run.to_vec()
    })
}

/// A tree of the strictly increasing `facts`, unsealed; `None` when empty.
pub(crate) fn build(facts: &[FactVersionRef]) -> Option<Arc<Node>> {
    if facts.is_empty() {
        return None;
    }
    let mut level: Vec<Arc<Node>> = even_runs(facts, LEAF_CAP).map(Node::leaf).collect();
    while level.len() > 1 {
        level = even_runs(&level, BRANCH_CAP).map(Node::branch).collect();
    }
    level.pop()
}

/// Whether the tree under `node` holds `fact`.
pub(crate) fn contains(node: &Node, fact: &FactVersionRef) -> bool {
    let mut node = node;
    loop {
        match &node.body {
            Body::Leaf(facts) => {
                super::fact_read_set::record_signature_work(1);
                return facts.binary_search(fact).is_ok();
            }
            Body::Branch(children) => node = &children[child_index(children, fact)],
        }
    }
}

/// The node in `slot`, mutable: in place when this edit built it and holds
/// it alone, otherwise a fresh unsealed copy, with room for `extra` more
/// entries, put in its place.
fn make_mut(slot: &mut Arc<Node>, extra: usize) -> &mut Node {
    let in_place = match Arc::get_mut(slot) {
        Some(node) => node.charge.get_mut().is_none(),
        None => false,
    };
    if !in_place {
        *slot = Arc::new(slot.shallow_copy(extra));
    }
    Arc::get_mut(slot).expect("an unsealed copy is held alone")
}

/// Insert `fact`, absent from the tree under `slot`. Answers the right
/// half when the node split.
fn insert_into(slot: &mut Arc<Node>, fact: FactVersionRef) -> Option<Arc<Node>> {
    if let Body::Leaf(facts) = &slot.body {
        if facts.len() >= LEAF_CAP {
            let (left, right) = split_leaf(facts, fact);
            // A leaf this edit built and holds alone keeps its node; a
            // shared one is replaced by a fresh left half.
            match Arc::get_mut(slot) {
                Some(node) if node.charge.lock().is_none() => {
                    node.len = left.len();
                    node.body = Body::Leaf(left);
                }
                _ => *slot = Node::leaf(left),
            }
            return Some(Node::leaf(right));
        }
    }
    let node = make_mut(slot, 1);
    node.len += 1;
    match &mut node.body {
        Body::Leaf(facts) => {
            let at = facts.partition_point(|held| *held < fact);
            facts.insert(at, fact);
            None
        }
        Body::Branch(children) => {
            let at = child_index(children, &fact);
            let split = insert_into(&mut children[at], fact)?;
            // A branch grows by exactly the one child, the length it seals at.
            children.reserve_exact(1);
            children.insert(at + 1, split);
            if children.len() > BRANCH_CAP {
                let right = children.split_off(children.len() / 2);
                node.len = children.iter().map(|child| child.len).sum();
                return Some(Node::branch(right));
            }
            None
        }
    }
}

/// The two halves a full leaf's `facts` and the inserted `fact` split
/// into, each allocated once at the length it seals at.
fn split_leaf(
    facts: &[FactVersionRef],
    fact: FactVersionRef,
) -> (Vec<FactVersionRef>, Vec<FactVersionRef>) {
    let at = facts.partition_point(|held| *held < fact);
    let total = facts.len() + 1;
    let left_len = total / 2;
    let mut left = Vec::with_capacity(left_len);
    let mut right = Vec::with_capacity(total - left_len);
    let mut fact = Some(fact);
    let mut push = |item: FactVersionRef| {
        if left.len() < left_len {
            left.push(item);
        } else {
            right.push(item);
        }
    };
    for (index, held) in facts.iter().enumerate() {
        if index == at {
            push(fact.take().expect("inserted once"));
        }
        push(held.clone());
    }
    if let Some(fact) = fact {
        push(fact);
    }
    (left, right)
}

/// Insert `fact`, absent from the tree.
pub(crate) fn insert(root: &mut Option<Arc<Node>>, fact: FactVersionRef) {
    match root {
        None => *root = Some(Node::leaf(vec![fact])),
        Some(slot) => {
            if let Some(right) = insert_into(slot, fact) {
                let left = slot.clone();
                *slot = Node::branch(vec![left, right]);
            }
        }
    }
}

/// Remove `fact`, present in the tree under `slot`.
fn remove_from(slot: &mut Arc<Node>, fact: &FactVersionRef) {
    let node = make_mut(slot, 0);
    node.len -= 1;
    match &mut node.body {
        Body::Leaf(facts) => {
            if let Ok(at) = facts.binary_search(fact) {
                facts.remove(at);
            }
        }
        Body::Branch(children) => {
            let at = child_index(children, fact);
            remove_from(&mut children[at], fact);
            if children[at].len == 0 {
                children.remove(at);
            }
        }
    }
}

/// Remove `fact`, present in the tree. A node emptied is dropped from its
/// parent and a root left with one child is replaced by it, so the tree
/// never holds an empty node; nodes left underfull are kept (removals are
/// rare and never deepen the tree).
pub(crate) fn remove(root: &mut Option<Arc<Node>>, fact: &FactVersionRef) {
    let Some(slot) = root else {
        return;
    };
    remove_from(slot, fact);
    if slot.len == 0 {
        *root = None;
        return;
    }
    loop {
        let only_child = match &slot.body {
            Body::Branch(children) if children.len() == 1 => children[0].clone(),
            _ => break,
        };
        *slot = only_child;
    }
}

/// The bytes `node`'s own allocations cost, with the owned names its facts
/// reach pinned (a leaf's facts are measured as one value, on a set of the
/// leaf's own). The node's children are their own allocations.
pub(crate) fn node_bytes(node: &Node, pin_names: bool) -> usize {
    use crate::retention::allocation::{AliasSet, FactMeasure};
    let own = arc_block_bytes(std::mem::size_of::<Node>());
    match &node.body {
        Body::Leaf(facts) => {
            let mut aliases = AliasSet::new();
            let heap = FactMeasure::new(&mut aliases).facts(facts.iter());
            if pin_names {
                aliases
                    .take_handles()
                    .pin(crate::retention::owned::ChargeCtx::new(
                        &SemanticRetentionAccount::process_local(),
                    ));
            }
            own + heap_block_bytes(facts.capacity() * std::mem::size_of::<FactVersionRef>()) + heap
        }
        Body::Branch(children) => {
            own + heap_block_bytes(children.capacity() * std::mem::size_of::<Arc<Node>>())
        }
    }
}

/// The owned names a leaf's facts reach, each once, for a claim to hold
/// with the leaf's share.
pub(crate) fn node_names(node: &Node) -> crate::retention::owned::OwnedHandles {
    use crate::retention::allocation::{AliasSet, FactMeasure};
    let mut aliases = AliasSet::new();
    if let Body::Leaf(facts) = &node.body {
        let _raw = FactMeasure::new(&mut aliases).facts(facts.iter());
    }
    aliases.take_handles()
}

/// Seal every node the edit built: trim each to its length, then pin
/// exactly its own bytes. A node another set already shares is sealed
/// already, and so is everything below it.
pub(crate) fn seal(slot: &mut Arc<Node>) {
    let Some(node) = Arc::get_mut(slot) else {
        return;
    };
    if node.charge.get_mut().is_some() {
        return;
    }
    match &mut node.body {
        Body::Leaf(facts) => facts.shrink_to_fit(),
        Body::Branch(children) => {
            for child in children.iter_mut() {
                seal(child);
            }
            children.shrink_to_fit();
        }
    }
    let bytes = node_bytes(node, true);
    *node.charge.get_mut() = Some(SemanticRetentionAccount::process_local().pin(bytes));
}

/// Every node under `root` no earlier admission claimed, each once:
/// the walk stops at a claimed node, whose subtree is claimed.
/// `seen` holds every node already collected (by address), so sets that
/// share storage are walked once in total.
pub(crate) fn unclaimed_nodes<'a>(
    root: &'a Arc<Node>,
    out: &mut Vec<&'a Arc<Node>>,
    seen: &mut rustc_hash::FxHashSet<*const Node>,
) {
    let mut stack: smallvec::SmallVec<[&'a Arc<Node>; 16]> = smallvec::smallvec![root];
    while let Some(node) = stack.pop() {
        if node.is_claimed() || !seen.insert(Arc::as_ptr(node)) {
            continue;
        }
        out.push(node);
        stack.extend(node.children().iter());
    }
}

/// The least fact of the tree.
pub(crate) fn first(root: &Node) -> &FactVersionRef {
    root.first()
}

/// The greatest fact of the tree.
pub(crate) fn last(root: &Node) -> &FactVersionRef {
    root.last()
}

/// A double-ended iterator over a tree's facts in canonical order.
pub struct Iter<'a> {
    /// Path to the next fact from the front: each branch with the index of
    /// the child being read, the leaf with the index of its next fact.
    front: smallvec::SmallVec<[(&'a Node, usize); 6]>,
    /// Path to the next fact from the back: each branch with the index of
    /// the child being read, the leaf with the count of facts left in it.
    back: smallvec::SmallVec<[(&'a Node, usize); 6]>,
    remaining: usize,
}

impl<'a> Iter<'a> {
    /// Every fact of the tree under `root`.
    pub(crate) fn new(root: &'a Node) -> Self {
        let mut iter = Self {
            front: smallvec::SmallVec::new(),
            back: smallvec::SmallVec::new(),
            remaining: root.len,
        };
        descend_left(&mut iter.front, root);
        descend_right(&mut iter.back, root);
        iter
    }

    /// The facts of the tree under `root` from `from` (inclusive) on.
    pub(crate) fn from(root: &'a Node, from: &FactVersionRef) -> Self {
        let mut front: smallvec::SmallVec<[(&'a Node, usize); 6]> = smallvec::SmallVec::new();
        let mut before = 0;
        let mut node = root;
        loop {
            match &node.body {
                Body::Leaf(facts) => {
                    let at = facts.partition_point(|fact| fact < from);
                    before += at;
                    front.push((node, at));
                    break;
                }
                Body::Branch(children) => {
                    let at = child_index(children, from);
                    before += children[..at].iter().map(|child| child.len).sum::<usize>();
                    front.push((node, at));
                    node = &children[at];
                }
            }
        }
        let mut back = smallvec::SmallVec::new();
        descend_right(&mut back, root);
        Self {
            front,
            back,
            remaining: root.len - before,
        }
    }
}

fn descend_left<'a>(stack: &mut smallvec::SmallVec<[(&'a Node, usize); 6]>, node: &'a Node) {
    let mut node = node;
    loop {
        stack.push((node, 0));
        match &node.body {
            Body::Leaf(_) => return,
            Body::Branch(children) => node = &children[0],
        }
    }
}

fn descend_right<'a>(stack: &mut smallvec::SmallVec<[(&'a Node, usize); 6]>, node: &'a Node) {
    let mut node = node;
    loop {
        match &node.body {
            Body::Leaf(facts) => {
                stack.push((node, facts.len()));
                return;
            }
            Body::Branch(children) => {
                stack.push((node, children.len() - 1));
                node = &children[children.len() - 1];
            }
        }
    }
}

impl<'a> Iterator for Iter<'a> {
    type Item = &'a FactVersionRef;

    fn next(&mut self) -> Option<&'a FactVersionRef> {
        if self.remaining == 0 {
            return None;
        }
        loop {
            let (node, at) = *self.front.last()?;
            if let Body::Leaf(facts) = &node.body {
                if at < facts.len() {
                    self.front.last_mut().expect("non-empty").1 += 1;
                    self.remaining -= 1;
                    return Some(&facts[at]);
                }
            }
            // This node is read: move to the next child of the nearest
            // ancestor that has one.
            self.front.pop();
            loop {
                let (parent, child) = self.front.last_mut()?;
                *child += 1;
                let (parent, child) = (*parent, *child);
                match &parent.body {
                    Body::Branch(children) if child < children.len() => {
                        descend_left(&mut self.front, &children[child]);
                        break;
                    }
                    _ => {
                        self.front.pop();
                    }
                }
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<'a> DoubleEndedIterator for Iter<'a> {
    fn next_back(&mut self) -> Option<&'a FactVersionRef> {
        if self.remaining == 0 {
            return None;
        }
        loop {
            let (node, left) = *self.back.last()?;
            if let Body::Leaf(facts) = &node.body {
                if left > 0 {
                    self.back.last_mut().expect("non-empty").1 -= 1;
                    self.remaining -= 1;
                    return Some(&facts[left - 1]);
                }
            }
            self.back.pop();
            loop {
                let (parent, child) = self.back.last_mut()?;
                if *child == 0 {
                    self.back.pop();
                    continue;
                }
                *child -= 1;
                let (parent, child) = (*parent, *child);
                if let Body::Branch(children) = &parent.body {
                    descend_right(&mut self.back, &children[child]);
                }
                break;
            }
        }
    }
}

impl ExactSizeIterator for Iter<'_> {}
