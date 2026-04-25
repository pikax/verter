//! Step 0 spike instrumentation (test-only).
//!
//! Provides thread-local hooks consumed by `meta_resolve_tests::spike_*`
//! tests in support of the "Architectural Debt Closure Plan" Step 0:
//!
//! 1. **Spike #1 (macro-shell substitution).** Pure black-box dispatch
//!    test — does NOT use this module's hooks.
//!
//! 2. **Spike #2 (engine-local cache classification).** Each of the ten
//!    `ComponentMetaQueryEngine` (b) caches has a single `#[cfg(test)]`
//!    call to [`record_cache_read`] just before its `.get(&key)` site;
//!    the dispatch entry [`shallow_lower_type_expr`](crate::project_semantic_dispatch)
//!    has one matching call to [`record_lower_called`]. Together those
//!    hooks let the spike test classify each cache empirically as
//!    PRE_LOWER (read happens before any dispatch lowering — MIGRATE) or
//!    POST_LOWER (read happens only after dispatch lowering — DELETE
//!    candidate, parity-test gated).
//!
//! Hooks are inert by default. The spike test calls [`enable`] inside
//! a request, runs the workload, and calls [`disable`] at the end so
//! the instrumentation does not bleed across test threads or across
//! tests. No production behaviour is altered: every recording site is
//! `#[cfg(test)]`, and every operation is a thread-local read/write
//! that takes <100ns when the spike is active and is compiled out
//! entirely otherwise.
//!
//! Per the Step 0 contract, this module is a test-only artifact. Once
//! Spike #2's empirical table is captured into the Step 3 disposition
//! commit body and the spike test's assertions are subsumed by Step 3
//! regression tests, this file is deleted along with the gated hook
//! call sites.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

thread_local! {
    /// `true` once any code path has entered
    /// `ProjectSemanticDispatch::shallow_lower_type_expr` on this
    /// thread for the current spike-active window.
    static LOWER_CALLED: Cell<bool> = const { Cell::new(false) };
    /// Read-count per cache name. Names are `'static` string slices so
    /// the map is allocation-light on the hot path.
    static CACHE_READS: RefCell<HashMap<&'static str, usize>> = RefCell::new(HashMap::new());
    /// Names of caches that recorded at least one read while
    /// `LOWER_CALLED` was still `false`. PRE_LOWER caches are the
    /// MIGRATE candidates; caches absent from this set despite having
    /// reads are POST_LOWER (DELETE-candidate, parity-test gated).
    static READ_BEFORE_LOWER: RefCell<HashSet<&'static str>> = RefCell::new(HashSet::new());
    /// When `false`, every record_* call is a single `Cell::get` and
    /// returns immediately. The spike test toggles this around its
    /// instrumented workload.
    static SPIKE_ACTIVE: Cell<bool> = const { Cell::new(false) };
}

/// Activate spike recording for the current thread. Call once at the
/// start of an instrumented workload; pair with [`disable`].
pub(crate) fn enable() {
    SPIKE_ACTIVE.with(|c| c.set(true));
}

/// Deactivate spike recording for the current thread.
pub(crate) fn disable() {
    SPIKE_ACTIVE.with(|c| c.set(false));
}

/// Reset all per-thread state so a fresh spike workload starts from a
/// blank slate. Does NOT toggle `SPIKE_ACTIVE`.
pub(crate) fn reset() {
    LOWER_CALLED.with(|c| c.set(false));
    CACHE_READS.with(|m| m.borrow_mut().clear());
    READ_BEFORE_LOWER.with(|m| m.borrow_mut().clear());
}

/// Reset only the per-request lower marker while keeping aggregate
/// cache-read counts. Spike #2 runs several independent fixture
/// requests under one active recording window; classification is
/// meaningful only relative to each request's first lowering call.
pub(crate) fn reset_lower_marker() {
    LOWER_CALLED.with(|c| c.set(false));
}

/// Hook called from the top of
/// `ProjectSemanticDispatch::shallow_lower_type_expr`. Marks that the
/// dispatch path has entered lowering on this thread; cache reads
/// after this call point are POST_LOWER.
#[inline]
pub(crate) fn record_lower_called() {
    if !SPIKE_ACTIVE.with(|c| c.get()) {
        return;
    }
    LOWER_CALLED.with(|c| c.set(true));
}

/// Hook called immediately before each engine-local (b) cache read.
/// `name` MUST be a `'static` string identifying the cache field
/// (e.g. `"imported_registry_symbols"`).
#[inline]
pub(crate) fn record_cache_read(name: &'static str) {
    if !SPIKE_ACTIVE.with(|c| c.get()) {
        return;
    }
    CACHE_READS.with(|m| {
        *m.borrow_mut().entry(name).or_insert(0) += 1;
    });
    if !LOWER_CALLED.with(|c| c.get()) {
        READ_BEFORE_LOWER.with(|s| {
            s.borrow_mut().insert(name);
        });
    }
}

/// Snapshot of the spike state for assertion / report generation.
pub(crate) fn snapshot() -> SpikeSnapshot {
    SpikeSnapshot {
        lower_called: LOWER_CALLED.with(|c| c.get()),
        reads: CACHE_READS.with(|m| m.borrow().clone()),
        pre_lower_caches: READ_BEFORE_LOWER.with(|s| s.borrow().clone()),
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SpikeSnapshot {
    pub lower_called: bool,
    pub reads: HashMap<&'static str, usize>,
    pub pre_lower_caches: HashSet<&'static str>,
}
