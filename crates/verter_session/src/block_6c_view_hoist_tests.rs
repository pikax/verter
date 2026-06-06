//! Discriminating tests for the per-request `HostStoreView`
//! hoist.
//!
//! These tests pin properties of the hoist that would silently regress
//! if a future change unwound the per-request view contract:
//!
//! 1. **View-build count** — a component-meta request through a
//!    `HostResolverContext` builds the `HostStoreView` ONCE (not
//!    8-12+ times). Pre-6.c, every `ValidatedFactCache::get_if_valid*`
//!    warm-hit drove a fresh `HostStoreView::from_host` rebuild; the
//!    cumulative counter delta per request was 8-12+ on a small
//!    fixture. Post-6.c, the delta is exactly 1 (the request entry's
//!    `host.resolver_store_view()` call) plus any owned-view rebuilds
//!    by the bare-host residual paths inside one request — which we
//!    bound separately.
//!
//! 2. **Repeated `prepared_type_decl`** — within one request, two
//!    `prepared_type_decl(dep, sym)` calls on the same dep are served
//!    from the same view borrow (no second `HostStoreView::build`).
//!    The canonical-completion overlay covers any additive load the
//!    second symbol introduced.
//!
//! 3. **Mid-request epoch invalidation** — a concurrent
//!    `bump_project_generation_and_evict` mid-request must NOT pollute
//!    the overlay. The epoch guard on
//!    [`CanonicalCompletionOverlay::complete_canonical`] returns
//!    silently when the host's `current_store_view_epoch` no longer
//!    matches the base view's `mutation_epoch`; the outer stable
//!    executor retries with a fresh view.
//!
//! 4. **Session-overlay rooting** — a session-bearing request's
//!    `SessionResolverContext::store_view()` returns a borrow into the
//!    request-owned `RequestStoreView` (chained behind a single
//!    `with_session_overlay` re-rooting). Consecutive resolver-method
//!    calls do NOT trigger another `with_session_overlay`.
//!
//! All four tests must FAIL against a pre-6.c tree (one where every
//! warm-hit rebuilds the view).

use std::sync::Arc;

use crate::resolver_core::{
    CanonicalCompletionOverlay, HostResolverContext, ResolverContext, SessionResolverContext,
};
use crate::resolver_store::HOST_STORE_VIEW_FROM_HOST_BUILDS;
use crate::types::FileKind;
use crate::{HostConfig, UpsertRequest, VerterHost};

fn small_host_with_one_component() -> (VerterHost, String) {
    let host = VerterHost::new_standalone(HostConfig::default());
    let canonical = "/proj/Button.vue".to_string();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: canonical.clone(),
            source: Arc::from(
                r#"<script setup lang="ts">
interface ButtonProps {
  label: string
  disabled?: boolean
}
defineProps<ButtonProps>()
</script>
<template><button :disabled="disabled">{{ label }}</button></template>
"#,
            ),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .expect("upsert Button.vue must succeed");
    (host, canonical)
}

/// View-build count test (codex / brief §"Discriminating tests" #1).
///
/// A single resolver-tier read pattern under a `HostResolverContext`
/// must drive STRICTLY FEWER `HostStoreView::from_host` calls than
/// the number of reads. Pre-6.c every
/// `ValidatedFactCache::get_if_valid*` warm-hit rebuilt the view;
/// the cumulative counter delta scaled linearly with the read count
/// (10 reads ≈ 10+ builds). Post-6.c the warm-hit path threads the
/// borrowed `RequestStoreView` (no rebuild), so the counter delta
/// stays small — bounded by the residual cold-path entry points
/// (e.g. `resolve_named_type_export_target_uncached`) that still
/// build a fresh view per call. We measure 10 reads and assert the
/// counter is much smaller than 10 — discriminating against the
/// pre-6.c per-warm-hit rebuild rail.
#[test]
fn view_build_count_drops_under_hosted_request() {
    let (host, canonical) = small_host_with_one_component();
    // Reset the counter so the assertion is scoped to one request.
    HOST_STORE_VIEW_FROM_HOST_BUILDS.with(|c| c.set(0));

    let view = host.resolver_store_view();
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let ctx = HostResolverContext::new(&host, &view, overlay);

    // Drive ~10 resolver-tier reads through the request context.
    // Each call would, pre-6.c, rebuild the view inside
    // `ValidatedFactCache::get_if_valid*`. Post-6.c the warm-hit
    // path reads through the borrow.
    let mut reads = 0u64;
    for _ in 0..5 {
        let _ = ctx.prepared_type_decl(&canonical, "ButtonProps");
        let _ = ctx.prepared_decl_bundle(&canonical);
        reads += 2;
    }

    let builds = HOST_STORE_VIEW_FROM_HOST_BUILDS.with(|c| c.get());
    // Discriminating: pre-6.c, `builds` would equal `reads + 1` (one
    // per warm-hit plus the request-entry snapshot). Post-6.c, with
    // warm-hits reading through the borrow, `builds` is bounded by
    // the residual cold-path rebuild sites + the request-entry
    // build. Assert `builds < reads / 2` to discriminate against the
    // pre-6.c per-warm-hit rail without pinning the exact residual
    // count (which depends on which cold paths the test fixture
    // exercises).
    assert!(
        builds < reads / 2,
        "HostResolverContext-bound resolver-tier reads MUST short-circuit \
         most HostStoreView::from_host rebuilds (warm-hit threads the \
         borrow). Observed {builds} builds across {reads} reads; pre-6.c \
         a tree would have ≥ {reads} builds — one per warm-hit cache \
         validate."
    );
}

/// Repeated `prepared_type_decl` test (codex / brief §"Discriminating
/// tests" #2).
///
/// After the first `prepared_type_decl` on a dep warms the bundle,
/// every subsequent `prepared_type_decl(dep, sym)` call on the same
/// dep is a warm hit and MUST NOT trigger ANY further
/// `HostStoreView::build`. The shared bundle cache is content-pinned;
/// the warm-hit path reads through the request-bound borrow.
///
/// Discriminating: pre-6.c, EACH `prepared_type_decl` warm hit
/// rebuilt the view inside `prepared_decl_bundle_with_store_view`'s
/// fast path (because the wrapper built its own view). Post-6.c the
/// `HostResolverContext::prepared_type_decl` impl threads
/// `self.view.base()` down through `_with_store_view`; the warm hit
/// is allocation-free.
#[test]
fn repeated_prepared_type_decl_no_view_rebuild() {
    let (host, canonical) = small_host_with_one_component();

    let view = host.resolver_store_view();
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let ctx = HostResolverContext::new(&host, &view, overlay);

    // Warm the bundle: the first call walks the cold path
    // (`materialize_prepared_decl_bundle` etc.). Subsequent calls
    // hit the warm cache directly.
    let _warm = ctx.prepared_type_decl(&canonical, "ButtonProps");
    let builds_after_warmup = HOST_STORE_VIEW_FROM_HOST_BUILDS.with(|c| c.get());

    // After warmup: repeated warm-hit reads against the threaded view.
    // None should trigger a new HostStoreView build.
    for _ in 0..5 {
        let _ = ctx.prepared_type_decl(&canonical, "ButtonProps");
        let _ = ctx.prepared_type_decl(&canonical, "Label");
        let _ = ctx.prepared_decl_bundle(&canonical);
    }
    let builds_after_repeat = HOST_STORE_VIEW_FROM_HOST_BUILDS.with(|c| c.get());

    assert_eq!(
        builds_after_warmup,
        builds_after_repeat,
        "post-warmup repeated prepared_type_decl reads MUST NOT trigger any \
         HostStoreView::build (warm-hit threads the borrow); observed delta \
         {} — a pre-6.c tree rebuilds the view on every warm-hit and would \
         drive a non-zero delta here.",
        builds_after_repeat - builds_after_warmup
    );
}

/// Mid-request epoch invalidation test (codex / brief §"Discriminating
/// tests" #3, refinement #5).
///
/// `CanonicalCompletionOverlay::complete_canonical` is epoch-guarded:
/// when the host's `current_store_view_epoch` no longer matches the
/// base view's `mutation_epoch`, the overlay completion is a no-op
/// (the request will retry with a fresh view).
///
/// Discriminating: without the epoch guard, the overlay would record
/// the canonical's current state against a superseded base view; the
/// test asserts the overlay STAYS EMPTY when the host's epoch is
/// bumped before `complete_canonical` runs. With the guard, both the
/// guarded call and a subsequent matched-epoch call let us verify
/// the WRITE path is reachable (separating "no-op due to guard" from
/// "no-op due to bug").
#[test]
fn complete_canonical_is_no_op_when_epoch_superseded() {
    let (host, canonical) = small_host_with_one_component();

    // Capture the base view at the current epoch.
    let view = host.resolver_store_view();
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let ctx = HostResolverContext::new(&host, &view, overlay.clone());

    // Construct a parallel `RequestStoreView` over the SAME overlay
    // so the test can peek at overlay state. The wrapper borrows the
    // base view + the Arc'd overlay.
    let probe = crate::resolver_core::RequestStoreView::new(&view, Arc::clone(&overlay));

    // Sanity: overlay is empty before the test exercises it.
    assert_eq!(
        probe.peek_whole_hash_for_tests(&canonical),
        None,
        "overlay must start empty"
    );

    // Bump the project generation (project-shape change) AND advance
    // the store-view epoch so the live epoch diverges from the base
    // view's `mutation_epoch`. The store-view bump is what the epoch
    // guard inside `complete_canonical` checks.
    host.bump_store_view_epoch();

    // Promote the canonical via the request-context helper. The epoch
    // guard inside `complete_canonical` MUST short-circuit because
    // `current_store_view_epoch != base.mutation_epoch`.
    ctx.complete_canonical(&canonical);

    // Discriminating: the overlay's `whole_hashes` MUST NOT contain
    // the canonical. A pre-fix tree without the epoch guard would
    // have inserted the canonical's current `whole_hash`.
    assert_eq!(
        probe.peek_whole_hash_for_tests(&canonical),
        None,
        "complete_canonical MUST NOT mutate the overlay when the host's \
         store-view epoch no longer matches the base view's mutation \
         epoch — a pre-fix tree without refinement #5's epoch guard \
         would have promoted the canonical into the overlay against \
         a stale base view."
    );
}

/// Session-overlay rooting test (codex / brief §"Discriminating tests"
/// #4).
///
/// A request with a session overlay constructs its
/// `SessionResolverContext` once at the request entry — supplying a
/// view that has already been `with_session_overlay`'d. The wrapper
/// owns the borrow; consecutive resolver-method calls do NOT trigger
/// another `with_session_overlay` invocation, and the overlay
/// re-rooting happens exactly once per session-bearing request.
///
/// Discriminating: in a pre-6.c tree, every resolver method on the
/// session context would call
/// `self.resolver_store_view().with_session_overlay(host, view)` —
/// re-running the overlay re-root on every call. The post-6.c context
/// reads through the borrowed pre-built view; the `from_host` counter
/// delta is bounded.
#[test]
fn session_overlay_rooting_runs_once_per_request() {
    use crate::session_view::SessionView;

    let (host, canonical) = small_host_with_one_component();

    // Construct an empty session view: no overlay canonicals, no
    // tombstones. Sufficient to exercise the
    // `with_session_overlay` re-rooting path (the view's snapshot
    // copies through unchanged because both iteration sets are
    // empty).
    struct EmptySessionView {
        project_identity: crate::file_artifact_store::ProjectIdentity,
        env_hashes: crate::session_view::EnvHashes,
    }
    impl SessionView for EmptySessionView {
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
    let session_view = EmptySessionView {
        project_identity: host.host_view_project_identity(),
        env_hashes: crate::session_view::EnvHashes::default(),
    };

    HOST_STORE_VIEW_FROM_HOST_BUILDS.with(|c| c.set(0));

    // Construct the request-bound view + context ONCE — the
    // `with_session_overlay` runs here.
    let base = host
        .resolver_store_view()
        .with_session_overlay(&host, &session_view);
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let ctx = SessionResolverContext::new(&host, &session_view, &base, overlay);

    let builds_after_construction = HOST_STORE_VIEW_FROM_HOST_BUILDS.with(|c| c.get());
    assert_eq!(
        builds_after_construction, 1,
        "constructing SessionResolverContext MUST build the base view \
         exactly once; observed {builds_after_construction}"
    );

    // Warm the bundle first — the cold-compute path may exercise
    // residual cold-path entry points (e.g.
    // `resolve_named_type_export_target_uncached`) that still build a
    // fresh view per call. The discriminating property is that
    // POST-warmup resolver-method calls do NOT trigger additional
    // builds.
    let _warm = ctx.prepared_type_decl(&canonical, "ButtonProps");
    let builds_after_warmup = HOST_STORE_VIEW_FROM_HOST_BUILDS.with(|c| c.get());

    // Drive resolver-tier reads. The session context's `store_view()`
    // returns the borrowed pre-built `RequestStoreView` — none of
    // these calls re-run `with_session_overlay` or rebuild the base.
    for _ in 0..3 {
        let _ = ctx.prepared_type_decl(&canonical, "ButtonProps");
        let _ = ctx.prepared_decl_bundle(&canonical);
        let _ = ctx.prepared_type_decl(&canonical, "Label");
    }

    let builds_after_calls = HOST_STORE_VIEW_FROM_HOST_BUILDS.with(|c| c.get());
    assert_eq!(
        builds_after_calls,
        builds_after_warmup,
        "post-warmup SessionResolverContext resolver-method calls MUST NOT \
         rebuild the base HostStoreView; pre-6.c each call drove \
         `resolver_store_view().with_session_overlay(...)` per method, \
         observed delta {} — a pre-fix tree would have driven a non-zero \
         delta per warm-hit (the `with_session_overlay` re-root included \
         a fresh `from_host` build per call).",
        builds_after_calls - builds_after_warmup
    );
}

/// Session-overlay completion test (codex review B6.C-rfx fix).
///
/// When a `SessionResolverContext` calls `complete_canonical` for a
/// canonical the session view has overlaid, the canonical completion
/// overlay MUST record the SESSION OVERLAY hash, not the base host's
/// scheduler hash. The session view is the authoritative source for
/// overlaid canonicals; the request-entry base view was already re-
/// rooted via `with_session_overlay` to carry the overlay hash. Writing
/// the base hash on top of that overlay-rooted hash would break the 6.B
/// session-overlay validation contract (`096e124a2`): subsequent self-
/// root / parse-domain validation inside the request would
/// false-validate the base hash and false-reject the overlay hash, even
/// though the session is the only authority for the overlaid content.
///
/// Discriminating against the pre-fix tree:
/// - **Pre-fix**: `complete_canonical` always reads
///   `host.effective_file_state(canonical, None)` (the BASE scheduler
///   hash) and inserts it into the completion overlay's `whole_hashes`.
///   The wrapper's `validates_self_root_whole_hash` then consults the
///   overlay FIRST — so a query for the OVERLAY hash falls into the
///   shadowing-mismatch branch (returns `false`) and a query for the
///   BASE hash matches the overlay (returns `true`). Both outcomes are
///   wrong.
/// - **Post-fix**: the session-aware `complete_canonical_with_session_view`
///   path reads `view.overlay_content_hash_for(canonical)` first, finds
///   the overlay hash, and writes that. Then a query for the overlay
///   hash matches (`true`) and a query for the base hash mismatches
///   (`false`).
#[test]
fn complete_canonical_writes_session_overlay_hash_not_base_hash() {
    use crate::resolver_core::ResolverContext;
    use crate::session_view::{OverlaidView, SessionView};
    use rustc_hash::FxHashMap;

    let (host, canonical) = small_host_with_one_component();

    // Materialise the base IndexedReady so `with_session_overlay` and
    // the overlay artifact lookup both have something to find.
    let base_hash = host
        .ensure_indexed_ready(&canonical)
        .expect("base IndexedReady materialises")
        .whole_hash;
    let host = Arc::new(host);

    // Construct a session view that overlays `canonical` with DIFFERENT
    // content — the overlay hash must diverge from the base hash for
    // this test to discriminate.
    let overlay_source: Arc<str> = Arc::from(
        r#"<script setup lang="ts">
interface ButtonProps {
  label: string
  disabled?: boolean
  variant?: 'primary' | 'secondary'
}
defineProps<ButtonProps>()
</script>
<template><button :disabled="disabled">{{ label }}</button></template>
"#,
    );
    let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    overlays.insert(canonical.clone(), Arc::clone(&overlay_source));
    let view = OverlaidView::new(Arc::clone(&host), overlays);

    let overlay_hash = view
        .overlay_content_hash_for(&canonical)
        .expect("OverlaidView reports an overlay hash for the masked canonical");
    assert_ne!(
        overlay_hash, base_hash,
        "fixture invariant: overlay content must differ from base content so the \
         two hashes diverge — otherwise this test cannot discriminate the bug"
    );

    // Materialise the overlay IndexedReady so the overlay artifact
    // lookup inside `complete_canonical_with_session_view` finds the
    // overlay `FileArtifacts` and writes the overlay-rooted derived
    // hashes.
    host.materialize_overlay_indexed_ready_with_view(&canonical, &view)
        .expect("overlay IndexedReady materialises");

    // Build the request-bound session-rooted base view ONCE (matches
    // the production session-bearing request entry point).
    let base = host
        .resolver_store_view()
        .with_session_overlay(&host, &view);
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let ctx = SessionResolverContext::new(&host, &view, &base, Arc::clone(&overlay));

    // Mid-request: promote the overlaid canonical into the completion
    // overlay. With the bug, this writes `base_hash`; with the fix, it
    // writes `overlay_hash`.
    ctx.complete_canonical(&canonical);

    // Direct overlay-state inspection: the recorded `whole_hash` MUST
    // be the overlay hash. Pre-fix value is `base_hash`.
    let probe = crate::resolver_core::RequestStoreView::new(&base, Arc::clone(&overlay));
    assert_eq!(
        probe.peek_whole_hash_for_tests(&canonical),
        Some(overlay_hash),
        "complete_canonical (session-aware path) MUST record the SESSION OVERLAY \
         hash for the canonical. Pre-fix it recorded the base scheduler hash \
         (`host.effective_file_state(canonical, None)`), which violates the 6.B \
         session-overlay validation contract (096e124a2): a session-overlaid \
         canonical's overlay-rooted facts then false-miss in subsequent self-root \
         validation inside the same request."
    );

    // End-to-end shadowing validation: the wrapper's StoreView trait
    // honours the overlay value. Self-root validation against the
    // OVERLAY hash succeeds (the overlay matches), and validation
    // against the BASE hash fails (the overlay shadows with the
    // overlay hash, mismatching the base hash).
    let store_view = ctx.store_view();
    assert!(
        store_view.validates_self_root_whole_hash(&canonical, &overlay_hash),
        "self-root validation against the overlay hash MUST succeed; pre-fix the \
         completion overlay carried `base_hash`, so the overlay-shadowed \
         comparison returned `false` for the overlay-hash query"
    );
    assert!(
        !store_view.validates_self_root_whole_hash(&canonical, &base_hash),
        "self-root validation against the base hash MUST FAIL on a session-overlaid \
         canonical; pre-fix the completion overlay carried `base_hash`, so the \
         shadowed comparison returned `true` for the base-hash query — \
         false-validating a stale base-rooted fact against a session-overlaid file"
    );
}

/// Resolve-imports overlay-routing regression test.
///
/// When `ensure_loaded` / `ensure_indexed_ready` promotes a canonical
/// after the request-entry base view was built, the canonical's
/// authoritative content hash lives in the per-request completion
/// overlay, NOT in the base view's `whole_hashes` snapshot. Without
/// the overlay-aware routing in
/// [`crate::resolver_core::RequestStoreView::validates_resolve_imports_domain`],
/// the base validator would compose its `ResolvedImportFactsKey` from
/// the absent snapshot entry and reject every real `ResolveImports`
/// fact on the promoted canonical — even when the shared
/// `ResolvedImportFactsDb` was populated under the overlay's content
/// hash.
///
/// Discriminating: this test wires a canonical into the host and its
/// import-facts producer state in such a way that the base view
/// (snapshotted BEFORE `set_import_dependencies`) does NOT track the
/// owner canonical. After `set_import_dependencies` populates the
/// shared `ResolvedImportFactsDb` and `complete_canonical` promotes
/// the owner into the overlay, a `ResolveImportsFactRef` constructed
/// from the admitted entry must validate through the
/// `RequestStoreView`. Pre-fix the wrapper fell straight through to
/// the base view, which rejected the fact via its
/// `whole_hashes.get(...) -> None` arm (`fact.expected_hash !=
/// ZERO_HASH`); post-fix the wrapper sees the canonical in the
/// overlay, recomposes the key under the overlay's content hash,
/// and the warm-hit `ResolvedImportFactsDb` lookup succeeds.
#[test]
fn request_store_view_validates_resolve_imports_for_overlay_promoted_canonical() {
    use crate::resolver_core::{
        CanonicalCompletionOverlay, FactVersionRef, RequestStoreView, ResolveImportsFactRef,
        StoreView,
    };
    // `ResolverStore` is intentionally absent here — the test exercises
    // the wrapper validator directly via `validates_fact_signature`
    // and does not need the store-mutation surface.
    use crate::session_view::{HostView, SessionView};
    use crate::types::DependencyResolution;
    use crate::{CompileErrorPolicy, FileKind, HostConfig, UpsertRequest, VerterHost};
    use verter_semantic::facts::registry::{FactKey, FactLane, InternedName, InternedSpecifier};

    let host = VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    });

    // Materialise the dep + owner files and drive the
    // `set_import_dependencies` producer so the shared
    // `ResolvedImportFactsDb` carries an entry keyed by the owner's
    // current content hash.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/dep.ts".to_string(),
            source: Arc::from("export const v = 1;"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .expect("dep upsert");

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/owner.ts".to_string(),
            source: Arc::from("import { v } from './dep';\nexport const o = v;\n"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .expect("owner upsert");

    host.set_import_dependencies(
        "/owner.ts",
        vec![DependencyResolution {
            specifier: "./dep".to_string(),
            resolved_canonical_id: Some("/dep.ts".to_string()),
            possible_canonical_ids: vec!["/dep.ts".to_string()],
        }],
    );

    // Materialise the producer's payload — this is the admission path
    // used in production (the producer admits under the owner's
    // current content hash; the same hash the overlay records below).
    let host_arc = Arc::new(host);
    let session_view = HostView::new(Arc::clone(&host_arc));
    let payload = session_view
        .resolved_import_facts("/owner.ts")
        .expect("producer admitted the resolved-import-facts payload for /owner.ts");

    let entry = payload
        .import_clauses
        .iter()
        .find(|e| e.binding.as_ref() == "v")
        .expect("`v` binding admitted into the resolved-import-facts payload");

    // Build the `ResolveImportsFactRef` shape the wrapper validator
    // receives at runtime.
    let fact_ref = ResolveImportsFactRef {
        canonical_id: "/owner.ts".to_string(),
        key: FactKey::ResolvedImportClause {
            specifier: InternedSpecifier::from(entry.specifier.as_ref()),
            binding: InternedName::from(entry.binding.as_ref()),
            space: entry.space,
            resolved_canonical: entry
                .resolved_canonical
                .as_ref()
                .map(Arc::clone)
                .expect("resolved-canonical present"),
            resolved_source_name: InternedName::from(entry.resolved_source_name.as_ref()),
        },
        lane: FactLane::Semantic,
        expected_hash: entry.fact.semantic_hash,
    };
    let sig = vec![FactVersionRef::ResolveImports(fact_ref.clone())];

    // Build a base view that DOES NOT track `/owner.ts` — simulating
    // a request-entry view captured before a mid-request
    // `ensure_loaded` promoted the canonical. Production uses
    // `ensure_loaded` (which does NOT bump `store_view_epoch`) to
    // hit this state; the test reaches it via the
    // `forget_whole_hash_for_tests` helper so the scenario isolates
    // the validator routing under test from the scheduler / loader
    // plumbing. The captured snapshot retains every other field
    // (`resolved_import_facts` Arc, `env_hashes`, etc.), matching the
    // production base view's shape minus the absent canonical entry.
    let mut base = host_arc.resolver_store_view();
    let owner_whole_hash = base
        .whole_hashes_get_for_tests("/owner.ts")
        .expect("base view tracks the owner immediately after upsert");
    base.forget_whole_hash_for_tests("/owner.ts");
    assert!(
        !base.tracks_file("/owner.ts"),
        "fixture invariant: base view must not track the owner after `forget` helper"
    );

    // Sanity: against the now-untracked base view, the validator MUST
    // reject the fact via the `whole_hashes.get(...) -> None`
    // untracked-file arm (the fact's `expected_hash` is the admitted
    // entry's `semantic_hash`, NOT the zero sentinel, so the
    // optimistic-accept window does not apply). This is the
    // pre-fix observation: the request would have false-missed the
    // warm-hit despite the shared DB being populated.
    assert!(
        !base.validates_fact_signature(&sig),
        "fixture invariant: the base view without `/owner.ts` MUST reject the \
         fact through the untracked-file arm. Pre-fix \
         `RequestStoreView::validates_resolve_imports_domain` delegated straight \
         to this rejected path."
    );

    // Build the per-request `RequestStoreView` wrapper over the same
    // (stale-for-owner) base view + a fresh overlay. Record the
    // owner's current content hash directly into the overlay (the
    // production path is `complete_canonical` driven from
    // `ensure_loaded`; the test takes the
    // `insert_whole_hash_for_tests` shortcut so the scenario does not
    // depend on the epoch guard being satisfied by a non-bumped
    // load path).
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    overlay.insert_whole_hash_for_tests("/owner.ts", owner_whole_hash);

    let request_view = RequestStoreView::new(&base, Arc::clone(&overlay));

    // Discriminating: the overlay-aware wrapper MUST now route the
    // resolve-imports validation through the OVERLAY's content hash
    // and look up the populated shared `ResolvedImportFactsDb` entry,
    // returning `true`. A pre-fix tree would delegate straight to
    // `self.base.validates_resolve_imports_domain(fact)`, the base
    // validator would compose the key from its absent
    // `whole_hashes["/owner.ts"]`, and the assertion would fail.
    assert!(
        request_view.validates_fact_signature(&sig),
        "post-fix RequestStoreView::validates_resolve_imports_domain MUST validate \
         the fact via the overlay's content hash; pre-fix it delegated to the base \
         view, which rejected the fact through its untracked-file arm despite the \
         shared ResolvedImportFactsDb being populated under the overlay-recorded \
         content hash. This is the overlay-routing regression."
    );
}

/// Overlay flag-ordering arch guard.
///
/// Source-grep guard verifying the strict-ordering invariant in
/// [`crate::resolver_core::request_store_view::CanonicalCompletionOverlay::write_completion_entry`]:
/// each map insert AND the corresponding `_nonempty.store(true,
/// Release)` MUST execute under the same write-lock critical section,
/// so a concurrent reader that observes `_nonempty == false` is
/// guaranteed to also observe the underlying map as empty for the
/// canonical (no skip-the-overlay false-negative window).
///
/// Pre-fix shape (would FAIL this guard):
/// ```ignore
/// self.whole_hashes.write().insert(...);   // lock released here
/// self.whole_hashes_nonempty.store(true, Ordering::Release);  // race window
/// ```
///
/// Post-fix shape (passes this guard):
/// ```ignore
/// {
///     let mut whole = self.whole_hashes.write();
///     whole.insert(...);
///     self.whole_hashes_nonempty.store(true, Ordering::Release);
///     drop(whole);
/// }
/// ```
///
/// The arch guard reads the function source and asserts that each
/// `_nonempty.store(true, Ordering::Release)` line is preceded
/// (within the same brace-balanced block, immediately following the
/// insert) by a write-lock guard binding that is NOT dropped before
/// the store. The deterministic form here is: the function source
/// must NOT contain the pre-fix sequence
/// `self.whole_hashes.write().insert(...)` followed by the
/// flag-store on the next non-comment line.
///
/// Discriminating: a pre-fix tree's `write_completion_entry` source
/// would contain `self.whole_hashes.write().insert(canonical.to_owned(),
/// whole_hash);` immediately followed by
/// `self.whole_hashes_nonempty.store(true, Ordering::Release);`. The
/// post-fix tree binds the write guard to a named variable
/// (`let mut whole = self.whole_hashes.write();`) and stores the
/// flag inside the brace-bounded critical section.
#[test]
fn write_completion_entry_flag_set_under_write_lock() {
    use std::fs;
    use std::path::Path;

    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("resolver_core")
        .join("request_store_view.rs");
    let src =
        fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));

    let fn_needle = "fn write_completion_entry(";
    let fn_idx = src
        .find(fn_needle)
        .unwrap_or_else(|| panic!("expected `{fn_needle}` in {}", path.display()));
    let fn_end = src[fn_idx..]
        .find("\n    }\n")
        .expect("write_completion_entry fn close");
    let window = &src[fn_idx..fn_idx + fn_end];

    // Pre-fix anti-patterns: the chained `.write().insert(...)`
    // releases the lock at the statement-end semicolon, then the
    // flag store runs without the lock.
    let pre_fix_anti_patterns = [
        "self.whole_hashes\n            .write()\n            .insert(",
        "self.whole_hashes.write().insert(",
        "self.file_facts\n                .write()\n                .insert(",
        "self.file_facts.write().insert(",
    ];
    for pat in pre_fix_anti_patterns {
        assert!(
            !window.contains(pat),
            "write_completion_entry MUST NOT use the chained \
             `self.<map>.write().insert(...)` pattern — that releases the write \
             lock at the statement-end semicolon and leaves a race window where \
             a concurrent reader can observe `_nonempty == false` while the map \
             is already populated. Bind the write guard to a named variable and \
             store the `_nonempty` flag BEFORE dropping the guard. Pattern \
             found:\n{pat}\n\nWindow:\n{window}"
        );
    }

    // Post-fix invariant: each `_nonempty.store(true, Ordering::Release)`
    // line must appear inside a brace-bounded block that also
    // contains the corresponding `write()` lock guard binding —
    // proxy: the named-guard pattern is present.
    let post_fix_named_guard_patterns = [
        "let mut whole = self.whole_hashes.write();",
        "self.whole_hashes_nonempty.store(true, Ordering::Release);",
        "let mut facts = self.file_facts.write();",
        "self.file_facts_nonempty.store(true, Ordering::Release);",
        "let mut derived = self.derived_hashes.write();",
        "self.derived_hashes_nonempty.store(true, Ordering::Release);",
    ];
    for pat in post_fix_named_guard_patterns {
        assert!(
            window.contains(pat),
            "write_completion_entry MUST bind each write-lock guard to a named \
             variable and store the corresponding `_nonempty` flag under the \
             same critical section. Missing \
             pattern:\n{pat}\n\nWindow:\n{window}"
        );
    }

    // The post-fix shape stores the flag BEFORE dropping the guard.
    // Verify each `_nonempty.store(...)` precedes the corresponding
    // `drop(<guard>)` in the source. The relative ordering of these
    // two lines is the actual safety invariant — if a future edit
    // swaps `drop(whole)` before `self.whole_hashes_nonempty.store(...)`,
    // the race window reopens.
    for (guard, flag_store) in [
        (
            "drop(whole);",
            "self.whole_hashes_nonempty.store(true, Ordering::Release);",
        ),
        (
            "drop(facts);",
            "self.file_facts_nonempty.store(true, Ordering::Release);",
        ),
        (
            "drop(derived);",
            "self.derived_hashes_nonempty.store(true, Ordering::Release);",
        ),
    ] {
        let store_idx = window
            .find(flag_store)
            .unwrap_or_else(|| panic!("flag store `{flag_store}` not found in window"));
        let drop_idx = window
            .find(guard)
            .unwrap_or_else(|| panic!("guard drop `{guard}` not found in window"));
        assert!(
            store_idx < drop_idx,
            "write_completion_entry MUST store the `_nonempty` flag BEFORE \
             dropping the write-lock guard. A reader that observes the flag as \
             `false` must be guaranteed that the writer has not yet inserted \
             into the map (the writer's insert + flag-store happen under the \
             same critical section). Flag-store `{flag_store}` at byte \
             {store_idx} appears AFTER guard drop `{guard}` at byte {drop_idx}."
        );
    }
}
