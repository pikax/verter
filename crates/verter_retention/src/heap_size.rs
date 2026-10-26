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

use super::pinned_heap::PinContext;
use super::shared::CallerHold;

pub use verter_retention_derive::HeapSize;

use super::allocation::{arc_block_bytes, heap_block_bytes, AliasSet};
use super::owned::{Measured, Owned, OwnedSlice, OwnedStr};

/// A value whose heap can be measured exhaustively.
pub trait HeapSize {
    /// Heap this value keeps reachable beyond its inline bytes.
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize;

    /// Heap owned by slice elements, excluding the slice's backing block.
    /// Implementations may summarize inline-only elements without visiting
    /// each one; owning elements must still measure their heap and offer
    /// every owned handle to the meter.
    fn heap_size_of_slice<'t>(values: &'t [Self], meter: &mut HeapMeter<'_, 't>) -> usize
    where
        Self: Sized,
    {
        values.iter().map(|value| value.heap_size(meter)).sum()
    }

    /// A mutable storage owner carries its own pin, including its outer
    /// allocation. Immutable parents then pay only for the owning handle.
    fn pin_storage(&self, _context: &crate::pinned_heap::PinContext) -> bool {
        false
    }
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
    raw: AliasSet,
    pending: Vec<(&'t dyn IterativeNode, Option<usize>)>,
    walking: bool,
    pin_context: Option<&'h PinContext>,
    pins: Vec<CallerHold>,
    allocation: Option<usize>,
    allocations: Vec<PendingAllocation<'t>>,
    #[cfg(test)]
    inline_element_visits: usize,
}

type AllocationRegistration<'t> = dyn FnOnce(&PinContext, usize) -> CallerHold + 't;

struct PendingAllocation<'t> {
    bytes: usize,
    register: Box<AllocationRegistration<'t>>,
}

impl<'h, 't> HeapMeter<'h, 't> {
    /// A measurement offering owned handles to `handles`.
    pub fn new(handles: &'h mut AliasSet) -> Self {
        Self {
            handles,
            raw: AliasSet::new(),
            pending: Vec::new(),
            walking: false,
            pin_context: None,
            pins: Vec::new(),
            allocation: None,
            allocations: Vec::new(),
            #[cfg(test)]
            inline_element_visits: 0,
        }
    }

    pub(crate) fn pinning(handles: &'h mut AliasSet, context: &'h PinContext) -> Self {
        Self {
            pin_context: Some(context),
            ..Self::new(handles)
        }
    }

    /// Measure a foreign shared allocation using its owner's exhaustive
    /// walker. In a pinned measurement it is charged once across owners.
    pub fn shared_arc<T: ?Sized + Send + Sync + 'static>(
        &mut self,
        value: &'t Arc<T>,
        measure: impl FnOnce(&'t T, &mut HeapMeter<'_, 't>) -> usize,
    ) -> usize {
        self.try_shared_arc(value, measure).unwrap_or(0)
    }

    /// As [`Self::shared_arc`], distinguishing an already visited block so
    /// an iterative shared chain can stop at its previously measured suffix.
    pub fn try_shared_arc<T: ?Sized + Send + Sync + 'static>(
        &mut self,
        value: &'t Arc<T>,
        measure: impl FnOnce(&'t T, &mut HeapMeter<'_, 't>) -> usize,
    ) -> Option<usize> {
        if !self.first(value) {
            return None;
        }
        Some(self.measure_shared_arc(value, measure))
    }

    /// Measure an allocation already recorded in this walk's visited set.
    fn measure_shared_arc<T: ?Sized + Send + Sync + 'static>(
        &mut self,
        value: &'t Arc<T>,
        measure: impl FnOnce(&'t T, &mut HeapMeter<'_, 't>) -> usize,
    ) -> usize {
        let inline = arc_block_bytes(std::mem::size_of_val(&**value));
        if self.pin_context.is_none() {
            return inline + measure(value, self);
        }
        let index = self.allocations.len();
        self.allocations.push(PendingAllocation {
            bytes: inline,
            register: Box::new(move |context, bytes| {
                context
                    .aliases
                    .pin_caller(context.account(), value, |_| bytes)
            }),
        });
        let previous = self.allocation.replace(index);
        let bytes = measure(value, self);
        self.allocations[index].bytes += bytes;
        self.allocation = previous;
        0
    }

    /// Settle registrations after the single iterative walk has attributed
    /// every deferred node to the allocation that owns its backing.
    pub(crate) fn finish_pins(mut self) -> Vec<CallerHold> {
        if let Some(context) = self.pin_context {
            for allocation in self.allocations {
                let hold = (allocation.register)(context, allocation.bytes);
                self.pins.push(hold);
            }
        }
        self.pins
    }

    /// `true` the first time the raw allocation of `arc` is offered.
    ///
    /// Only a data tree's own cell ([`Tree`]) owns raw `Arc`s; any other
    /// owned cell's value reaches none (see [`super::owned`]), so a debug
    /// build fails the measurement of such a value that reaches one.
    pub fn first<T: ?Sized>(&mut self, arc: &Arc<T>) -> bool {
        verter_debug_assert::verter_debug_assert!(
            !self.handles.measures_cell(),
            "an owned value reaches a raw `Arc<{}>`: an allocation a caller can clone out of a \
             delivered value must be an owned handle charged by itself",
            std::any::type_name::<T>()
        );
        self.raw.first_allocation(arc)
    }

    /// The handle set owned allocations are offered to.
    pub fn handles(&mut self) -> &mut AliasSet {
        self.handles
    }

    /// Bytes a raw `Arc<str>` of the tree keeps alive: its block the first
    /// time the measurement reaches it, nothing after, and nothing for the
    /// shared empty text ([`crate::allocation::is_shared_empty_str`]).
    pub fn arc_str_bytes(&mut self, value: &Arc<str>) -> usize {
        if crate::allocation::is_shared_empty_str(value) || !self.first(value) {
            0
        } else if let Some(context) = self.pin_context {
            let hold = context.aliases.pin_leaf(context.account(), value);
            self.pins.push(hold);
            0
        } else {
            arc_block_bytes(value.len())
        }
    }

    /// Measure `node` from the work-list: inside a walk it is queued and
    /// counted when popped; otherwise it starts the walk and drains it.
    pub fn iterate(&mut self, node: &'t dyn IterativeNode) -> usize {
        if self.walking {
            self.pending.push((node, self.allocation));
            return 0;
        }
        self.walking = true;
        let previous = self.allocation;
        let mut total = 0;
        self.pending.push((node, previous));
        while let Some((next, owner)) = self.pending.pop() {
            self.allocation = owner;
            let bytes = next.node_heap_size(self);
            if let Some(owner) = owner {
                self.allocations[owner].bytes += bytes;
            } else {
                total += bytes;
            }
        }
        self.allocation = previous;
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
                #[cfg(test)]
                {
                    _meter.inline_element_visits += 1;
                }
                0
            }

            #[inline]
            fn heap_size_of_slice<'t>(_: &'t [Self], _meter: &mut HeapMeter<'_, 't>) -> usize {
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
    verter_span::Span,
);

impl<T: Copy, const N: usize> HeapSize for [T; N] {
    fn heap_size<'t>(&'t self, _meter: &mut HeapMeter<'_, 't>) -> usize {
        #[cfg(test)]
        {
            _meter.inline_element_visits += 1;
        }
        0
    }

    #[inline]
    fn heap_size_of_slice<'t>(_: &'t [Self], _meter: &mut HeapMeter<'_, 't>) -> usize {
        0
    }
}

impl<T: ?Sized> HeapSize for std::marker::PhantomData<T> {
    fn heap_size<'t>(&'t self, _meter: &mut HeapMeter<'_, 't>) -> usize {
        0
    }
}

impl HeapSize for str {
    /// A borrowed `str` (a `&'static` label) owns nothing.
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
        heap_block_bytes(std::mem::size_of_val::<[T]>(self)) + T::heap_size_of_slice(self, meter)
    }
}

impl<T: HeapSize> HeapSize for Vec<T> {
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        heap_block_bytes(self.capacity() * std::mem::size_of::<T>())
            + T::heap_size_of_slice(self, meter)
    }
}

impl<T: HeapSize> HeapSize for Option<T> {
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        self.as_ref().map_or(0, |value| value.heap_size(meter))
    }
}

impl<T: HeapSize> HeapSize for std::sync::OnceLock<T> {
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        self.get().map_or(0, |value| value.heap_size(meter))
    }
}

impl<T: HeapSize> HeapSize for std::ops::Range<T> {
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        self.start.heap_size(meter) + self.end.heap_size(meter)
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
        meter.arc_str_bytes(self)
    }
}

impl<T: HeapSize + Send + Sync + 'static> HeapSize for Arc<[T]> {
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        meter.shared_arc(self, |items, meter| T::heap_size_of_slice(items, meter))
    }
}

impl<T: HeapSize + Send + Sync + 'static> HeapSize for Arc<T> {
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        if !meter.first(self) {
            return 0;
        }
        if let Some(context) = meter.pin_context {
            if (**self).pin_storage(context) {
                return 0;
            }
        }
        meter.measure_shared_arc(self, |value, meter| value.heap_size(meter))
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

impl HeapSize for super::RetentionCharge {
    /// A charge token: the bytes it holds are the account's, not its
    /// holder's heap.
    fn heap_size<'t>(&'t self, _meter: &mut HeapMeter<'_, 't>) -> usize {
        0
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

impl HeapSize for verter_audit::RequestTimingAudit {
    /// Wall-clock figures only: no heap.
    fn heap_size<'t>(&'t self, _meter: &mut HeapMeter<'_, 't>) -> usize {
        let Self {
            total_ms: _,
            capture_inputs_ms: _,
            store_read_ms: _,
            store_merge_ms: _,
            direct_import_proof_ms: _,
            imported_root_proof_ms: _,
            solver_ms: _,
            materialize_ms: _,
            serialize_ms: _,
            request_critical_path_ms: _,
        } = self;
        0
    }
}

impl HeapSize for verter_audit::ComponentMetaPayload {
    /// Its counters are inline; its diagnostic list and their messages are
    /// its heap.
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        let Self {
            total_resolve_steps: _,
            solve_count: _,
            materialize_structure_calls: _,
            materialize_structure_cache_hits: _,
            node_arena_lock_acquisitions: _,
            family_map_lock_acquisitions: _,
            dep_signature_merges: _,
            dep_signature_intern_hits: _,
            diagnostics,
            should_suppress: _,
            expanded_instantiate_calls: _,
            synthesis_expanded_instantiate_calls: _,
            memo_insertions: _,
            memo_publish_suppressed: _,
        } = self;
        diagnostics.heap_size(meter)
    }
}

impl HeapSize for verter_audit::AuditDiagnosticEntry {
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        let Self {
            kind: _,
            message,
            span: _,
            macro_index: _,
        } = self;
        message.heap_size(meter)
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

impl<T> Tree<T> {
    /// The tree, out of its cell's value ([`Owned::try_unwrap`]).
    pub fn into_inner(self) -> T {
        self.0
    }
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
    /// The tree's cell owns the raw `Arc`s the tree keeps (see the module
    /// docs), so they are counted on a set of the tree's own; the owned
    /// handles the tree reaches pass to the cell's set.
    fn heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        let mut tree = AliasSet::new();
        let bytes = self.0.heap_size(&mut HeapMeter::new(&mut tree));
        bytes + aliases.absorb(&mut tree)
    }
}

/// A cached value a cache hands its readers: a self-charged allocation
/// holding a data tree ([`OwnedTree`]) that derefs straight to the value.
///
/// Every reader's clone shares the one cell and its one charge, so a value
/// a reader keeps after the cache let go stays charged — pinned — until the
/// reader's own last clone drops, and no accessor exposes the raw `Arc`.
pub struct OwnedValue<T: HeapSize + Send + Sync + 'static>(OwnedTree<T>);

impl<T: HeapSize + Send + Sync + 'static> OwnedValue<T> {
    /// A new allocation of `value`, charged by the first owner that keeps
    /// it.
    #[must_use]
    pub fn new(value: T) -> Self {
        Self(owned_tree(value))
    }

    /// Offer this nested handle to a measurement (see [`Owned::offer`]).
    pub fn offer(&self, aliases: &mut AliasSet) -> usize {
        self.0.offer(aliases)
    }

    /// Charge it to its callers if no owner has yet (see [`Owned::pin`]).
    pub fn pin(&self, ctx: super::owned::ChargeCtx<'_>) {
        self.0.pin(ctx);
    }

    /// A cache's hold on it (see [`Owned::hold`]).
    ///
    /// # Errors
    ///
    /// The account's refusal: nothing is retained.
    pub fn hold(
        &self,
        ctx: super::owned::ChargeCtx<'_>,
    ) -> Result<super::owned::OwnedHold, super::RetentionRefusal> {
        self.0.hold(ctx)
    }

    /// Bytes its charge holds; `None` while no owner has charged it.
    #[must_use]
    pub fn charge_bytes(&self) -> Option<usize> {
        self.0.charge_bytes()
    }

    /// Its charge's class; `None` while no owner has charged it.
    #[must_use]
    pub fn charge_class(&self) -> Option<super::ChargeClass> {
        self.0.charge_class()
    }

    /// Whether two handles share one allocation.
    #[must_use]
    pub fn ptr_eq(this: &Self, other: &Self) -> bool {
        Owned::ptr_eq(&this.0, &other.0)
    }

    /// Owning handles of this allocation.
    #[must_use]
    pub fn strong_count(this: &Self) -> usize {
        Owned::strong_count(&this.0)
    }

    /// Bytes the cell's block holds beside the value's heap.
    #[must_use]
    pub fn cell_block_bytes() -> usize {
        OwnedTree::<T>::cell_block_bytes()
    }
}

impl<T: HeapSize + Send + Sync + 'static> Clone for OwnedValue<T> {
    /// Another owner of the same allocation: shares its one charge.
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: HeapSize + Send + Sync + 'static> std::ops::Deref for OwnedValue<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T: HeapSize + Send + Sync + 'static> AsRef<T> for OwnedValue<T> {
    fn as_ref(&self) -> &T {
        self
    }
}

impl<T: HeapSize + Send + Sync + 'static> std::borrow::Borrow<T> for OwnedValue<T> {
    fn borrow(&self) -> &T {
        self
    }
}

impl<T: HeapSize + Send + Sync + 'static> From<T> for OwnedValue<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl<T: HeapSize + Send + Sync + 'static + PartialEq> PartialEq for OwnedValue<T> {
    fn eq(&self, other: &Self) -> bool {
        Self::ptr_eq(self, other) || **self == **other
    }
}

impl<T: HeapSize + Send + Sync + 'static + Eq> Eq for OwnedValue<T> {}

impl<T: HeapSize + Send + Sync + 'static + std::hash::Hash> std::hash::Hash for OwnedValue<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (**self).hash(state);
    }
}

impl<T: HeapSize + Send + Sync + 'static + std::fmt::Debug> std::fmt::Debug for OwnedValue<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        (**self).fmt(f)
    }
}

impl<T: HeapSize + Send + Sync + 'static + Default> Default for OwnedValue<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: HeapSize + Send + Sync + 'static + serde::Serialize> serde::Serialize for OwnedValue<T> {
    /// Serializes as its value.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (**self).serialize(serializer)
    }
}

impl<'de, T: HeapSize + Send + Sync + 'static + serde::Deserialize<'de>> serde::Deserialize<'de>
    for OwnedValue<T>
{
    /// A new allocation of the decoded value, charged by its first owner.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(deserializer).map(Self::new)
    }
}

impl<T: HeapSize + Send + Sync + 'static> HeapSize for OwnedValue<T> {
    /// Charged by its own cell.
    fn heap_size<'t>(&'t self, meter: &mut HeapMeter<'_, 't>) -> usize {
        self.offer(meter.handles())
    }
}

// A cached value cell owns exactly its value: it carries a type expression
// or a stored span exactly when the value does.
impl<T: HeapSize + Send + Sync + 'static + verter_no_typeexpr::__private::NoTypeExprWitness>
    verter_no_typeexpr::__private::NoTypeExprWitness for OwnedValue<T>
{
}
impl<
        T: HeapSize + Send + Sync + 'static + verter_no_storedspan::__private::NoStoredSpanWitness,
    > verter_no_storedspan::__private::NoStoredSpanWitness for OwnedValue<T>
{
}

impl<T: HeapSize + Send + Sync + 'static> Measured for OwnedValue<T> {
    /// A nested owned value: charged by its own cell.
    fn heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        self.offer(aliases)
    }
}

/// The heap `value` keeps reachable, measured by its [`HeapSize`] with
/// nested owned handles offered to `aliases`: the [`Measured`] figure of a
/// data tree an owned list keeps as its items ([`measured_by_heap_size`]).
pub fn measured_heap_bytes<T: HeapSize>(value: &T, aliases: &mut AliasSet) -> usize {
    value.heap_size(&mut HeapMeter::new(aliases))
}

/// Implement [`Measured`] for data trees through their [`HeapSize`], so an
/// [`super::owned::OwnedSlice`] can keep them as items.
#[macro_export]
macro_rules! measured_by_heap_size {
    ($($t:ty),* $(,)?) => {
        $(impl $crate::owned::Measured for $t {
            fn heap_bytes(&self, aliases: &mut $crate::allocation::AliasSet) -> usize {
                $crate::heap_size::measured_heap_bytes(self, aliases)
            }
        })*
    };
}

#[cfg(test)]
#[path = "heap_size_tests.rs"]
mod tests;
