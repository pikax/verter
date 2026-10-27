//! One allocation's charge, shared by every alias of it and handed from
//! cache retention to the callers that outlive the cache.
//!
//! An allocation a cache retains is often aliased: sibling candidates share
//! one fact carrier, a warm read hands the cached diagnostics to its caller,
//! one interned dependency list backs many entries. Charging each alias
//! would count one allocation many times; charging only the cache would
//! leave the allocation uncharged the moment the cache drains while a caller
//! still holds it. A shared charge does neither: it is ONE reservation, held
//! by every alias, released exactly once when the last of them drops.
//!
//! # Holders
//!
//! * A [`CacheHold`] is a cache's ownership of the allocation. While any
//!   cache hold lives, the charge is [`ChargeClass::Retained`] — refusable,
//!   discretionary retention.
//! * A [`CallerHold`] is a caller's alias: a delivered read, a joined
//!   subscriber. It keeps the charge alive but does not make it retained.
//!
//! When the last cache hold drops while callers remain, the charge is
//! transferred to [`ChargeClass::Pinned`] in one step at the account's
//! linearization point ([`RetentionCharge::transfer_to_pinned`]): there is
//! no interval in which the bytes are uncharged and none in which they are
//! charged twice, and a pin is never refused, so pressure can never revoke
//! held data. A cache that later retains the same allocation again adopts
//! the pin back into retention, refusably ([`CallerHold::adopt`]); a refusal
//! leaves the pin and the caller's data untouched.
//!
//! The holders are counted explicitly, under the charge's own lock, so the
//! class always matches who holds the allocation, and the last holder's
//! release is decided in the same critical section that would have to
//! admit a new holder.
//!
//! # Exactly once across independent owners
//!
//! [`AliasCharges`] keys a charge by the allocation's address, so two caches
//! that never see each other's handles still share one charge for one
//! allocation. A registered charge co-owns the allocation it is keyed by, so
//! the address cannot be freed and reused while the registration lives. The
//! last holder's release removes the registration and releases the charge
//! under the registry lock, and a new first holder reserves under the same
//! lock, so a teardown in flight is either revived or finished before a
//! fresh charge exists: the registry holds exactly one entry per live
//! shared charge — bounded by the allocations actually retained, never by
//! history — and never two charges for one allocation.
//!
//! The registry's own table is charged by its capacity, pinned for as long
//! as the table holds it, and the table gives its backing back as
//! registrations drain: a registry whose every charge is released holds no
//! backing and no charge.
//!
//! # Leaves
//!
//! A registered carrier names the leaf allocations it reaches (its leaf
//! `Arc<str>` names) and holds each leaf's own registered charge, so a leaf
//! two carriers share is charged once. A carrier holds its leaves in its
//! own class: as a cache while any cache holds the carrier, as a caller
//! otherwise — the carrier's transfer to its callers moves its leaf holds
//! with it, and a cache adopting the carrier adopts its leaves too, or
//! neither. A leaf is released with the last carrier holding it.
//!
//! A registered carrier's owned handles (an owned name in a fact key) are
//! charged by their own cells: the carrier pins each when it is first
//! charged for a caller, and holds each as a cache while a cache holds the
//! carrier, all or nothing, exactly as it holds its leaves.
//!
//! # Caller-first charges
//!
//! An allocation a caller receives before any cache retains it — a
//! freshly computed result the cache may then refuse — is charged the
//! moment the caller holds it ([`AliasCharges::pin_caller`]): pinned,
//! never refused. A cache that later retains it adopts that one charge
//! refusably, exactly as it adopts a charge its own callers kept alive.

use std::sync::Arc;

use parking_lot::Mutex;
use rustc_hash::FxHashMap;

use super::allocation::{arc_block_bytes, heap_block_bytes, AliasSet};
use super::granular::GranularPin;
use super::owned::{hold_nodes, pin_node, ChargeCtx, HeldHandles, Node};
use super::{
    ChargeClass, RetentionAdmission, RetentionCharge, RetentionRefusal, SemanticRetentionAccount,
};

/// The charge one allocation's aliases share.
struct SharedCharge {
    state: Mutex<SharedState>,
    /// The registry this charge is keyed in, and its key.
    registration: Option<(Arc<Registry>, usize)>,
    /// Keeps the keyed allocation alive for as long as its registration, so
    /// its address names it for the registration's whole life.
    _allocation: Option<Box<dyn Send + Sync>>,
}

struct SharedState {
    /// `None` once released by the last holder.
    charge: Option<RetentionCharge>,
    cache_holds: usize,
    caller_holds: usize,
    /// The holds this allocation keeps on its leaves and its owned
    /// handles, in its own class.
    leaves: LeafHolds,
    /// The owned handles the allocation reaches, each charged by its own
    /// cell.
    children: Box<[Arc<dyn Node>]>,
}

/// A carrier's holds on its leaves and owned handles: cache holds while the
/// carrier is retained; while only callers hold it, caller holds on its
/// leaves, and its handles are their own callers' (pinned).
enum LeafHolds {
    Cache {
        leaves: Box<[CacheHold]>,
        /// Held for their release on drop.
        _children: HeldHandles,
    },
    Caller(Box<[CallerHold]>),
}

impl LeafHolds {
    fn none() -> Self {
        Self::Cache {
            leaves: Box::new([]),
            _children: HeldHandles::default(),
        }
    }
}

impl SharedState {
    fn charge(&self) -> &RetentionCharge {
        self.charge
            .as_ref()
            .expect("a charge some holder references is not yet released")
    }
}

impl SharedCharge {
    fn new(charge: RetentionCharge) -> Self {
        Self::with_holds(charge, 1, 0)
    }

    fn with_holds(charge: RetentionCharge, cache_holds: usize, caller_holds: usize) -> Self {
        Self {
            state: Mutex::new(SharedState {
                charge: Some(charge),
                cache_holds,
                caller_holds,
                leaves: LeafHolds::none(),
                children: Box::new([]),
            }),
            registration: None,
            _allocation: None,
        }
    }

    /// One more cache hold: shares a retained charge, or adopts a charge
    /// only callers hold back into retention. `Err` leaves every holder and
    /// the class unchanged. Called with `state` locked — possibly under the
    /// registry lock, so it never drops a hold whose release could take it.
    fn add_cache_hold(state: &mut SharedState) -> Result<(), RetentionRefusal> {
        if state.cache_holds == 0 {
            // The leaves are adopted first; a refusal of any of them, or of
            // the allocation itself, drops the adopted leaf holds, which
            // hand those leaves straight back to the callers still holding
            // them through this allocation — none is released. Leaves still
            // held as a cache's belong to an allocation whose last cache
            // hold is releasing it with no caller left: the revival keeps
            // them, and its charge is still retained.
            let adopted = match &state.leaves {
                LeafHolds::Caller(callers) => {
                    let leaves = callers
                        .iter()
                        .map(CallerHold::adopt)
                        .collect::<Result<Box<[CacheHold]>, _>>()?;
                    let account = state
                        .charge()
                        .account()
                        .expect("a reachable charge is not yet released");
                    let children = hold_nodes(&state.children, ChargeCtx::new(account))?;
                    Some((leaves, children))
                }
                LeafHolds::Cache { .. } => None,
            };
            state
                .charge
                .as_mut()
                .expect("a reachable charge is not yet released")
                .try_adopt_retained()?;
            if let Some((leaves, children)) = adopted {
                // The callers' leaf holds drop after their leaves gained a
                // cache hold: nothing is released.
                state.leaves = LeafHolds::Cache {
                    leaves,
                    _children: children,
                };
            }
        }
        state.cache_holds += 1;
        Ok(())
    }

    /// One more caller hold. A revival of an allocation whose last cache
    /// hold is releasing it with no caller left hands its charge and its
    /// leaves to the reviving caller, exactly as a cache hold dropping
    /// beside a live caller would. Called with `state` locked — possibly
    /// under the registry lock: the leaf cache holds it drops each keep the
    /// caller hold just taken, so none is released.
    fn add_caller_hold(state: &mut SharedState) {
        state.caller_holds += 1;
        if state.cache_holds == 0 {
            state
                .charge
                .as_mut()
                .expect("a reachable charge is not yet released")
                .transfer_to_pinned();
            Self::hand_leaves_to_callers(state);
        }
    }

    /// Move this allocation's leaf holds to its callers, before its last
    /// cache hold is dropped. Called with `state` locked.
    fn hand_leaves_to_callers(state: &mut SharedState) {
        if let LeafHolds::Cache { leaves, .. } = &state.leaves {
            let callers = leaves.iter().map(CacheHold::caller).collect();
            // The cache holds drop here, after their leaves gained a caller:
            // each leaf transfers to its callers, never released, and each
            // owned handle's charge moves to its own callers (this carrier
            // among them).
            state.leaves = LeafHolds::Caller(callers);
        }
    }

    /// Release the charge once its last holder is gone.
    fn release_if_unheld(&self) {
        match &self.registration {
            None => {
                let released = {
                    let mut state = self.state.lock();
                    if state.cache_holds + state.caller_holds == 0 {
                        Some((
                            state.charge.take(),
                            std::mem::replace(&mut state.leaves, LeafHolds::none()),
                        ))
                    } else {
                        None
                    }
                };
                drop(released);
            }
            Some((registry, key)) => {
                let mut table = registry.table.lock();
                let mut state = self.state.lock();
                // A holder may have revived the charge between the last
                // release and this lock: then it is not released.
                if state.cache_holds + state.caller_holds != 0 {
                    return;
                }
                // The charge is released before the registry is: a new first
                // holder reserving under the registry lock never overlaps it.
                drop(state.charge.take());
                // The leaf holds are released once both locks are free: a
                // leaf's own release takes the registry lock.
                let leaves = std::mem::replace(&mut state.leaves, LeafHolds::none());
                drop(state);
                let unregistered = if table
                    .entries
                    .get(key)
                    .is_some_and(|entry| std::ptr::eq(Arc::as_ptr(entry), self))
                {
                    table.entries.remove(key)
                } else {
                    None
                };
                table.reclaim();
                drop(table);
                // The registry's own reference is dropped outside its lock.
                drop(unregistered);
                drop(leaves);
            }
        }
    }
}

/// A cache's ownership of one charged allocation. See the module docs.
pub struct CacheHold(Arc<SharedCharge>);

impl CacheHold {
    /// Reserve `bytes` as a new retained allocation's charge.
    ///
    /// # Errors
    ///
    /// The account's refusal: the cache keeps nothing and its caller
    /// returns the value uncached.
    pub fn reserve(
        account: &Arc<SemanticRetentionAccount>,
        bytes: usize,
    ) -> Result<Self, RetentionRefusal> {
        match account.reserve(ChargeClass::Retained, bytes) {
            RetentionAdmission::Admitted(charge) => Ok(Self::from_retained(charge)),
            RetentionAdmission::Refused(refusal) => Err(refusal),
        }
    }

    /// Take ownership of a refusable charge a cache was just granted: the
    /// charge is already retained, and becomes this hold's shared charge.
    /// A pin is a caller's obligation, never a cache grant: a cache adopts
    /// one only through [`CallerHold::adopt`], which decides the adoption
    /// refusably. An in-flight charge is decided the same way — its owner
    /// hands it to the cache through [`CallerHold::adopt`] too.
    #[must_use]
    pub fn from_retained(charge: RetentionCharge) -> Self {
        let class = charge.class();
        verter_debug_assert::verter_debug_assert_ne!(
            class,
            ChargeClass::Pinned,
            "a cache hold is minted from a granted refusable charge"
        );
        verter_debug_assert::verter_debug_assert_ne!(
            class,
            ChargeClass::Active,
            "a cache hold adopts in-flight charges through CallerHold::adopt"
        );
        Self(Arc::new(SharedCharge::new(charge.commit_retained())))
    }

    /// A caller's alias of the same allocation: it keeps the charge alive,
    /// pinned, after every cache hold is gone.
    #[must_use]
    pub fn caller(&self) -> CallerHold {
        self.0.state.lock().caller_holds += 1;
        CallerHold(Arc::clone(&self.0))
    }

    /// Bytes the shared charge holds.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.0.state.lock().charge().bytes()
    }

    /// The shared charge's current class.
    #[must_use]
    pub fn class(&self) -> ChargeClass {
        self.0.state.lock().charge().class()
    }

    /// Whether two holds share one charge.
    #[must_use]
    pub fn shares_charge_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Clone for CacheHold {
    /// Another cache owner of the same allocation (a sibling candidate, a
    /// reader snapshot of the cache's entry): shares the one charge.
    fn clone(&self) -> Self {
        self.0.state.lock().cache_holds += 1;
        Self(Arc::clone(&self.0))
    }
}

impl Drop for CacheHold {
    fn drop(&mut self) {
        let unheld = {
            let mut state = self.0.state.lock();
            state.cache_holds -= 1;
            if state.cache_holds == 0 && state.caller_holds > 0 {
                // Callers still hold the allocation: its charge, and its
                // leaves', are theirs now.
                state
                    .charge
                    .as_mut()
                    .expect("a held charge is not yet released")
                    .transfer_to_pinned();
                SharedCharge::hand_leaves_to_callers(&mut state);
            }
            state.cache_holds + state.caller_holds == 0
        };
        if unheld {
            self.0.release_if_unheld();
        }
    }
}

impl std::fmt::Debug for CacheHold {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.0.state.lock();
        f.debug_struct("CacheHold")
            .field("bytes", &state.charge().bytes())
            .field("class", &state.charge().class())
            .field("cache_holds", &state.cache_holds)
            .field("caller_holds", &state.caller_holds)
            .finish()
    }
}

/// A caller's alias of one charged allocation. See the module docs.
pub struct CallerHold(Arc<SharedCharge>);

impl CallerHold {
    /// A new caller-first charge of `bytes` for an allocation no registry
    /// keys: pinned, never refused, until a cache retaining the same
    /// allocation adopts it ([`Self::adopt`]).
    #[must_use]
    pub fn pin(account: &Arc<SemanticRetentionAccount>, bytes: usize) -> Self {
        Self(Arc::new(SharedCharge::with_holds(account.pin(bytes), 0, 1)))
    }

    /// A new caller-first charge of IN-FLIGHT `bytes`: the computation
    /// that owns them has not yet published or handed its value over, so
    /// they are charged [`ChargeClass::Active`] — unconditionally, never
    /// refused — until a cache retaining the allocation adopts them
    /// (committing the charge to retention) or the delivery hands them to
    /// their caller ([`Self::hand_off_in_flight`], transferring the
    /// charge to a pin).
    #[must_use]
    pub fn in_flight(account: &Arc<SemanticRetentionAccount>, bytes: usize) -> Self {
        Self(Arc::new(SharedCharge::with_holds(
            account.reserve_in_flight(bytes),
            0,
            1,
        )))
    }

    /// Settle this hold's still-in-flight charge at its caller handoff:
    /// an [`ChargeClass::Active`] charge becomes
    /// [`ChargeClass::Pinned`] — the delivery is complete and its
    /// callers hold it — while a retained or pinned charge keeps its
    /// class. Never released, never charged twice.
    pub fn hand_off_in_flight(&self) {
        let mut state = self.0.state.lock();
        if state.cache_holds == 0 {
            if let Some(charge) = state.charge.as_mut() {
                if charge.class() == ChargeClass::Active {
                    charge.transfer_to_pinned();
                }
            }
        }
    }

    /// A cache retaining the allocation this caller already holds: shares a
    /// still-retained charge, or adopts the callers' pin back into
    /// retention.
    ///
    /// # Errors
    ///
    /// The account's refusal of the adoption: the cache keeps nothing, and
    /// the callers' pin and data are unchanged.
    pub fn adopt(&self) -> Result<CacheHold, RetentionRefusal> {
        SharedCharge::add_cache_hold(&mut self.0.state.lock())?;
        Ok(CacheHold(Arc::clone(&self.0)))
    }

    /// Bytes the shared charge holds.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.0.state.lock().charge().bytes()
    }

    /// The shared charge's current class.
    #[must_use]
    pub fn class(&self) -> ChargeClass {
        self.0.state.lock().charge().class()
    }

    /// The shared charge's address: every caller and cache hold on it names
    /// the same one. For identity checks that never deref.
    #[cfg(any(test, feature = "test-support", feature = "soak-support"))]
    pub(crate) fn identity(&self) -> usize {
        Arc::as_ptr(&self.0).cast::<()>() as usize
    }
}

impl Clone for CallerHold {
    fn clone(&self) -> Self {
        self.0.state.lock().caller_holds += 1;
        Self(Arc::clone(&self.0))
    }
}

impl Drop for CallerHold {
    fn drop(&mut self) {
        let unheld = {
            let mut state = self.0.state.lock();
            state.caller_holds -= 1;
            state.cache_holds + state.caller_holds == 0
        };
        if unheld {
            self.0.release_if_unheld();
        }
    }
}

impl std::fmt::Debug for CallerHold {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.0.state.lock();
        f.debug_struct("CallerHold")
            .field("bytes", &state.charge().bytes())
            .field("class", &state.charge().class())
            .finish()
    }
}

#[derive(Default)]
struct Registry {
    table: Mutex<RegistryTable>,
}

/// The registry's table and the pinned charge of its backing.
#[derive(Default)]
struct RegistryTable {
    entries: FxHashMap<usize, Arc<SharedCharge>>,
    /// The table's backing, charged by capacity while the table holds it.
    backing: Option<GranularPin>,
}

/// Granule the registry's table backing is charged in.
const REGISTRY_TABLE_GRANULE: usize = 1024;

/// Bytes one bucket of the registry's table costs: its `(key, charge)`
/// slot and its control byte.
const REGISTRY_BUCKET_BYTES: usize = std::mem::size_of::<(usize, Arc<SharedCharge>)>() + 1;

impl RegistryTable {
    /// Settle the backing charge with the table's current capacity.
    fn settle(&mut self, account: &Arc<SemanticRetentionAccount>) {
        let bytes = self.entries.capacity() * REGISTRY_BUCKET_BYTES;
        let backing = self
            .backing
            .get_or_insert_with(|| GranularPin::new(account, REGISTRY_TABLE_GRANULE));
        if bytes > backing.used() {
            backing.add(bytes - backing.used());
        } else {
            backing.sub(backing.used() - bytes);
        }
    }

    /// Give back backing a drained table no longer needs — all of it once
    /// the table is empty — and settle the charge with what remains.
    fn reclaim(&mut self) {
        let len = self.entries.len();
        if len == 0 {
            self.entries = FxHashMap::default();
        } else if len * 4 <= self.entries.capacity() {
            self.entries.shrink_to(len * 2);
        }
        let bytes = self.entries.capacity() * REGISTRY_BUCKET_BYTES;
        if let Some(backing) = self.backing.as_mut() {
            if bytes > backing.used() {
                backing.add(bytes - backing.used());
            } else {
                backing.sub(backing.used() - bytes);
            }
        }
    }

    /// Shrink the table to the capacity its live registrations need and
    /// charge exactly that: the capacity the same registrations had before
    /// a burst of others grew the table. Per-release reclamation keeps its
    /// hysteresis so a table hovering at a size class is not rehashed on
    /// every release; this runs only at an owner's release boundary.
    fn reclaim_exact(&mut self) {
        self.entries.shrink_to(self.entries.len());
        self.reclaim();
        if let Some(backing) = self.backing.as_mut() {
            backing.release_slack();
        }
    }
}

/// Bytes one registered shared charge adds beside the allocation it keys:
/// the charge's own block and the keep-alive box. The registry's table
/// slot is charged with the table's backing.
#[must_use]
pub fn registration_bytes() -> usize {
    arc_block_bytes(std::mem::size_of::<SharedCharge>())
        + heap_block_bytes(std::mem::size_of::<usize>())
}

/// Bytes a registered carrier's own bookkeeping keeps for as long as it is
/// registered: the slice of holds on its `leaves` leaf names (cache or
/// caller holds, one pointer each) and the slice recording its `children`
/// owned allocations. Neither allocates when empty.
fn bookkeeping_bytes(leaves: usize, children: usize) -> usize {
    const _: () = assert!(std::mem::size_of::<CacheHold>() == std::mem::size_of::<CallerHold>());
    heap_block_bytes(leaves * std::mem::size_of::<CacheHold>())
        + heap_block_bytes(children * std::mem::size_of::<Arc<dyn Node>>())
}

/// Charges keyed by allocation, so independently charged owners of one
/// allocation share one charge. See the module docs.
#[derive(Clone, Default)]
pub struct AliasCharges(Arc<Registry>);

impl AliasCharges {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A cache hold on `allocation`'s charge: the existing shared charge
    /// when any holder still keeps one (adopted back into retention if only
    /// callers hold it), otherwise a fresh reservation of what `measure`
    /// counts plus [`registration_bytes`], holding a cache hold on every leaf
    /// `measure` reaches. `measure` runs only when a fresh reservation is
    /// needed, over a collecting [`AliasSet`]: the leaves it reaches are
    /// charged as their own allocations, not counted in its figure.
    ///
    /// # Errors
    ///
    /// The account's refusal of the reservation or adoption — of the
    /// allocation or of any of its leaves. Nothing is registered and every
    /// existing holder is unchanged.
    pub fn hold<T>(
        &self,
        account: &Arc<SemanticRetentionAccount>,
        allocation: &Arc<T>,
        measure: impl FnOnce(&mut AliasSet) -> usize,
    ) -> Result<CacheHold, RetentionRefusal>
    where
        T: ?Sized + Send + Sync + 'static,
    {
        let mut deferred = Vec::new();
        let mut table = self.0.table.lock();
        let held = self.hold_in(&mut table, account, allocation, measure, &mut deferred);
        drop(table);
        // A refused registration's leaf holds are released here, outside
        // the registry lock their release takes.
        drop(deferred);
        held
    }

    /// [`Self::hold`] under the held registry lock. Leaf holds a refusal
    /// rolls back are moved to `deferred`, to be dropped once the lock is
    /// free.
    fn hold_in<T>(
        &self,
        table: &mut RegistryTable,
        account: &Arc<SemanticRetentionAccount>,
        allocation: &Arc<T>,
        measure: impl FnOnce(&mut AliasSet) -> usize,
        deferred: &mut Vec<CacheHold>,
    ) -> Result<CacheHold, RetentionRefusal>
    where
        T: ?Sized + Send + Sync + 'static,
    {
        let key = Arc::as_ptr(allocation).cast::<()>() as usize;
        if let Some(existing) = table.entries.get(&key) {
            // Revives a charge whose last holder is releasing it: its
            // release re-checks the holders under this same lock.
            SharedCharge::add_cache_hold(&mut existing.state.lock())?;
            return Ok(CacheHold(Arc::clone(existing)));
        }
        let mut aliases = AliasSet::collecting();
        let bytes = measure(&mut aliases);
        let leaves = aliases.take_leaves();
        let children = aliases.take_handles().into_children();
        // Reserved under the registry lock, so two first holders of one
        // allocation cannot both reserve it.
        let charge = match account.reserve(
            ChargeClass::Retained,
            bytes + registration_bytes() + bookkeeping_bytes(leaves.len(), children.len()),
        ) {
            RetentionAdmission::Admitted(charge) => charge,
            RetentionAdmission::Refused(refusal) => return Err(refusal),
        };
        // A refused child hold drops the child holds taken so far; an owned
        // cell's release never takes the registry lock.
        let child_holds = hold_nodes(&children, ChargeCtx::new(account))?;
        let mut leaf_holds = Vec::with_capacity(leaves.len());
        for leaf in &leaves {
            match self.hold_leaf_in(table, account, leaf) {
                Ok(hold) => leaf_holds.push(hold),
                Err(refusal) => {
                    deferred.extend(leaf_holds);
                    return Err(refusal);
                }
            }
        }
        let shared = SharedCharge::new(charge);
        {
            let mut state = shared.state.lock();
            state.leaves = LeafHolds::Cache {
                leaves: leaf_holds.into_boxed_slice(),
                _children: child_holds,
            };
            state.children = children;
        }
        Ok(CacheHold(
            self.register(table, account, key, allocation, shared),
        ))
    }

    /// A caller's hold on `allocation`'s charge: the existing shared charge
    /// when any holder still keeps one, otherwise a fresh PINNED charge of
    /// what `measure` counts plus [`registration_bytes`], holding a caller
    /// hold on every leaf it reaches — never refused, so a caller's live
    /// data is charged whatever the pressure. A cache retaining the
    /// allocation later adopts this charge ([`Self::hold`]).
    pub fn pin_caller<T>(
        &self,
        account: &Arc<SemanticRetentionAccount>,
        allocation: &Arc<T>,
        measure: impl FnOnce(&mut AliasSet) -> usize,
    ) -> CallerHold
    where
        T: ?Sized + Send + Sync + 'static,
    {
        let mut table = self.0.table.lock();
        self.pin_in(&mut table, account, allocation, measure)
    }

    /// [`Self::pin_caller`] under the held registry lock.
    fn pin_in<T>(
        &self,
        table: &mut RegistryTable,
        account: &Arc<SemanticRetentionAccount>,
        allocation: &Arc<T>,
        measure: impl FnOnce(&mut AliasSet) -> usize,
    ) -> CallerHold
    where
        T: ?Sized + Send + Sync + 'static,
    {
        let key = Arc::as_ptr(allocation).cast::<()>() as usize;
        if let Some(existing) = table.entries.get(&key) {
            SharedCharge::add_caller_hold(&mut existing.state.lock());
            return CallerHold(Arc::clone(existing));
        }
        let mut aliases = AliasSet::collecting();
        let bytes = measure(&mut aliases);
        let leaves = aliases.take_leaves();
        let children = aliases.take_handles().into_children();
        for child in children.iter() {
            pin_node(&**child, ChargeCtx::new(account));
        }
        let charge = account
            .pin(bytes + registration_bytes() + bookkeeping_bytes(leaves.len(), children.len()));
        let leaf_holds: Box<[CallerHold]> = leaves
            .iter()
            .map(|leaf| self.pin_leaf_in(table, account, leaf))
            .collect();
        let shared = SharedCharge::with_holds(charge, 0, 1);
        {
            let mut state = shared.state.lock();
            state.leaves = LeafHolds::Caller(leaf_holds);
            state.children = children;
        }
        CallerHold(self.register(table, account, key, allocation, shared))
    }

    /// A cache hold on one leaf's own charge, under the held registry lock.
    /// A leaf reaches nothing further, so its refusal needs no rollback.
    fn hold_leaf_in(
        &self,
        table: &mut RegistryTable,
        account: &Arc<SemanticRetentionAccount>,
        leaf: &Arc<str>,
    ) -> Result<CacheHold, RetentionRefusal> {
        let key = Arc::as_ptr(leaf).cast::<()>() as usize;
        if let Some(existing) = table.entries.get(&key) {
            SharedCharge::add_cache_hold(&mut existing.state.lock())?;
            return Ok(CacheHold(Arc::clone(existing)));
        }
        let bytes = arc_block_bytes(leaf.len()) + registration_bytes();
        let charge = match account.reserve(ChargeClass::Retained, bytes) {
            RetentionAdmission::Admitted(charge) => charge,
            RetentionAdmission::Refused(refusal) => return Err(refusal),
        };
        Ok(CacheHold(self.register(
            table,
            account,
            key,
            leaf,
            SharedCharge::new(charge),
        )))
    }

    /// A caller hold on one leaf's own charge, under the held registry
    /// lock; never refused.
    fn pin_leaf_in(
        &self,
        table: &mut RegistryTable,
        account: &Arc<SemanticRetentionAccount>,
        leaf: &Arc<str>,
    ) -> CallerHold {
        let key = Arc::as_ptr(leaf).cast::<()>() as usize;
        if let Some(existing) = table.entries.get(&key) {
            SharedCharge::add_caller_hold(&mut existing.state.lock());
            return CallerHold(Arc::clone(existing));
        }
        let charge = account.pin(arc_block_bytes(leaf.len()) + registration_bytes());
        CallerHold(self.register(
            table,
            account,
            key,
            leaf,
            SharedCharge::with_holds(charge, 0, 1),
        ))
    }

    /// A cache hold on one leaf name's own registered charge, shared with
    /// every other holder of the same allocation.
    ///
    /// # Errors
    ///
    /// The account's refusal of the reservation or adoption.
    pub fn hold_leaf(
        &self,
        account: &Arc<SemanticRetentionAccount>,
        leaf: &Arc<str>,
    ) -> Result<CacheHold, RetentionRefusal> {
        let mut table = self.0.table.lock();
        self.hold_leaf_in(&mut table, account, leaf)
    }

    /// A caller hold on one leaf name's own registered charge; never
    /// refused.
    #[must_use]
    pub fn pin_leaf(&self, account: &Arc<SemanticRetentionAccount>, leaf: &Arc<str>) -> CallerHold {
        let mut table = self.0.table.lock();
        self.pin_leaf_in(&mut table, account, leaf)
    }

    /// Key `shared` by `allocation` and settle the table's backing charge.
    /// Called with the registry lock held.
    fn register<T>(
        &self,
        table: &mut RegistryTable,
        account: &Arc<SemanticRetentionAccount>,
        key: usize,
        allocation: &Arc<T>,
        mut shared: SharedCharge,
    ) -> Arc<SharedCharge>
    where
        T: ?Sized + Send + Sync + 'static,
    {
        shared.registration = Some((Arc::clone(&self.0), key));
        shared._allocation = Some(Box::new(Arc::clone(allocation)));
        let shared = Arc::new(shared);
        table.entries.insert(key, Arc::clone(&shared));
        table.settle(account);
        shared
    }

    /// Give back the table capacity released registrations left behind,
    /// down to what the live registrations need. For release boundaries
    /// (a host's close); a live registry regrows on its next registration.
    pub fn reclaim_table(&self) {
        self.0.table.lock().reclaim_exact();
    }

    /// Bytes the registry's table backing is charged.
    #[must_use]
    pub fn table_charged_bytes(&self) -> usize {
        self.0
            .table
            .lock()
            .backing
            .as_ref()
            .map_or(0, GranularPin::charged)
    }

    /// Buckets the registry's table currently holds.
    #[must_use]
    pub fn table_capacity(&self) -> usize {
        self.0.table.lock().entries.capacity()
    }

    /// Live registrations: one per allocation some holder still charges.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.table.lock().entries.len()
    }

    /// Whether no allocation is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl std::fmt::Debug for AliasCharges {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AliasCharges")
            .field("registrations", &self.len())
            .finish()
    }
}

#[cfg(test)]
#[path = "shared_tests.rs"]
mod tests;
