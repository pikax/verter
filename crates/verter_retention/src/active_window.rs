//! Bounded ACTIVE-occupancy measurement windows over the one account.
//!
//! A window answers one question a tooling consumer cannot answer from a
//! [`crate::RetentionAccountSnapshot`]: what was the largest number of
//! [`ChargeClass::Active`] bytes charged at once WHILE a given workload
//! ran? The snapshot's `peak_total_bytes` mixes every class and covers the
//! account's whole lifetime, so a large retained or pinned total, or an
//! ACTIVE peak reached before the workload started, would pass for the
//! workload's own. A window instead starts its peak at the ACTIVE bytes
//! charged when it opens and raises it only on ACTIVE transitions it
//! observes until it closes.
//!
//! ## Owners and provenance
//!
//! A window measures ONE explicit [`ActiveOwner`]: a token minted by the
//! account ([`SemanticRetentionAccount::mint_active_owner`]) that the
//! measured workload reserves through
//! ([`SemanticRetentionAccount::reserve_as`]). The owner travels with the
//! charge — through sharing, `split_off`, `absorb`, holding adoption,
//! release and class transfers — and the account keeps each owner's ACTIVE
//! bytes exactly. A window's peak is its owner's own ACTIVE peak: bytes of
//! any other owner never raise it, however they overlap the interval.
//!
//! A charge reserved without an owner ([`SemanticRetentionAccount::reserve`])
//! has UNKNOWN provenance and is never implicitly attributed to a window; a
//! charge that merged bytes of two different owners is UNKNOWN too. An ACTIVE
//! transition of an UNKNOWN charge inside the interval rejects the witness
//! ([`ActiveWitnessRejection::UnknownProvenance`]) because it could be the
//! workload's own work or foreign work. Identified foreign owners are
//! excluded and merely counted.
//!
//! ## What a window observes
//!
//! Every transition of the ACTIVE total — an admitted ACTIVE reservation,
//! an ACTIVE release, an ACTIVE charge committed to retained or transferred
//! to pinned, and a retained slice joining an ACTIVE charge — is noted by
//! the account under the same lock that decides admission, so a window
//! opened or closed concurrently with transitions sees each of them
//! exactly once: on one side of its boundary or the other. Each window
//! keeps its own counters: opening or finishing one never resets another
//! reader's, and overlapping windows report independently.
//!
//! The account numbers its ACTIVE transitions; a report carries the
//! sequence numbers its interval spans and conserves its owner's bytes:
//! `active_at_open + rise_bytes - fall_bytes == active_at_close`.
//!
//! Only [`ActiveWindowReport::attributed`] yields a peak a crossing check
//! may read; a rejected report never satisfies one. It also rejects the
//! owner's own ACTIVE bytes charged at open (pre-window work) or still
//! charged at close (work in flight, or the workload's own leak).
//!
//! ## Build availability
//!
//! The capability is optional test/tooling state: it exists only under
//! `test`, `test-support` or the default-off `soak-support` feature, and
//! [`crate::ACTIVE_OCCUPANCY_WINDOW_AVAILABLE`] reports which. An ordinary
//! production build carries no window registry, no transition sequence
//! and no per-charge provenance; the account's current occupancy
//! ([`crate::SemanticRetentionAccount::snapshot`]) stays available there.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use crate::{ChargeClass, Provenance, RetentionAdmission, SemanticRetentionAccount};

/// The account-side window registry. Every method that mutates it is
/// called with the account's admission lock held.
#[derive(Debug, Default)]
pub(crate) struct ActiveWindows {
    /// Owners and windows minted so far; ids are their minting order.
    ids: AtomicU64,
    /// ACTIVE transitions the account has made.
    transitions: AtomicU64,
    /// Open windows — the transition hook's lock-free miss path.
    open: AtomicUsize,
    inner: parking_lot::Mutex<Registry>,
}

#[derive(Debug, Default)]
struct Registry {
    /// Each owner's ACTIVE bytes charged right now; absent means zero.
    owner_active: HashMap<u64, usize>,
    states: Vec<WindowState>,
}

#[derive(Debug, Clone, Copy)]
struct WindowState {
    id: u64,
    owner: u64,
    opened_at: u64,
    active_at_open: usize,
    peak_active_bytes: usize,
    rise_bytes: usize,
    fall_bytes: usize,
    transitions: u64,
    foreign_transitions: u64,
    unknown_transitions: u64,
}

impl ActiveWindows {
    fn next_id(&self) -> u64 {
        self.ids.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Note one ACTIVE transition of a charge attributed to `owner` (or to
    /// no one): the ACTIVE total moved by `rise` or `fall` bytes. Called
    /// with the admission lock held.
    pub(crate) fn note_locked(&self, owner: Option<u64>, rise: usize, fall: usize) {
        if rise == 0 && fall == 0 {
            return;
        }
        self.transitions.fetch_add(1, Ordering::Relaxed);
        let mut registry = self.inner.lock();
        let owner_active = owner.map(|owner| {
            let active = registry.owner_active.entry(owner).or_default();
            *active = (*active + rise)
                .checked_sub(fall)
                .expect("a fall never exceeds the owner's recorded ACTIVE bytes");
            let active = *active;
            if active == 0 {
                registry.owner_active.remove(&owner);
            }
            active
        });
        if self.open.load(Ordering::Relaxed) == 0 {
            return;
        }
        for state in &mut registry.states {
            match (owner, owner_active) {
                (Some(owner), Some(active)) if owner == state.owner => {
                    state.peak_active_bytes = state.peak_active_bytes.max(active);
                    state.rise_bytes += rise;
                    state.fall_bytes += fall;
                    state.transitions += 1;
                }
                (Some(_), _) => state.foreign_transitions += 1,
                (None, _) => state.unknown_transitions += 1,
            }
        }
    }

    fn open_locked(&self, owner: u64) -> u64 {
        let id = self.next_id();
        let mut registry = self.inner.lock();
        let active = registry.owner_active.get(&owner).copied().unwrap_or(0);
        registry.states.push(WindowState {
            id,
            owner,
            opened_at: self.transitions.load(Ordering::Relaxed),
            active_at_open: active,
            peak_active_bytes: active,
            rise_bytes: 0,
            fall_bytes: 0,
            transitions: 0,
            foreign_transitions: 0,
            unknown_transitions: 0,
        });
        self.open.fetch_add(1, Ordering::Relaxed);
        id
    }

    fn report_locked(&self, id: u64, close: bool) -> ActiveWindowReport {
        let mut registry = self.inner.lock();
        let index = registry
            .states
            .iter()
            .position(|state| state.id == id)
            .expect("an unfinished window stays registered");
        let state = if close {
            self.open.fetch_sub(1, Ordering::Relaxed);
            registry.states.swap_remove(index)
        } else {
            registry.states[index]
        };
        let active = registry
            .owner_active
            .get(&state.owner)
            .copied()
            .unwrap_or(0);
        ActiveWindowReport {
            window: ActiveWindowId(state.id),
            owner: ActiveOwnerId(state.owner),
            opened_at: state.opened_at,
            closed_at: self.transitions.load(Ordering::Relaxed),
            active_at_open: state.active_at_open,
            active_at_close: active,
            peak_active_bytes: state.peak_active_bytes,
            rise_bytes: state.rise_bytes,
            fall_bytes: state.fall_bytes,
            transitions: state.transitions,
            foreign_transitions: state.foreign_transitions,
            unknown_transitions: state.unknown_transitions,
        }
    }
}

/// An explicit workload identity bound to ONE account. Reserving through it
/// ([`SemanticRetentionAccount::reserve_as`]) attributes the charge — and
/// everything that charge is shared, split, merged or transferred into — to
/// this owner.
#[derive(Debug, Clone)]
pub struct ActiveOwner {
    account: Arc<SemanticRetentionAccount>,
    id: u64,
}

impl ActiveOwner {
    /// This owner's identity on its account.
    #[must_use]
    pub fn id(&self) -> ActiveOwnerId {
        ActiveOwnerId(self.id)
    }

    pub(crate) fn assert_bound_to(&self, account: &Arc<SemanticRetentionAccount>) {
        assert!(
            Arc::ptr_eq(&self.account, account),
            "an ACTIVE owner is bound to the account that minted it"
        );
    }
}

/// An owner's identity: its minting order on its account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActiveOwnerId(u64);

impl ActiveOwnerId {
    pub(crate) fn raw(self) -> u64 {
        self.0
    }
}

impl SemanticRetentionAccount {
    /// Mint a fresh workload identity bound to this account.
    #[must_use]
    pub fn mint_active_owner(self: &Arc<Self>) -> ActiveOwner {
        ActiveOwner {
            account: Arc::clone(self),
            id: self.active_windows.next_id(),
        }
    }

    /// Reserve exactly as [`Self::reserve`], attributing the charge to
    /// `owner`. Admission, refusal and allocation classification are
    /// unchanged.
    ///
    /// # Panics
    ///
    /// When `owner` was minted by a different account.
    pub fn reserve_as(
        self: &Arc<Self>,
        owner: &ActiveOwner,
        class: ChargeClass,
        bytes: usize,
    ) -> RetentionAdmission {
        self.reserve_with(class, bytes, Provenance::of(owner, self))
    }

    /// Open an ACTIVE-occupancy window on this account measuring `owner`.
    /// See the [module docs](crate::active_window) for what it observes and
    /// when its witness is attributable.
    ///
    /// # Panics
    ///
    /// When `owner` was minted by a different account.
    #[must_use = "a window measures only until it is finished or dropped"]
    pub fn open_active_window(self: &Arc<Self>, owner: &ActiveOwner) -> ActiveOccupancyWindow {
        owner.assert_bound_to(self);
        let _admission_lock = self.lock_aggregate_state();
        let id = self.active_windows.open_locked(owner.id);
        ActiveOccupancyWindow {
            account: Some(Arc::clone(self)),
            id,
        }
    }

    /// ACTIVE transitions this account has made, in sequence; a report's
    /// interval is a span of this sequence.
    #[must_use]
    pub fn active_transition_count(&self) -> u64 {
        let _admission_lock = self.lock_aggregate_state();
        self.active_windows.transitions.load(Ordering::Relaxed)
    }
}

/// A window's identity: its opening order on its account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActiveWindowId(u64);

/// An open ACTIVE-occupancy window. Dropping it unfinished deregisters it.
#[derive(Debug)]
pub struct ActiveOccupancyWindow {
    /// `None` only once finished.
    account: Option<Arc<SemanticRetentionAccount>>,
    id: u64,
}

impl ActiveOccupancyWindow {
    /// This window's identity.
    #[must_use]
    pub fn id(&self) -> ActiveWindowId {
        ActiveWindowId(self.id)
    }

    /// The window so far, without closing it: `closed_at` and
    /// `active_at_close` are the current values.
    #[must_use]
    pub fn observe(&self) -> ActiveWindowReport {
        let account = self
            .account
            .as_ref()
            .expect("an open window has its account");
        let _admission_lock = account.lock_aggregate_state();
        account.active_windows.report_locked(self.id, false)
    }

    /// Close the window and report its interval. Transitions after this
    /// point are not observed.
    #[must_use]
    pub fn finish(mut self) -> ActiveWindowReport {
        let account = self.account.take().expect("an open window has its account");
        let _admission_lock = account.lock_aggregate_state();
        account.active_windows.report_locked(self.id, true)
    }
}

impl Drop for ActiveOccupancyWindow {
    fn drop(&mut self) {
        if let Some(account) = self.account.take() {
            let _admission_lock = account.lock_aggregate_state();
            let _ = account.active_windows.report_locked(self.id, true);
        }
    }
}

/// What one window observed of its owner's ACTIVE bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveWindowReport {
    /// The window this reports.
    pub window: ActiveWindowId,
    /// The owner it measured.
    pub owner: ActiveOwnerId,
    /// The account's ACTIVE transition count when the window opened.
    pub opened_at: u64,
    /// The account's ACTIVE transition count when the window closed (or
    /// was observed); transitions numbered `opened_at + 1 ..= closed_at`
    /// fall inside the interval.
    pub closed_at: u64,
    /// The owner's ACTIVE bytes charged when the window opened.
    pub active_at_open: usize,
    /// The owner's ACTIVE bytes charged when the window closed (or was
    /// observed).
    pub active_at_close: usize,
    /// The most ACTIVE bytes the owner held at once inside the interval,
    /// counting the bytes charged at open.
    pub peak_active_bytes: usize,
    /// ACTIVE bytes the owner added inside the interval.
    pub rise_bytes: usize,
    /// ACTIVE bytes the owner removed inside the interval.
    pub fall_bytes: usize,
    /// The owner's ACTIVE transitions inside the interval.
    pub transitions: u64,
    /// ACTIVE transitions of other identified owners inside the interval;
    /// excluded from every figure above.
    pub foreign_transitions: u64,
    /// ACTIVE transitions of charges with unknown or mixed provenance inside
    /// the interval.
    pub unknown_transitions: u64,
}

impl ActiveWindowReport {
    /// The interval's peak as the measured owner's own, or why it is not
    /// attributable to it. See the [module docs](crate::active_window).
    ///
    /// # Errors
    ///
    /// Unknown-provenance transitions in the interval, or the owner's own
    /// ACTIVE bytes charged at open or still charged at close.
    pub fn attributed(&self) -> Result<AttributedActivePeak, ActiveWitnessRejection> {
        if self.unknown_transitions > 0 {
            return Err(ActiveWitnessRejection::UnknownProvenance {
                transitions: self.unknown_transitions,
            });
        }
        if self.active_at_open > 0 {
            return Err(ActiveWitnessRejection::ActiveAtOpen {
                bytes: self.active_at_open,
            });
        }
        if self.active_at_close > 0 {
            return Err(ActiveWitnessRejection::ActiveAtClose {
                bytes: self.active_at_close,
            });
        }
        Ok(AttributedActivePeak {
            peak_active_bytes: self.peak_active_bytes,
        })
    }
}

/// An ACTIVE peak attributable to the measured owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttributedActivePeak {
    /// The most ACTIVE bytes the owner held at once.
    pub peak_active_bytes: usize,
}

impl AttributedActivePeak {
    /// Whether the owner's ACTIVE occupancy reached `threshold` bytes.
    #[must_use]
    pub fn crossed(&self, threshold: usize) -> bool {
        self.peak_active_bytes >= threshold
    }
}

/// Why a window's peak is not attributable to the measured owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveWitnessRejection {
    /// Charges of unknown or mixed provenance made ACTIVE transitions
    /// inside the window.
    UnknownProvenance {
        /// How many such transitions.
        transitions: u64,
    },
    /// The owner already held ACTIVE bytes when the window opened.
    ActiveAtOpen {
        /// Bytes charged at open.
        bytes: usize,
    },
    /// The owner still held ACTIVE bytes when the window closed.
    ActiveAtClose {
        /// Bytes charged at close.
        bytes: usize,
    },
}

#[cfg(test)]
#[path = "active_window_tests.rs"]
mod tests;
