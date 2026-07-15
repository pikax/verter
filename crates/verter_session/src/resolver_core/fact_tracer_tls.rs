//! Thread-local backing store for the fact-read tracer.
//!
//! One cold compute on one thread holds a [`FactReadSetCell`] for its
//! lifetime. The installer — [`crate::VerterHost::with_fact_tracer`], which
//! lives in `resolver_context.rs` — plants the cell into this per-thread stack
//! and the trait method `ResolverContext::current_fact_tracer` reads it back.
//! The documented R18 carve-out that justifies this per-compute thread-local
//! (and the installer itself) lives above the installer in
//! `resolver_context.rs`, where the `r18_carve_out_documented_for_tls_installer`
//! architecture guard verifies it.
//!
//! Nesting is supported: the active tracers form a per-thread STACK
//! (`ACTIVE_TRACERS`), and every observation / non-cacheability mark fans out
//! to ALL active levels, so an inner scope's observations are also seen by
//! every enclosing scope. Readers must go through
//! `ResolverContext::current_fact_tracer`, never through the TLS slot directly;
//! the slot is private to this module.

use std::cell::RefCell;

use smallvec::SmallVec;

use crate::resolver_core::fact_read_set::NonCacheablePropagation;
use crate::resolver_core::{FactReadSetCell, FactVersionRef};

thread_local! {
    /// Per-thread tracer stack.
    ///
    /// Each entry is a raw pointer to the `FactReadSetCell` owned by
    /// one `with_fact_tracer` scope on this thread. The stack allows
    /// nested fact-tracer scopes: the innermost scope sits at the top;
    /// `observe_fan_out*` fans observations into **all** levels so every
    /// outer scope captures the inner scope's observations.
    ///
    /// SAFETY contract: each pointer is valid for exactly the duration of
    /// the `with_fact_tracer` call that installed it. `install` pushes the
    /// pointer and `clear` (called in the RAII drop) pops the top.
    /// Between push and pop no other thread can mutate the TLS slot, and
    /// the `FactReadSetCell` is stack-allocated in `with_fact_tracer` on
    /// the same thread — so the pointee outlives its slot entry.
    ///
    /// `RefCell` storage with a clone-then-release-then-iterate
    /// access pattern (see `observe_fan_out{,_borrowed}` below)
    /// is what makes this design reentrancy-safe: each fan-out
    /// borrows the slot only long enough to clone the small
    /// `SmallVec` of raw pointers, drops the borrow, and iterates
    /// the clone. No borrow is held when the per-cell `observe`
    /// runs, so a re-entrant `install` / `clear` inside an
    /// observer cannot trigger `BorrowMutError`. `Cell::take()`
    /// + `Cell::set()` would also satisfy this contract — and
    /// works with non-`Copy` payloads because `Cell::take()`
    /// internally calls `mem::replace`. The borrow-clone-release
    /// pattern is exercised by
    /// `tests/cases/g_misc0/tracer_stack_reentrant_observe_safe.rs`. All access
    /// is single-threaded (TLS).
    static ACTIVE_TRACERS: RefCell<SmallVec<[*const FactReadSetCell; 8]>> =
        RefCell::new(SmallVec::new());
}

/// Push `cell` onto the tracer stack.
///
/// Nesting is intentional: a nested `with_fact_tracer` scope adds its
/// cell to the stack so `observe_fan_out*` delivers observations to both
/// the inner scope and all outer scopes simultaneously.
///
/// SAFETY: the caller (`with_fact_tracer`) keeps `cell` alive for the
/// entire scope duration. `clear` is called on the RAII guard's drop —
/// even on panic — so the pointer is removed before the cell is freed.
pub(super) fn install(cell: &FactReadSetCell) {
    ACTIVE_TRACERS.with(|slot| {
        slot.borrow_mut().push(cell as *const FactReadSetCell);
    });
}

/// Pop the top-of-stack entry. Called on the installer's `Drop`.
pub(super) fn clear() {
    ACTIVE_TRACERS.with(|slot| {
        slot.borrow_mut().pop();
    });
}

/// Return the top-of-stack tracer, or `None` when the stack is empty.
///
/// Used by existing single-tracer callers that only need the innermost
/// active scope. These callers write into the top cell; the fan-out
/// functions below reach all cells.
#[inline]
pub(super) fn current_tracer<'a>() -> Option<&'a FactReadSetCell> {
    ACTIVE_TRACERS.with(|slot| {
        let stack = slot.borrow();
        let ptr = stack.last().copied();
        drop(stack);
        match ptr {
            Some(p) if !p.is_null() => {
                // SAFETY: each live stack entry is installed by
                // `with_fact_tracer`; the RAII guard (`TracerScope`)
                // calls `clear()` on drop (including on unwind), so
                // no dangling pointer can remain on the stack.
                Some(unsafe { &*p })
            }
            _ => None,
        }
    })
}

/// Fan an observed fact into **every** active tracer on the stack.
///
/// Snapshot-then-iterate: collect the pointer set under a borrow,
/// drop the borrow, then iterate the collected set. No borrow is held
/// during the `observe` calls, so re-entrant `install`/`clear` calls
/// from inside a tracer are safe.
#[inline]
pub(super) fn observe_fan_out(fact: FactVersionRef) {
    // Collect pointers under a short borrow, then drop the borrow
    // before calling into FactReadSetCell so re-entrant installs
    // from inside an observer don't cause RefCell panics.
    let ptrs: SmallVec<[*const FactReadSetCell; 8]> =
        ACTIVE_TRACERS.with(|slot| slot.borrow().clone());
    for ptr in ptrs {
        if !ptr.is_null() {
            // SAFETY: see module-level SAFETY contract.
            unsafe { &*ptr }.observe(fact.clone());
        }
    }
}

/// Fan a borrowed signature into **every** active tracer on the stack.
#[inline]
pub(super) fn observe_fan_out_borrowed(sig: &[FactVersionRef]) {
    if sig.is_empty() {
        return;
    }
    let ptrs: SmallVec<[*const FactReadSetCell; 8]> =
        ACTIVE_TRACERS.with(|slot| slot.borrow().clone());
    for ptr in ptrs {
        if !ptr.is_null() {
            // SAFETY: see module-level SAFETY contract.
            unsafe { &*ptr }.observe_borrowed_signature(sig);
        }
    }
}

/// Mark **every** active tracer on the stack as having consumed a
/// NON-CACHEABLE read (a fenced serve, a broken decl-body lease, an
/// unrootable route, or an unobservable source-env identity).
///
/// Called from the non-cacheability marking chokepoints (the
/// `IndexedReady` serve chokepoint, the overlay materialiser, the
/// frontier route reader's per-walk memo, and the typed `LeaseMiss`
/// collapse boundaries) on the consuming thread, so every enclosing
/// traced cold compute — the semantic-memo build, the
/// owner-import-surface producer, the component-meta proof producers —
/// observes the non-cacheable consumption by value and can refuse
/// shared-cache admission. Same snapshot-then-iterate reentrancy
/// discipline as [`observe_fan_out`].
#[inline]
pub(super) fn note_non_cacheable_read(propagation: NonCacheablePropagation) {
    let ptrs: SmallVec<[*const FactReadSetCell; 8]> =
        ACTIVE_TRACERS.with(|slot| slot.borrow().clone());
    let first = match propagation {
        NonCacheablePropagation::LocalOnly => ptrs.len().saturating_sub(1),
        NonCacheablePropagation::Transitive => 0,
    };
    for ptr in ptrs.into_iter().skip(first) {
        if !ptr.is_null() {
            // SAFETY: see module-level SAFETY contract.
            unsafe { &*ptr }.note_non_cacheable_read(propagation);
        }
    }
}

#[cfg(test)]
mod propagation_tests {
    use super::*;
    use crate::resolver_core::fact_read_set::NonCacheablePropagation;

    #[test]
    fn local_only_refusal_marks_only_the_owning_tracer() {
        let outer = FactReadSetCell::new();
        let inner = FactReadSetCell::new();
        install(&outer);
        install(&inner);

        note_non_cacheable_read(NonCacheablePropagation::LocalOnly);

        clear();
        clear();
        assert!(!outer.non_cacheable_read_observed());
        assert!(inner.non_cacheable_read_observed());
    }

    #[test]
    fn transitive_hazard_marks_every_enclosing_tracer() {
        let outer = FactReadSetCell::new();
        let inner = FactReadSetCell::new();
        install(&outer);
        install(&inner);

        note_non_cacheable_read(NonCacheablePropagation::Transitive);

        clear();
        clear();
        assert!(outer.non_cacheable_read_observed());
        assert!(inner.non_cacheable_read_observed());
    }
}
