//! Per-request thread-local accumulators + test-only BFS counters.
//!
//! Phase 11a domain 6 — Step 6.6.A dep-signature accumulator + 3 accessors,
//! plus `#[cfg(test)]` BFS counters for the cycle-walk regression suite.
//!
//! The counters are declared `pub(crate)` so test code anywhere in the crate
//! can probe them. Sibling modules access them via `super::dep_signature::*`
//! or through the shell's narrow re-exports.

thread_local! {
    /// Step 6.6.A dep-signature accumulator.
    ///
    /// `materialize_component_meta_type_expr_until_stable_full` populates
    /// this thread-local with each dispatch round-trip's
    /// `DepSignature`; `compute_component_meta_state_inner` reads + clears
    /// it before publish and merges the accumulated facts into
    /// `ResolvedComponentMetaState.fact_versions` (D31). The thread-local
    /// is request-scoped — the compute entry point clears it; if any
    /// recursive materialize call accumulates without a matching read,
    /// the next request's compute clears it before populating fresh
    /// facts.
    ///
    /// **Why thread-local, not host-owned cache:** the accumulator is
    /// transient per-request channel state, not a reusable cache. It
    /// crosses caller boundaries (deep materialize stacks), but the
    /// completion-fence design already uses thread-locals for the same
    /// reason. CLAUDE.md "host-owned cache principle" applies to
    /// reusable semantic caches, not request-scoped instrumentation
    /// accumulators.
    static DISPATCH_DEP_SIGNATURE_ACCUMULATOR: std::cell::RefCell<
        Vec<crate::resolver_core::FactVersionRef>,
    > = const { std::cell::RefCell::new(Vec::new()) };
}

/// Reset the per-request dep-signature accumulator. Called at the
/// entry of `compute_component_meta_state_inner` so each request
/// starts with a clean slate.
pub(crate) fn reset_dispatch_dep_signature_accumulator() {
    DISPATCH_DEP_SIGNATURE_ACCUMULATOR.with(|cell| cell.borrow_mut().clear());
}

/// Drain the per-request dep-signature accumulator. Called at publish
/// time in `compute_component_meta_state_inner` so accumulated facts
/// merge into `ResolvedComponentMetaState.fact_versions` (Step 6.6.A).
pub(crate) fn drain_dispatch_dep_signature_accumulator() -> Vec<crate::resolver_core::FactVersionRef>
{
    DISPATCH_DEP_SIGNATURE_ACCUMULATOR.with(|cell| std::mem::take(&mut *cell.borrow_mut()))
}

/// Convert dispatch's `DepSignature` (canonical-id + DepVersion pairs)
/// into session-layer `FactVersionRef` entries and merge them into the
/// thread-local accumulator. Deduplicates against entries already in
/// the accumulator on the way in (linear scan; the accumulator is
/// short for a typical request).
pub(crate) fn accumulate_dispatch_dep_signature(sig: &crate::semantic_query::DepSignature) {
    use crate::resolver_core::FactVersionRef;
    use crate::semantic_query::DepVersion;

    DISPATCH_DEP_SIGNATURE_ACCUMULATOR.with(|cell| {
        let mut accumulator = cell.borrow_mut();
        for (canonical, version) in sig.iter() {
            let canonical_id = canonical.as_ref().to_string();
            let fact = match version {
                DepVersion::WholeHash(hash) => FactVersionRef::FileWholeHash {
                    canonical_id,
                    hash: *hash,
                },
                DepVersion::RouteGeneration(_) | DepVersion::ProjectGeneration(_) => {
                    // Route / project generation are coarse-grained
                    // counters; they don't map to a content-hash and
                    // would force the warm-cache validator to invalidate
                    // on unrelated activity. Skip them at the merge
                    // point — `HostFenceValidator`'s revalidation
                    // covers the project-generation lifecycle directly.
                    continue;
                }
            };
            if !accumulator.iter().any(|existing| existing == &fact) {
                accumulator.push(fact);
            }
        }
    });
}

// =====================================================================
// Plan §6.2 / A0 — cycle-BFS visit counter for unit tests.
//
// `ref_root_reaches_transitive_cycle_node` increments this counter
// once per body the BFS visits. Tests use `with_visited_counter` to
// reset it, run a BFS, and read back the visit count to assert
// first-visit-wins / depth-fuse / hop-cap properties.
//
// `#[cfg(test)]`-only: zero footprint outside test builds.
// =====================================================================
#[cfg(test)]
thread_local! {
    pub(crate) static BFS_VISITED_COUNTER: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn with_visited_counter<F, R>(f: F) -> (usize, R)
where
    F: FnOnce() -> R,
{
    BFS_VISITED_COUNTER.with(|c| c.set(0));
    let r = f();
    let count = BFS_VISITED_COUNTER.with(|c| c.get());
    (count, r)
}

// =====================================================================
// Plan §6.13 / Commit R — BFS_COMPUTE_COUNTER per-thread counter.
//
// Counts the number of times the cold-path `bfs_compute_inner` body
// runs on the current thread. Tests use this to verify that
// warm-path generation-local fast hits skip dispatch entirely
// (counter stays at 0 on second call within the same generation).
//
// Per-thread (RefCell-backed) so concurrent tests in the workspace
// pool do not interfere with each other's counters. Tests that
// exercise multi-thread cooperative-admission must observe the
// winner via the host-owned cache's `live_counter_for_test()`
// instead.
//
// `#[cfg(test)]`-only: zero footprint outside test builds.
// =====================================================================
#[cfg(test)]
thread_local! {
    pub(crate) static BFS_COMPUTE_COUNTER: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_bfs_compute_counter_for_test() {
    BFS_COMPUTE_COUNTER.with(|c| c.set(0));
}

#[cfg(test)]
pub(crate) fn bfs_compute_counter_for_test() -> usize {
    BFS_COMPUTE_COUNTER.with(|c| c.get())
}

// =====================================================================
// Plan §6.2 / §6.6.5 — F-prep canonical-fixture A0 test #3b helper.
//
// `with_bfs_child_refs_observer_for_test(target_name, f)` instruments
// `ref_root_reaches_transitive_cycle_node`'s child-ref collection step
// to record `child_refs.len()` per visited identity name. Returns the
// observed count for the target name (or `None` if the BFS did not
// visit it).
//
// Used by F-prep test #3b to mechanically discriminate the rev-9 BFS
// bug (Navigate → 0 refs at DotPathKeys hop) from the rev-10 fix
// (Skeleton → ≥1 refs at DotPathKeys hop). Without this assertion the
// canonical nuxt-ui fixture's pass/fail outcome could be misattributed
// to other code paths.
//
// `#[cfg(test)]`-only: zero footprint outside test builds.
// =====================================================================
#[cfg(test)]
thread_local! {
    pub(crate) static BFS_CHILD_REFS_OBSERVER: std::cell::RefCell<
        Option<(String, std::collections::HashMap<String, usize>)>,
    > = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn with_bfs_child_refs_observer_for_test<F, R>(target_name: &str, f: F) -> Option<usize>
where
    F: FnOnce() -> R,
{
    BFS_CHILD_REFS_OBSERVER.with(|c| {
        *c.borrow_mut() = Some((target_name.to_string(), std::collections::HashMap::new()));
    });
    let _r = f();
    let observed = BFS_CHILD_REFS_OBSERVER.with(|c| {
        let borrowed = c.borrow();
        borrowed
            .as_ref()
            .and_then(|(target, observations)| observations.get(target).copied())
    });
    BFS_CHILD_REFS_OBSERVER.with(|c| {
        *c.borrow_mut() = None;
    });
    observed
}

/// Test instrumentation: record `count` child refs at the BFS hop for
/// `decl_name`. No-op outside test builds. No-op if observer not active.
#[cfg(test)]
pub(crate) fn record_bfs_child_refs_count_for_test(decl_name: &str, count: usize) {
    BFS_CHILD_REFS_OBSERVER.with(|c| {
        if let Some((_, observations)) = c.borrow_mut().as_mut() {
            observations.insert(decl_name.to_string(), count);
        }
    });
}
