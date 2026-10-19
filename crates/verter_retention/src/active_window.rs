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
//! sequence numbers its interval spans and conserves its bytes:
//! `active_at_open + rise_bytes - fall_bytes == active_at_close`.
//!
//! ## Attribution
//!
//! A charge carries no owner identity, so the window attributes EVERY
//! ACTIVE transition on the account inside its interval to the measured
//! workload. That is sound only for a workload running alone on the
//! process account — the precondition a tooling runner meets by running
//! the workload in its own process. The window rejects the witness
//! ([`ActiveWitnessRejection`]) whenever foreign work is detectable:
//!
//! - ACTIVE bytes already charged when it opened (pre-window work in
//!   flight, whose bytes would inflate the peak);
//! - a transition of a charge minted before it opened (pre-window work
//!   that settled or grew inside the interval);
//! - ACTIVE bytes still charged when it closed (work still in flight — an
//!   unrelated owner, or the workload's own leak).
//!
//! Only [`ActiveWindowReport::attributed`] yields a peak a crossing check
//! may read; a rejected report never satisfies one.
//!
//! ## Build availability
//!
//! The capability is optional test/tooling state: it exists only under
//! `test`, `test-support` or the default-off `soak-support` feature, and
//! [`crate::ACTIVE_OCCUPANCY_WINDOW_AVAILABLE`] reports which. An ordinary
//! production build carries no window registry, no transition sequence
//! and no per-charge mint epoch; the account's current occupancy
//! ([`crate::SemanticRetentionAccount::snapshot`]) stays available there.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use crate::SemanticRetentionAccount;

/// The account-side window registry. Every method that mutates it is
/// called with the account's admission lock held.
#[derive(Debug, Default)]
pub(crate) struct ActiveWindows {
    /// Windows opened so far; a window's epoch is its opening count. A
    /// charge minted at epoch `e` predates every window whose epoch is
    /// greater than `e`.
    epoch: AtomicU64,
    /// ACTIVE transitions the account has made.
    transitions: AtomicU64,
    /// Open windows — the transition hooks' lock-free miss path.
    open: AtomicUsize,
    states: parking_lot::Mutex<Vec<WindowState>>,
}

#[derive(Debug, Clone, Copy)]
struct WindowState {
    epoch: u64,
    opened_at: u64,
    active_at_open: usize,
    peak_active_bytes: usize,
    rise_bytes: usize,
    fall_bytes: usize,
    transitions: u64,
    pre_window_transitions: u64,
}

impl ActiveWindows {
    /// The epoch a charge minted now carries.
    pub(crate) fn current_epoch(&self) -> u64 {
        self.epoch.load(Ordering::Relaxed)
    }

    /// Note one ACTIVE transition of a charge minted at `minted_epoch`:
    /// the ACTIVE total moved by `rise` or `fall` bytes to `active_after`.
    /// Called with the admission lock held.
    pub(crate) fn note_locked(
        &self,
        minted_epoch: u64,
        rise: usize,
        fall: usize,
        active_after: usize,
    ) {
        if rise == 0 && fall == 0 {
            return;
        }
        self.transitions.fetch_add(1, Ordering::Relaxed);
        if self.open.load(Ordering::Relaxed) == 0 {
            return;
        }
        for state in self.states.lock().iter_mut() {
            state.peak_active_bytes = state.peak_active_bytes.max(active_after);
            state.rise_bytes += rise;
            state.fall_bytes += fall;
            state.transitions += 1;
            if minted_epoch < state.epoch {
                state.pre_window_transitions += 1;
            }
        }
    }

    fn open_locked(&self, active: usize) -> u64 {
        let epoch = self.epoch.fetch_add(1, Ordering::Relaxed) + 1;
        self.states.lock().push(WindowState {
            epoch,
            opened_at: self.transitions.load(Ordering::Relaxed),
            active_at_open: active,
            peak_active_bytes: active,
            rise_bytes: 0,
            fall_bytes: 0,
            transitions: 0,
            pre_window_transitions: 0,
        });
        self.open.fetch_add(1, Ordering::Relaxed);
        epoch
    }

    fn report_locked(&self, epoch: u64, active: usize, close: bool) -> ActiveWindowReport {
        let mut states = self.states.lock();
        let index = states
            .iter()
            .position(|state| state.epoch == epoch)
            .expect("an unfinished window stays registered");
        let state = if close {
            self.open.fetch_sub(1, Ordering::Relaxed);
            states.swap_remove(index)
        } else {
            states[index]
        };
        ActiveWindowReport {
            window: ActiveWindowId(state.epoch),
            opened_at: state.opened_at,
            closed_at: self.transitions.load(Ordering::Relaxed),
            active_at_open: state.active_at_open,
            active_at_close: active,
            peak_active_bytes: state.peak_active_bytes,
            rise_bytes: state.rise_bytes,
            fall_bytes: state.fall_bytes,
            transitions: state.transitions,
            pre_window_transitions: state.pre_window_transitions,
        }
    }
}

impl SemanticRetentionAccount {
    /// Open an ACTIVE-occupancy window on this account. See the
    /// [module docs](crate::active_window) for what it observes and when
    /// its witness is attributable.
    #[must_use = "a window measures only until it is finished or dropped"]
    pub fn open_active_window(self: &Arc<Self>) -> ActiveOccupancyWindow {
        let _admission_lock = self.lock_aggregate_state();
        let active = self.active_bytes.load(Ordering::Relaxed);
        let epoch = self.active_windows.open_locked(active);
        ActiveOccupancyWindow {
            account: Some(Arc::clone(self)),
            epoch,
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
    epoch: u64,
}

impl ActiveOccupancyWindow {
    /// This window's identity.
    #[must_use]
    pub fn id(&self) -> ActiveWindowId {
        ActiveWindowId(self.epoch)
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
        let active = account.active_bytes.load(Ordering::Relaxed);
        account
            .active_windows
            .report_locked(self.epoch, active, false)
    }

    /// Close the window and report its interval. Transitions after this
    /// point are not observed.
    #[must_use]
    pub fn finish(mut self) -> ActiveWindowReport {
        let account = self.account.take().expect("an open window has its account");
        let _admission_lock = account.lock_aggregate_state();
        let active = account.active_bytes.load(Ordering::Relaxed);
        account
            .active_windows
            .report_locked(self.epoch, active, true)
    }
}

impl Drop for ActiveOccupancyWindow {
    fn drop(&mut self) {
        if let Some(account) = self.account.take() {
            let _admission_lock = account.lock_aggregate_state();
            let active = account.active_bytes.load(Ordering::Relaxed);
            let _ = account
                .active_windows
                .report_locked(self.epoch, active, true);
        }
    }
}

/// What one window observed of the ACTIVE class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveWindowReport {
    /// The window this reports.
    pub window: ActiveWindowId,
    /// The account's ACTIVE transition count when the window opened.
    pub opened_at: u64,
    /// The account's ACTIVE transition count when the window closed (or
    /// was observed); transitions numbered `opened_at + 1 ..= closed_at`
    /// fall inside the interval.
    pub closed_at: u64,
    /// ACTIVE bytes charged when the window opened.
    pub active_at_open: usize,
    /// ACTIVE bytes charged when the window closed (or was observed).
    pub active_at_close: usize,
    /// The most ACTIVE bytes charged at once inside the interval, counting
    /// the bytes charged at open.
    pub peak_active_bytes: usize,
    /// ACTIVE bytes added inside the interval.
    pub rise_bytes: usize,
    /// ACTIVE bytes removed inside the interval.
    pub fall_bytes: usize,
    /// ACTIVE transitions inside the interval.
    pub transitions: u64,
    /// Of those, transitions of charges minted before the window opened.
    pub pre_window_transitions: u64,
}

impl ActiveWindowReport {
    /// The interval's peak as the measured workload's own, or why it is
    /// not attributable to it. See the [module docs](crate::active_window).
    ///
    /// # Errors
    ///
    /// The first detectable foreign contribution: pre-window ACTIVE bytes,
    /// a transition of a pre-window charge, or ACTIVE bytes still charged
    /// at close.
    pub fn attributed(&self) -> Result<AttributedActivePeak, ActiveWitnessRejection> {
        if self.active_at_open > 0 {
            return Err(ActiveWitnessRejection::ActiveAtOpen {
                bytes: self.active_at_open,
            });
        }
        if self.pre_window_transitions > 0 {
            return Err(ActiveWitnessRejection::PreWindowCharge {
                transitions: self.pre_window_transitions,
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

/// An ACTIVE peak attributable to the measured workload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttributedActivePeak {
    /// The most ACTIVE bytes the workload held at once.
    pub peak_active_bytes: usize,
}

impl AttributedActivePeak {
    /// Whether the workload's ACTIVE occupancy reached `threshold` bytes.
    #[must_use]
    pub fn crossed(&self, threshold: usize) -> bool {
        self.peak_active_bytes >= threshold
    }
}

/// Why a window's peak is not attributable to the measured workload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveWitnessRejection {
    /// ACTIVE bytes were already charged when the window opened.
    ActiveAtOpen {
        /// Bytes charged at open.
        bytes: usize,
    },
    /// Charges minted before the window opened made ACTIVE transitions
    /// inside it.
    PreWindowCharge {
        /// How many such transitions.
        transitions: u64,
    },
    /// ACTIVE bytes were still charged when the window closed.
    ActiveAtClose {
        /// Bytes charged at close.
        bytes: usize,
    },
}

#[cfg(test)]
#[path = "active_window_tests.rs"]
mod tests;
