//! The one process-local aggregate semantic-retention account.
//!
//! Every host-owned store that retains semantic bytes charges THIS
//! account. There is exactly one account per process (shared by every
//! loaded project / [`ProjectTypeStore`](crate::project_type_store::ProjectTypeStore)),
//! so a workspace that loads three projects cannot retain three times
//! the ratified ceiling. Per-family caps — the count caps in
//! [`crate::bounded_query_retention`] and
//! [`FamilyKey::candidate_cap`](crate::semantic_query_memo::family::FamilyKey)
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
//!   [`crate::decl_lowering`], and any other allocation whose release is
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
//! ## The byte figures are ESTIMATES, never a validity oracle
//!
//! [`RetainedFootprint`] estimates a payload's retained bytes. Nothing
//! in cache VALIDITY reads it: an entry's correctness is decided by its
//! `ReadSetSignature` fact rail exactly as before. The estimate only
//! decides whether the process may retain the entry at all, so an
//! imprecise estimate costs hit rate, never correctness.

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
    /// Largest single reservation the account will ever admit. A value
    /// above this is refused as [`RetentionRefusal::Oversized`] without
    /// consulting occupancy — retaining it would evict an unbounded
    /// number of useful entries to store one outlier.
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
    /// Estimated bytes this value keeps alive.
    fn retained_footprint_bytes(&self) -> usize;
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

/// Per-entry constant covering the entry header, its map slot, and the
/// index/ledger records that travel with it. Applied by callers on top
/// of their own payload estimate so a cache of many tiny entries is not
/// accounted as free.
pub const ENTRY_OVERHEAD_BYTES: usize = 128;

/// Multiplier turning source bytes into the resident size of the OXC
/// parse arena retained for them. Arena residency tracks source size
/// closely and superlinearly-ish in practice; a fixed conservative
/// factor keeps pin accounting cheap (no arena walk on the hot lease
/// path) while staying on the safe side of the real figure.
pub const PARSE_SNAPSHOT_BYTES_PER_SOURCE_BYTE: usize = 12;

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
    /// Serializes every aggregate occupancy transition. Acquiring it is
    /// the admission linearization point: refusable reservations and pins
    /// therefore cannot validate against different aggregate states.
    admission_lock: parking_lot::Mutex<()>,
    /// `active + retained` — the refusable occupancy. Kept as ONE cell
    /// so the two refusable classes share one aggregate state.
    refusable_bytes: AtomicUsize,
    active_bytes: AtomicUsize,
    pinned_bytes: AtomicUsize,
    peak_total_bytes: AtomicUsize,
    admissions: AtomicUsize,
    refusals_oversized: AtomicUsize,
    refusals_pressure: AtomicUsize,
    refusals_active: AtomicUsize,
    releases: AtomicUsize,
}

static PROCESS_LOCAL: OnceLock<Arc<SemanticRetentionAccount>> = OnceLock::new();

impl SemanticRetentionAccount {
    /// Construct a private account. Production wiring goes through
    /// [`Self::process_local`]; a private instance exists so tests can
    /// drive pressure deterministically.
    #[must_use]
    pub fn new(limits: RetentionLimits) -> Arc<Self> {
        Arc::new(Self {
            limits,
            admission_lock: parking_lot::Mutex::new(()),
            refusable_bytes: AtomicUsize::new(0),
            active_bytes: AtomicUsize::new(0),
            pinned_bytes: AtomicUsize::new(0),
            peak_total_bytes: AtomicUsize::new(0),
            admissions: AtomicUsize::new(0),
            refusals_oversized: AtomicUsize::new(0),
            refusals_pressure: AtomicUsize::new(0),
            refusals_active: AtomicUsize::new(0),
            releases: AtomicUsize::new(0),
        })
    }

    /// THE process-local account every production store charges. Shared
    /// across projects by construction: one `OnceLock`, one account, no
    /// per-project ceiling multiplication.
    #[must_use]
    pub fn process_local() -> Arc<Self> {
        Arc::clone(PROCESS_LOCAL.get_or_init(|| Self::new(RetentionLimits::defaults())))
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
        if !class.is_refusable() {
            return RetentionAdmission::Admitted(self.pin(bytes));
        }
        if bytes > self.limits.max_entry_bytes {
            self.refusals_oversized.fetch_add(1, Ordering::Relaxed);
            verter_audit::attribute!(RetentionAdmitRefused);
            return RetentionAdmission::Refused(RetentionRefusal::Oversized {
                requested: bytes,
                max_entry_bytes: self.limits.max_entry_bytes,
            });
        }
        if bytes == 0 {
            self.admissions.fetch_add(1, Ordering::Relaxed);
            return RetentionAdmission::Admitted(self.mint_charge(class, 0));
        }
        let _admission_lock = self.lock_aggregate_state();
        let current = self.refusable_bytes.load(Ordering::Relaxed);
        let pinned = self.pinned_bytes.load(Ordering::Relaxed);
        let headroom = self
            .limits
            .aggregate_ceiling_bytes
            .saturating_sub(current)
            .saturating_sub(pinned);
        if bytes > headroom {
            self.refusals_pressure.fetch_add(1, Ordering::Relaxed);
            verter_audit::attribute!(RetentionAdmitRefused);
            return RetentionAdmission::Refused(RetentionRefusal::Pressure {
                requested: bytes,
                headroom,
            });
        }
        if class == ChargeClass::Active {
            let active = self.active_bytes.load(Ordering::Relaxed);
            let active_headroom = self.limits.active_ceiling_bytes.saturating_sub(active);
            if bytes > active_headroom {
                self.refusals_active.fetch_add(1, Ordering::Relaxed);
                verter_audit::attribute!(RetentionAdmitRefused);
                return RetentionAdmission::Refused(RetentionRefusal::ActiveExhausted {
                    requested: bytes,
                    headroom: active_headroom,
                });
            }
            self.active_bytes.store(active + bytes, Ordering::Relaxed);
        }
        self.refusable_bytes
            .store(current + bytes, Ordering::Relaxed);
        self.admissions.fetch_add(1, Ordering::Relaxed);
        self.note_peak_locked();
        RetentionAdmission::Admitted(self.mint_charge(class, bytes))
    }

    /// Reserve `bytes` for a LIVE handle: always admitted, always
    /// charged. The single legitimate way to account a pin.
    #[must_use]
    pub fn pin(self: &Arc<Self>, bytes: usize) -> RetentionCharge {
        let _admission_lock = self.lock_aggregate_state();
        if bytes > 0 {
            let pinned = self.pinned_bytes.load(Ordering::Relaxed);
            self.pinned_bytes.store(pinned + bytes, Ordering::Relaxed);
        }
        self.admissions.fetch_add(1, Ordering::Relaxed);
        self.note_peak_locked();
        self.mint_charge(ChargeClass::Pinned, bytes)
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
        let _admission_lock = self.lock_aggregate_state();
        self.pinned_bytes.load(Ordering::Relaxed) > self.limits.pin_threshold_bytes
    }

    /// Headroom a refusable reservation would see right now. Advisory
    /// only — a concurrent reservation may consume it before the caller
    /// acts, which is why admission itself is a CAS and not a
    /// check-then-reserve.
    #[must_use]
    pub fn refusable_headroom_bytes(&self) -> usize {
        let _admission_lock = self.lock_aggregate_state();
        self.limits
            .aggregate_ceiling_bytes
            .saturating_sub(self.refusable_bytes.load(Ordering::Relaxed))
            .saturating_sub(self.pinned_bytes.load(Ordering::Relaxed))
    }

    /// Snapshot occupancy and decision counters.
    #[must_use]
    pub fn snapshot(&self) -> RetentionAccountSnapshot {
        let _admission_lock = self.lock_aggregate_state();
        let active = self.active_bytes.load(Ordering::Relaxed);
        let refusable = self.refusable_bytes.load(Ordering::Relaxed);
        RetentionAccountSnapshot {
            active_bytes: active,
            retained_bytes: refusable.saturating_sub(active),
            pinned_bytes: self.pinned_bytes.load(Ordering::Relaxed),
            peak_total_bytes: self.peak_total_bytes.load(Ordering::Relaxed),
            admissions: self.admissions.load(Ordering::Relaxed) as u64,
            refusals_oversized: self.refusals_oversized.load(Ordering::Relaxed) as u64,
            refusals_pressure: self.refusals_pressure.load(Ordering::Relaxed) as u64,
            refusals_active: self.refusals_active.load(Ordering::Relaxed) as u64,
            releases: self.releases.load(Ordering::Relaxed) as u64,
        }
    }

    fn mint_charge(self: &Arc<Self>, class: ChargeClass, bytes: usize) -> RetentionCharge {
        RetentionCharge {
            account: Some(Arc::clone(self)),
            class,
            bytes,
        }
    }

    /// The single release path. Reached ONLY from [`RetentionCharge`]'s
    /// `Drop`, which is why exactly-once release is structural rather
    /// than a discipline callers must remember.
    fn release(&self, class: ChargeClass, bytes: usize) {
        let _admission_lock = self.lock_aggregate_state();
        match class {
            ChargeClass::Pinned => {
                if bytes > 0 {
                    let pinned = self.pinned_bytes.load(Ordering::Relaxed);
                    self.pinned_bytes.store(pinned - bytes, Ordering::Relaxed);
                }
            }
            ChargeClass::Active => {
                if bytes > 0 {
                    let active = self.active_bytes.load(Ordering::Relaxed);
                    let refusable = self.refusable_bytes.load(Ordering::Relaxed);
                    self.active_bytes.store(active - bytes, Ordering::Relaxed);
                    self.refusable_bytes
                        .store(refusable - bytes, Ordering::Relaxed);
                }
            }
            ChargeClass::Retained => {
                if bytes > 0 {
                    let refusable = self.refusable_bytes.load(Ordering::Relaxed);
                    self.refusable_bytes
                        .store(refusable - bytes, Ordering::Relaxed);
                }
            }
        }
        self.releases.fetch_add(1, Ordering::Relaxed);
    }

    fn lock_aggregate_state(&self) -> parking_lot::MutexGuard<'_, ()> {
        self.admission_lock.lock()
    }

    /// Called only while [`Self::lock_aggregate_state`] is held.
    fn note_peak_locked(&self) {
        let total = self.refusable_bytes.load(Ordering::Relaxed)
            + self.pinned_bytes.load(Ordering::Relaxed);
        self.peak_total_bytes.fetch_max(total, Ordering::Relaxed);
        verter_audit::attribute_max!(StoreRetainedBytes, total);
    }
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
/// pressure binds its private account explicitly through [`Self::new`].
#[derive(Debug, Clone)]
pub struct StoreAccount(Arc<SemanticRetentionAccount>);

impl Default for StoreAccount {
    fn default() -> Self {
        Self(SemanticRetentionAccount::process_local())
    }
}

impl StoreAccount {
    /// Bind an explicit account. Production hosts thread the project's
    /// account — which IS the process-local one — through here.
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
}

impl RetentionCharge {
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
                let _admission_lock = account.lock_aggregate_state();
                if self.bytes > 0 {
                    let active = account.active_bytes.load(Ordering::Relaxed);
                    account
                        .active_bytes
                        .store(active - self.bytes, Ordering::Relaxed);
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
            account.release(self.class, self.bytes);
        }
    }
}
