//! Discriminating regression tests for the non-current
//! (`ReturnOnly`) store-view contract at the general
//! [`crate::VerterHost::resolver_store_view`] accessor.
//!
//! The accessor returns the capability-split
//! [`crate::resolver_store::StoreViewRead`]; a warm validator consumes
//! only the `Current` arm and a fenced cold builder consumes a
//! [`crate::resolver_store::ColdSeedHostStoreView`] (which exposes no
//! `validates*` surface). These tests force the store-view manager to
//! hand back a known-stale `ReturnOnly` read — via the existing
//! `arm_supersede_always_for_tests` knob, which bumps the validation
//! epoch on every build attempt so `base_view` exhausts its bounded
//! retry — and assert the contract holds:
//!
//! 1. a typeinfo query-returner (`resolve_named_symbol`) resolves
//!    against a proven-current view or, under sustained churn, MISSES
//!    (returns `None`) rather than resolving against superseded state
//!    and returning a stale node;
//! 2. a cacheable typeinfo evaluation (`evaluate_type_expression`)
//!    under sustained churn returns `None`, does NOT warm the
//!    host-owned scratch cache from a non-current execution, and fully
//!    removes the scratch it upserted before the proven-current
//!    acquisition (the scratch never gained an LRU owner, so leaving it
//!    in the live set would leak host/scheduler state across repeated
//!    misses);
//! 3. a request-bound context built from a cold-seed `ReturnOnly` view
//!    fails its `validates*` family CLOSED, so every nested warm-cache
//!    probe misses;
//! 4. a fenced cold component-meta builder under sustained churn does
//!    NOT publish a shared result-cache entry (its publish fence
//!    rejects the non-current seed), and a later quiescent run does;
//! 5. a SESSION-bound context built from a cold-seed `ReturnOnly` view
//!    (the view-bound component-meta cold compute) fails its
//!    `validates*` family CLOSED — the session cold-seed constructor
//!    threads the seed's currentness into the request-bound view, so a
//!    nested warm-cache probe inside a view-bound cold compute cannot
//!    validate against the stale seed;
//! 6. the fallthrough resolver's per-element / per-child / per-root
//!    node-cache validation view fails CLOSED when the cold compute's
//!    seed is non-current — the resolver validates through the
//!    request-bound `ctx.store_view()` (currentness-gated), not a raw
//!    re-read of the store view that drops the currentness flag.
//!
//! The static-guard half of the contract (part 5) lives in
//! `tests/architecture_guards.rs`
//! (`resolver_store_view_returns_store_view_read`,
//! `cold_seed_store_view_exposes_no_validation_surface`,
//! `warm_validation_entry_points_require_current_store_view`,
//! `resolver_store_view_into_owned_view_is_allowlisted`).

use std::sync::Arc;
use std::time::Duration;

use crate::resolver_store::HostStoreView;
use crate::types::{FileKind, UpsertRequest};
use crate::{HostConfig, VerterHost};

/// Build a standalone host with a single resolvable TS declaration.
fn host_with_decl() -> (Arc<VerterHost>, String) {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let canonical = "/proj/decl.ts".to_string();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: canonical.clone(),
            source: Arc::from("export interface Widget { label: string }\n"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .expect("upsert decl.ts must succeed");
    (host, canonical)
}

/// Run `f` on a watchdog thread and assert it returns within `secs`.
///
/// The non-current store-view path is bounded (the manager's
/// `base_view` retry cap plus the typeinfo current-view retry cap), so
/// a regression that re-loops a never-coherent build forever would hang
/// here. The watchdog turns that hang into a deterministic failure
/// instead of stalling the suite.
fn run_with_watchdog<T, F>(secs: u64, f: F) -> T
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel::<T>();
    let handle = std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    match rx.recv_timeout(Duration::from_secs(secs)) {
        Ok(value) => {
            handle.join().expect("watchdog thread must not panic");
            value
        }
        Err(_) => panic!(
            "REGRESSION: the non-current store-view path did not return within {secs}s — \
             a bounded retry is missing and the path spins under sustained churn"
        ),
    }
}

#[test]
fn resolve_named_symbol_misses_under_sustained_non_current_view() {
    // Control: a quiescent host resolves the declaration to a node.
    let (host, canonical) = host_with_decl();
    let quiescent = host.resolve_named_symbol(&canonical, "Widget", &[], None);
    assert!(
        quiescent.is_some(),
        "control: a quiescent host must resolve `Widget` to a semantic node"
    );

    // Force sustained validation-token churn so every base-view read on
    // this thread is a known-stale `ReturnOnly`. The typeinfo
    // query-returner builds its dispatch context from
    // `resolver_store_view`; pre-contract it resolved against the stale
    // view and returned a node anyway, post-contract it acquires a
    // proven-current view via a bounded retry and, finding none, MISSES.
    let host_for_query = Arc::clone(&host);
    let canonical_for_query = canonical.clone();
    let resolved_under_churn = run_with_watchdog(10, move || {
        HostStoreView::arm_supersede_always_for_tests();
        // Invalidate any manager-cached `Current` entry the control
        // resolution warmed so the query's view read must REBUILD — the
        // sustained churn knob then supersedes every rebuild, deterministically
        // forcing the bounded-retry-then-miss path.
        host_for_query.bump_store_view_epoch();
        let resolved =
            host_for_query.resolve_named_symbol(&canonical_for_query, "Widget", &[], None);
        HostStoreView::disarm_supersede_always_for_tests();
        resolved
    });
    assert!(
        resolved_under_churn.is_none(),
        "under sustained non-current churn `resolve_named_symbol` MUST miss (return \
         None) rather than resolve `Widget` against a known-stale snapshot and return \
         a node — the query-returner must resolve against a proven-current view"
    );

    // After the churn is disarmed a fresh resolution succeeds again — the
    // miss was the transient non-current outcome, not a permanent break.
    let recovered = host.resolve_named_symbol(&canonical, "Widget", &[], None);
    assert!(
        recovered.is_some(),
        "a quiescent resolution after the churn is disarmed must resolve `Widget` again"
    );
}

#[test]
fn evaluate_type_expression_does_not_cache_from_non_current_view() {
    use crate::typeinfo::types::EvaluateTypeExpressionRequest;
    use crate::types::ProjectionMode;

    let (host, canonical) = host_with_decl();

    // The scratch cache starts empty.
    assert_eq!(
        host.scratch_cache().lock().len(),
        0,
        "control: the scratch cache must start empty"
    );

    let req = EvaluateTypeExpressionRequest {
        scope: canonical.clone(),
        expression: "Widget['label']".to_string(),
        extra_imports: Vec::new(),
        mode: ProjectionMode::Expanded,
        cacheable: true,
    };

    // Under sustained churn the cacheable evaluation must (a) miss and
    // (b) NOT warm the host-owned scratch cache from a non-current
    // execution. Pre-contract `evaluate_inner` resolved against the stale
    // view and inserted the result into `scratch_cache`.
    let host_for_eval = Arc::clone(&host);
    let req_for_eval = req.clone();
    let audited = run_with_watchdog(10, move || {
        HostStoreView::arm_supersede_always_for_tests();
        let result = host_for_eval.evaluate_type_expression_with_audit(req_for_eval);
        HostStoreView::disarm_supersede_always_for_tests();
        result
    });
    // A non-current execution is a non-fault miss (`Ok(None)`); a fault
    // also yields no resolution. Both collapse to "no node".
    let resolved = audited.into_result().ok().flatten();
    assert!(
        resolved.is_none(),
        "under sustained non-current churn `evaluate_type_expression` MUST miss rather \
         than evaluate against a known-stale snapshot"
    );
    assert_eq!(
        host.scratch_cache().lock().len(),
        0,
        "a non-current evaluation MUST NOT warm the scratch cache — the cached node \
         would be a result computed against an already-superseded snapshot"
    );

    // Control: a quiescent cacheable evaluation DOES resolve and warm the
    // scratch cache, proving the cache path is otherwise live.
    let quiescent_resolved = host
        .evaluate_type_expression_with_audit(req)
        .into_result()
        .ok()
        .flatten();
    assert!(
        quiescent_resolved.is_some(),
        "control: a quiescent cacheable evaluation must resolve the projection"
    );
    assert_eq!(
        host.scratch_cache().lock().len(),
        1,
        "control: a quiescent cacheable evaluation must warm the scratch cache"
    );
}

#[test]
fn evaluate_type_expression_removes_orphan_scratch_on_non_current_miss() {
    use crate::typeinfo::evaluate_type_expression::compute_scratch_uri;
    use crate::typeinfo::types::EvaluateTypeExpressionRequest;
    use crate::types::ProjectionMode;

    let (host, canonical) = host_with_decl();

    // A CACHEABLE evaluation upserts a scratch file BEFORE it acquires the
    // proven-current view it resolves against. The scratch becomes a live
    // host source the instant the upsert lands; it only gains an LRU owner
    // (an entry in the host-owned scratch cache) once resolution SUCCEEDS.
    // If the proven-current acquisition then exhausts under sustained churn,
    // the call returns a miss having upserted the scratch but never inserted
    // it into the scratch cache — so on this path the scratch has no LRU
    // owner and MUST be removed unconditionally, regardless of `cacheable`.
    let req = EvaluateTypeExpressionRequest {
        scope: canonical.clone(),
        expression: "Widget['label']".to_string(),
        extra_imports: Vec::new(),
        mode: ProjectionMode::Expanded,
        cacheable: true,
    };
    let scratch_uri = compute_scratch_uri(&req.scope, &req.expression, &req.extra_imports);

    // The scratch URI is not a live host source before the call.
    assert!(
        host.scheduler.try_get_source(&scratch_uri).is_none(),
        "control: the scratch URI must not be a live host source before the evaluation"
    );

    // Drive enough cacheable misses under sustained churn that an
    // unconditionally-leaked orphan would accumulate. Each call upserts the
    // SAME scratch URI (the request triple is identical), so a leak shows up
    // as the scratch lingering as a live, non-evicted host source after the
    // misses — not as a growing count, since the URI is content-addressed.
    let host_for_eval = Arc::clone(&host);
    let req_for_eval = req.clone();
    let resolved = run_with_watchdog(15, move || {
        HostStoreView::arm_supersede_always_for_tests();
        let mut last = None;
        for _ in 0..4 {
            host_for_eval.bump_store_view_epoch();
            let node = host_for_eval
                .evaluate_type_expression_with_audit(req_for_eval.clone())
                .into_result()
                .ok()
                .flatten();
            last = Some(node);
        }
        HostStoreView::disarm_supersede_always_for_tests();
        last.expect("the loop runs at least once")
    });
    assert!(
        resolved.is_none(),
        "under sustained non-current churn the cacheable evaluation MUST miss"
    );

    // The scratch cache stays empty — a non-current execution never warms it,
    // so the orphan scratch has no LRU owner.
    assert_eq!(
        host.scratch_cache().lock().len(),
        0,
        "a non-current evaluation must not warm the scratch cache, so the orphan \
         scratch has no LRU owner and would never be reclaimed by an eviction sweep"
    );

    // The orphan MUST have been removed on the non-current miss path. The
    // host treats a canonical as a live file when it is BOTH non-evicted AND
    // scheduler-tracked (`VerterHost::ensure_loaded`'s liveness gate). The
    // unconditional `host.remove` drops the scratch's scheduler node, so the
    // canonical is no longer a live host source. A cleanup that skipped
    // removal for a cacheable request would be a no-op on this path (the
    // scratch never entered the cache), leaving it non-evicted AND
    // scheduler-tracked — a leaked live host source.
    let evicted_flag = host
        .derived_raw_cache()
        .get(&scratch_uri)
        .map(|d| d.evicted)
        .unwrap_or(false);
    let still_live = !evicted_flag && host.scheduler.try_get_source(&scratch_uri).is_some();
    assert!(
        !still_live,
        "the orphan scratch upserted before the proven-current acquisition MUST be \
         evicted on the non-current miss path — it has no LRU owner (the scratch cache \
         stays empty on a non-current execution), so leaving it non-evicted and \
         scheduler-tracked accumulates host/scheduler state across repeated failed \
         expressions"
    );

    // After the churn is disarmed a fresh cacheable evaluation resolves and
    // warms the cache, proving the eviction did not break the live path.
    let recovered = host
        .evaluate_type_expression_with_audit(req)
        .into_result()
        .ok()
        .flatten();
    assert!(
        recovered.is_some(),
        "a quiescent cacheable evaluation after the churn is disarmed must resolve and \
         re-establish the scratch"
    );
    assert_eq!(
        host.scratch_cache().lock().len(),
        1,
        "the recovered quiescent evaluation must warm the scratch cache (the orphan \
         eviction left the live path intact)"
    );
}

#[test]
fn cold_seed_context_fails_warm_probes_closed() {
    use crate::resolver_core::{CanonicalCompletionOverlay, HostResolverContext};

    let (host, canonical) = host_with_decl();

    // A self-root whole-hash fact for the live file content. Against a
    // proven-current view it validates; against a cold-seed `ReturnOnly`
    // view it must fail CLOSED.
    let whole_hash = host
        .shallow_file_state(&canonical)
        .expect("decl.ts must have shallow state")
        .whole_hash;
    let fact = crate::resolver_core::FactVersionRef::FileWholeHash {
        canonical_id: canonical.clone(),
        hash: whole_hash,
    };

    // Control: a context built from a proven-CURRENT view validates the
    // live self-root fact.
    {
        let current = host
            .resolver_store_view_read()
            .current()
            .expect("a quiescent host must yield a Current read");
        let overlay = Arc::new(CanonicalCompletionOverlay::new());
        let ctx = HostResolverContext::from_current(&host, &current, overlay);
        use crate::resolver_core::resolver_context::ResolverContext;
        assert!(
            ctx.store_view().validates(&fact),
            "control: a current-rooted context must validate the live self-root fact"
        );
    }

    // A cold-seed context whose seed is non-current must fail the
    // validation CLOSED — every nested warm-cache probe through the
    // request-bound view misses, so no warm entry can validate against
    // the stale seed.
    let host_for_seed = Arc::clone(&host);
    let fact_for_seed = fact.clone();
    let validated_under_cold_seed = run_with_watchdog(10, move || {
        HostStoreView::arm_supersede_always_for_tests();
        // Invalidate any manager-cached `Current` entry the control block
        // warmed, so this read must REBUILD — and the sustained churn knob
        // supersedes every rebuild, forcing a `ReturnOnly`.
        host_for_seed.bump_store_view_epoch();
        let cold_seed = host_for_seed
            .resolver_store_view_read()
            .into_cold_seed_view();
        // The seed must be classified non-current under sustained churn.
        let seed_is_current = cold_seed.is_current();
        let overlay = Arc::new(CanonicalCompletionOverlay::new());
        let ctx = HostResolverContext::from_cold_seed(&host_for_seed, &cold_seed, overlay);
        use crate::resolver_core::resolver_context::ResolverContext;
        let validates = ctx.store_view().validates(&fact_for_seed);
        HostStoreView::disarm_supersede_always_for_tests();
        (seed_is_current, validates)
    });
    assert!(
        !validated_under_cold_seed.0,
        "under sustained churn the cold-seed view must be classified non-current"
    );
    assert!(
        !validated_under_cold_seed.1,
        "a cold-seed (`ReturnOnly`) context MUST fail its `validates*` family closed — \
         a nested warm-cache probe that validated the live self-root fact against the \
         stale seed is the leak this contract closes"
    );
}

#[test]
fn fenced_cold_component_meta_builder_does_not_publish_under_churn() {
    use crate::component_meta_result_db::ComponentMetaResultKey;

    // A `.vue` owner whose component-meta is a fenced cold build.
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let owner = "/proj/Comp.vue".to_string();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: owner.clone(),
            source: Arc::from(
                "<script setup lang=\"ts\">\ndefineProps<{ msg: string }>()\n</script>\n<template><div /></template>\n",
            ),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .expect("upsert Comp.vue must succeed");

    let key = ComponentMetaResultKey {
        owner_canonical: Arc::from(owner.as_str()),
        options_fingerprint: crate::host_manage::component_meta_options_fingerprint(
            &crate::host_manage::ComponentMetaOptions::default(),
        ),
    };
    let results = host.project_type_store().component_meta_results();

    // Under sustained churn the fenced cold builder may run, but its
    // publish fence (seed currentness) must reject promotion: no shared
    // result-cache entry is admitted for the owner.
    let host_for_build = Arc::clone(&host);
    let owner_for_build = owner.clone();
    run_with_watchdog(20, move || {
        HostStoreView::arm_supersede_always_for_tests();
        let _ = host_for_build.get_component_meta(&owner_for_build);
        HostStoreView::disarm_supersede_always_for_tests();
    });
    let whole_hash = host
        .shallow_file_state(&owner)
        .expect("Comp.vue must have shallow state")
        .whole_hash;
    assert!(
        results.get(&key, whole_hash).is_none(),
        "a fenced cold component-meta build under sustained non-current churn MUST NOT \
         publish a shared result-cache entry — its publish fence rejects the \
         non-current seed"
    );

    // A quiescent build publishes the entry, proving the cache path is
    // otherwise live (the churn-time non-publish was the fence acting, not
    // a broken cache).
    let _ = host.get_component_meta(&owner);
    let whole_hash_after = host
        .shallow_file_state(&owner)
        .expect("Comp.vue must have shallow state")
        .whole_hash;
    assert!(
        results.get(&key, whole_hash_after).is_some(),
        "a quiescent fenced cold build must publish a shared result-cache entry"
    );
}

/// Minimal base-only session view: no overlay canonicals, no tombstones.
/// Sufficient to construct a [`crate::resolver_core::SessionResolverContext`]
/// and exercise the cold-seed overlay re-rooting path (both iteration sets
/// are empty, so the snapshot copies through unchanged).
struct EmptyBaseSessionView {
    project_identity: crate::file_artifact_store::ProjectIdentity,
    env_hashes: crate::session_view::EnvHashes,
}

impl crate::session_view::SessionView for EmptyBaseSessionView {
    fn source(&self, _canonical: &str) -> Option<Arc<str>> {
        None
    }
    fn content_hash_for(&self, _canonical: &str) -> Option<crate::types::Hash16> {
        None
    }
    fn project_identity(&self) -> crate::file_artifact_store::ProjectIdentity {
        self.project_identity
    }
    fn env_hashes(&self) -> &crate::session_view::EnvHashes {
        &self.env_hashes
    }
    fn resolved_import_facts(
        &self,
        _canonical: &str,
    ) -> Option<Arc<crate::resolved_import_facts::ResolvedImportFacts>> {
        None
    }
}

#[test]
fn session_cold_seed_context_fails_warm_probes_closed() {
    use crate::resolver_core::{CanonicalCompletionOverlay, SessionResolverContext};

    let (host, canonical) = host_with_decl();

    // A self-root whole-hash fact for the live file content. Against a
    // proven-current session view it validates; against a session
    // cold-seed `ReturnOnly` view it must fail CLOSED.
    let whole_hash = host
        .shallow_file_state(&canonical)
        .expect("decl.ts must have shallow state")
        .whole_hash;
    let fact = crate::resolver_core::FactVersionRef::FileWholeHash {
        canonical_id: canonical.clone(),
        hash: whole_hash,
    };
    let session_view = EmptyBaseSessionView {
        project_identity: host.host_view_project_identity(),
        env_hashes: crate::session_view::EnvHashes::default(),
    };

    // Control: a SESSION context built from a proven-CURRENT cold seed
    // validates the live self-root fact (the seed is coherent, so the
    // view-bound cold compute's nested probes may validate).
    {
        let current_seed = host.resolver_store_view_read().into_cold_seed_view();
        assert!(
            current_seed.is_current(),
            "control: a quiescent host must yield a Current cold-seed"
        );
        let overlay = Arc::new(CanonicalCompletionOverlay::new());
        let ctx =
            SessionResolverContext::from_cold_seed(&host, &session_view, &current_seed, overlay);
        use crate::resolver_core::resolver_context::ResolverContext;
        assert!(
            ctx.store_view().validates(&fact),
            "control: a current-rooted session context must validate the live self-root fact"
        );
    }

    // A session cold-seed context whose seed is non-current must fail the
    // validation CLOSED — the view-bound component-meta cold compute
    // (`compute_component_meta_state_with_view`) builds exactly this
    // context, so a nested warm-cache probe inside it cannot validate
    // against the stale seed.
    let host_for_seed = Arc::clone(&host);
    let fact_for_seed = fact.clone();
    let validated_under_cold_seed = run_with_watchdog(10, move || {
        HostStoreView::arm_supersede_always_for_tests();
        // Invalidate any manager-cached `Current` entry the control block
        // warmed so this read must REBUILD; the sustained churn knob then
        // supersedes every rebuild, forcing a `ReturnOnly`.
        host_for_seed.bump_store_view_epoch();
        let session_view = EmptyBaseSessionView {
            project_identity: host_for_seed.host_view_project_identity(),
            env_hashes: crate::session_view::EnvHashes::default(),
        };
        let cold_seed = host_for_seed
            .resolver_store_view_read()
            .into_cold_seed_view();
        let seed_is_current = cold_seed.is_current();
        let overlay = Arc::new(CanonicalCompletionOverlay::new());
        let ctx = SessionResolverContext::from_cold_seed(
            &host_for_seed,
            &session_view,
            &cold_seed,
            overlay,
        );
        use crate::resolver_core::resolver_context::ResolverContext;
        let validates = ctx.store_view().validates(&fact_for_seed);
        HostStoreView::disarm_supersede_always_for_tests();
        (seed_is_current, validates)
    });
    assert!(
        !validated_under_cold_seed.0,
        "under sustained churn the session cold-seed view must be classified non-current"
    );
    assert!(
        !validated_under_cold_seed.1,
        "a session cold-seed (`ReturnOnly`) context MUST fail its `validates*` family closed — \
         a nested warm-cache probe inside a view-bound component-meta cold compute that \
         validated the live self-root fact against the stale seed is the leak this closes"
    );
}

#[test]
fn view_bound_cold_seed_currentness_comes_from_its_own_read() {
    // The view-bound component-meta cold compute
    // (`compute_component_meta_state_with_view` /
    // `_from_captured_with_view`) builds its overlay-rooted cold-seed from a
    // FRESH base read. The currentness of that seed MUST come from the SAME
    // read as its view — never from an EARLIER snapshot's currentness flag.
    //
    // The closed leak: the earlier shape read a fresh view but paired it with
    // the executor's EARLIER `base_is_current`. Under reset/supersede churn
    // the executor's first snapshot can be `Current` (flag `true`) while the
    // helper's second read falls back to `ReturnOnly` — so a stale view was
    // marked `Current` and the `SessionResolverContext` validated nested
    // warm-cache entries against it.
    use crate::resolver_core::{
        resolver_context::ResolverContext, CanonicalCompletionOverlay, SessionResolverContext,
    };

    let (host, canonical) = host_with_decl();
    let whole_hash = host
        .shallow_file_state(&canonical)
        .expect("decl.ts must have shallow state")
        .whole_hash;
    let fact = crate::resolver_core::FactVersionRef::FileWholeHash {
        canonical_id: canonical.clone(),
        hash: whole_hash,
    };

    // Half A — characterize WHY a foreign currentness flag is unsound. Read
    // #1 is quiescent and proven `Current` (flag `true`). Read #2 is taken
    // under sustained churn and is `ReturnOnly` (a stale view). Pairing read
    // #2's stale VIEW with read #1's `Current` FLAG — the exact divergence
    // the closed bug produced — re-binds a stale view as `Current`, and the
    // session context built from it VALIDATES the live self-root fact. This
    // is the leak; it must not be reachable from a production cold path.
    let host_a = Arc::clone(&host);
    let fact_a = fact.clone();
    let proj_a = host.host_view_project_identity();
    let mismatch_validated = run_with_watchdog(10, move || {
        // Read #1: quiescent → Current → flag `true`.
        let earlier_is_current = host_a.resolver_store_view_read().is_current_for_promotion();
        assert!(
            earlier_is_current,
            "control: read #1 (quiescent) must be proven Current"
        );
        // Read #2: under churn → ReturnOnly (stale view).
        HostStoreView::arm_supersede_always_for_tests();
        host_a.bump_store_view_epoch();
        let second_read = host_a.resolver_store_view_read();
        let second_is_current = second_read.is_current_for_promotion();
        let stale_view = second_read.into_cold_seed_view().into_inner();
        // The MISMATCH: read #2's stale view + read #1's `Current` flag.
        let session_view_a = EmptyBaseSessionView {
            project_identity: proj_a,
            env_hashes: crate::session_view::EnvHashes::default(),
        };
        let mismatched_seed = crate::resolver_store::StoreViewRead::from_executor_snapshot(
            stale_view,
            earlier_is_current,
        )
        .into_cold_seed_view()
        .with_session_overlay(&host_a, &session_view_a);
        let overlay = Arc::new(CanonicalCompletionOverlay::new());
        let ctx = SessionResolverContext::from_cold_seed(
            &host_a,
            &session_view_a,
            &mismatched_seed,
            overlay,
        );
        let validates = ctx.store_view().validates(&fact_a);
        HostStoreView::disarm_supersede_always_for_tests();
        (second_is_current, validates)
    });
    assert!(
        !mismatch_validated.0,
        "read #2 under sustained churn must be classified non-current"
    );
    assert!(
        mismatch_validated.1,
        "characterization: a stale (`ReturnOnly`) view re-bound with an EARLIER read's `Current` \
         flag validates the live self-root fact — the unsound divergence the production cold path \
         must never produce"
    );

    // Half B — the fix. The production view-bound cold-seed builder derives
    // currentness from its OWN fresh read, so under the same churn it is
    // non-current and the session context built from it MISSES. Pre-fix this
    // builder paired its fresh `ReturnOnly` read with the executor's
    // `base_is_current` (`true`) and the context VALIDATED (the leak); the
    // intrinsic-currentness build fails the probe closed.
    let host_b = Arc::clone(&host);
    let fact_b = fact.clone();
    let production_validated = run_with_watchdog(10, move || {
        HostStoreView::arm_supersede_always_for_tests();
        host_b.bump_store_view_epoch();
        let session_view_b = EmptyBaseSessionView {
            project_identity: host_b.host_view_project_identity(),
            env_hashes: crate::session_view::EnvHashes::default(),
        };
        // The EXACT production builder the view-bound cold compute uses.
        let cold_seed = host_b.view_bound_cold_seed(&session_view_b);
        let seed_is_current = cold_seed.is_current();
        let overlay = Arc::new(CanonicalCompletionOverlay::new());
        let ctx =
            SessionResolverContext::from_cold_seed(&host_b, &session_view_b, &cold_seed, overlay);
        let validates = ctx.store_view().validates(&fact_b);
        HostStoreView::disarm_supersede_always_for_tests();
        (seed_is_current, validates)
    });
    assert!(
        !production_validated.0,
        "the production view-bound cold-seed must derive non-currentness from its own fresh read \
         under sustained churn — its currentness is intrinsic to the read, not an earlier flag"
    );
    assert!(
        !production_validated.1,
        "the production view-bound cold compute's nested warm-cache probe MUST miss under a \
         non-current read — currentness intrinsic to the read closes the view+flag divergence \
         that let a stale second read be marked current and validate against it"
    );
}

#[test]
fn fallthrough_cold_compute_node_cache_validation_fails_closed_under_churn() {
    // The fallthrough resolver validates per-element / per-child /
    // per-root fallthrough-node cache entries through its request-bound
    // `ctx.store_view()`. When the cold compute's seed is non-current the
    // ctx must be a cold-seed context whose `validates*` family fails
    // CLOSED, so a stale warm fallthrough-node hit cannot be consumed.
    //
    // This exercises the SAME currentness-carrying context the fallthrough
    // cold compute (`compute_fallthrough_surface_uncached`) builds: a
    // `HostResolverContext::from_cold_seed` rooted on the (non-current)
    // snapshot. Pre-fix the cold compute built `HostResolverContext::new`
    // from a raw `.into_inner()` view (currentness dropped → always
    // validates), so the fallthrough node-cache validation consumed stale
    // warm hits under churn.
    use crate::resolver_core::{CanonicalCompletionOverlay, HostResolverContext};

    let (host, canonical) = host_with_decl();
    let whole_hash = host
        .shallow_file_state(&canonical)
        .expect("decl.ts must have shallow state")
        .whole_hash;
    let fact = crate::resolver_core::FactVersionRef::FileWholeHash {
        canonical_id: canonical.clone(),
        hash: whole_hash,
    };

    let host_for_seed = Arc::clone(&host);
    let fact_for_seed = fact.clone();
    let validated_under_cold_seed = run_with_watchdog(10, move || {
        HostStoreView::arm_supersede_always_for_tests();
        host_for_seed.bump_store_view_epoch();
        let (snapshot_view, is_current) = host_for_seed.resolver_store_view_with_currentness();
        // The fallthrough executor threads exactly this `(view, is_current)`
        // SINGLE-read pair into the cold compute; the cold compute re-binds
        // it through `StoreViewRead::from_executor_snapshot` so currentness
        // stays intrinsic to the seed. A non-current snapshot must yield a
        // context whose node-cache validation view fails closed.
        let cold_seed =
            crate::resolver_store::StoreViewRead::from_executor_snapshot(snapshot_view, is_current)
                .into_cold_seed_view();
        let overlay = Arc::new(CanonicalCompletionOverlay::new());
        let ctx = HostResolverContext::from_cold_seed(&host_for_seed, &cold_seed, overlay);
        use crate::resolver_core::resolver_context::ResolverContext;
        let validates = ctx.store_view().validates(&fact_for_seed);
        HostStoreView::disarm_supersede_always_for_tests();
        (is_current, validates)
    });
    assert!(
        !validated_under_cold_seed.0,
        "under sustained churn the fallthrough cold-compute snapshot must be non-current"
    );
    assert!(
        !validated_under_cold_seed.1,
        "the fallthrough cold-compute context MUST fail its node-cache validation closed under a \
         non-current seed — a stale warm fallthrough-node hit consumed against the superseded \
         snapshot is the leak this closes"
    );
}

/// Two-flag condvar gate: a one-way `wait`/`signal` rendezvous shared
/// across the cleaning and driving threads of the ownership-race test.
struct Gate {
    raised: std::sync::Mutex<bool>,
    cond: std::sync::Condvar,
}

impl Gate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            raised: std::sync::Mutex::new(false),
            cond: std::sync::Condvar::new(),
        })
    }

    /// Raise the gate and wake any waiter.
    fn signal(&self) {
        *self.raised.lock().expect("gate poisoned") = true;
        self.cond.notify_all();
    }

    /// Block until the gate is raised.
    fn wait(&self) {
        let mut raised = self.raised.lock().expect("gate poisoned");
        while !*raised {
            raised = self.cond.wait(raised).expect("gate poisoned");
        }
    }
}

/// Ownership-aware scratch cleanup: a cacheable evaluation that OWNS the
/// scratch (it reached the `scratch_cache` insert) must survive a
/// concurrent same-URI request reaching the cleanup path.
///
/// The scratch URI is content-addressed, so two requests for the same
/// `(scope, expression, extra_imports)` triple synthesise the SAME URI.
/// When one request takes ownership (upsert → `scratch_cache` insert) and
/// the other reaches a cleanup terminal, an unconditional `host.remove`
/// on the cleanup path deletes the owned scratch's host file while the
/// cache still maps the URI → the owner's node. The cache fast-path would
/// then hand back a `SemanticNodeId` for a removed host file.
///
/// This test deterministically pins that interleaving via the
/// `evaluate_type_expression::test_interleave` hook fired at the top of
/// `remove_scratch`:
///
/// 1. the CLEANING thread arms sustained store-view churn (thread-local),
///    so its evaluation upserts the scratch then exhausts the
///    proven-current acquisition and reaches the non-current cleanup —
///    where the hook parks it (after the upsert, before the cleanup
///    decision);
/// 2. the OWNING evaluation then runs quiescently on the driver thread:
///    it re-synthesises the same URI, resolves, and inserts into
///    `scratch_cache` — claiming ownership of a live host file;
/// 3. the cleaning thread is released and runs its (now ownership-aware)
///    cleanup, which must SKIP removal because the URI is cache-owned.
///
/// Discriminating outcome — the cache-owned scratch SURVIVES: the host
/// file the cache entry references is still a live host source, and the
/// fast-path read returns the owner's node id. Against the pre-fix
/// unconditional `remove_scratch` the cleaning thread deletes the owner's
/// host file (the scheduler node + resolver caches), so the "still a live
/// host source" assertion FAILS while the cache keeps the now-dangling
/// entry.
#[test]
fn evaluate_type_expression_cleanup_preserves_concurrently_owned_scratch() {
    use crate::typeinfo::evaluate_type_expression::{compute_scratch_uri, test_interleave};
    use crate::typeinfo::types::EvaluateTypeExpressionRequest;
    use crate::types::ProjectionMode;

    // Use a scope file + expression UNIQUE to this test so the synthesised
    // scratch URI (a content hash of `scope || expression || imports`)
    // cannot collide with any sibling contract test's URI. The
    // cleanup-window hook below is a process-global keyed on this URI; a
    // shared URI would let a sibling test's `remove_scratch` fire this
    // test's rendezvous and contaminate the run under parallel execution.
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let canonical = "/proj/ownership_race_decl.ts".to_string();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: canonical.clone(),
            source: Arc::from(
                "export interface OwnershipRaceWidget { ownershipRaceLabel: string }\n",
            ),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .expect("upsert ownership_race_decl.ts must succeed");
    let req = EvaluateTypeExpressionRequest {
        scope: canonical.clone(),
        expression: "OwnershipRaceWidget['ownershipRaceLabel']".to_string(),
        extra_imports: Vec::new(),
        mode: ProjectionMode::Expanded,
        cacheable: true,
    };
    let scratch_uri = compute_scratch_uri(&req.scope, &req.expression, &req.extra_imports);

    // The scratch URI is not a live host source before the call.
    assert!(
        host.scheduler.try_get_source(&scratch_uri).is_none(),
        "control: the scratch URI must not be a live host source before the evaluation"
    );

    // Rendezvous gates: the cleaning thread raises `parked` once it is
    // inside the cleanup window (scratch upserted, about to decide on
    // removal); the driver raises `release` once the owning evaluation has
    // claimed the URI in `scratch_cache`.
    let parked = Gate::new();
    let release = Gate::new();

    // Install the cleanup-window hook. It blocks EXACTLY the cleaning
    // thread's first cleanup of this URI: gated by URI equality (only the
    // cleaning evaluation removes this URI in this test — the owning
    // evaluation succeeds and never removes it, and the LRU does not evict
    // at len <= 1) and by a once-flag so any later `remove_scratch` does
    // not re-block.
    let hook_uri = scratch_uri.clone();
    let hook_parked = Arc::clone(&parked);
    let hook_release = Arc::clone(&release);
    let fired = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let hook_fired = Arc::clone(&fired);
    test_interleave::install(move |uri: &str| {
        if uri != hook_uri {
            return;
        }
        if hook_fired.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        // Signal that the cleaning thread is parked in the cleanup window,
        // then block until the driver has established ownership.
        hook_parked.signal();
        hook_release.wait();
    });

    // The CLEANING thread: sustained churn (thread-local) forces its
    // evaluation onto the non-current cleanup path, where the hook parks
    // it after the scratch upsert.
    let host_clean = Arc::clone(&host);
    let req_clean = req.clone();
    let cleaner = std::thread::spawn(move || {
        HostStoreView::arm_supersede_always_for_tests();
        host_clean.bump_store_view_epoch();
        let out = host_clean
            .evaluate_type_expression_with_audit(req_clean)
            .into_result()
            .ok()
            .flatten();
        HostStoreView::disarm_supersede_always_for_tests();
        out
    });

    // Wait until the cleaning thread is parked in the cleanup window. A
    // bounded join-watchdog turns a missing rendezvous into a failure
    // rather than a hung suite.
    let parked_for_wait = Arc::clone(&parked);
    run_with_watchdog(15, move || parked_for_wait.wait());

    // The OWNING evaluation runs quiescently on the driver thread: it
    // re-synthesises the same URI, resolves against a current view, and
    // inserts into `scratch_cache` — claiming ownership of a live file.
    let owner_node = host
        .evaluate_type_expression_with_audit(req.clone())
        .into_result()
        .ok()
        .flatten()
        .expect("the owning quiescent evaluation must resolve and own the scratch");
    assert!(
        host.scratch_cache().lock().contains(&scratch_uri),
        "the owning evaluation must have claimed the scratch URI in scratch_cache"
    );

    // Release the cleaning thread; its ownership-aware cleanup must now
    // observe the cache-owned URI and SKIP removal.
    release.signal();
    let cleaner_out =
        run_with_watchdog(15, move || cleaner.join().expect("cleaner must not panic"));
    assert!(
        cleaner_out.is_none(),
        "the cleaning evaluation runs under sustained churn and MUST miss (return None)"
    );
    test_interleave::clear();

    // Discriminating outcome: the cache-owned scratch SURVIVED. The host
    // treats a canonical as a live source when it is BOTH non-evicted AND
    // scheduler-tracked. The pre-fix unconditional `host.remove` on the
    // cleanup path drops the scheduler node (and resolver caches) for the
    // owner's file, so this gate FAILS while `scratch_cache` keeps the now
    // dangling URI → node entry.
    let evicted_flag = host
        .derived_raw_cache()
        .get(&scratch_uri)
        .map(|d| d.evicted)
        .unwrap_or(false);
    let still_live = !evicted_flag && host.scheduler.try_get_source(&scratch_uri).is_some();
    assert!(
        still_live,
        "the concurrently-owned scratch MUST remain a live host source — a cleanup-path \
         removal that ignored ownership would delete the owner's scheduler node and resolver \
         caches, leaving the scratch_cache entry pointing at a removed host file"
    );

    // The cache fast-path returns the owner's node, and it still backs a
    // live file: a fresh cacheable evaluation hits the warm entry and
    // resolves, proving the cached node is not dangling.
    let cached = host.scratch_cache().lock().get(&scratch_uri);
    assert_eq!(
        cached,
        Some(owner_node),
        "the scratch_cache fast-path must still return the owning evaluation's node id"
    );
    let warm = host
        .evaluate_type_expression_with_audit(req)
        .into_result()
        .ok()
        .flatten();
    assert_eq!(
        warm,
        Some(owner_node),
        "a repeat cacheable evaluation must hit the surviving cache entry and resolve the \
         owner's node — a removed host file behind a live cache entry would not"
    );
}
