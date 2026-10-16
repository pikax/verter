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

use std::sync::Arc;

use parking_lot::Mutex;
use rustc_hash::FxHashMap;

use super::allocation::{arc_block_bytes, heap_block_bytes};
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
        Self {
            state: Mutex::new(SharedState {
                charge: Some(charge),
                cache_holds: 1,
                caller_holds: 0,
            }),
            registration: None,
            _allocation: None,
        }
    }

    /// One more cache hold: shares a retained charge, or adopts a charge
    /// only callers hold back into retention. `Err` leaves every holder and
    /// the class unchanged. Called with `state` locked.
    fn add_cache_hold(state: &mut SharedState) -> Result<(), RetentionRefusal> {
        if state.cache_holds == 0 {
            state
                .charge
                .as_mut()
                .expect("a reachable charge is not yet released")
                .try_adopt_retained()?;
        }
        state.cache_holds += 1;
        Ok(())
    }

    /// Release the charge once its last holder is gone.
    fn release_if_unheld(&self) {
        match &self.registration {
            None => {
                let released = {
                    let mut state = self.state.lock();
                    if state.cache_holds + state.caller_holds == 0 {
                        state.charge.take()
                    } else {
                        None
                    }
                };
                drop(released);
            }
            Some((registry, key)) => {
                let mut entries = registry.entries.lock();
                let mut state = self.state.lock();
                // A holder may have revived the charge between the last
                // release and this lock: then it is not released.
                if state.cache_holds + state.caller_holds != 0 {
                    return;
                }
                // The charge is released before the registry is: a new first
                // holder reserving under the registry lock never overlaps it.
                drop(state.charge.take());
                drop(state);
                let unregistered = if entries
                    .get(key)
                    .is_some_and(|entry| std::ptr::eq(Arc::as_ptr(entry), self))
                {
                    entries.remove(key)
                } else {
                    None
                };
                drop(entries);
                // The registry's own reference is dropped outside its lock.
                drop(unregistered);
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

    /// Take ownership of a refusable charge a cache was just granted. An
    /// active charge is committed to retention. A pin is a caller's
    /// obligation, never a cache grant: a cache adopts one only through
    /// [`CallerHold::adopt`], which decides the adoption refusably.
    #[must_use]
    pub fn from_retained(charge: RetentionCharge) -> Self {
        let class = charge.class();
        verter_debug_assert::verter_debug_assert_ne!(
            class,
            ChargeClass::Pinned,
            "a cache hold is minted from a granted refusable charge"
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
                // Callers still hold the allocation: its charge is theirs now.
                state
                    .charge
                    .as_mut()
                    .expect("a held charge is not yet released")
                    .transfer_to_pinned();
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
    entries: Mutex<FxHashMap<usize, Arc<SharedCharge>>>,
}

/// Bytes one registered shared charge adds beside the allocation it keys:
/// the charge's own block, the keep-alive box, and the registry slot (key,
/// handle and the map's control byte).
#[must_use]
pub fn registration_bytes() -> usize {
    arc_block_bytes(std::mem::size_of::<SharedCharge>())
        + heap_block_bytes(std::mem::size_of::<usize>())
        + std::mem::size_of::<(usize, Arc<SharedCharge>)>()
        + 1
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
    /// callers hold it), otherwise a fresh reservation of `bytes()` plus
    /// [`registration_bytes`]. `bytes` is evaluated only when a fresh
    /// reservation is needed.
    ///
    /// # Errors
    ///
    /// The account's refusal of the reservation or adoption. Nothing is
    /// registered and every existing holder is unchanged.
    pub fn hold<T>(
        &self,
        account: &Arc<SemanticRetentionAccount>,
        allocation: &Arc<T>,
        bytes: impl FnOnce() -> usize,
    ) -> Result<CacheHold, RetentionRefusal>
    where
        T: ?Sized + Send + Sync + 'static,
    {
        let key = Arc::as_ptr(allocation).cast::<()>() as usize;
        let mut entries = self.0.entries.lock();
        if let Some(existing) = entries.get(&key) {
            // Revives a charge whose last holder is releasing it: its
            // release re-checks the holders under this same lock.
            SharedCharge::add_cache_hold(&mut existing.state.lock())?;
            return Ok(CacheHold(Arc::clone(existing)));
        }
        // Reserved under the registry lock, so two first holders of one
        // allocation cannot both reserve it.
        let charge = match account.reserve(ChargeClass::Retained, bytes() + registration_bytes()) {
            RetentionAdmission::Admitted(charge) => charge,
            RetentionAdmission::Refused(refusal) => return Err(refusal),
        };
        let mut shared = SharedCharge::new(charge);
        shared.registration = Some((Arc::clone(&self.0), key));
        shared._allocation = Some(Box::new(Arc::clone(allocation)));
        let shared = Arc::new(shared);
        entries.insert(key, Arc::clone(&shared));
        Ok(CacheHold(shared))
    }

    /// Live registrations: one per allocation some holder still charges.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.entries.lock().len()
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
