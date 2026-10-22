//! The one process-local aggregate semantic-retention account.
//!
//! Every host-owned store that retains semantic bytes charges THIS
//! account. There is exactly one account per process (shared by every
//! loaded project's type store),
//! so a workspace that loads three projects cannot retain three times
//! the ratified ceiling. Per-family caps — the count caps in
//! the session's bounded query retention and each
//! semantic memo family's candidate cap
//! — still bound each family's own occupancy; this account bounds the
//! SUM, and a store must never carry a second private byte quota beside
//! it.
//!
//! ## Charge classes
//!
//! A charge is exactly one of three classes, and the class decides
//! whether a reservation can be REFUSED:
//!
//! - [`ChargeClass::Active`] — bytes an in-flight computation holds
//!   before it decides whether to publish. REFUSABLE, and additionally
//!   bounded by its own sub-limit
//!   ([`RetentionLimits::active_ceiling_bytes`]) so one runaway request
//!   cannot starve every cache in the process.
//! - [`ChargeClass::Retained`] — bytes a warm cache entry owns.
//!   REFUSABLE: a refused admission returns the freshly-computed value
//!   to its caller UNCACHED (the existing `ReturnOnly` rail) under
//!   [`verter_audit::NonAdmissionReason::RetentionPressure`].
//! - [`ChargeClass::Pinned`] — bytes a LIVE handle obliges the process
//!   to keep: the lease-pinned retained parse snapshots in
//!   the session's declaration lowering, and any other allocation whose release is
//!   owned by a handle the caller already holds. A pin is NEVER refused.
//!   Refusing one would either revoke a live public handle or force a
//!   live artifact to silently re-parse, both of which the retained-parse
//!   lease contract forbids. Pins are nonetheless CHARGED, so they
//!   consume headroom and push back on discretionary retention.
//!
//! The hard invariant the account enforces is therefore stated over the
//! refusable classes: **`active + retained` never exceeds
//! `aggregate_ceiling_bytes` minus the bytes currently pinned**, under
//! any interleaving of concurrent reservations. Pins may themselves push
//! the total past the ceiling (they are liveness obligations, not policy
//! choices); when they do, every refusable reservation is refused until
//! pins drop. That is the pressure behaviour, and it is intentional: the
//! process stops ADDING discretionary bytes rather than dropping bytes a
//! live reader still needs.
//!
//! ## Reserve / commit / release ownership
//!
//! [`SemanticRetentionAccount::reserve`] hands back a
//! [`RetentionCharge`] — an RAII token. The charge is the ONLY thing
//! that releases bytes, and it releases them EXACTLY ONCE, in `Drop`.
//! Moving the token transfers ownership without releasing; a cache that
//! stores the token beside its entry releases the bytes precisely when
//! the entry (and every reader snapshot that outlived the eviction)
//! drops. Cancellation, a failed build and an abandoned publish all
//! release by dropping the token on the way out — there is no separate
//! "release" call to forget on an early-return path.
//!
//! A charge shared by several owners is held behind an `Arc`: cloning
//! the `Arc` shares ONE reservation rather than charging twice, which is
//! what a same-payload cache backfill needs (the clone shares the
//! payload's allocations, so a second charge would double-count bytes
//! that exist once).
//!
//! An allocation a cache retains and a caller also holds is charged once
//! through [`shared`]: the cache's [`shared::CacheHold`] keeps it
//! [`ChargeClass::Retained`], and when the last cache hold drops while a
//! [`shared::CallerHold`] remains, the same bytes move to
//! [`ChargeClass::Pinned`] in one step ([`RetentionCharge::transfer_to_pinned`]) —
//! never uncharged, never charged twice, never refused. Independently
//! charged owners of one allocation share its charge through
//! [`shared::AliasCharges`].
//!
//! ## What a charge measures
//!
//! A charge covers the allocations its owner keeps reachable, by capacity,
//! under the declared conventions of [`allocation`] — never a fixed
//! per-item proxy standing in for a variable payload.
//!
//! ## The byte figures are ESTIMATES, never a validity oracle
//!
//! [`RetainedFootprint`] estimates a payload's retained bytes. Nothing
//! in cache VALIDITY reads it: an entry's correctness is decided by its
//! `ReadSetSignature` fact rail exactly as before. The estimate only
//! decides whether the process may retain the entry at all, so an
//! imprecise estimate costs hit rate, never correctness.

// The `HeapSize` derive names this crate by its absolute path.
extern crate self as verter_retention;

#[cfg(any(test, feature = "test-support", feature = "soak-support"))]
pub mod active_window;
pub mod allocation;
pub mod granular;
pub mod heap_size;
pub mod owned;
pub mod shared;
pub mod unshared;
#[cfg(any(test, feature = "test-support", feature = "soak-support"))]
pub mod witness;
pub mod word_set;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use verter_audit::NonAdmissionReason;

// ──────────────────────────────────────────────────────────────────────
// Limits
// ──────────────────────────────────────────────────────────────────────

/// The finite byte limits the account admits against.
///
/// The defaults are the ratified aggregate semantic-memory budget: a
/// 1 GiB retained-allocation ceiling, a 512 MiB pin threshold, and a
/// 4 MiB per-request cap. They are CONSTANTS here rather than a config
/// surface — the budget is a ratified contract, not a per-host knob —
/// and a test injects its own instance instead of mutating them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionLimits {
    /// Total refusable + pinned bytes the process aims to retain.
    pub aggregate_ceiling_bytes: usize,
    /// Sub-limit on [`ChargeClass::Active`] bytes alone, so in-flight
    /// working sets cannot consume the whole aggregate ceiling and
    /// starve every cache.
    pub active_ceiling_bytes: usize,
    /// Largest single cache entry the account will ever retain. A
    /// [`ChargeClass::Retained`] reservation above this is refused as
    /// [`RetentionRefusal::Oversized`] without consulting occupancy —
    /// retaining it would evict an unbounded number of useful entries to
    /// store one outlier. It is a rule about what a cache keeps, not about
    /// what a computation may build: an [`ChargeClass::Active`]
    /// reservation is bounded by [`Self::active_ceiling_bytes`] alone.
    pub max_entry_bytes: usize,
    /// Pinned bytes past which the process is considered to be under PIN
    /// pressure ([`SemanticRetentionAccount::under_pin_pressure`]).
    ///
    /// This is a REPORTING threshold, never an admission gate: a pin is
    /// a liveness obligation and is charged unconditionally (see the
    /// module docs). Crossing it says the process's discretionary
    /// headroom is being consumed by live handles rather than by caches,
    /// which is a different diagnosis from ordinary cache pressure and
    /// has a different remedy — release handles, not evict entries.
    pub pin_threshold_bytes: usize,
}

impl RetentionLimits {
    /// Ratified retained-allocation ceiling for the shared process-local
    /// account.
    pub const DEFAULT_AGGREGATE_CEILING_BYTES: usize = 1024 * 1024 * 1024;
    /// Ratified pin threshold — the pinned-byte level at which the
    /// process reports pin pressure.
    pub const DEFAULT_PIN_THRESHOLD_BYTES: usize = 512 * 1024 * 1024;
    /// Ratified per-request cap, applied here as the largest single
    /// reservation any one request may contribute.
    pub const DEFAULT_MAX_ENTRY_BYTES: usize = 4 * 1024 * 1024;
    /// Process-wide in-flight sub-limit. Sized at the pin threshold so
    /// concurrent active work can never, on its own, leave a live pin
    /// set without headroom.
    pub const DEFAULT_ACTIVE_CEILING_BYTES: usize = Self::DEFAULT_PIN_THRESHOLD_BYTES;

    /// The default ratified limits.
    #[must_use]
    pub const fn defaults() -> Self {
        Self {
            aggregate_ceiling_bytes: Self::DEFAULT_AGGREGATE_CEILING_BYTES,
            active_ceiling_bytes: Self::DEFAULT_ACTIVE_CEILING_BYTES,
            max_entry_bytes: Self::DEFAULT_MAX_ENTRY_BYTES,
            pin_threshold_bytes: Self::DEFAULT_PIN_THRESHOLD_BYTES,
        }
    }
}

impl Default for RetentionLimits {
    fn default() -> Self {
        Self::defaults()
    }
}

// ──────────────────────────────────────────────────────────────────────
// Classes, refusals, outcomes
// ──────────────────────────────────────────────────────────────────────

/// Which accounting class a reservation belongs to. See the module docs
/// for the refusable / pinned split.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChargeClass {
    /// In-flight computation bytes. Refusable; sub-limited.
    Active,
    /// Warm cache-entry bytes. Refusable.
    Retained,
    /// Bytes a live handle obliges the process to keep. Never refused.
    Pinned,
}

impl ChargeClass {
    /// Whether a reservation in this class can be refused.
    #[must_use]
    pub const fn is_refusable(self) -> bool {
        matches!(self, Self::Active | Self::Retained)
    }
}

/// Why a reservation was refused. Carries the figures the refusal was
/// decided on so a caller can report a truthful pressure diagnosis
/// instead of a bare boolean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionRefusal {
    /// The single reservation exceeds [`RetentionLimits::max_entry_bytes`].
    Oversized {
        /// Bytes the caller asked to retain.
        requested: usize,
        /// The per-reservation ceiling it exceeded.
        max_entry_bytes: usize,
    },
    /// The aggregate ceiling has no headroom for this reservation.
    Pressure {
        /// Bytes the caller asked to retain.
        requested: usize,
        /// Bytes available when the decision was taken.
        headroom: usize,
    },
    /// The [`ChargeClass::Active`] sub-limit has no headroom. The work
    /// is exhausted of its resource budget, not merely unable to cache.
    ActiveExhausted {
        /// Bytes the caller asked to hold active.
        requested: usize,
        /// Active bytes available when the decision was taken.
        headroom: usize,
    },
}

impl RetentionRefusal {
    /// The typed cache-runtime refusal reason this maps onto.
    ///
    /// All three arms describe a COMPLETE, correct value the process
    /// declines to retain, so they carry the same locally-confined
    /// reason: the caller still returns its value, and an enclosing
    /// derivation that consumed it may still cache its own result.
    #[must_use]
    pub const fn non_admission_reason(self) -> NonAdmissionReason {
        NonAdmissionReason::RetentionPressure
    }
}

impl std::fmt::Display for RetentionRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Oversized {
                requested,
                max_entry_bytes,
            } => write!(
                f,
                "oversized ({requested} B > {max_entry_bytes} B per entry)"
            ),
            Self::Pressure {
                requested,
                headroom,
            } => write!(f, "pressure ({requested} B > {headroom} B headroom)"),
            Self::ActiveExhausted {
                requested,
                headroom,
            } => write!(
                f,
                "active budget exhausted ({requested} B > {headroom} B active headroom)"
            ),
        }
    }
}

/// Outcome of a refusable reservation.
#[derive(Debug)]
pub enum RetentionAdmission {
    /// Bytes reserved. The caller owns the charge and must keep it for
    /// as long as it keeps the payload.
    Admitted(RetentionCharge),
    /// Bytes refused. The caller returns its value uncached.
    Refused(RetentionRefusal),
}

impl RetentionAdmission {
    /// The charge when admitted.
    #[must_use]
    pub fn admitted(self) -> Option<RetentionCharge> {
        match self {
            Self::Admitted(charge) => Some(charge),
            Self::Refused(_) => None,
        }
    }

    /// The refusal when refused.
    #[must_use]
    pub fn refusal(&self) -> Option<RetentionRefusal> {
        match self {
            Self::Admitted(_) => None,
            Self::Refused(refusal) => Some(*refusal),
        }
    }
}

// ──────────────────────────────────────────────────────────────────────
// Footprint estimation
// ──────────────────────────────────────────────────────────────────────

/// Estimated retained bytes of a payload the process is about to keep.
///
/// This is an ACCOUNTING estimate and never participates in cache
/// validity — see the module docs. Implementations count the heap the
/// payload keeps alive on its own; bytes reached only through an `Arc`
/// that a DIFFERENT charged owner already accounts for are deliberately
/// left out so one allocation is charged once.
pub trait RetainedFootprint {
    /// Estimated bytes this value keeps alive, counting nothing an owned
    /// handle it reaches charges by itself.
    fn retained_footprint_bytes(&self) -> usize;

    /// [`Self::retained_footprint_bytes`] together with the owned
    /// allocations the value reaches ([`owned::OwnedHandles`]): a cache
    /// retaining the value holds them beside its own charge
    /// ([`RetentionCharge::holding`]), so each is charged once, by its own
    /// cell. A value that reaches no owned handle keeps the default.
    fn retained_footprint(&self) -> Footprint {
        Footprint {
            bytes: self.retained_footprint_bytes(),
            handles: owned::OwnedHandles::default(),
        }
    }
}

/// What retaining one value costs: the bytes its own charge holds and the
/// owned allocations it reaches, each charged by its own cell.
#[must_use = "the owned allocations a retained value reaches must be held"]
pub struct Footprint {
    /// Bytes the retaining owner's own charge covers.
    pub bytes: usize,
    /// The owned allocations the value reaches.
    pub handles: owned::OwnedHandles,
}

/// A payload that owns no heap beyond itself costs exactly its own
/// size. Covers the trivial cache payloads the substrate fixtures store,
/// so a generic cache is charged uniformly rather than having an
/// uncharged branch for small payload types.
macro_rules! impl_inline_footprint {
    ($($t:ty),* $(,)?) => {
        $(impl RetainedFootprint for $t {
            fn retained_footprint_bytes(&self) -> usize {
                std::mem::size_of::<Self>()
            }
        })*
    };
}

impl_inline_footprint!((), u8, u16, u32, u64, usize);

/// The declared container-slot convention: the fixed bytes one cache slot
/// costs its container — the map bucket (key hash, control byte, the
/// slot's share of the table's growth slack) and the eviction-order record
/// (FIFO / recency slot) that travel with one entry. A slot is fixed-size,
/// so a fixed figure is its exact convention; everything variable an entry
/// keeps — its payload, its fact rail, its reverse-index registrations —
/// is charged on top from its allocations
/// ([`allocation`]), never folded into this constant.
pub const ENTRY_OVERHEAD_BYTES: usize = 128;

// ──────────────────────────────────────────────────────────────────────
// The account
// ──────────────────────────────────────────────────────────────────────

/// Immutable snapshot of the account's occupancy and decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RetentionAccountSnapshot {
    /// Currently charged [`ChargeClass::Active`] bytes.
    pub active_bytes: usize,
    /// Currently charged [`ChargeClass::Retained`] bytes.
    pub retained_bytes: usize,
    /// Currently charged [`ChargeClass::Pinned`] bytes.
    pub pinned_bytes: usize,
    /// High-water mark of `active + retained + pinned`.
    pub peak_total_bytes: usize,
    /// Reservations admitted over the account's lifetime.
    pub admissions: u64,
    /// Reservations refused as [`RetentionRefusal::Oversized`].
    pub refusals_oversized: u64,
    /// Reservations refused as [`RetentionRefusal::Pressure`].
    pub refusals_pressure: u64,
    /// Reservations refused as [`RetentionRefusal::ActiveExhausted`].
    pub refusals_active: u64,
    /// Charges released over the account's lifetime.
    pub releases: u64,
}

impl RetentionAccountSnapshot {
    /// Currently charged bytes across every class.
    #[must_use]
    pub const fn total_bytes(&self) -> usize {
        self.active_bytes + self.retained_bytes + self.pinned_bytes
    }

    /// Refusals of every kind.
    #[must_use]
    pub const fn refusals(&self) -> u64 {
        self.refusals_oversized + self.refusals_pressure + self.refusals_active
    }
}

/// The aggregate byte account. One per process in production; tests
/// construct private instances so a pressure fixture never perturbs a
/// concurrently-running test.
#[derive(Debug)]
pub struct SemanticRetentionAccount {
    limits: RetentionLimits,
    /// The aggregate occupancy, which every transition changes under this
    /// lock. Acquiring it is the admission linearization point: refusable
    /// reservations and pins therefore cannot validate against different
    /// aggregate states.
    admission_lock: parking_lot::Mutex<Occupancy>,
    admissions: AtomicUsize,
    refusals_oversized: AtomicUsize,
    refusals_pressure: AtomicUsize,
    refusals_active: AtomicUsize,
    releases: AtomicUsize,
    /// Open ACTIVE-occupancy windows and the transition sequence they
    /// span. Optional test/tooling state: absent from production builds.
    #[cfg(any(test, feature = "test-support", feature = "soak-support"))]
    active_windows: active_window::ActiveWindows,
}

/// The account's byte occupancy, read and written only under its
/// admission lock.
#[derive(Debug, Default)]
struct Occupancy {
    /// `active + retained` — the refusable occupancy. Kept as ONE figure
    /// so the two refusable classes share one aggregate state.
    refusable_bytes: usize,
    active_bytes: usize,
    pinned_bytes: usize,
    peak_total_bytes: usize,
}

impl Occupancy {
    /// Raise the peak to the current total.
    fn note_peak(&mut self) {
        let total = self.refusable_bytes + self.pinned_bytes;
        if total > self.peak_total_bytes {
            self.peak_total_bytes = total;
        }
        verter_audit::attribute_max!(StoreRetainedBytes, total);
    }
}

/// Whether this build carries the ACTIVE-occupancy measurement window
/// (the `active_window` module): `true` only under `test`, `test-support` or the
/// default-off `soak-support` feature. A build without it has no window to
/// report from — never a window that reports zero.
pub const ACTIVE_OCCUPANCY_WINDOW_AVAILABLE: bool = cfg!(any(
    test,
    feature = "test-support",
    feature = "soak-support"
));

static PROCESS_LOCAL: OnceLock<Arc<SemanticRetentionAccount>> = OnceLock::new();

impl SemanticRetentionAccount {
    /// Construct an isolated account. Test-support only: production code
    /// obtains the one account through [`Self::process_local`], so a
    /// production build cannot mint a private byte quota beside the
    /// aggregate ceiling. An isolated instance exists so tests can drive
    /// pressure deterministically.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn new(limits: RetentionLimits) -> Arc<Self> {
        Self::build(limits)
    }

    fn build(limits: RetentionLimits) -> Arc<Self> {
        Arc::new(Self {
            limits,
            admission_lock: parking_lot::Mutex::new(Occupancy::default()),
            admissions: AtomicUsize::new(0),
            refusals_oversized: AtomicUsize::new(0),
            refusals_pressure: AtomicUsize::new(0),
            refusals_active: AtomicUsize::new(0),
            releases: AtomicUsize::new(0),
            #[cfg(any(test, feature = "test-support", feature = "soak-support"))]
            active_windows: active_window::ActiveWindows::default(),
        })
    }

    /// THE process-local account every production store charges. Shared
    /// across projects by construction: one `OnceLock`, one account, no
    /// per-project ceiling multiplication.
    #[must_use]
    pub fn process_local() -> Arc<Self> {
        Arc::clone(PROCESS_LOCAL.get_or_init(|| Self::build(RetentionLimits::defaults())))
    }

    /// The limits this account admits against.
    #[must_use]
    pub fn limits(&self) -> RetentionLimits {
        self.limits
    }

    /// Reserve `bytes` in `class`.
    ///
    /// A [`ChargeClass::Pinned`] request is charged unconditionally and
    /// always returns [`RetentionAdmission::Admitted`] — see the module
    /// docs for why a pin is not a policy choice. The refusable classes
    /// admit only when the reservation fits under the ceiling at the
    /// CAS-guarded aggregate-state linearization point. Pins take that
    /// same guard, so a refusable admission can never validate against a
    /// stale pinned total.
    ///
    /// A zero-byte reservation is always admitted and charges nothing.
    pub fn reserve(self: &Arc<Self>, class: ChargeClass, bytes: usize) -> RetentionAdmission {
        self.reserve_with(class, bytes, Provenance::UNKNOWN)
    }

    fn reserve_with(
        self: &Arc<Self>,
        class: ChargeClass,
        bytes: usize,
        provenance: Provenance,
    ) -> RetentionAdmission {
        if !class.is_refusable() {
            return RetentionAdmission::Admitted(self.pin(bytes));
        }
        if class == ChargeClass::Retained && bytes > self.limits.max_entry_bytes {
            self.refusals_oversized.fetch_add(1, Ordering::Relaxed);
            verter_audit::attribute!(RetentionAdmitRefused);
            return RetentionAdmission::Refused(RetentionRefusal::Oversized {
                requested: bytes,
                max_entry_bytes: self.limits.max_entry_bytes,
            });
        }
        if bytes == 0 {
            self.admissions.fetch_add(1, Ordering::Relaxed);
            return RetentionAdmission::Admitted(self.mint_charge(class, 0, provenance));
        }
        let this: &Self = self;
        let mut state = this.lock_aggregate_state();
        let current = state.refusable_bytes;
        let headroom = this
            .limits
            .aggregate_ceiling_bytes
            .saturating_sub(current)
            .saturating_sub(state.pinned_bytes);
        if bytes > headroom {
            self.refusals_pressure.fetch_add(1, Ordering::Relaxed);
            verter_audit::attribute!(RetentionAdmitRefused);
            return RetentionAdmission::Refused(RetentionRefusal::Pressure {
                requested: bytes,
                headroom,
            });
        }
        if class == ChargeClass::Active {
            let active = state.active_bytes;
            let active_headroom = this.limits.active_ceiling_bytes.saturating_sub(active);
            if bytes > active_headroom {
                this.refusals_active.fetch_add(1, Ordering::Relaxed);
                verter_audit::attribute!(RetentionAdmitRefused);
                return RetentionAdmission::Refused(RetentionRefusal::ActiveExhausted {
                    requested: bytes,
                    headroom: active_headroom,
                });
            }
            state.active_bytes = active + bytes;
            this.note_active_locked(provenance, bytes, 0);
        }
        state.refusable_bytes = current + bytes;
        this.admissions.fetch_add(1, Ordering::Relaxed);
        state.note_peak();
        drop(state);
        RetentionAdmission::Admitted(self.mint_charge(class, bytes, provenance))
    }

    /// Reserve `bytes` for a LIVE handle: always admitted, always
    /// charged. The single legitimate way to account a pin.
    #[must_use]
    pub fn pin(self: &Arc<Self>, bytes: usize) -> RetentionCharge {
        let this: &Self = self;
        let mut state = this.lock_aggregate_state();
        state.pinned_bytes += bytes;
        this.admissions.fetch_add(1, Ordering::Relaxed);
        state.note_peak();
        drop(state);
        self.mint_charge(ChargeClass::Pinned, bytes, Provenance::UNKNOWN)
    }

    /// Whether pinned bytes have passed
    /// [`RetentionLimits::pin_threshold_bytes`].
    ///
    /// REPORTING only — no admission path branches on it. When this is
    /// true, discretionary refusals are being driven by live handles
    /// rather than by cache growth, so evicting cache entries would not
    /// recover the headroom.
    #[must_use]
    pub fn under_pin_pressure(&self) -> bool {
        self.lock_aggregate_state().pinned_bytes > self.limits.pin_threshold_bytes
    }

    /// Headroom a refusable reservation would see right now. Advisory
    /// only — a concurrent reservation may consume it before the caller
    /// acts, which is why admission itself is a CAS and not a
    /// check-then-reserve.
    #[must_use]
    pub fn refusable_headroom_bytes(&self) -> usize {
        let state = self.lock_aggregate_state();
        self.limits
            .aggregate_ceiling_bytes
            .saturating_sub(state.refusable_bytes)
            .saturating_sub(state.pinned_bytes)
    }

    /// Snapshot occupancy and decision counters.
    #[must_use]
    pub fn snapshot(&self) -> RetentionAccountSnapshot {
        let state = self.lock_aggregate_state();
        let active = state.active_bytes;
        let refusable = state.refusable_bytes;
        RetentionAccountSnapshot {
            active_bytes: active,
            retained_bytes: refusable.saturating_sub(active),
            pinned_bytes: state.pinned_bytes,
            peak_total_bytes: state.peak_total_bytes,
            admissions: self.admissions.load(Ordering::Relaxed) as u64,
            refusals_oversized: self.refusals_oversized.load(Ordering::Relaxed) as u64,
            refusals_pressure: self.refusals_pressure.load(Ordering::Relaxed) as u64,
            refusals_active: self.refusals_active.load(Ordering::Relaxed) as u64,
            releases: self.releases.load(Ordering::Relaxed) as u64,
        }
    }

    fn mint_charge(
        self: &Arc<Self>,
        class: ChargeClass,
        bytes: usize,
        provenance: Provenance,
    ) -> RetentionCharge {
        RetentionCharge {
            account: Some(Arc::clone(self)),
            class,
            bytes,
            held: None,
            held_backing: 0,
            provenance,
        }
    }

    /// The single release path. Reached ONLY from [`RetentionCharge`]'s
    /// `Drop`, which is why exactly-once release is structural rather
    /// than a discipline callers must remember.
    fn release(&self, class: ChargeClass, bytes: usize, provenance: Provenance) {
        let mut state = self.lock_aggregate_state();
        match class {
            ChargeClass::Pinned => state.pinned_bytes -= bytes,
            ChargeClass::Active => {
                if bytes > 0 {
                    state.active_bytes -= bytes;
                    state.refusable_bytes -= bytes;
                    self.note_active_locked(provenance, 0, bytes);
                }
            }
            ChargeClass::Retained => state.refusable_bytes -= bytes,
        }
        drop(state);
        self.releases.fetch_add(1, Ordering::Relaxed);
    }

    /// Move `bytes` from `from` to `to` without releasing them: the single
    /// transition both classes observe at once. Reached only from
    /// [`RetentionCharge::transfer_to_pinned`], whose target never refuses.
    fn reclassify(&self, from: ChargeClass, to: ChargeClass, bytes: usize, provenance: Provenance) {
        let mut state = self.lock_aggregate_state();
        self.reclassify_locked(&mut state, from, to, bytes, provenance);
    }

    /// [`Self::reclassify`] with the admission lock already held.
    fn reclassify_locked(
        &self,
        state: &mut Occupancy,
        from: ChargeClass,
        to: ChargeClass,
        bytes: usize,
        provenance: Provenance,
    ) {
        if bytes == 0 || from == to {
            return;
        }
        let refusable = state.refusable_bytes;
        let pinned = state.pinned_bytes;
        let active = state.active_bytes;
        let (refusable, pinned, active) = match (from, to) {
            (ChargeClass::Active, ChargeClass::Pinned) => {
                (refusable - bytes, pinned + bytes, active - bytes)
            }
            (ChargeClass::Retained, ChargeClass::Pinned) => {
                (refusable - bytes, pinned + bytes, active)
            }
            // A retained slice of cache holds joining the in-flight charge
            // that keeps it.
            (ChargeClass::Retained, ChargeClass::Active) => (refusable, pinned, active + bytes),
            _ => unreachable!("only refusable charges are handed to callers"),
        };
        state.refusable_bytes = refusable;
        state.pinned_bytes = pinned;
        let before = std::mem::replace(&mut state.active_bytes, active);
        self.note_active_locked(
            provenance,
            active.saturating_sub(before),
            before.saturating_sub(active),
        );
    }

    /// Re-admit `bytes` already pinned as retained, under the admission
    /// rules a fresh retained reservation of them meets. The pinned bytes
    /// are this very allocation, so they are not counted against their own
    /// adoption.
    fn adopt_pinned(&self, bytes: usize) -> Result<(), RetentionRefusal> {
        if bytes > self.limits.max_entry_bytes {
            self.refusals_oversized.fetch_add(1, Ordering::Relaxed);
            verter_audit::attribute!(RetentionAdmitRefused);
            return Err(RetentionRefusal::Oversized {
                requested: bytes,
                max_entry_bytes: self.limits.max_entry_bytes,
            });
        }
        let mut state = self.lock_aggregate_state();
        let refusable = state.refusable_bytes;
        let pinned = state.pinned_bytes;
        let headroom = self
            .limits
            .aggregate_ceiling_bytes
            .saturating_sub(refusable)
            .saturating_sub(pinned - bytes);
        if bytes > headroom {
            self.refusals_pressure.fetch_add(1, Ordering::Relaxed);
            verter_audit::attribute!(RetentionAdmitRefused);
            return Err(RetentionRefusal::Pressure {
                requested: bytes,
                headroom,
            });
        }
        state.pinned_bytes = pinned - bytes;
        state.refusable_bytes = refusable + bytes;
        self.admissions.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn lock_aggregate_state(&self) -> parking_lot::MutexGuard<'_, Occupancy> {
        self.admission_lock.lock()
    }
}

/// Whom a charge's ACTIVE transitions are attributed to by an ACTIVE-occupancy
/// window (the `active_window` module): an explicit `ActiveOwner`, or no
/// owner at all. A charge reserved without an owner is UNKNOWN and is never
/// implicitly attributed to any window. Zero-sized, and never read, in a build
/// without the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Provenance {
    #[cfg(any(test, feature = "test-support", feature = "soak-support"))]
    owner: Option<u64>,
}

impl Provenance {
    const UNKNOWN: Self = Self {
        #[cfg(any(test, feature = "test-support", feature = "soak-support"))]
        owner: None,
    };

    /// Attribution to `owner`, which must be bound to `account`.
    #[cfg(any(test, feature = "test-support", feature = "soak-support"))]
    fn of(owner: &active_window::ActiveOwner, account: &Arc<SemanticRetentionAccount>) -> Self {
        owner.assert_bound_to(account);
        Self {
            owner: Some(owner.id().raw()),
        }
    }

    /// The provenance of one charge that absorbed or adopted bytes of
    /// another: the shared owner, or UNKNOWN when the two differ — mixed
    /// provenance is never attributed to either side.
    #[inline]
    fn merged(self, other: Self) -> Self {
        #[cfg(any(test, feature = "test-support", feature = "soak-support"))]
        if self != other {
            return Self::UNKNOWN;
        }
        #[cfg(not(any(test, feature = "test-support", feature = "soak-support")))]
        let _ = other;
        self
    }
}

impl SemanticRetentionAccount {
    /// A charge of `class` holding `held_bytes` adopts the backing slice of
    /// another charge: ONE account-locked transaction settles both the
    /// provenance change (the held bytes move between ACTIVE balances, the
    /// total unchanged) and the backing's class change, so no window or
    /// admission observes the adoption half-applied.
    fn adopt_backing(&self, adoption: BackingAdoption) {
        let mut state = self.lock_aggregate_state();
        if adoption.class == ChargeClass::Active {
            self.rehome_active_locked(adoption.from, adoption.to, adoption.held_bytes);
        }
        self.reclassify_locked(
            &mut state,
            adoption.backing_class,
            adoption.class,
            adoption.backing_bytes,
            adoption.to,
        );
    }

    /// Move `bytes` of ACTIVE attribution from one provenance to another.
    /// Called only while [`Self::lock_aggregate_state`] is held; compiles to
    /// nothing in a build without the window.
    #[inline]
    fn rehome_active_locked(&self, from: Provenance, to: Provenance, bytes: usize) {
        #[cfg(any(test, feature = "test-support", feature = "soak-support"))]
        self.active_windows
            .rehome_locked(from.owner, to.owner, bytes);
        #[cfg(not(any(test, feature = "test-support", feature = "soak-support")))]
        let _ = (from, to, bytes);
    }

    /// Note an ACTIVE transition of a charge of `provenance` to the open
    /// measurement windows: the ACTIVE total just moved by `rise` or `fall`
    /// bytes. Called only while [`Self::lock_aggregate_state`] is held,
    /// after the move; compiles to nothing in a build without the window.
    #[inline]
    fn note_active_locked(&self, provenance: Provenance, rise: usize, fall: usize) {
        #[cfg(any(test, feature = "test-support", feature = "soak-support"))]
        self.active_windows
            .note_locked(provenance.owner, rise, fall);
        #[cfg(not(any(test, feature = "test-support", feature = "soak-support")))]
        let _ = (provenance, rise, fall);
    }
}

/// One charge adopting another's backing slice: what
/// [`SemanticRetentionAccount::adopt_backing`] settles in a single
/// transaction.
struct BackingAdoption {
    /// The adopting charge's class.
    class: ChargeClass,
    /// Bytes the adopting charge held before adopting.
    held_bytes: usize,
    /// Its provenance before, and after merging the backing's.
    from: Provenance,
    to: Provenance,
    /// The adopted backing's class and bytes.
    backing_class: ChargeClass,
    backing_bytes: usize,
}

// ──────────────────────────────────────────────────────────────────────
// StoreAccount — the store-side account handle
// ──────────────────────────────────────────────────────────────────────

/// The account handle a candidate-retaining store holds.
///
/// This is a HANDLE, never a second authority: [`SemanticRetentionAccount`]
/// still owns every limit, admission decision and byte figure. What the
/// handle adds is that it has NO account-less variant — a store field typed
/// `StoreAccount` cannot be `None`, so "this store retains candidates but
/// consumes no aggregate headroom" is unrepresentable rather than merely
/// unused. That is the whole point: an optional account is a bypass route
/// that compiles, and the only durable way to reject it is to delete the
/// variant.
///
/// [`Default`] resolves to [`SemanticRetentionAccount::process_local`] —
/// the ONE process account — and deliberately NOT to a freshly minted
/// private account. A per-store account would be exactly the private byte
/// quota beside the aggregate ceiling that the retention contract forbids:
/// N stores would admit against N ceilings. A test that needs deterministic
/// pressure binds its private account explicitly through [`Self::new`],
/// which exists only under test support: in a production build every
/// `StoreAccount` is the process-local account.
#[derive(Debug, Clone)]
pub struct StoreAccount(Arc<SemanticRetentionAccount>);

impl Default for StoreAccount {
    fn default() -> Self {
        Self(SemanticRetentionAccount::process_local())
    }
}

impl StoreAccount {
    /// Bind an explicit isolated account. Test-support only; production
    /// stores take [`Self::default`], the process-local account.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn new(account: Arc<SemanticRetentionAccount>) -> Self {
        Self(account)
    }

    /// The bound account. Always present; see the type docs.
    #[must_use]
    pub fn get(&self) -> &Arc<SemanticRetentionAccount> {
        &self.0
    }
}

// ──────────────────────────────────────────────────────────────────────
// RetentionCharge — the exactly-once release token
// ──────────────────────────────────────────────────────────────────────

/// An owned reservation against a [`SemanticRetentionAccount`].
///
/// Dropping it releases its bytes — once, and only once. Moving it
/// transfers the obligation; there is no way to release twice and no
/// early-return path that can forget to release, because `Drop` is the
/// only releaser.
///
/// A charge shared by several owners lives behind an `Arc`, which shares
/// ONE reservation rather than charging the same allocation twice.
#[derive(Debug)]
pub struct RetentionCharge {
    /// `None` only transiently inside `Drop` — the take is what makes
    /// release exactly-once.
    account: Option<Arc<SemanticRetentionAccount>>,
    class: ChargeClass,
    bytes: usize,
    /// The cache holds on the owned allocations the charged value reaches
    /// ([`Self::holding`]); released with the charge.
    held: Option<Box<[owned::OwnedHold]>>,
    /// The part of `bytes` that charges the `held` slice's backing: released
    /// with the slice when the holds end before the charge does.
    held_backing: usize,
    /// The owner this charge is attributed to for ACTIVE-occupancy windows;
    /// zero-sized in builds without the measurement window.
    provenance: Provenance,
}

impl RetentionCharge {
    /// The account this charge is held against; `None` only while it is
    /// being released.
    pub(crate) fn account(&self) -> Option<&Arc<SemanticRetentionAccount>> {
        self.account.as_ref()
    }

    /// This cache charge together with the cache's holds on the owned
    /// allocations its value reaches: the holds live and end with the
    /// charge, so the cache retains the value's whole footprint as one
    /// token.
    ///
    /// The slice keeping the holds is charged with them: its backing charge
    /// joins this one, and leaves it again with the slice.
    #[must_use]
    pub fn holding(mut self, holds: owned::HeldHandles) -> Self {
        let (holds, backing) = holds.into_parts();
        if let Some(mut backing) = backing {
            verter_debug_assert::verter_debug_assert!(
                self.held.is_none(),
                "a charge holds one slice of cache holds"
            );
            let bytes = std::mem::take(&mut backing.bytes);
            if let Some(account) = self.account.as_ref() {
                verter_debug_assert::verter_debug_assert!(
                    backing
                        .account
                        .as_ref()
                        .is_some_and(|a| Arc::ptr_eq(a, account)),
                    "the holds' backing is charged to the account their charge is"
                );
                let merged = if bytes > 0 {
                    self.provenance.merged(backing.provenance)
                } else {
                    self.provenance
                };
                account.adopt_backing(BackingAdoption {
                    class: self.class,
                    held_bytes: self.bytes,
                    from: self.provenance,
                    to: merged,
                    backing_class: backing.class,
                    backing_bytes: bytes,
                });
                self.provenance = merged;
            }
            self.bytes += bytes;
            self.held_backing += bytes;
        }
        if !holds.is_empty() {
            self.held = Some(holds);
        }
        self
    }

    /// Bytes this charge holds.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// The class this charge was reserved in.
    #[must_use]
    pub fn class(&self) -> ChargeClass {
        self.class
    }

    /// Move `bytes` of this charge into a separate charge of the same class
    /// on the same account.
    ///
    /// The account is not touched: the bytes stay charged exactly once,
    /// now released by whichever of the two charges holds them. This is how
    /// one reservation granted for several allocations is handed to the
    /// owners of each, so each is released when its own owner drops.
    ///
    /// # Panics
    ///
    /// When `bytes` exceeds what this charge holds: a reservation is split
    /// only into the shares it was granted for, so an over-split is a
    /// miscount that would leave an allocation holding less than its
    /// storage — never clamped into a plausible smaller share.
    #[must_use]
    pub fn split_off(&mut self, bytes: usize) -> Self {
        assert!(
            bytes <= self.bytes - self.held_backing,
            "split_off({bytes}) exceeds the {} bytes this charge holds beside its holds' slice",
            self.bytes - self.held_backing
        );
        self.bytes -= bytes;
        Self {
            account: self.account.clone(),
            class: self.class,
            bytes,
            held: None,
            held_backing: 0,
            provenance: self.provenance,
        }
    }

    /// Hand this charge from cache retention to the callers still holding
    /// its allocation: a refusable charge becomes [`ChargeClass::Pinned`].
    ///
    /// Infallible and gap-free: the bytes stay charged throughout, under the
    /// account's linearization point, so no reservation can observe them
    /// uncharged and none can observe them twice. A pin is never refused —
    /// the data is held, and pressure cannot revoke it. A pinned charge is
    /// unchanged.
    pub fn transfer_to_pinned(&mut self) {
        if self.class == ChargeClass::Pinned {
            return;
        }
        if let Some(account) = self.account.as_ref() {
            account.reclassify(self.class, ChargeClass::Pinned, self.bytes, self.provenance);
        }
        self.class = ChargeClass::Pinned;
        // No cache keeps the value now: its owned allocations are their
        // own callers' (each moves to pinned in its cell), and the slice
        // that kept the holds is freed with its charge.
        self.held = None;
        let backing = std::mem::take(&mut self.held_backing);
        if backing > 0 {
            self.bytes -= backing;
            drop(Self {
                account: self.account.clone(),
                class: ChargeClass::Pinned,
                bytes: backing,
                held: None,
                held_backing: 0,
                provenance: self.provenance,
            });
        }
    }

    /// Adopt a pinned charge into cache retention: a cache is about to
    /// retain an allocation callers already hold, so its bytes become a
    /// refusable [`ChargeClass::Retained`] charge again.
    ///
    /// Decided at the same linearization point as [`SemanticRetentionAccount::reserve`],
    /// counting the bytes once (they leave the pinned total as they join the
    /// refusable one). On refusal nothing changes: the allocation stays
    /// pinned by the callers holding it and the cache keeps nothing. A
    /// charge already refusable is unchanged and admitted.
    ///
    /// # Errors
    ///
    /// The account's refusal, exactly as a fresh reservation of these bytes
    /// would be refused.
    pub fn try_adopt_retained(&mut self) -> Result<(), RetentionRefusal> {
        if self.class != ChargeClass::Pinned {
            return Ok(());
        }
        if let Some(account) = self.account.as_ref() {
            account.adopt_pinned(self.bytes)?;
        }
        self.class = ChargeClass::Retained;
        Ok(())
    }

    /// Reclassify an [`ChargeClass::Active`] charge as
    /// [`ChargeClass::Retained`] when the in-flight work it covered
    /// decided to publish.
    ///
    /// Infallible and gap-free: the bytes stay charged throughout, so a
    /// concurrent reservation can never slip into a window where this
    /// payload is unaccounted, and the commit only RELEASES active
    /// sub-limit pressure. Calling it on a non-active charge is a no-op.
    #[must_use]
    pub fn commit_retained(mut self) -> Self {
        if self.class == ChargeClass::Active {
            if let Some(account) = self.account.as_ref() {
                let mut state = account.lock_aggregate_state();
                if self.bytes > 0 {
                    state.active_bytes -= self.bytes;
                    account.note_active_locked(self.provenance, 0, self.bytes);
                }
            }
            self.class = ChargeClass::Retained;
        }
        self
    }
}

impl Drop for RetentionCharge {
    fn drop(&mut self) {
        if let Some(account) = self.account.take() {
            account.release(self.class, self.bytes, self.provenance);
        }
    }
}
