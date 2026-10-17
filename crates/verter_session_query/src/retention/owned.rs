//! Sealed owning handles: allocations that carry their own charge.
//!
//! A semantic carrier a caller can clone — a delivered value, a dependency
//! rail, a diagnostic list, one of their names — may outlive every cache
//! and read that handed it out. A charge held beside the allocation (by a
//! cache entry, by a read) cannot follow such a clone: nothing is told when
//! the clone drops. An [`Owned`] handle closes that gap by putting the
//! charge INSIDE the allocation. Every owning clone shares the one
//! allocation and therefore its one charge, the charge is released in the
//! same step that frees the allocation — when its last handle, wherever it
//! lives, drops — and no accessor exposes the raw `Arc` underneath, so no
//! alias can escape the charge.
//!
//! # Charge lifecycle
//!
//! A fresh handle is the unretained intermediate of the computation that
//! built it. The first semantic owner to keep it charges it:
//!
//! * a delivery pins it for its caller ([`Owned::pin`]): never refused, so
//!   a caller's live data is charged whatever the pressure;
//! * a cache retains it ([`Owned::hold`]): a refusable
//!   [`ChargeClass::Retained`] reservation, or the adoption of the callers'
//!   pin back into retention.
//!
//! While any [`OwnedHold`] lives the charge is retained; when the last one
//! drops while handles remain, the charge moves to [`ChargeClass::Pinned`]
//! at the account's linearization point — no uncharged interval, no double
//! charge, never refused, so pressure cannot revoke held data. A cache hold
//! keeps the allocation alive, so the allocation is never freed while a
//! hold counts it, and a charge can never be revived after its release.
//!
//! # Nested allocations
//!
//! An owned value can embed other owned handles: a diagnostic list names
//! owned strings, a rail its canonicals. Measuring a value for its charge
//! offers each nested handle to the [`AliasSet`] instead of counting it
//! ([`Owned::offer`]): the nested allocation is charged by its own cell,
//! once however many parents share it. A parent pins every child it
//! reaches when it is first charged, and a cache retaining the parent
//! retains each child too — all or nothing — and hands them back with it.
//!
//! Leaf `Arc<str>` names a value still holds raw are charged through the
//! registry ([`AliasCharges`]): the cell holds their shared charges in its
//! own class for as long as the allocation lives.

use std::sync::{Arc, Weak};

use parking_lot::Mutex;

use super::allocation::{arc_block_bytes, heap_block_bytes, AliasSet};
use super::shared::{AliasCharges, CacheHold, CallerHold};
use super::{
    ChargeClass, RetentionAdmission, RetentionCharge, RetentionRefusal, SemanticRetentionAccount,
};

/// A value an [`Owned`] cell can charge for.
pub trait Measured: Send + Sync + 'static {
    /// Heap this value keeps reachable beyond its inline bytes, which ride
    /// the block of whatever embeds it. Nested [`Owned`] handles are
    /// offered to `aliases` ([`Owned::offer`]), and each distinct `Arc`
    /// allocation is counted once per `aliases`.
    fn heap_bytes(&self, aliases: &mut AliasSet) -> usize;
}

impl Measured for Box<str> {
    fn heap_bytes(&self, _aliases: &mut AliasSet) -> usize {
        heap_block_bytes(self.len())
    }
}

impl<T: Measured> Measured for Box<[T]> {
    fn heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        heap_block_bytes(std::mem::size_of_val::<[T]>(self))
            + self
                .iter()
                .map(|item| item.heap_bytes(aliases))
                .sum::<usize>()
    }
}

impl<T: Measured> Measured for Option<T> {
    fn heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        self.as_ref().map_or(0, |value| value.heap_bytes(aliases))
    }
}

/// Where a charge is taken: the account it is charged to and the registry
/// that shares the charges of raw leaf names.
#[derive(Clone, Copy)]
pub struct ChargeCtx<'a> {
    account: &'a Arc<SemanticRetentionAccount>,
    leaves: &'a AliasCharges,
}

impl<'a> ChargeCtx<'a> {
    /// Charges to `account`, sharing raw leaf names through `leaves`.
    #[must_use]
    pub fn new(account: &'a Arc<SemanticRetentionAccount>, leaves: &'a AliasCharges) -> Self {
        Self { account, leaves }
    }
}

/// The charge state an owned cell carries.
#[derive(Default)]
pub(crate) struct Slot {
    /// `None` until a semantic owner first keeps the allocation.
    charge: Option<RetentionCharge>,
    /// Whether the cell's own block is charged by a weak holder that keeps
    /// it allocated longer than the value (an interner).
    block_elsewhere: bool,
    /// Live cache holds.
    cache_holds: usize,
    /// The owned children the value reaches, recorded when it is charged.
    children: Box<[Arc<dyn Node>]>,
    /// The cache holds on those children while this cell is retained.
    child_holds: Box<[OwnedHold]>,
    /// The shared charges of the raw leaf names the value reaches, in this
    /// cell's class.
    leaves: Leaves,
}

/// A cell's holds on its raw leaves: cache holds while it is retained,
/// caller holds otherwise.
enum Leaves {
    Cache(Box<[CacheHold]>),
    Caller(Box<[CallerHold]>),
}

impl Default for Leaves {
    fn default() -> Self {
        Self::Caller(Box::new([]))
    }
}

/// One owned allocation: its charge beside its value.
struct Cell<T> {
    slot: Mutex<Slot>,
    value: T,
}

/// The type-erased cell interface the charge lifecycle runs over.
pub(crate) trait Node: Send + Sync {
    fn slot(&self) -> &Mutex<Slot>;
    /// The cell's block: the counters, the slot and the inline value.
    fn block_bytes(&self) -> usize;
    /// The value's heap, with nested handles offered to `aliases`.
    fn value_heap_bytes(&self, aliases: &mut AliasSet) -> usize;
}

impl<T: Measured> Node for Cell<T> {
    fn slot(&self) -> &Mutex<Slot> {
        &self.slot
    }
    fn block_bytes(&self) -> usize {
        arc_block_bytes(std::mem::size_of::<Self>())
    }
    fn value_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        self.value.heap_bytes(aliases)
    }
}

/// What a first charge covers: the bytes, the children and the raw leaves
/// the measurement reached.
struct Measurement {
    bytes: usize,
    children: Box<[Arc<dyn Node>]>,
    leaves: Vec<Arc<str>>,
}

fn measure(node: &dyn Node, block_elsewhere: bool) -> Measurement {
    let mut aliases = AliasSet::collecting();
    let heap = node.value_heap_bytes(&mut aliases);
    let block = if block_elsewhere {
        0
    } else {
        node.block_bytes()
    };
    Measurement {
        bytes: block + heap,
        children: aliases.take_children(),
        leaves: aliases.take_leaves(),
    }
}

/// Charge `node` for its callers if no owner has yet. Locks parent before
/// child, so it is safe under any ancestor's slot lock.
fn pin_node(node: &dyn Node, ctx: ChargeCtx<'_>) {
    let mut slot = node.slot().lock();
    if slot.charge.is_some() {
        return;
    }
    let Measurement {
        bytes,
        children,
        leaves,
    } = measure(node, slot.block_elsewhere);
    slot.charge = Some(ctx.account.pin(bytes));
    slot.leaves = Leaves::Caller(
        leaves
            .iter()
            .map(|leaf| ctx.leaves.pin_leaf(ctx.account, leaf))
            .collect(),
    );
    for child in children.iter() {
        pin_node(&**child, ctx);
    }
    slot.children = children;
}

/// One more cache hold on `node`: a first refusable reservation, the
/// adoption of its callers' pin, or a share of its retained charge. `Err`
/// leaves every holder and every class unchanged.
fn hold_node(node: Arc<dyn Node>, ctx: ChargeCtx<'_>) -> Result<OwnedHold, RetentionRefusal> {
    {
        let mut guard = node.slot().lock();
        let slot = &mut *guard;
        if slot.cache_holds == 0 {
            if let Some(charge) = slot.charge.as_mut() {
                // Only callers hold it: its children, its leaves and then
                // its own pin are adopted back into retention. A refusal
                // hands every adopted part straight back to the callers
                // still holding it — none is released.
                let child_holds = slot
                    .children
                    .iter()
                    .map(|child| hold_node(Arc::clone(child), ctx))
                    .collect::<Result<Box<[OwnedHold]>, _>>()?;
                let leaf_holds = match &slot.leaves {
                    Leaves::Caller(callers) => callers
                        .iter()
                        .map(CallerHold::adopt)
                        .collect::<Result<Box<[CacheHold]>, _>>()?,
                    Leaves::Cache(_) => {
                        unreachable!("an allocation no cache holds holds its leaves as a caller")
                    }
                };
                charge.try_adopt_retained()?;
                slot.leaves = Leaves::Cache(leaf_holds);
                slot.child_holds = child_holds;
            } else {
                let Measurement {
                    bytes,
                    children,
                    leaves,
                } = measure(&*node, slot.block_elsewhere);
                let charge = match ctx.account.reserve(ChargeClass::Retained, bytes) {
                    RetentionAdmission::Admitted(charge) => charge,
                    RetentionAdmission::Refused(refusal) => return Err(refusal),
                };
                // A refusal below drops what was taken so far: the charge,
                // the leaf holds and the child holds each release only what
                // this hold just granted.
                let leaf_holds = leaves
                    .iter()
                    .map(|leaf| ctx.leaves.hold_leaf(ctx.account, leaf))
                    .collect::<Result<Box<[CacheHold]>, _>>()?;
                let child_holds = children
                    .iter()
                    .map(|child| hold_node(Arc::clone(child), ctx))
                    .collect::<Result<Box<[OwnedHold]>, _>>()?;
                slot.charge = Some(charge);
                slot.leaves = Leaves::Cache(leaf_holds);
                slot.children = children;
                slot.child_holds = child_holds;
            }
        }
        slot.cache_holds += 1;
    }
    Ok(OwnedHold(node))
}

/// A sealed owning handle on one charged allocation. See the module docs.
///
/// Cloning shares the allocation and its one charge; dropping the last
/// handle releases both. The value is reachable only by borrow.
pub struct Owned<T: Measured>(Arc<Cell<T>>);

impl<T: Measured> Owned<T> {
    /// A new allocation of `value`, charged by the first owner that keeps
    /// it.
    #[must_use]
    pub fn new(value: T) -> Self {
        Self(Arc::new(Cell {
            slot: Mutex::new(Slot::default()),
            value,
        }))
    }

    /// [`Self::new`] for an allocation a weak holder keeps allocated past
    /// its value and charges the block of for that whole time (an
    /// interner): the handle's charge covers the value's heap only.
    #[must_use]
    pub fn new_with_block_charged_elsewhere(value: T) -> Self {
        Self(Arc::new(Cell {
            slot: Mutex::new(Slot {
                block_elsewhere: true,
                ..Slot::default()
            }),
            value,
        }))
    }

    /// Charge this allocation to its callers if no owner has yet: pinned,
    /// never refused, together with every owned child it reaches.
    pub fn pin(&self, ctx: ChargeCtx<'_>) {
        pin_node(&*self.0, ctx);
    }

    /// A cache's hold on this allocation: retains its charge — and its
    /// children's — refusably while the hold lives.
    ///
    /// # Errors
    ///
    /// The account's refusal: nothing is retained and every existing
    /// holder, charge and class is unchanged.
    pub fn hold(&self, ctx: ChargeCtx<'_>) -> Result<OwnedHold, RetentionRefusal> {
        hold_node(Arc::clone(&self.0) as Arc<dyn Node>, ctx)
    }

    /// Offer this nested handle to the measurement of a value embedding
    /// it. A charging measurement collects it as a child, charged by its
    /// own cell, and counts nothing; any other measurement counts its
    /// block and heap in full, once per `aliases`.
    pub fn offer(&self, aliases: &mut AliasSet) -> usize {
        if !aliases.first(&self.0) {
            return 0;
        }
        if aliases.collect_child(|| Arc::clone(&self.0) as Arc<dyn Node>) {
            return 0;
        }
        let block = if self.0.slot.lock().block_elsewhere {
            0
        } else {
            self.0.block_bytes()
        };
        block + self.0.value.heap_bytes(aliases)
    }

    /// Bytes the allocation's charge holds; `None` while no owner has
    /// charged it.
    #[must_use]
    pub fn charge_bytes(&self) -> Option<usize> {
        self.0
            .slot
            .lock()
            .charge
            .as_ref()
            .map(RetentionCharge::bytes)
    }

    /// The charge's class; `None` while no owner has charged it.
    #[must_use]
    pub fn charge_class(&self) -> Option<ChargeClass> {
        self.0
            .slot
            .lock()
            .charge
            .as_ref()
            .map(RetentionCharge::class)
    }

    /// Whether two handles share one allocation.
    #[must_use]
    pub fn ptr_eq(this: &Self, other: &Self) -> bool {
        Arc::ptr_eq(&this.0, &other.0)
    }

    /// The allocation's address, for identity checks that never deref.
    #[must_use]
    pub fn addr(this: &Self) -> usize {
        Arc::as_ptr(&this.0).cast::<()>() as usize
    }

    /// A weak view that keeps the cell's block — not its value or its
    /// charge — allocated.
    #[must_use]
    pub fn downgrade(this: &Self) -> WeakOwned<T> {
        WeakOwned(Arc::downgrade(&this.0))
    }

    /// Weak views of this allocation.
    #[must_use]
    pub fn weak_count(this: &Self) -> usize {
        Arc::weak_count(&this.0)
    }

    /// Bytes the cell's block holds beside its value's heap.
    #[must_use]
    pub fn cell_block_bytes() -> usize {
        arc_block_bytes(std::mem::size_of::<Cell<T>>())
    }
}

impl<T: Measured> Clone for Owned<T> {
    /// Another owner of the same allocation: shares its one charge.
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<T: Measured> std::ops::Deref for Owned<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0.value
    }
}

impl<T: Measured> From<T> for Owned<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl<T: Measured> AsRef<T> for Owned<T> {
    fn as_ref(&self) -> &T {
        &self.0.value
    }
}

impl<T: Measured + PartialEq> PartialEq for Owned<T> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0.value == other.0.value
    }
}

impl<T: Measured + Eq> Eq for Owned<T> {}

impl<T: Measured + std::hash::Hash> std::hash::Hash for Owned<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.value.hash(state);
    }
}

impl<T: Measured + PartialOrd> PartialOrd for Owned<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.0.value.partial_cmp(&other.0.value)
    }
}

impl<T: Measured + Ord> Ord for Owned<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.value.cmp(&other.0.value)
    }
}

impl<T: Measured + std::fmt::Debug> std::fmt::Debug for Owned<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.value.fmt(f)
    }
}

impl<T: Measured + Default> Default for Owned<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

/// An owned slice: a sealed, self-charged list allocation.
pub type OwnedSlice<T> = Owned<Box<[T]>>;

impl<T: Measured> From<Vec<T>> for Owned<Box<[T]>> {
    fn from(items: Vec<T>) -> Self {
        Self::new(items.into_boxed_slice())
    }
}

impl<T: Measured> FromIterator<T> for Owned<Box<[T]>> {
    fn from_iter<I: IntoIterator<Item = T>>(items: I) -> Self {
        Self::new(items.into_iter().collect())
    }
}

/// A weak view of an [`Owned`] allocation.
pub struct WeakOwned<T: Measured>(Weak<Cell<T>>);

impl<T: Measured> WeakOwned<T> {
    /// The allocation, while any handle keeps it.
    #[must_use]
    pub fn upgrade(&self) -> Option<Owned<T>> {
        self.0.upgrade().map(Owned)
    }

    /// Live handles of the allocation.
    #[must_use]
    pub fn strong_count(&self) -> usize {
        self.0.strong_count()
    }
}

impl<T: Measured> Clone for WeakOwned<T> {
    fn clone(&self) -> Self {
        Self(Weak::clone(&self.0))
    }
}

/// A cache's hold on one owned allocation. See the module docs.
pub struct OwnedHold(Arc<dyn Node>);

impl OwnedHold {
    /// Bytes the held allocation's charge holds.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.0
            .slot()
            .lock()
            .charge
            .as_ref()
            .map_or(0, RetentionCharge::bytes)
    }

    /// The held allocation's charge class.
    #[must_use]
    pub fn class(&self) -> ChargeClass {
        self.0
            .slot()
            .lock()
            .charge
            .as_ref()
            .expect("a held allocation is charged")
            .class()
    }
}

impl Drop for OwnedHold {
    fn drop(&mut self) {
        let released = {
            let mut guard = self.0.slot().lock();
            let slot = &mut *guard;
            slot.cache_holds -= 1;
            if slot.cache_holds == 0 {
                // The remaining handles are callers now: the charge, and
                // its leaves', are theirs. A hold that was the last handle
                // frees the cell right after, releasing the pin with it.
                slot.charge
                    .as_mut()
                    .expect("a held allocation is charged")
                    .transfer_to_pinned();
                if let Leaves::Cache(caches) = &slot.leaves {
                    let callers = caches.iter().map(CacheHold::caller).collect();
                    slot.leaves = Leaves::Caller(callers);
                }
                std::mem::take(&mut slot.child_holds)
            } else {
                Box::new([])
            }
        };
        // The children transfer to whoever else still holds them.
        drop(released);
    }
}

impl std::fmt::Debug for OwnedHold {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let slot = self.0.slot().lock();
        f.debug_struct("OwnedHold")
            .field(
                "bytes",
                &slot.charge.as_ref().map_or(0, RetentionCharge::bytes),
            )
            .field("cache_holds", &slot.cache_holds)
            .finish()
    }
}

/// An owned string: a sealed, self-charged text allocation.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct OwnedStr(Owned<Box<str>>);

impl OwnedStr {
    /// A new owned copy of `text`.
    #[must_use]
    pub fn new(text: &str) -> Self {
        Self(Owned::new(Box::from(text)))
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Offer this string to a measurement (see [`Owned::offer`]).
    pub fn offer(&self, aliases: &mut AliasSet) -> usize {
        self.0.offer(aliases)
    }

    /// Charge it to its callers (see [`Owned::pin`]).
    pub fn pin(&self, ctx: ChargeCtx<'_>) {
        self.0.pin(ctx);
    }

    /// A cache's hold on it (see [`Owned::hold`]).
    ///
    /// # Errors
    ///
    /// The account's refusal.
    pub fn hold(&self, ctx: ChargeCtx<'_>) -> Result<OwnedHold, RetentionRefusal> {
        self.0.hold(ctx)
    }

    /// Bytes its charge holds, once charged.
    #[must_use]
    pub fn charge_bytes(&self) -> Option<usize> {
        self.0.charge_bytes()
    }

    /// Its charge's class, once charged.
    #[must_use]
    pub fn charge_class(&self) -> Option<ChargeClass> {
        self.0.charge_class()
    }

    /// Whether two strings share one allocation.
    #[must_use]
    pub fn ptr_eq(this: &Self, other: &Self) -> bool {
        Owned::ptr_eq(&this.0, &other.0)
    }

    /// Bytes one owned string of `len` bytes is charged.
    #[must_use]
    pub fn charge_for_len(len: usize) -> usize {
        Owned::<Box<str>>::cell_block_bytes() + heap_block_bytes(len)
    }
}

impl std::ops::Deref for OwnedStr {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for OwnedStr {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<str> for OwnedStr {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl std::hash::Hash for OwnedStr {
    /// Hashes as its text, so a `str` borrow finds it.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl PartialEq<str> for OwnedStr {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for OwnedStr {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl From<&str> for OwnedStr {
    fn from(text: &str) -> Self {
        Self::new(text)
    }
}

impl From<String> for OwnedStr {
    fn from(text: String) -> Self {
        Self(Owned::new(text.into_boxed_str()))
    }
}

impl From<Arc<str>> for OwnedStr {
    /// An owned copy of a raw shared string: a new allocation, charged by
    /// its own cell.
    fn from(text: Arc<str>) -> Self {
        Self::new(&text)
    }
}

impl From<&Arc<str>> for OwnedStr {
    fn from(text: &Arc<str>) -> Self {
        Self::new(text)
    }
}

impl From<&String> for OwnedStr {
    fn from(text: &String) -> Self {
        Self::new(text)
    }
}

impl std::fmt::Debug for OwnedStr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.as_str().fmt(f)
    }
}

impl std::fmt::Display for OwnedStr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.as_str().fmt(f)
    }
}

impl Measured for OwnedStr {
    /// A string embedded in a measured value is a nested allocation.
    fn heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        self.offer(aliases)
    }
}

#[cfg(test)]
#[path = "owned_tests.rs"]
mod tests;
