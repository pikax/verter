//! Thread-local backing store for the fact-read tracer.
//!
//! One cold compute on one thread holds a [`FactReadSetCell`] for its
//! lifetime. The installer — [`crate::VerterHost::with_fact_tracer`], which
//! lives in `resolver_context.rs` — plants the cell into this per-thread stack
//! and the trait method `ResolverContext::current_fact_tracer` reads it back.
//! This module also APPLIES non-cacheability: a non-cacheable read mark
//! reaches the tracers and recorders on this stack by a direct call
//! ([`apply_non_cacheable_propagation`]), never through a process-global
//! hook.
//! The documented R18 carve-out that justifies this per-compute thread-local
//! (and the installer itself) lives above the installer in
//! `resolver_context.rs`, where the `r18_carve_out_documented_for_tls_installer`
//! architecture guard verifies it.
//!
//! Nesting is supported: the active tracers form a per-thread STACK
//! (`ACTIVE_TRACERS`), and every observation fans out to every enclosing
//! scope that would not otherwise see it: down the stack from the innermost
//! scope to the first DELIVERING scope ([`install_delivering`]) inclusive. A
//! delivering scope's owner hands its whole completed read set (or a
//! stronger one) to the scopes active when it completes, so the scopes
//! below it receive its observations exactly once, already deduplicated,
//! instead of once per observation. Without a delivering scope an
//! observation reaches every level. A non-cacheability mark reaches every
//! level it names regardless. Readers must go through
//! `ResolverContext::current_fact_tracer`, never through the TLS slot directly;
//! the slot is private to this module.

use std::cell::{Cell, RefCell};

use smallvec::SmallVec;

use verter_session_query::facts::fact_read_set::{NonCacheableBasis, NonCacheablePropagation};
use verter_session_query::facts::{fact_cache::FactVersionRef, fact_read_set::FactReadSetCell};

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
    static ACTIVE_TRACERS: RefCell<SmallVec<[ActiveTracer; 8]>> =
        RefCell::new(SmallVec::new());

    /// Per-thread stack of passive [`FactReadRecorder`]s. Same SAFETY
    /// contract and reentrancy discipline as `ACTIVE_TRACERS`: each pointer
    /// is installed and removed by one `record_fact_reads` scope, whose
    /// guard pops it on drop, unwinding included.
    static ACTIVE_RECORDERS: RefCell<SmallVec<[*const FactReadRecorder; 4]>> =
        RefCell::new(SmallVec::new());
}

/// One installed tracer: its cell, and whether its owner delivers its
/// completed read set to the scopes below it.
#[derive(Clone, Copy)]
struct ActiveTracer {
    cell: *const FactReadSetCell,
    delivers: bool,
}

/// Every installed tracer cell, innermost last.
fn active_cells() -> SmallVec<[*const FactReadSetCell; 8]> {
    ACTIVE_TRACERS.with(|slot| {
        slot.borrow()
            .iter()
            .map(|tracer| tracer.cell)
            .filter(|cell| !cell.is_null())
            .collect()
    })
}

/// The tracer cells an observation made now must reach, innermost last:
/// from the innermost scope down to the first delivering scope inclusive.
fn observing_cells() -> SmallVec<[*const FactReadSetCell; 8]> {
    ACTIVE_TRACERS.with(|slot| {
        let stack = slot.borrow();
        // Every observation runs this, so it is written as plain loops.
        let mut first = stack.len();
        while first > 0 && !stack[first - 1].delivers {
            first -= 1;
        }
        let first = first.saturating_sub(1);
        let mut cells = SmallVec::new();
        for tracer in &stack[first..] {
            if !tracer.cell.is_null() {
                cells.push(tracer.cell);
            }
        }
        cells
    })
}

/// A passive witness of the fact reads made while it is installed: every
/// fanned-out observation, and whether any non-cacheable read was marked.
///
/// It is NOT a tracer. It never becomes [`current_tracer`], owns no
/// local-only non-cacheability mark, and no admission decision reads it,
/// so installing one changes nothing the tracers observe. It exists so a
/// computation's reads can be REPLAYED into scopes that were not live when
/// it ran — what a warm hit does with its stored signature.
#[derive(Default)]
pub(crate) struct FactReadRecorder {
    /// Every observation in fan-out order (a completed result's collapsed
    /// into its receipt, see [`collapse_evidence`]).
    facts: RefCell<Vec<FactVersionRef>>,
    non_cacheable: Cell<bool>,
}

impl FactReadRecorder {
    /// The distinct facts observed, in canonical order, and whether a
    /// non-cacheable read was.
    pub(crate) fn into_parts(self) -> (Vec<FactVersionRef>, bool) {
        let mut facts = self.facts.into_inner();
        facts.sort_unstable();
        facts.dedup();
        (facts, self.non_cacheable.get())
    }
}

/// Where every active tracer and recorder stood at one instant (see
/// [`mark_evidence`]); two of them, taken when a computation began and when
/// it ended, bound what it observed.
pub(crate) struct EvidenceMarks {
    tracers: SmallVec<
        [(
            *const FactReadSetCell,
            verter_session_query::facts::fact_read_set::ObservationMark,
        ); 8],
    >,
    recorders: SmallVec<[(*const FactReadRecorder, usize); 4]>,
}

/// Mark every active tracer and recorder: before a result that may
/// complete with a receipt begins, and again when it ends.
pub(crate) fn mark_evidence() -> EvidenceMarks {
    let tracers = active_cells()
        .into_iter()
        // SAFETY: see module-level SAFETY contract.
        .map(|ptr| (ptr, unsafe { &*ptr }.mark()))
        .collect();
    let recorders = ACTIVE_RECORDERS.with(|slot| {
        slot.borrow()
            .iter()
            // SAFETY: see `ACTIVE_RECORDERS`.
            .map(|&ptr| (ptr, unsafe { &*ptr }.facts.borrow().len()))
            .collect()
    });
    EvidenceMarks { tracers, recorders }
}

/// The result computed between `start` and `end` completed with
/// `receipt`: in every scope marked at both and still active, replace what
/// it observed in that range with the receipt, which stands for exactly
/// those observations — its evidence was recorded from the same fan-out.
/// An enclosing scope then holds the completed result's receipt, never a
/// copy of its facts; what it observed outside the range stays.
pub(crate) fn collapse_evidence(
    start: &EvidenceMarks,
    end: &EvidenceMarks,
    receipt: &FactVersionRef,
) {
    let active = active_cells();
    for (ptr, start_mark) in &start.tracers {
        let Some((_, end_mark)) = end.tracers.iter().find(|(end_ptr, _)| end_ptr == ptr) else {
            continue;
        };
        if active.contains(ptr) {
            // SAFETY: `ptr` is still on the stack, so its cell is alive.
            unsafe { &**ptr }.collapse_into_receipt(*start_mark, *end_mark, receipt.clone());
        }
    }
    let active: SmallVec<[*const FactReadRecorder; 4]> = active_recorders();
    for (ptr, start_len) in &start.recorders {
        let Some((_, end_len)) = end.recorders.iter().find(|(end_ptr, _)| end_ptr == ptr) else {
            continue;
        };
        if active.contains(ptr) {
            // SAFETY: `ptr` is still installed, so its recorder is alive.
            let mut facts = unsafe { &**ptr }.facts.borrow_mut();
            if start_len <= end_len && *end_len <= facts.len() {
                facts.splice(*start_len..*end_len, std::iter::once(receipt.clone()));
            } else {
                facts.push(receipt.clone());
            }
        }
    }
}

/// Pops the recorder its scope installed, unwinding included.
pub(crate) struct RecorderScope(());

impl Drop for RecorderScope {
    fn drop(&mut self) {
        ACTIVE_RECORDERS.with(|slot| {
            slot.borrow_mut().pop();
        });
    }
}

/// Install `recorder` for the lifetime of the returned scope.
///
/// SAFETY: the caller keeps `recorder` alive for as long as the scope,
/// and drops the scope first.
pub(crate) fn install_recorder(recorder: &FactReadRecorder) -> RecorderScope {
    ACTIVE_RECORDERS.with(|slot| {
        slot.borrow_mut().push(recorder as *const FactReadRecorder);
    });
    RecorderScope(())
}

fn active_recorders() -> SmallVec<[*const FactReadRecorder; 4]> {
    ACTIVE_RECORDERS.with(|slot| {
        let recorders = slot.borrow();
        if recorders.is_empty() {
            SmallVec::new()
        } else {
            recorders.clone()
        }
    })
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
pub(crate) fn install(cell: &FactReadSetCell) {
    push_tracer(cell, false);
}

/// Push `cell` as a DELIVERING scope: observations made while it is
/// installed reach it and the scopes above it, but not the scopes below.
///
/// The caller owns the delivery: when the scope completes, its whole read
/// set — or a carrier holding at least those facts — must reach the scopes
/// active then, through the ordinary fan-out. Same SAFETY contract as
/// [`install`].
pub(crate) fn install_delivering(cell: &FactReadSetCell) {
    push_tracer(cell, true);
}

fn push_tracer(cell: &FactReadSetCell, delivers: bool) {
    ACTIVE_TRACERS.with(|slot| {
        slot.borrow_mut().push(ActiveTracer {
            cell: cell as *const FactReadSetCell,
            delivers,
        });
    });
}

/// Pop the top-of-stack entry. Called on the installer's `Drop`.
pub(crate) fn clear() {
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
pub fn current_tracer<'a>() -> Option<&'a FactReadSetCell> {
    ACTIVE_TRACERS.with(|slot| {
        let stack = slot.borrow();
        let ptr = stack.last().map(|tracer| tracer.cell);
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

/// Fan an observed fact into every active tracer that must observe it (see
/// the module docs: down to the first delivering scope).
///
/// Snapshot-then-iterate: collect the pointer set under a borrow,
/// drop the borrow, then iterate the collected set. No borrow is held
/// during the `observe` calls, so re-entrant `install`/`clear` calls
/// from inside a tracer are safe.
#[inline]
pub(crate) fn observe_fan_out(fact: FactVersionRef) {
    verter_audit::attribute_n!(FactObserve, 1);
    for recorder in active_recorders() {
        // SAFETY: see `ACTIVE_RECORDERS`.
        let mut facts = unsafe { &*recorder }.facts.borrow_mut();
        if facts.last() != Some(&fact) {
            facts.push(fact.clone());
        }
    }
    // Collect pointers under a short borrow, then drop the borrow
    // before calling into FactReadSetCell so re-entrant installs
    // from inside an observer don't cause RefCell panics.
    let ptrs = observing_cells();
    // Move the owned fact into the LAST live tracer; an earlier one copies
    // it only when its recent window does not already hold it. With the
    // common single-tracer stack this fans out ZERO fact clones (a
    // `FactVersionRef` carries an owned canonical `String`, so a clone is a
    // real allocation on every traced read).
    let mut live = ptrs.into_iter().filter(|ptr| !ptr.is_null()).peekable();
    while let Some(ptr) = live.next() {
        // SAFETY: see module-level SAFETY contract.
        let cell = unsafe { &*ptr };
        if live.peek().is_some() {
            cell.observe_ref(&fact);
        } else {
            cell.observe(fact);
            return;
        }
    }
}

/// Fan a borrowed signature into every active tracer that must observe it.
#[inline]
pub(crate) fn observe_fan_out_borrowed(sig: &[FactVersionRef]) {
    observe_signature_consumers(sig, |cell| cell.observe_borrowed_signature(sig));
}

/// Replay a completed compute's canonical witness to every active consumer.
/// The read set verifies its order and falls back for an unordered witness.
#[inline]
pub(crate) fn observe_fan_out_canonical(sig: &std::sync::Arc<[FactVersionRef]>) {
    // The witness's order is verified once for every consumer, by the
    // first that does not already hold it.
    let verified = std::cell::OnceCell::new();
    observe_signature_consumers(sig, |cell| {
        cell.absorb_canonical_signature_verified_once(sig, &verified);
    });
}

#[inline]
fn observe_signature_consumers(sig: &[FactVersionRef], observe: impl Fn(&FactReadSetCell)) {
    if sig.is_empty() {
        return;
    }
    verter_audit::attribute_n!(FactObserve, sig.len());
    for recorder in active_recorders() {
        // SAFETY: see `ACTIVE_RECORDERS`.
        unsafe { &*recorder }
            .facts
            .borrow_mut()
            .extend(sig.iter().cloned());
    }
    let ptrs = observing_cells();
    for ptr in ptrs {
        if !ptr.is_null() {
            // SAFETY: see module-level SAFETY contract.
            observe(unsafe { &*ptr });
        }
    }
}

/// Mark **every** active tracer on the stack as having consumed a
/// NON-CACHEABLE read (a fenced serve, a broken decl-body lease, an
/// unrootable route, or an unobservable source-env identity).
///
/// Reached only through the fan-out in [`super`]
/// (`note_non_cacheable_read_fan_out` / `note_non_cacheable_propagation`),
/// which the non-cacheability marking chokepoints (the `IndexedReady` serve
/// chokepoint, the overlay materialiser, the frontier route reader's
/// per-walk memo, and the engine consumers of source-side `LeaseMiss`
/// evidence) call on the consuming thread, so every enclosing
/// traced cold compute — the semantic-memo build, the
/// owner-import-surface producer, the component-meta proof producers —
/// observes the non-cacheable consumption by value and can refuse
/// shared-cache admission. Same snapshot-then-iterate reentrancy
/// discipline as [`observe_fan_out`].
#[inline]
pub(super) fn apply_non_cacheable_propagation(propagation: NonCacheablePropagation) {
    apply_non_cacheable_mark(propagation, NonCacheableBasis::Untrusted);
}

/// [`apply_non_cacheable_propagation`], stating what the mark says about
/// the value it taints (see [`NonCacheableBasis`]).
#[inline]
pub(super) fn apply_non_cacheable_mark(
    propagation: NonCacheablePropagation,
    basis: NonCacheableBasis,
) {
    // A recorder notes EVERY mark, local-only included: a recording that
    // saw one is never replayed, so the conservative read costs only reuse.
    for recorder in active_recorders() {
        // SAFETY: see `ACTIVE_RECORDERS`.
        unsafe { &*recorder }.non_cacheable.set(true);
    }
    let ptrs = active_cells();
    let first = match propagation {
        NonCacheablePropagation::LocalOnly => ptrs.len().saturating_sub(1),
        NonCacheablePropagation::Transitive => 0,
    };
    for ptr in ptrs.into_iter().skip(first) {
        if !ptr.is_null() {
            // SAFETY: see module-level SAFETY contract.
            unsafe { &*ptr }.note_non_cacheable_read_with_basis(propagation, basis);
        }
    }
}

#[cfg(test)]
mod propagation_tests {
    use super::*;
    use verter_session_query::facts::fact_read_set::NonCacheablePropagation;

    #[test]
    fn local_only_refusal_marks_only_the_owning_tracer() {
        let outer = FactReadSetCell::new();
        let inner = FactReadSetCell::new();
        install(&outer);
        install(&inner);

        apply_non_cacheable_propagation(NonCacheablePropagation::LocalOnly);

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

        apply_non_cacheable_propagation(NonCacheablePropagation::Transitive);

        clear();
        clear();
        assert!(outer.non_cacheable_read_observed());
        assert!(inner.non_cacheable_read_observed());
    }
}

#[cfg(test)]
mod recorder_tests {
    use super::*;

    /// A recorder sees what the tracers see, but is never one: the tracer
    /// stays the top scope and still owns a local-only mark.
    #[test]
    fn a_recorder_witnesses_fan_out_without_becoming_a_tracer() {
        let tracer = FactReadSetCell::new();
        install(&tracer);
        let recorder = FactReadRecorder::default();
        {
            let _scope = install_recorder(&recorder);
            assert!(std::ptr::eq(
                current_tracer().expect("the tracer stays installed"),
                &tracer
            ));
            let fact = FactVersionRef::FileWholeHash {
                canonical_id: "/rec.ts".into(),
                hash: [3u8; 16],
            };
            observe_fan_out(fact.clone());
            observe_fan_out_borrowed(&[fact]);
            apply_non_cacheable_propagation(NonCacheablePropagation::LocalOnly);
        }
        // Observed after the scope closed: not recorded.
        observe_fan_out(FactVersionRef::FileWholeHash {
            canonical_id: "/after.ts".into(),
            hash: [4u8; 16],
        });
        clear();
        assert!(
            tracer.non_cacheable_read_observed(),
            "the tracer still owns the local-only mark"
        );
        let (facts, non_cacheable) = recorder.into_parts();
        assert_eq!(facts.len(), 1, "one distinct fact, recorded once");
        assert!(non_cacheable);
    }
}

#[cfg(test)]
mod fan_out_tests {
    use super::*;

    #[test]
    fn canonical_fan_out_preserves_every_consumer_without_resorting() {
        use verter_session_query::facts::fact_read_set::{
            seal_canonical_signature, signature_work_for_tests, FactReadSetFinalise,
        };
        let signature = seal_canonical_signature(
            (0..16)
                .map(|i| FactVersionRef::FileWholeHash {
                    canonical_id: format!("/fact-{i}.ts").into(),
                    hash: [i; 16],
                })
                .collect(),
        );
        let recorder = FactReadRecorder::default();
        let before = signature_work_for_tests();
        let mut repeat_work = 0;
        let (inner, outer) = crate::resolver_core::resolver_context::with_fact_tracer_cell(
            Default::default(),
            |_| {
                crate::resolver_core::resolver_context::with_fact_tracer_cell(Default::default(), |_| {
                    let _scope = install_recorder(&recorder);
                    observe_fan_out_canonical(&signature);
                    let first = signature_work_for_tests();
                    for _ in 0..3 {
                        observe_fan_out_canonical(&signature);
                    }
                    repeat_work = signature_work_for_tests() - first;
                    apply_non_cacheable_propagation(
                        verter_session_query::facts::fact_read_set::NonCacheablePropagation::Transitive,
                    );
                })
                .1
            },
        );
        let inner = inner.finalise();
        let outer = outer.finalise();
        let work = signature_work_for_tests() - before;
        println!("canonical_replay_work={work}");
        for result in [inner, outer] {
            let FactReadSetFinalise::NonCacheable(facts) = result else {
                panic!("canonical replay must retain refusal: {result:?}");
            };
            assert_eq!(facts, signature);
        }
        let (facts, non_cacheable) = recorder.into_parts();
        assert_eq!(facts.as_slice(), signature.as_ref());
        assert!(non_cacheable);
        assert!(
            work <= 16 * signature.len(),
            "canonical replay proof_work={work}"
        );
        // A consumer already holding the witness recognises it before any
        // order verification: a repeated replay costs no work proportional
        // to the witness's width.
        assert!(
            repeat_work < signature.len(),
            "repeated canonical replay proof_work={repeat_work}"
        );
    }

    /// One owned fan-out must reach EVERY active tracer on the stack —
    /// pins the fan-out against a last-only / first-only regression
    /// (the owned value is moved into the LAST tracer; the earlier
    /// ones observe clones).
    #[test]
    fn owned_fan_out_reaches_every_active_tracer() {
        let outer = FactReadSetCell::new();
        let inner = FactReadSetCell::new();
        install(&outer);
        install(&inner);

        observe_fan_out(FactVersionRef::FileWholeHash {
            canonical_id: "/fan.ts".into(),
            hash: [7u8; 16],
        });

        clear();
        clear();
        assert_eq!(outer.len(), 1, "outer tracer must observe the fact");
        assert_eq!(inner.len(), 1, "inner tracer must observe the fact");
    }

    /// A delivering scope keeps what is observed inside it from the scopes
    /// below it, which receive it once when its owner delivers the
    /// completed set; the scopes above it, a recorder and a transitive
    /// refusal still see everything at once.
    #[test]
    fn a_delivering_scope_stops_the_fan_out_below_it() {
        let fact = |canonical: &str| FactVersionRef::FileWholeHash {
            canonical_id: canonical.into(),
            hash: [3u8; 16],
        };
        let outer = FactReadSetCell::new();
        let delivering = FactReadSetCell::new();
        let inner = FactReadSetCell::new();
        let recorder = FactReadRecorder::default();
        install(&outer);
        install_delivering(&delivering);
        install(&inner);
        let scope = install_recorder(&recorder);
        observe_fan_out(fact("/inner.ts"));
        observe_fan_out_borrowed(&[fact("/borrowed.ts")]);
        apply_non_cacheable_propagation(
            verter_session_query::facts::fact_read_set::NonCacheablePropagation::Transitive,
        );
        drop(scope);
        clear();
        assert_eq!((inner.len(), delivering.len(), outer.len()), (2, 2, 0));
        assert!(outer.non_cacheable_read_observed());
        // The delivering scope completes: its owner hands its set down.
        clear();
        let delivered: std::sync::Arc<[FactVersionRef]> =
            std::sync::Arc::from([fact("/borrowed.ts"), fact("/inner.ts")]);
        observe_fan_out_canonical(&delivered);
        clear();
        assert_eq!(outer.len(), 2, "the delivered set reaches the scope below");
        let (recorded, non_cacheable) = recorder.into_parts();
        assert_eq!(recorded.len(), 2);
        assert!(non_cacheable);
    }

    /// A fan-out with NO active tracer is a no-op (no panic, nothing
    /// recorded anywhere) — the moved-value fast path must not assume
    /// a non-empty stack.
    #[test]
    fn owned_fan_out_without_tracers_is_a_noop() {
        observe_fan_out(FactVersionRef::FileWholeHash {
            canonical_id: "/none.ts".into(),
            hash: [9u8; 16],
        });
    }
}
