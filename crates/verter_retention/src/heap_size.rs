//! Exhaustive heap measurement of a retained data tree.
//!
//! A cache retaining a plain data tree — a type expression, a lowered flow
//! slice, a compile product — pays for every allocation the tree keeps
//! alive. [`HeapSize`] walks the tree under the conventions of
//! [`super::allocation`]: a `String` / `Vec` / map costs its capacity, a
//! `Box` its block, each distinct raw `Arc` its block once per measurement,
//! and an owned handle ([`super::owned::Owned`], [`super::owned::OwnedStr`],
//! [`super::owned::OwnedSlice`]) nothing — it is offered to the
//! measurement's handle set and charged by its own cell.
//!
//! [`Tree`] makes such a tree the value of an owned cell: every clone of the
//! delivered handle shares the cell and its one charge, which covers the
//! whole tree and is released by the handle's last holder. The tree's raw
//! `Arc`s are counted inside that charge; they stay plain data the cell owns
//! (an owned cell's value otherwise reaches no raw `Arc`, see
//! [`super::owned`]).
//!
//! Hash maps follow the declared table convention [`hash_table_bytes`]:
//! one block holding a bucket array sized to the map's capacity and its
//! control bytes.
//!
//! `#[derive(HeapSize)]` measures every field of every variant.

use std::sync::Arc;

use rustc_hash::FxHashSet;

pub use verter_retention_derive::HeapSize;

use super::allocation::{arc_block_bytes, heap_block_bytes, AliasSet};
use super::owned::{Measured, Owned, OwnedSlice, OwnedStr};

/// A value whose heap can be measured exhaustively.
pub trait HeapSize {
    /// Heap this value keeps reachable beyond its inline bytes.
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize;
}

/// One node of a recursive tree, measured from an explicit work-list rather
/// than the call stack (`#[heap_size(iterative)]`): a deeply nested tree
/// costs heap, never stack.
pub trait IterativeNode {
    /// This node's own heap; each nested node of the same tree is deferred
    /// to the work-list as its `heap_size` is reached.
    fn node_heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize;
}

/// One measurement over a tree borrowed for `'t`: the raw `Arc` allocations
/// already counted, the handle set the owned allocations it reaches are
/// offered to, and the iterative nodes still to measure.
pub struct HeapMeter<'h, 't> {
    handles: &'h mut AliasSet,
    raw: FxHashSet<usize>,
    pending: Vec<&'t dyn IterativeNode>,
    walking: bool,
}

impl<'h, 't> HeapMeter<'h, 't> {
    /// A measurement offering owned handles to `handles`.
    pub fn new(handles: &'h mut AliasSet) -> Self {
        Self {
            handles,
            raw: FxHashSet::default(),
            pending: Vec::new(),
            walking: false,
        }
    }

    /// `true` the first time the raw allocation of `arc` is offered.
    pub fn first<T: ?Sized>(&mut self, arc: &Arc<T>) -> bool {
        self.raw.insert(Arc::as_ptr(arc).cast::<()>() as usize)
    }

    /// The handle set owned allocations are offered to.
    pub fn handles(&mut self) -> &mut AliasSet {
        self.handles
    }

    /// Measure `node` from the work-list: inside a walk it is queued and
    /// counted when popped; otherwise it starts the walk and drains it.
    pub fn iterate(&mut self, node: &'t dyn IterativeNode) -> usize {
        if self.walking {
            self.pending.push(node);
            return 0;
        }
        self.walking = true;
        let mut total = node.node_heap_size(self);
        while let Some(next) = self.pending.pop() {
            total += next.node_heap_size(self);
        }
        self.walking = false;
        total
    }
}

/// `value`'s heap measured on its own: the bytes and the owned allocations
/// it reaches, for an owner that charges the one and pins or holds the
/// other.
pub fn footprint(value: &impl HeapSize) -> super::Footprint {
    let mut handles = AliasSet::new();
    let bytes = value.heap_size(&mut HeapMeter::new(&mut handles));
    super::Footprint {
        bytes,
        handles: handles.take_handles(),
    }
}

impl<T: HeapSize + ?Sized> HeapSize for &T {
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        (**self).heap_size(meter)
    }
}

/// A field that owns no heap: `Copy` proves it, no heap owner is `Copy`.
pub fn inline_copy<T: Copy>(_value: &T) -> usize {
    0
}

/// Bytes one hash table holding `capacity` entries of `entry` bytes costs:
/// its bucket array (the smallest power of two keeping the load at most
/// seven eighths, at least four buckets) and one control byte per bucket
/// plus a probe group, in one block. An unallocated table costs nothing.
#[must_use]
pub const fn hash_table_bytes(capacity: usize, entry: usize) -> usize {
    if capacity == 0 {
        return 0;
    }
    let buckets = if capacity < 4 {
        4
    } else if capacity < 8 {
        8
    } else {
        (capacity * 8 / 7).next_power_of_two()
    };
    heap_block_bytes(buckets * entry + buckets + 16)
}

macro_rules! inline_heap_size {
    ($($t:ty),* $(,)?) => {
        $(impl HeapSize for $t {
            #[inline]
            fn heap_size<'t>(&'t self, _meter: &mut HeapMeter<'_, 't>) -> usize {
                0
            }
        })*
    };
}

inline_heap_size!(
    (),
    bool,
    char,
    u8,
    u16,
    u32,
    u64,
    u128,
    usize,
    i8,
    i16,
    i32,
    i64,
    i128,
    isize,
    f32,
    f64,
    std::num::NonZeroU32,
    std::num::NonZeroU64,
    std::num::NonZeroUsize,
);

impl<T: Copy, const N: usize> HeapSize for [T; N] {
    fn heap_size<'t>(&'t self, _meter: &mut HeapMeter<'_, 't>) -> usize {
        0
    }
}

impl<T: ?Sized> HeapSize for std::marker::PhantomData<T> {
    fn heap_size<'t>(&'t self, _meter: &mut HeapMeter<'_, 't>) -> usize {
        0
    }
}

impl HeapSize for String {
    fn heap_size<'t>(&'t self, _meter: &mut HeapMeter<'_, 't>) -> usize {
        heap_block_bytes(self.capacity())
    }
}

impl HeapSize for Box<str> {
    fn heap_size<'t>(&'t self, _meter: &mut HeapMeter<'_, 't>) -> usize {
        heap_block_bytes(self.len())
    }
}

impl<T: HeapSize> HeapSize for Box<T> {
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        heap_block_bytes(std::mem::size_of::<T>()) + (**self).heap_size(meter)
    }
}

impl<T: HeapSize> HeapSize for Box<[T]> {
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        heap_block_bytes(std::mem::size_of_val::<[T]>(self))
            + self.iter().map(|item| item.heap_size(meter)).sum::<usize>()
    }
}

impl<T: HeapSize> HeapSize for Vec<T> {
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        heap_block_bytes(self.capacity() * std::mem::size_of::<T>())
            + self.iter().map(|item| item.heap_size(meter)).sum::<usize>()
    }
}

impl<T: HeapSize> HeapSize for Option<T> {
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        self.as_ref().map_or(0, |value| value.heap_size(meter))
    }
}

impl<T: HeapSize, E: HeapSize> HeapSize for Result<T, E> {
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        match self {
            Ok(value) => value.heap_size(meter),
            Err(error) => error.heap_size(meter),
        }
    }
}

impl<A: HeapSize, B: HeapSize> HeapSize for (A, B) {
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        self.0.heap_size(meter) + self.1.heap_size(meter)
    }
}

impl<A: HeapSize, B: HeapSize, C: HeapSize> HeapSize for (A, B, C) {
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        self.0.heap_size(meter) + self.1.heap_size(meter) + self.2.heap_size(meter)
    }
}

impl HeapSize for Arc<str> {
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        if meter.first(self) {
            arc_block_bytes(self.len())
        } else {
            0
        }
    }
}

impl<T: HeapSize> HeapSize for Arc<[T]> {
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        if !meter.first(self) {
            return 0;
        }
        arc_block_bytes(std::mem::size_of_val::<[T]>(self))
            + self.iter().map(|item| item.heap_size(meter)).sum::<usize>()
    }
}

impl<T: HeapSize> HeapSize for Arc<T> {
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        if !meter.first(self) {
            return 0;
        }
        arc_block_bytes(std::mem::size_of::<T>()) + (**self).heap_size(meter)
    }
}

impl<K: HeapSize, V: HeapSize, S> HeapSize for std::collections::HashMap<K, V, S> {
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        hash_table_bytes(self.capacity(), std::mem::size_of::<(K, V)>())
            + self
                .iter()
                .map(|(key, value)| key.heap_size(meter) + value.heap_size(meter))
                .sum::<usize>()
    }
}

impl<T: HeapSize, S> HeapSize for std::collections::HashSet<T, S> {
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        hash_table_bytes(self.capacity(), std::mem::size_of::<T>())
            + self.iter().map(|item| item.heap_size(meter)).sum::<usize>()
    }
}

impl<A: smallvec::Array> HeapSize for smallvec::SmallVec<A>
where
    A::Item: HeapSize,
{
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        let spilled = if self.spilled() {
            heap_block_bytes(self.capacity() * std::mem::size_of::<A::Item>())
        } else {
            0
        };
        spilled + self.iter().map(|item| item.heap_size(meter)).sum::<usize>()
    }
}

impl HeapSize for OwnedStr {
    /// Charged by its own cell.
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        self.offer(meter.handles())
    }
}

impl<T: Measured> HeapSize for OwnedSlice<T> {
    /// Charged by its own cell.
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        self.offer(meter.handles())
    }
}

impl<T: Measured> HeapSize for Owned<T> {
    /// Charged by its own cell.
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        self.offer(meter.handles())
    }
}

/// A data tree held as an owned cell's value (see the module docs).
#[derive(PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Tree<T>(T);

/// A self-charged allocation holding a data tree.
pub type OwnedTree<T> = Owned<Tree<T>>;

/// A new self-charged allocation of `value`, charged by the first owner
/// that keeps it.
pub fn owned_tree<T: HeapSize + Send + Sync + 'static>(value: T) -> OwnedTree<T> {
    Owned::new(Tree(value))
}

impl<T> std::ops::Deref for Tree<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> AsRef<T> for Tree<T> {
    fn as_ref(&self) -> &T {
        &self.0
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Tree<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl<T: HeapSize + Send + Sync + 'static> Measured for Tree<T> {
    fn heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        self.0.heap_size(&mut HeapMeter::new(aliases))
    }
}

#[cfg(test)]
#[path = "heap_size_tests.rs"]
mod tests;
