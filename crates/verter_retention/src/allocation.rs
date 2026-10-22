//! Allocation footprints: the bytes a retained value actually keeps alive.
//!
//! A charge follows the ALLOCATIONS its owner keeps reachable — never a
//! fixed per-item proxy. A fact naming a ten-kilobyte canonical path costs
//! ten kilobytes, a diagnostic carrying a long name costs that name, and a
//! slice costs its whole backing block. The helpers here measure one
//! allocation each; the semantic owners compose them over their payloads.
//!
//! # Declared conventions
//!
//! * **Heap block.** One heap allocation of `n > 0` payload bytes costs `n`
//!   rounded up to [`ALLOCATION_GRANULE_BYTES`], plus
//!   [`ALLOCATION_HEADER_BYTES`] of allocator bookkeeping
//!   ([`heap_block_bytes`]). A zero-byte payload allocates nothing. These
//!   are the physical-policy allocator-overhead conventions: a real
//!   allocator's size classes differ, but the convention is the same for
//!   every owner, so no owner is charged as free and none twice.
//! * **Capacity, not length.** A `String` or `Vec` costs its capacity.
//! * **`Arc` blocks.** An `Arc<T>` block holds its two reference counters
//!   beside `T`; an `Arc<[T]>` / `Arc<str>` block holds them beside the
//!   whole slice, so even an empty `Arc` slice costs its counters.
//! * **Embedding.** A field's INLINE bytes live inside the block of the
//!   value that embeds it and are charged with that block; only the heap a
//!   field points at is charged on its own. A value held directly in a
//!   container slot is charged by the container's slot convention.
//! * **Self-charged allocations.** An allocation that carries its own
//!   charge — an evidence page, a cost receipt, a pooled identity string, a
//!   graph node — is never charged again by a holder: holding it costs the
//!   holder only the handle's inline bytes.
//! * **Aliases inside one charge.** Within one charge each distinct `Arc`
//!   allocation is counted once ([`AliasSet`]). Allocations several
//!   independently charged owners share are charged once: a delivered
//!   allocation carries its charge inside itself
//!   ([`super::owned::Owned`]), a cache-internal one is charged through
//!   [`super::shared::AliasCharges`].
//! * **Nested owned allocations.** An [`super::owned::Owned`] handle nested
//!   in a measured value is its own allocation, charged by its own cell
//!   once however many owners reach it: a measurement collects it instead
//!   of counting it, and the owner taking the charge pins or holds every
//!   handle its measurement reached ([`AliasSet::take_handles`]). Only a
//!   figure that charges nothing counts handles in full
//!   ([`AliasSet::estimating`]).
//! * **Owned values reach no raw `Arc`.** An owned cell's value keeps each
//!   allocation either exclusively (a `Box`, `Vec`, `String`, counted with
//!   the cell) or as an owned handle (charged by itself). A raw `Arc` in
//!   it would be an allocation a caller could clone out and keep past the
//!   cell's charge; a debug build fails the measurement that reaches one.
//! * **Leaf text.** A raw leaf `Arc<str>` name inside a registered,
//!   cache-internal carrier (a fact's interned names) is its own shared
//!   allocation: measuring for a charge collects its leaves instead of
//!   counting them ([`AliasSet::collecting`]) and the registry charges each
//!   leaf once, held by every holder that reaches it
//!   ([`super::shared::AliasCharges`]). Outside a charge a measurement
//!   counts each distinct leaf it reaches in full.
//!
//! The figures are admission ESTIMATES (see the crate docs): nothing in
//! cache validity reads them.

use std::sync::Arc;

use super::word_set::WordSet;

/// Allocator bookkeeping one heap block costs beside its payload.
pub const ALLOCATION_HEADER_BYTES: usize = 16;
/// Allocator size-class granularity a heap block's payload rounds up to.
pub const ALLOCATION_GRANULE_BYTES: usize = 16;
// [`heap_block_bytes`] rounds up by masking.
const _: () = assert!(ALLOCATION_GRANULE_BYTES.is_power_of_two());

/// Bytes one heap block holding `payload` bytes costs.
///
/// Written with plain integer operators: every retained charge measures
/// through it, and a debug build calls out to no helper for them.
#[must_use]
#[inline(always)]
pub const fn heap_block_bytes(payload: usize) -> usize {
    if payload == 0 {
        return 0;
    }
    // The granule is a power of two, so rounding up is a mask.
    ((payload + (ALLOCATION_GRANULE_BYTES - 1)) & !(ALLOCATION_GRANULE_BYTES - 1))
        + ALLOCATION_HEADER_BYTES
}

/// Bytes one `Arc` block holding `payload` bytes beside its two counters
/// costs.
#[must_use]
#[inline(always)]
pub const fn arc_block_bytes(payload: usize) -> usize {
    heap_block_bytes(2 * std::mem::size_of::<usize>() + payload)
}

/// Bytes a `String`'s backing costs: its capacity.
#[must_use]
pub fn string_bytes(value: &String) -> usize {
    heap_block_bytes(value.capacity())
}

/// Bytes a `Vec`'s backing costs: its capacity, not its length.
#[must_use]
pub fn vec_bytes<T>(value: &Vec<T>) -> usize {
    heap_block_bytes(value.capacity() * std::mem::size_of::<T>())
}

/// Bytes a boxed `T` costs.
#[must_use]
pub const fn box_bytes<T>() -> usize {
    heap_block_bytes(std::mem::size_of::<T>())
}

/// Distinct `Arc` allocations one charge has already counted, and the
/// owned allocations the measurement reached.
///
/// Keyed by the allocation's address. Every key names an allocation the
/// measured value holds for the whole measurement, so an address cannot be
/// freed and reused while the set is live. The first few keys stay inline,
/// so measuring a small value allocates nothing.
///
/// A measurement never counts an owned handle ([`super::owned::Owned`],
/// [`super::owned::OwnedStr`]) it reaches: the handle's allocation is
/// charged by its own cell, once however many owners reach it. The set
/// collects each such handle instead, and the owner taking the charge must
/// take them too ([`Self::take_handles`]) and charge them — a set dropped
/// with handles it never handed over is an omitted charge, and fails a
/// debug build. Only a figure that charges nothing — an expectation in a
/// test, a bound — counts handles in full ([`Self::estimating`]).
pub struct AliasSet {
    /// The first [`ALIAS_INLINE`] keys, `inline[..inline_len]`; a plain
    /// array scanned by index, which costs a debug build no call per key.
    inline: [u64; ALIAS_INLINE],
    inline_len: usize,
    spilled: Option<WordSet>,
    /// When collecting leaves, the distinct leaf names the measurement
    /// reached, which it then does not count.
    leaves: Option<Vec<Arc<str>>>,
    /// The distinct owned allocations the measurement reached, each charged
    /// by its own cell and not counted here; `None` when they are counted
    /// in full instead.
    children: Option<Children>,
    /// Whether this measures an owned cell's value, which reaches no raw
    /// `Arc`: each allocation it keeps is exclusively its own or an owned
    /// handle charged by itself.
    in_cell: bool,
}

/// Allocations an [`AliasSet`] tracks before it spills to a hash set.
const ALIAS_INLINE: usize = 8;

/// The owned children a measurement reached. Most measurements reach none
/// or one, so those are held without an allocation, and an empty list —
/// built and dropped by every measurement — costs a debug build no call to
/// create, read or drop.
#[derive(Default)]
pub(crate) enum Children {
    #[default]
    Empty,
    One(Arc<dyn super::owned::Node>),
    Many(Vec<Arc<dyn super::owned::Node>>),
}

impl Children {
    pub(crate) const fn new() -> Self {
        Self::Empty
    }

    pub(crate) fn push(&mut self, child: Arc<dyn super::owned::Node>) {
        match self {
            Self::Empty => *self = Self::One(child),
            Self::One(_) => {
                let Self::One(first) = std::mem::take(self) else {
                    unreachable!("matched a single child");
                };
                *self = Self::Many(vec![first, child]);
            }
            Self::Many(children) => children.push(child),
        }
    }

    pub(crate) fn reserve(&mut self, additional: usize) {
        if let Self::Many(children) = self {
            children.reserve(additional);
        }
    }

    pub(crate) fn as_slice(&self) -> &[Arc<dyn super::owned::Node>] {
        match self {
            Self::Empty => &[],
            Self::One(child) => std::slice::from_ref(child),
            Self::Many(children) => children,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Empty => 0,
            Self::One(_) => 1,
            Self::Many(children) => children.len(),
        }
    }

    pub(crate) fn iter(&self) -> std::slice::Iter<'_, Arc<dyn super::owned::Node>> {
        self.as_slice().iter()
    }

    pub(crate) fn into_boxed_slice(self) -> Box<[Arc<dyn super::owned::Node>]> {
        match self {
            Self::Empty => Box::new([]),
            Self::One(child) => Box::new([child]),
            Self::Many(children) => children.into_boxed_slice(),
        }
    }
}

impl std::ops::Deref for Children {
    type Target = [Arc<dyn super::owned::Node>];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl IntoIterator for Children {
    type Item = Arc<dyn super::owned::Node>;
    type IntoIter = ChildrenIntoIter;

    fn into_iter(self) -> ChildrenIntoIter {
        match self {
            Self::Empty => ChildrenIntoIter::One(None),
            Self::One(child) => ChildrenIntoIter::One(Some(child)),
            Self::Many(children) => ChildrenIntoIter::Many(children.into_iter()),
        }
    }
}

/// The owned children of a [`Children`] list, by value.
pub(crate) enum ChildrenIntoIter {
    One(Option<Arc<dyn super::owned::Node>>),
    Many(std::vec::IntoIter<Arc<dyn super::owned::Node>>),
}

impl Iterator for ChildrenIntoIter {
    type Item = Arc<dyn super::owned::Node>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::One(child) => child.take(),
            Self::Many(children) => children.next(),
        }
    }
}

impl Default for AliasSet {
    fn default() -> Self {
        Self::with(None, Some(Children::new()), false)
    }
}

impl AliasSet {
    fn with(leaves: Option<Vec<Arc<str>>>, children: Option<Children>, in_cell: bool) -> Self {
        Self {
            inline: [0; ALIAS_INLINE],
            inline_len: 0,
            spilled: None,
            leaves,
            children,
            in_cell,
        }
    }

    /// An empty set: counts raw leaves, collects owned handles.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty set that also collects every distinct raw leaf name a
    /// measurement reaches instead of counting it: a registered carrier's
    /// leaves are charged as allocations of their own.
    #[must_use]
    pub fn collecting() -> Self {
        Self::with(Some(Vec::new()), Some(Children::new()), false)
    }

    /// An empty set measuring an owned cell's value.
    pub(crate) fn for_cell() -> Self {
        Self::with(None, Some(Children::new()), true)
    }

    /// Whether this measures an owned cell's value, which reaches no raw
    /// `Arc`.
    pub(crate) fn measures_cell(&self) -> bool {
        self.in_cell
    }

    /// An empty set for a figure that charges nothing: it counts every
    /// owned handle it reaches in full, once.
    #[must_use]
    pub fn estimating() -> Self {
        Self::with(None, None, false)
    }

    /// Record an owned child; `false` when this measurement counts children
    /// in full instead.
    pub(crate) fn collect_child(
        &mut self,
        child: impl FnOnce() -> Arc<dyn super::owned::Node>,
    ) -> bool {
        match self.children.as_mut() {
            Some(children) => {
                children.push(child());
                true
            }
            None => false,
        }
    }

    /// Take over the owned allocations `inner` reached, each once across
    /// both sets, and return the bytes of those this set counts in full
    /// instead of collecting. A data tree whose part is measured on a set
    /// of its own — one that counts the part's raw `Arc`s, which a tree's
    /// cell owns — hands that part's owned handles to the cell's set.
    pub fn absorb(&mut self, inner: &mut AliasSet) -> usize {
        let mut counted = 0;
        for child in inner.take_children() {
            if !self.first_allocation(&child) {
                continue;
            }
            if !self.collect_child(|| Arc::clone(&child)) {
                counted += super::owned::counted_in_full(&*child, self);
            }
        }
        counted
    }

    /// The owned children the measurement reached, each once.
    pub(crate) fn take_children(&mut self) -> Children {
        self.children
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }

    /// The owned allocations the measurement reached, each once, for the
    /// owner taking the charge to charge them.
    pub fn take_handles(&mut self) -> super::owned::OwnedHandles {
        super::owned::OwnedHandles::from_children(self.take_children())
    }

    /// The leaf names a collecting measurement reached, each once.
    #[must_use]
    pub fn take_leaves(&mut self) -> Vec<Arc<str>> {
        self.leaves.take().unwrap_or_default()
    }

    /// `true` the first time `arc`'s allocation is offered, `false` after.
    ///
    /// An owned cell's value reaches no raw `Arc`: a debug build fails the
    /// measurement that offers one.
    pub fn first<T: ?Sized>(&mut self, arc: &Arc<T>) -> bool {
        verter_debug_assert::verter_debug_assert!(
            !self.in_cell,
            "an owned value reaches a raw `Arc<{}>`: an allocation a caller can clone out of a \
             delivered value must be an owned handle charged by itself",
            std::any::type_name::<T>()
        );
        self.first_allocation(arc)
    }

    /// [`Self::first`] for an owned cell's own allocation.
    pub(crate) fn first_allocation<T: ?Sized>(&mut self, arc: &Arc<T>) -> bool {
        let key = Arc::as_ptr(arc).cast::<()>() as u64;
        if let Some(spilled) = self.spilled.as_mut() {
            return spilled.insert(key);
        }
        let mut index = 0;
        while index < self.inline_len {
            if self.inline[index] == key {
                return false;
            }
            index += 1;
        }
        if self.inline_len < ALIAS_INLINE {
            self.inline[self.inline_len] = key;
            self.inline_len += 1;
            return true;
        }
        let mut spilled: WordSet = self.inline.iter().copied().collect();
        self.inline_len = 0;
        spilled.insert(key);
        self.spilled = Some(spilled);
        true
    }

    /// Distinct allocations counted so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.spilled.as_ref().map_or(self.inline_len, WordSet::len)
    }

    /// Whether nothing has been counted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Drop for AliasSet {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            verter_debug_assert::verter_debug_assert!(
                self.children.as_ref().is_none_or(Children::is_empty),
                "a measurement reached owned allocations its owner never charged: take them \
                 with `AliasSet::take_handles` and pin or hold them"
            );
        }
    }
}

/// Whether `text` is the empty text `Arc::<str>::default()` hands out. The
/// standard library shares one static for it, so a handle to it keeps no
/// allocation alive; an empty text built any other way is its own
/// allocation and is charged like any other.
#[must_use]
pub fn is_shared_empty_str(text: &Arc<str>) -> bool {
    static EMPTY: std::sync::OnceLock<Arc<str>> = std::sync::OnceLock::new();
    text.is_empty() && Arc::ptr_eq(text, EMPTY.get_or_init(Arc::default))
}

/// Bytes an `Arc<str>` keeps alive, or `0` when this charge already counted
/// its allocation or it is the shared empty text
/// ([`is_shared_empty_str`]).
#[must_use]
pub fn arc_str_bytes(value: &Arc<str>, aliases: &mut AliasSet) -> usize {
    if is_shared_empty_str(value) || !aliases.first(value) {
        return 0;
    }
    match aliases.leaves.as_mut() {
        Some(leaves) => {
            leaves.push(Arc::clone(value));
            0
        }
        None => arc_block_bytes(value.len()),
    }
}

/// Bytes an `Arc<[T]>`'s own block keeps alive — the slice's inline
/// elements and the counters — or `None` when this charge already counted
/// the allocation (its elements' heap then counted with it).
#[must_use]
pub fn arc_slice_block_bytes<T>(value: &Arc<[T]>, aliases: &mut AliasSet) -> Option<usize> {
    aliases
        .first(value)
        .then(|| arc_block_bytes(std::mem::size_of_val::<[T]>(value)))
}

/// Bytes a list of leaf names keeps alive: the list's block and each
/// distinct name's text.
#[must_use]
pub fn arc_str_list_bytes(value: &Arc<[Arc<str>]>, aliases: &mut AliasSet) -> usize {
    let Some(block) = arc_slice_block_bytes(value, aliases) else {
        return 0;
    };
    block
        + value
            .iter()
            .map(|name| arc_str_bytes(name, aliases))
            .sum::<usize>()
}
