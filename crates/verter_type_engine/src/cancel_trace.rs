//! Where a cancellation met the request's work, recorded on the
//! request's own thread while [`traced`] runs: which
//! semantic query family was innermost at every instant, and the first
//! poll that observed the cancellation. The signature-kernel
//! cancellation probe reads it to split a stop into the delay before a
//! poll saw the cancellation and the unwind after it.
//!
//! Owner: the one `traced` call that installs it on its thread.
//! Lifetime: that call. Release: the call's guard takes it back out of
//! the thread-local, on return and on unwind; nothing is recorded on a
//! thread with no trace installed.

use std::cell::RefCell;
use std::panic::Location;
use std::time::Instant;

use crate::semantic_query::SemanticQueryKeyTag;

/// What one traced request recorded.
#[derive(Debug, Default, Clone)]
pub struct CancelTrace {
    /// Every change of the innermost active query family: from
    /// when, and which family (`None`: no family open).
    pub transitions: Vec<(Instant, Option<SemanticQueryKeyTag>)>,
    /// The first poll that observed the cancellation, and when.
    pub first_observed: Option<(&'static Location<'static>, Instant)>,
    /// Named points of the request's own lifecycle, each at its
    /// first occurrence: `evaluated` (the dispatch step returned)
    /// and `released` (the dispatch and its transaction dropped).
    pub marks: Vec<(&'static str, Instant)>,
}

impl CancelTrace {
    /// The innermost query family open at `at`.
    #[must_use]
    pub fn active_at(&self, at: Instant) -> Option<SemanticQueryKeyTag> {
        let index = self.transitions.partition_point(|(from, _)| *from <= at);
        index.checked_sub(1).and_then(|i| self.transitions[i].1)
    }
}

struct State {
    trace: CancelTrace,
    open: Vec<SemanticQueryKeyTag>,
}

thread_local! {
    static ACTIVE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Run `work` with a trace installed on this thread and return
/// what it recorded.
pub fn traced<R>(work: impl FnOnce() -> R) -> (R, CancelTrace) {
    struct Installed;
    impl Drop for Installed {
        fn drop(&mut self) {
            ACTIVE.with(|cell| cell.borrow_mut().take());
        }
    }
    ACTIVE.with(|cell| {
        *cell.borrow_mut() = Some(State {
            trace: CancelTrace::default(),
            open: Vec::new(),
        });
    });
    let installed = Installed;
    let result = work();
    let trace = ACTIVE
        .with(|cell| cell.borrow_mut().take())
        .map(|state| state.trace)
        .unwrap_or_default();
    drop(installed);
    (result, trace)
}

thread_local! {
    static CANCEL_AT_FAMILY: RefCell<Option<(verter_execution::cancellation::CancellationToken, usize)>> =
        const { RefCell::new(None) };
}

/// Cancel `token` as the `nth` query family (1-based) opens on this
/// thread, while the returned guard lives: the poll that follows the
/// family's opening — the connected demand's entry at that query
/// boundary — is the first to observe it. A test cancels a request at
/// a chosen point of its work this way, without a second thread.
pub fn cancel_at_family_entry(
    token: verter_execution::cancellation::CancellationToken,
    nth: usize,
) -> impl Drop {
    struct Disarm;
    impl Drop for Disarm {
        fn drop(&mut self) {
            CANCEL_AT_FAMILY.with(|cell| cell.borrow_mut().take());
        }
    }
    CANCEL_AT_FAMILY.with(|cell| *cell.borrow_mut() = Some((token, nth.max(1))));
    Disarm
}

/// An open query family; closing it restores the enclosing one.
pub(crate) struct FamilyFrame {
    recorded: bool,
}

/// Open `family` as the innermost query family until the returned
/// frame drops.
pub(crate) fn enter_family(family: SemanticQueryKeyTag) -> FamilyFrame {
    CANCEL_AT_FAMILY.with(|cell| {
        let mut slot = cell.borrow_mut();
        if let Some((token, remaining)) = slot.as_mut() {
            *remaining = remaining.saturating_sub(1);
            if *remaining == 0 {
                token.cancel();
                *slot = None;
            }
        }
    });
    let recorded = ACTIVE.with(|cell| {
        let mut slot = cell.borrow_mut();
        let Some(state) = slot.as_mut() else {
            return false;
        };
        state.open.push(family);
        state.trace.transitions.push((Instant::now(), Some(family)));
        true
    });
    FamilyFrame { recorded }
}

impl Drop for FamilyFrame {
    fn drop(&mut self) {
        if !self.recorded {
            return;
        }
        ACTIVE.with(|cell| {
            if let Some(state) = cell.borrow_mut().as_mut() {
                state.open.pop();
                let enclosing = state.open.last().copied();
                state.trace.transitions.push((Instant::now(), enclosing));
            }
        });
    }
}

/// The request reached the lifecycle point `label`.
pub fn mark(label: &'static str) {
    ACTIVE.with(|cell| {
        if let Some(state) = cell.borrow_mut().as_mut() {
            if !state.trace.marks.iter().any(|(seen, _)| *seen == label) {
                state.trace.marks.push((label, Instant::now()));
            }
        }
    });
}

/// A poll at `site` observed the cancellation.
pub(crate) fn observed(site: &'static Location<'static>) {
    ACTIVE.with(|cell| {
        if let Some(state) = cell.borrow_mut().as_mut() {
            state
                .trace
                .first_observed
                .get_or_insert_with(|| (site, Instant::now()));
        }
    });
}
