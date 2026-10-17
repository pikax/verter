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
//! An owned value reaches no raw `Arc`: every allocation it keeps is
//! either exclusively its own (counted with the cell) or an owned handle
//! (charged by itself). A raw `Arc` in it would be an allocation a caller
//! could clone out and keep past the cell's charge, so a debug build fails
//! the measurement that reaches one.

use std::sync::{Arc, Weak};

use parking_lot::Mutex;

use super::allocation::{arc_block_bytes, heap_block_bytes, AliasSet};
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

impl<A: Measured, B: Measured> Measured for (A, B) {
    fn heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        self.0.heap_bytes(aliases) + self.1.heap_bytes(aliases)
    }
}

/// Values whose every byte is inline.
macro_rules! inline_measured {
    ($($t:ty),* $(,)?) => {
        $(impl Measured for $t {
            fn heap_bytes(&self, _aliases: &mut AliasSet) -> usize {
                0
            }
        })*
    };
}

inline_measured!(bool, u8, u16, u32, u64, u128, usize, i32, i64, [u8; 16]);

impl Measured for String {
    /// An exclusively owned string: its backing, by capacity.
    fn heap_bytes(&self, _aliases: &mut AliasSet) -> usize {
        heap_block_bytes(self.capacity())
    }
}

/// Where a charge is taken: the account it is charged to.
#[derive(Clone, Copy)]
pub struct ChargeCtx<'a> {
    account: &'a Arc<SemanticRetentionAccount>,
}

impl<'a> ChargeCtx<'a> {
    /// Charges to `account`.
    #[must_use]
    pub fn new(account: &'a Arc<SemanticRetentionAccount>) -> Self {
        Self { account }
    }
}

/// The charge state an owned cell carries.
#[derive(Default)]
pub(crate) struct Slot {
    /// `None` until a semantic owner first keeps the allocation.
    charge: Option<RetentionCharge>,
    /// Live cache holds.
    cache_holds: u32,
    /// The charge on the cell's own block when weak holders (an interner)
    /// may keep the block allocated after its value: shared by the cell and
    /// each such holder, so it lasts until the last of them — whichever
    /// that is — lets go, exactly as the block does.
    block_share: Option<BlockShare>,
    /// The owned children the value reaches and the cache holds on them
    /// while this cell is retained; `None` for a value that reaches none.
    nested: Option<Box<Nested>>,
}

/// An owned cell's children.
///
/// This box and the `children` slice are allocated when the cell is first
/// charged and live as long as the cell, so the cell's own charge covers
/// them ([`registration_bytes`]). The `holds` slice exists only while the
/// cell is retained and carries its own charge.
struct Nested {
    /// Recorded when the cell is first charged.
    children: Box<[Arc<dyn Node>]>,
    /// The cache holds on them while this cell is retained.
    holds: HeldHandles,
}

/// Bytes a cell's child registration keeps for its whole life once it
/// reaches `children` owned allocations: the [`Nested`] box and the
/// children slice. A cell that reaches none allocates neither.
fn registration_bytes(children: usize) -> usize {
    if children == 0 {
        return 0;
    }
    heap_block_bytes(std::mem::size_of::<Nested>())
        + heap_block_bytes(children * std::mem::size_of::<Arc<dyn Node>>())
}

/// A cache's holds on several owned allocations, with the charge on the
/// slice that keeps them: the slice is cache bookkeeping that lives exactly
/// as long as the holds, so its backing is charged
/// [`ChargeClass::Retained`] beside them and released with them.
#[derive(Default)]
pub struct HeldHandles {
    holds: Box<[OwnedHold]>,
    /// The slice's backing; `None` for no holds, which allocate nothing.
    backing: Option<RetentionCharge>,
}

impl HeldHandles {
    /// Bytes the slice of `holds` cache holds keeps: its backing block.
    #[must_use]
    pub fn backing_bytes_for(holds: usize) -> usize {
        heap_block_bytes(holds * std::mem::size_of::<OwnedHold>())
    }

    /// How many allocations these holds keep.
    #[must_use]
    pub fn len(&self) -> usize {
        self.holds.len()
    }

    /// Whether these holds keep no allocation.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.holds.is_empty()
    }

    /// Bytes the holds' own slice is charged.
    #[must_use]
    pub fn backing_bytes(&self) -> usize {
        self.backing.as_ref().map_or(0, RetentionCharge::bytes)
    }

    /// The holds.
    #[must_use]
    pub fn holds(&self) -> &[OwnedHold] {
        &self.holds
    }

    /// The holds and the charge on their slice, for the charge that takes
    /// both over ([`RetentionCharge::holding`]).
    pub(crate) fn into_parts(self) -> (Box<[OwnedHold]>, Option<RetentionCharge>) {
        (self.holds, self.backing)
    }
}

impl std::fmt::Debug for HeldHandles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeldHandles")
            .field("holds", &self.holds.len())
            .field("backing_bytes", &self.backing_bytes())
            .finish()
    }
}

impl Slot {
    fn children(&self) -> &[Arc<dyn Node>] {
        self.nested.as_ref().map_or(&[], |nested| &nested.children)
    }
}

/// One owned allocation: its charge beside its value.
struct Cell<T> {
    slot: Mutex<Slot>,
    value: T,
}

// A cell's charge state stays consistent across an unwind: every slot
// transition completes under its lock before anything can panic, and a
// parking_lot lock never poisons. The value is shared only by borrow.
impl<T: std::panic::RefUnwindSafe> std::panic::RefUnwindSafe for Cell<T> {}
impl<T: std::panic::RefUnwindSafe> std::panic::UnwindSafe for Cell<T> {}

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

/// What a first charge covers: the bytes and the children the measurement
/// reached.
/// The children's registration is allocated with the first charge and
/// lives as long as the cell, so it is part of that charge.
fn measure(node: &dyn Node, block_shared: bool) -> (usize, Box<[Arc<dyn Node>]>) {
    let mut aliases = AliasSet::for_cell();
    let heap = node.value_heap_bytes(&mut aliases);
    let block = if block_shared { 0 } else { node.block_bytes() };
    let children = aliases.take_children();
    (block + heap + registration_bytes(children.len()), children)
}

/// Charge `node` for its callers if no owner has yet. Locks parent before
/// child, so it is safe under any ancestor's slot lock.
pub(crate) fn pin_node(node: &dyn Node, ctx: ChargeCtx<'_>) {
    let mut slot = node.slot().lock();
    if slot.charge.is_some() {
        return;
    }
    let (bytes, children) = measure(node, slot.block_share.is_some());
    slot.charge = Some(ctx.account.pin(bytes));
    for child in children.iter() {
        pin_node(&**child, ctx);
    }
    if !children.is_empty() {
        slot.nested = Some(Box::new(Nested {
            children,
            holds: HeldHandles::default(),
        }));
    }
}

/// A cache hold on each of `children`, all or nothing: a refusal drops the
/// holds taken so far, handing each child straight back to its callers.
/// The slice keeping the holds is charged with them, refusably and all or
/// nothing too.
pub(crate) fn hold_nodes(
    children: &[Arc<dyn Node>],
    ctx: ChargeCtx<'_>,
) -> Result<HeldHandles, RetentionRefusal> {
    if children.is_empty() {
        return Ok(HeldHandles::default());
    }
    let backing = match ctx.account.reserve(
        ChargeClass::Retained,
        HeldHandles::backing_bytes_for(children.len()),
    ) {
        RetentionAdmission::Admitted(charge) => charge,
        RetentionAdmission::Refused(refusal) => return Err(refusal),
    };
    let holds = children
        .iter()
        .map(|child| hold_node(Arc::clone(child), ctx))
        .collect::<Result<Box<[OwnedHold]>, RetentionRefusal>>()?;
    Ok(HeldHandles {
        holds,
        backing: Some(backing),
    })
}

/// One more cache hold on `node`: a first refusable reservation, the
/// adoption of its callers' pin, or a share of its retained charge. `Err`
/// leaves every holder and every class unchanged.
pub(crate) fn hold_node(
    node: Arc<dyn Node>,
    ctx: ChargeCtx<'_>,
) -> Result<OwnedHold, RetentionRefusal> {
    {
        let mut guard = node.slot().lock();
        let slot = &mut *guard;
        if slot.cache_holds == 0 {
            if slot.charge.is_some() {
                // Only callers hold it: its children and then its own pin
                // are adopted back into retention. A refusal hands every
                // adopted child straight back to the callers still holding
                // it — none is released.
                let holds = hold_nodes(slot.children(), ctx)?;
                if let Some(charge) = slot.charge.as_mut() {
                    charge.try_adopt_retained()?;
                }
                if let Some(nested) = slot.nested.as_mut() {
                    nested.holds = holds;
                }
            } else {
                let (bytes, children) = measure(&*node, slot.block_share.is_some());
                let charge = match ctx.account.reserve(ChargeClass::Retained, bytes) {
                    RetentionAdmission::Admitted(charge) => charge,
                    RetentionAdmission::Refused(refusal) => return Err(refusal),
                };
                // A refusal below drops what was taken so far: the charge
                // and the child holds each release only what this hold
                // just granted.
                let holds = hold_nodes(&children, ctx)?;
                slot.charge = Some(charge);
                if !children.is_empty() {
                    slot.nested = Some(Box::new(Nested { children, holds }));
                }
            }
        }
        slot.cache_holds += 1;
    }
    Ok(OwnedHold(Some(node)))
}

/// The owned allocations a carrier reaches at its top level, each once
/// ([`AliasSet::take_handles`]): what an owner keeping the carrier pins or
/// holds, so each is charged by its own cell.
#[must_use = "an owned allocation an owner reaches must be pinned or held"]
#[derive(Default)]
pub struct OwnedHandles(Box<[Arc<dyn Node>]>);

impl OwnedHandles {
    /// These handles and `other`'s, each allocation once.
    pub fn merge(self, other: Self) -> Self {
        if other.0.is_empty() {
            return self;
        }
        if self.0.is_empty() {
            return other;
        }
        let mut seen = rustc_hash::FxHashSet::default();
        Self(
            self.0
                .into_vec()
                .into_iter()
                .chain(other.0.into_vec())
                .filter(|node| seen.insert(Arc::as_ptr(node).cast::<()>() as usize))
                .collect(),
        )
    }

    pub(crate) fn from_children(children: Box<[Arc<dyn Node>]>) -> Self {
        Self(children)
    }

    pub(crate) fn into_children(self) -> Box<[Arc<dyn Node>]> {
        self.0
    }

    /// The handles a carrier `measure` reaches. The carrier is delivered
    /// whole: it reaches no raw `Arc` and keeps no heap outside its
    /// handles, which a debug build checks.
    pub fn of(measure: impl FnOnce(&mut AliasSet) -> usize) -> Self {
        let mut aliases = AliasSet::for_cell();
        let unowned = measure(&mut aliases);
        verter_debug_assert::verter_debug_assert_eq!(
            unowned,
            0,
            "a delivered carrier keeps heap outside its owned handles"
        );
        aliases.take_handles()
    }

    /// Charge each to its callers if no owner has yet (see [`Owned::pin`]).
    pub fn pin(self, ctx: ChargeCtx<'_>) {
        for child in self.0.iter() {
            pin_node(&**child, ctx);
        }
    }

    /// A cache's holds on all of them, all or nothing (see
    /// [`Owned::hold`]).
    ///
    /// # Errors
    ///
    /// The account's refusal: nothing is retained and every existing
    /// holder, charge and class is unchanged.
    pub fn hold(self, ctx: ChargeCtx<'_>) -> Result<HeldHandles, RetentionRefusal> {
        hold_nodes(&self.0, ctx)
    }

    /// A cache's holds on all of them; on refusal they are charged to
    /// their callers instead, so a value returned uncached is charged
    /// whatever the pressure.
    ///
    /// # Errors
    ///
    /// The account's refusal: nothing is retained; every allocation is
    /// pinned for the callers keeping it.
    pub fn hold_or_pin(self, ctx: ChargeCtx<'_>) -> Result<HeldHandles, RetentionRefusal> {
        hold_nodes(&self.0, ctx).inspect_err(|_| self.pin(ctx))
    }

    /// `charge` holding these allocations too: the retaining cache's whole
    /// footprint as one token ([`RetentionCharge::holding`]).
    ///
    /// # Errors
    ///
    /// The account's refusal of a hold; `charge` is dropped with it, so
    /// nothing is retained.
    pub fn retained_with(
        self,
        charge: RetentionCharge,
        ctx: ChargeCtx<'_>,
    ) -> Result<RetentionCharge, RetentionRefusal> {
        Ok(charge.holding(self.hold(ctx)?))
    }

    /// Bytes the allocations' own charges hold now; an uncharged one counts
    /// nothing.
    #[must_use]
    pub fn charged_bytes(&self) -> usize {
        self.0
            .iter()
            .map(|node| {
                node.slot()
                    .lock()
                    .charge
                    .as_ref()
                    .map_or(0, RetentionCharge::bytes)
            })
            .sum()
    }

    /// Whether the carrier reached no owned allocation.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// How many owned allocations it reached.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

/// The pinned charge on an owned cell's block, shared by the cell and the
/// weak holders that may keep the block allocated after its value (see
/// [`Owned::new_with_shared_block`]). The share is its own allocation, so
/// it charges its own block too; it is released when its last clone drops.
#[derive(Clone)]
pub struct BlockShare(Arc<RetentionCharge>);

impl BlockShare {
    /// The block of `block` bytes and this share's own allocation, pinned.
    fn for_block(block: usize, ctx: ChargeCtx<'_>) -> Self {
        Self(Arc::new(ctx.account.pin(
            block + arc_block_bytes(std::mem::size_of::<RetentionCharge>()),
        )))
    }

    /// Bytes the share charges: the cell's block and its own.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.0.bytes()
    }

    /// Bytes a share on a cell of `T` charges.
    #[must_use]
    pub fn bytes_for<T: Measured>() -> usize {
        Owned::<T>::cell_block_bytes() + arc_block_bytes(std::mem::size_of::<RetentionCharge>())
    }
}

impl std::fmt::Debug for BlockShare {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("BlockShare").field(&self.bytes()).finish()
    }
}

/// Bytes a cell reaching `children` distinct owned allocations keeps for
/// its record of them for its whole life — part of its own charge.
#[must_use]
pub fn child_registration_bytes(children: usize) -> usize {
    registration_bytes(children)
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

    /// [`Self::new`] for an allocation weak holders (an interner) may keep
    /// allocated past its value. Its block is charged at once, pinned, by a
    /// [`BlockShare`] the cell keeps and each weak holder keeps a clone of
    /// beside its weak view: the block's charge lasts until the cell's value
    /// is gone AND every weak holder has let go — the block's own lifetime —
    /// in either order. The handle's own charge covers the value's heap.
    #[must_use]
    pub fn new_with_shared_block(value: T, ctx: ChargeCtx<'_>) -> (Self, BlockShare) {
        let share = BlockShare::for_block(Self::cell_block_bytes(), ctx);
        let owned = Self(Arc::new(Cell {
            slot: Mutex::new(Slot {
                block_share: Some(share.clone()),
                ..Slot::default()
            }),
            value,
        }));
        (owned, share)
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
        if !aliases.first_allocation(&self.0) {
            return 0;
        }
        if aliases.collect_child(|| Arc::clone(&self.0) as Arc<dyn Node>) {
            return 0;
        }
        let block = if self.0.slot.lock().block_share.is_some() {
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

    /// Owning handles of this allocation.
    #[must_use]
    pub fn strong_count(this: &Self) -> usize {
        Arc::strong_count(&this.0)
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

/// An owned slice: a sealed, self-charged list allocation. Derefs to the
/// slice; cloning shares the allocation and its one charge. An empty slice
/// allocates nothing, so it has no charge and nothing to hold.
pub struct OwnedSlice<T: Measured>(Option<Owned<Box<[T]>>>);

impl<T: Measured> OwnedSlice<T> {
    /// A new allocation of `items`, charged by the first owner that keeps
    /// it; none for no items.
    #[must_use]
    pub fn new(items: Box<[T]>) -> Self {
        Self((!items.is_empty()).then(|| Owned::new(items)))
    }

    /// Offer this nested handle to a measurement (see [`Owned::offer`]).
    pub fn offer(&self, aliases: &mut AliasSet) -> usize {
        self.0.as_ref().map_or(0, |owned| owned.offer(aliases))
    }

    /// Charge it to its callers (see [`Owned::pin`]).
    pub fn pin(&self, ctx: ChargeCtx<'_>) {
        if let Some(owned) = &self.0 {
            owned.pin(ctx);
        }
    }

    /// A cache's hold on it (see [`Owned::hold`]); an empty slice's holds
    /// nothing.
    ///
    /// # Errors
    ///
    /// The account's refusal.
    pub fn hold(&self, ctx: ChargeCtx<'_>) -> Result<OwnedHold, RetentionRefusal> {
        self.0
            .as_ref()
            .map_or(Ok(OwnedHold(None)), |owned| owned.hold(ctx))
    }

    /// Bytes its charge holds, once charged; an empty slice's is none.
    #[must_use]
    pub fn charge_bytes(&self) -> Option<usize> {
        self.0.as_ref().and_then(Owned::charge_bytes)
    }

    /// Its charge's class, once charged.
    #[must_use]
    pub fn charge_class(&self) -> Option<ChargeClass> {
        self.0.as_ref().and_then(Owned::charge_class)
    }

    /// Whether two slices share one allocation; two empty slices share
    /// none and compare as one.
    #[must_use]
    pub fn ptr_eq(this: &Self, other: &Self) -> bool {
        match (&this.0, &other.0) {
            (Some(this), Some(other)) => Owned::ptr_eq(this, other),
            (None, None) => true,
            _ => false,
        }
    }

    /// The allocation's address, for identity checks that never deref; `0`
    /// for an empty slice.
    #[must_use]
    pub fn addr(this: &Self) -> usize {
        this.0.as_ref().map_or(0, Owned::addr)
    }

    /// Owning handles of this allocation; `0` for an empty slice.
    #[must_use]
    pub fn strong_count(this: &Self) -> usize {
        this.0.as_ref().map_or(0, Owned::strong_count)
    }

    /// Bytes the cell's block holds beside the slice's backing.
    #[must_use]
    pub fn cell_block_bytes() -> usize {
        Owned::<Box<[T]>>::cell_block_bytes()
    }
}

impl<T: Measured> Clone for OwnedSlice<T> {
    /// Another owner of the same allocation: shares its one charge.
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Measured> std::ops::Deref for OwnedSlice<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        self.0.as_deref().map_or(&[], |items| items)
    }
}

impl<T: Measured> AsRef<[T]> for OwnedSlice<T> {
    fn as_ref(&self) -> &[T] {
        self
    }
}

impl<T: Measured> std::borrow::Borrow<[T]> for OwnedSlice<T> {
    fn borrow(&self) -> &[T] {
        self
    }
}

impl<'a, T: Measured> IntoIterator for &'a OwnedSlice<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<T: Measured> Default for OwnedSlice<T> {
    fn default() -> Self {
        Self(None)
    }
}

impl<T: Measured> From<Vec<T>> for OwnedSlice<T> {
    fn from(items: Vec<T>) -> Self {
        Self::new(items.into_boxed_slice())
    }
}

impl<T: Measured> From<Box<[T]>> for OwnedSlice<T> {
    fn from(items: Box<[T]>) -> Self {
        Self::new(items)
    }
}

impl<T: Measured, const N: usize> From<[T; N]> for OwnedSlice<T> {
    fn from(items: [T; N]) -> Self {
        Self::new(Box::new(items))
    }
}

impl<T: Measured + Clone> From<&[T]> for OwnedSlice<T> {
    fn from(items: &[T]) -> Self {
        Self::new(items.into())
    }
}

impl<T: Measured + Clone> From<&Vec<T>> for OwnedSlice<T> {
    fn from(items: &Vec<T>) -> Self {
        Self::new(items.as_slice().into())
    }
}

impl<T: Measured + Clone> From<std::sync::Arc<[T]>> for OwnedSlice<T> {
    /// An owned copy of a raw shared slice: a new allocation, charged by
    /// its own cell.
    fn from(items: std::sync::Arc<[T]>) -> Self {
        Self::new(items.iter().cloned().collect())
    }
}

impl From<std::sync::Arc<[Arc<str>]>> for OwnedSlice<OwnedStr> {
    /// Owned copies of a raw shared name list: new allocations, each
    /// charged by its own cell.
    fn from(names: std::sync::Arc<[Arc<str>]>) -> Self {
        names.iter().map(OwnedStr::from).collect()
    }
}

impl From<Box<[Arc<str>]>> for OwnedSlice<OwnedStr> {
    fn from(names: Box<[Arc<str>]>) -> Self {
        names.iter().map(OwnedStr::from).collect()
    }
}

impl From<&[Arc<str>]> for OwnedSlice<OwnedStr> {
    fn from(names: &[Arc<str>]) -> Self {
        names.iter().map(OwnedStr::from).collect()
    }
}

impl From<Vec<Arc<str>>> for OwnedSlice<OwnedStr> {
    fn from(names: Vec<Arc<str>>) -> Self {
        names.iter().map(OwnedStr::from).collect()
    }
}

impl From<&OwnedSlice<OwnedStr>> for std::sync::Arc<[Arc<str>]> {
    /// A raw shared copy of the names for a layer that keeps raw strings.
    fn from(names: &OwnedSlice<OwnedStr>) -> Self {
        names.iter().map(Arc::<str>::from).collect()
    }
}

impl From<OwnedSlice<OwnedStr>> for std::sync::Arc<[Arc<str>]> {
    fn from(names: OwnedSlice<OwnedStr>) -> Self {
        names.iter().map(Arc::<str>::from).collect()
    }
}

impl<T: Measured> FromIterator<T> for OwnedSlice<T> {
    fn from_iter<I: IntoIterator<Item = T>>(items: I) -> Self {
        Self::new(items.into_iter().collect())
    }
}

impl<T: Measured + PartialEq> PartialEq for OwnedSlice<T> {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl<T: Measured + Eq> Eq for OwnedSlice<T> {}

impl<T: Measured + PartialEq> PartialEq<[T]> for OwnedSlice<T> {
    fn eq(&self, other: &[T]) -> bool {
        **self == *other
    }
}

impl<T: Measured + PartialEq, const N: usize> PartialEq<[T; N]> for OwnedSlice<T> {
    fn eq(&self, other: &[T; N]) -> bool {
        **self == other[..]
    }
}

impl<T: Measured + PartialEq> PartialEq<Vec<T>> for OwnedSlice<T> {
    fn eq(&self, other: &Vec<T>) -> bool {
        **self == other[..]
    }
}

impl<T: Measured + std::hash::Hash> std::hash::Hash for OwnedSlice<T> {
    /// Hashes as its slice.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (**self).hash(state);
    }
}

impl<T: Measured + PartialOrd> PartialOrd for OwnedSlice<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        (**self).partial_cmp(&**other)
    }
}

impl<T: Measured + Ord> Ord for OwnedSlice<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (**self).cmp(&**other)
    }
}

impl<T: Measured + std::fmt::Debug> std::fmt::Debug for OwnedSlice<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        (**self).fmt(f)
    }
}

impl<T: Measured> Measured for OwnedSlice<T> {
    /// A nested owned slice: charged by its own cell.
    fn heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        self.offer(aliases)
    }
}

impl<T: Measured + serde::Serialize> serde::Serialize for OwnedSlice<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (**self).serialize(serializer)
    }
}

impl<'de, T: Measured + serde::Deserialize<'de>> serde::Deserialize<'de> for OwnedSlice<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Vec::<T>::deserialize(deserializer).map(Self::from)
    }
}

impl<T: Measured + verter_no_typeexpr::__private::NoTypeExprWitness>
    verter_no_typeexpr::__private::NoTypeExprWitness for OwnedSlice<T>
{
}
impl<T: Measured + verter_no_storedspan::__private::NoStoredSpanWitness>
    verter_no_storedspan::__private::NoStoredSpanWitness for OwnedSlice<T>
{
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

    /// Whether `owned` is the allocation this view names. The view keeps
    /// the block allocated, so its address is never reused while it lives.
    #[must_use]
    pub fn is(&self, owned: &Owned<T>) -> bool {
        std::ptr::eq(self.0.as_ptr(), Arc::as_ptr(&owned.0))
    }
}

impl<T: Measured> Clone for WeakOwned<T> {
    fn clone(&self) -> Self {
        Self(Weak::clone(&self.0))
    }
}

/// A cache's hold on one owned allocation (none for an empty slice, which
/// allocates nothing). See the module docs.
pub struct OwnedHold(Option<Arc<dyn Node>>);

impl OwnedHold {
    /// Bytes the held allocation's charge holds.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.0.as_ref().map_or(0, |node| {
            node.slot()
                .lock()
                .charge
                .as_ref()
                .map_or(0, RetentionCharge::bytes)
        })
    }

    /// The held allocation's charge class; an empty hold's is
    /// [`ChargeClass::Retained`], the class a hold keeps.
    #[must_use]
    pub fn class(&self) -> ChargeClass {
        self.0.as_ref().map_or(ChargeClass::Retained, |node| {
            node.slot()
                .lock()
                .charge
                .as_ref()
                .expect("a held allocation is charged")
                .class()
        })
    }
}

impl Drop for OwnedHold {
    fn drop(&mut self) {
        let Some(node) = &self.0 else {
            return;
        };
        let released = {
            let mut guard = node.slot().lock();
            let slot = &mut *guard;
            slot.cache_holds -= 1;
            if slot.cache_holds == 0 {
                // The remaining handles are callers now: the charge is
                // theirs. A hold that was the last handle frees the cell
                // right after, releasing the pin with it.
                slot.charge
                    .as_mut()
                    .expect("a held allocation is charged")
                    .transfer_to_pinned();
                slot.nested
                    .as_mut()
                    .map(|nested| std::mem::take(&mut nested.holds))
            } else {
                None
            }
        };
        // The children transfer to whoever else still holds them.
        drop(released);
    }
}

impl std::fmt::Debug for OwnedHold {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Some(node) = &self.0 else {
            return f.write_str("OwnedHold(empty)");
        };
        let slot = node.slot().lock();
        f.debug_struct("OwnedHold")
            .field(
                "bytes",
                &slot.charge.as_ref().map_or(0, RetentionCharge::bytes),
            )
            .field("cache_holds", &slot.cache_holds)
            .finish()
    }
}

/// A weak view of an [`OwnedStr`] allocation (see [`OwnedStr::downgrade`]).
#[derive(Clone)]
pub struct WeakOwnedStr(WeakOwned<Box<str>>);

impl WeakOwnedStr {
    /// Whether `owned` is the string allocation this view names.
    #[must_use]
    pub fn is(&self, owned: &OwnedStr) -> bool {
        self.0.is(&owned.0)
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

    /// A weak view that names this string's allocation without keeping its
    /// text or its charge: an identity check that outlives no owner.
    #[must_use]
    pub fn downgrade(this: &Self) -> WeakOwnedStr {
        WeakOwnedStr(Owned::downgrade(&this.0))
    }

    /// Bytes one owned string of `len` bytes is charged.
    #[must_use]
    pub fn charge_for_len(len: usize) -> usize {
        Owned::<Box<str>>::cell_block_bytes() + heap_block_bytes(len)
    }

    /// Owning handles of this string's allocation.
    #[must_use]
    pub fn strong_count(this: &Self) -> usize {
        Owned::strong_count(&this.0)
    }

    /// The allocation's address, for identity checks that never deref.
    #[must_use]
    pub fn addr(this: &Self) -> usize {
        Owned::addr(&this.0)
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

impl From<&OwnedStr> for OwnedStr {
    /// Another owning handle of the same allocation.
    fn from(text: &OwnedStr) -> Self {
        text.clone()
    }
}

impl From<&String> for OwnedStr {
    fn from(text: &String) -> Self {
        Self::new(text)
    }
}

impl PartialEq<String> for OwnedStr {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other.as_str()
    }
}

impl PartialEq<Arc<str>> for OwnedStr {
    fn eq(&self, other: &Arc<str>) -> bool {
        self.as_str() == &**other
    }
}

impl PartialEq<OwnedStr> for Arc<str> {
    fn eq(&self, other: &OwnedStr) -> bool {
        &**self == other.as_str()
    }
}

impl From<OwnedStr> for Arc<str> {
    /// A raw shared copy of the text for a layer that keeps raw strings: a
    /// new allocation, owned and charged by whoever keeps it.
    fn from(text: OwnedStr) -> Self {
        Arc::from(text.as_str())
    }
}

impl From<&OwnedStr> for Arc<str> {
    /// See the by-value conversion.
    fn from(text: &OwnedStr) -> Self {
        Arc::from(text.as_str())
    }
}

impl PartialEq<OwnedStr> for str {
    fn eq(&self, other: &OwnedStr) -> bool {
        self == other.as_str()
    }
}

impl PartialEq<OwnedStr> for &str {
    fn eq(&self, other: &OwnedStr) -> bool {
        *self == other.as_str()
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

impl serde::Serialize for OwnedStr {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for OwnedStr {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::from)
    }
}

impl<T: Measured + serde::Serialize> serde::Serialize for Owned<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.value.serialize(serializer)
    }
}

impl<'de, T: Measured + serde::Deserialize<'de>> serde::Deserialize<'de> for Owned<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(deserializer).map(Self::new)
    }
}

// An owned allocation owns exactly its value: it carries a type expression
// or a stored span exactly when the value does. An owned string is text.
impl verter_no_typeexpr::__private::NoTypeExprWitness for OwnedStr {}
impl verter_no_storedspan::__private::NoStoredSpanWitness for OwnedStr {}
impl<T: Measured + verter_no_typeexpr::__private::NoTypeExprWitness>
    verter_no_typeexpr::__private::NoTypeExprWitness for Owned<T>
{
}
impl<T: Measured + verter_no_storedspan::__private::NoStoredSpanWitness>
    verter_no_storedspan::__private::NoStoredSpanWitness for Owned<T>
{
}

impl<T: Measured> Measured for Owned<T> {
    /// An owned allocation embedded in a measured value is a nested
    /// allocation, charged by its own cell.
    fn heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        self.offer(aliases)
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
