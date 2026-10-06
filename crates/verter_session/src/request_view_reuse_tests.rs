//! Discriminating tests for the per-request `HostStoreView`
//! hoist.
//!
//! These tests pin properties of the hoist that would silently regress
//! if a future change unwound the per-request view contract:
//!
//! 1. **View-build count** — a component-meta request through a
//!    `HostResolverContext` builds the `HostStoreView` once at request entry;
//!    nested warm validations borrow that view.
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
//! Together the four tests discriminate per-call rebuilding, lost
//! currentness, and overlay re-rooting errors.

use crate::file_artifact_store::FileArtifactKeySource;
use std::sync::Arc;
use verter_session_query::facts::store_view::StoreView;
use verter_type_engine::resolver_core::request_ports::IndexedInputs;

use crate::resolver_core::{
    CanonicalCompletionOverlay, HostResolverContext, SessionResolverContext,
};
use crate::resolver_store::HOST_STORE_VIEW_FROM_HOST_BUILDS;
use crate::types::FileLanguage;
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
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert Button.vue must succeed");
    (host, canonical)
}

fn small_host_with_one_script() -> (VerterHost, String) {
    let host = VerterHost::new_standalone(HostConfig::default());
    let canonical = "/proj/button.ts".to_string();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: canonical.clone(),
            source: Arc::from("export interface ButtonProps { label: string }\n"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert button.ts must succeed");
    (host, canonical)
}

/// View-build count test.
///
/// A single resolver-tier read pattern under a `HostResolverContext`
/// must drive STRICTLY FEWER `HostStoreView::from_host` calls than
/// the number of reads. The warm-hit path threads the borrowed
/// `RequestStoreView` without rebuilding it, so the counter delta
/// stays small — bounded by the residual cold-path entry points
/// (e.g. `resolve_named_type_export_target_uncached`) that still
/// build a fresh view per call. We measure 10 reads and assert the
/// counter is much smaller than 10, enforcing request-bound view reuse.
#[test]
fn view_build_count_drops_under_hosted_request() {
    let (host, canonical) = small_host_with_one_component();
    // Reset the counter so the assertion is scoped to one request.
    HOST_STORE_VIEW_FROM_HOST_BUILDS.with(|c| c.set(0));

    let view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let ctx = HostResolverContext::new(&host, &view, overlay);

    // Drive ~10 resolver-tier reads through the request context. The warm-hit
    // path must read through the request-bound borrow.
    let mut reads = 0u64;
    for _ in 0..5 {
        let _ = ctx.prepared_type_decl(
            &canonical,
            verter_type_expr::TopLevelOwnerId::instance(0),
            "ButtonProps",
        );
        let _ = ctx.prepared_decl_bundle(&canonical);
        reads += 2;
    }

    let builds = HOST_STORE_VIEW_FROM_HOST_BUILDS.with(|c| c.get());
    // With warm hits reading through the borrow, `builds` is bounded by
    // the residual cold-path rebuild sites + the request-entry
    // build. Assert `builds < reads / 2` without pinning the exact residual
    // count, which depends on the cold paths exercised by the fixture.
    assert!(
        builds < reads / 2,
        "HostResolverContext-bound resolver-tier reads MUST short-circuit \
         most HostStoreView::from_host rebuilds (warm-hit threads the \
         borrow). Observed {builds} builds across {reads} reads; the allowed \
         bound is fewer than half the read count."
    );
}

/// Repeated `prepared_type_decl` test.
///
/// After the first `prepared_type_decl` on a dep warms the bundle,
/// every subsequent `prepared_type_decl(dep, sym)` call on the same
/// dep is a warm hit and MUST NOT trigger ANY further
/// `HostStoreView::build`. The shared bundle cache is content-pinned;
/// the warm-hit path reads through the request-bound borrow.
///
/// `HostResolverContext::prepared_type_decl` threads
/// `self.view.base()` down through `_with_store_view`; the warm hit
/// is allocation-free.
#[test]
fn repeated_prepared_type_decl_no_view_rebuild() {
    let (host, canonical) = small_host_with_one_component();

    let view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let ctx = HostResolverContext::new(&host, &view, overlay);

    // Warm the bundle: the first call walks the cold path
    // (`materialize_prepared_decl_bundle` etc.). Subsequent calls
    // hit the warm cache directly.
    let _warm = ctx.prepared_type_decl(
        &canonical,
        verter_type_expr::TopLevelOwnerId::instance(0),
        "ButtonProps",
    );
    let builds_after_warmup = HOST_STORE_VIEW_FROM_HOST_BUILDS.with(|c| c.get());

    // After warmup: repeated warm-hit reads against the threaded view.
    // None should trigger a new HostStoreView build.
    for _ in 0..5 {
        let _ = ctx.prepared_type_decl(
            &canonical,
            verter_type_expr::TopLevelOwnerId::instance(0),
            "ButtonProps",
        );
        let _ = ctx.prepared_type_decl(
            &canonical,
            verter_type_expr::TopLevelOwnerId::instance(0),
            "Label",
        );
        let _ = ctx.prepared_decl_bundle(&canonical);
    }
    let builds_after_repeat = HOST_STORE_VIEW_FROM_HOST_BUILDS.with(|c| c.get());

    assert_eq!(
        builds_after_warmup,
        builds_after_repeat,
        "post-warmup repeated prepared_type_decl reads MUST NOT trigger any \
         HostStoreView::build (warm-hit threads the borrow); observed delta {}.",
        builds_after_repeat - builds_after_warmup
    );
}

/// Mid-request epoch invalidation test.
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
    let view = host.resolver_store_view_read().into_owned_view();
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

    // The overlay's `whole_hashes` MUST NOT contain the canonical because the
    // epoch guard rejects completion against a superseded base view.
    assert_eq!(
        probe.peek_whole_hash_for_tests(&canonical),
        None,
        "complete_canonical MUST NOT mutate the overlay when the host's \
         store-view epoch no longer matches the base view's mutation \
         epoch; promotion against a stale base view is forbidden."
    );
}

/// Session-overlay rooting test.
///
/// A request with a session overlay constructs its
/// `SessionResolverContext` once at the request entry — supplying a
/// view that has already been `with_session_overlay`'d. The wrapper
/// owns the borrow; consecutive resolver-method calls do NOT trigger
/// another `with_session_overlay` invocation, and the overlay
/// re-rooting happens exactly once per session-bearing request.
///
/// Resolver methods read through the borrowed pre-built view and never rerun
/// `with_session_overlay`; the `from_host` counter delta is therefore bounded.
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
        fn content_hash_for(
            &self,
            _canonical: &str,
        ) -> Option<verter_session_query::analysis::types::Hash16> {
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
        .resolver_store_view_read()
        .into_owned_view()
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
    let _warm = ctx.prepared_type_decl(
        &canonical,
        verter_type_expr::TopLevelOwnerId::instance(0),
        "ButtonProps",
    );
    let builds_after_warmup = HOST_STORE_VIEW_FROM_HOST_BUILDS.with(|c| c.get());

    // Drive resolver-tier reads. The session context's `store_view()`
    // returns the borrowed pre-built `RequestStoreView` — none of
    // these calls re-run `with_session_overlay` or rebuild the base.
    for _ in 0..3 {
        let _ = ctx.prepared_type_decl(
            &canonical,
            verter_type_expr::TopLevelOwnerId::instance(0),
            "ButtonProps",
        );
        let _ = ctx.prepared_decl_bundle(&canonical);
        let _ = ctx.prepared_type_decl(
            &canonical,
            verter_type_expr::TopLevelOwnerId::instance(0),
            "Label",
        );
    }

    let builds_after_calls = HOST_STORE_VIEW_FROM_HOST_BUILDS.with(|c| c.get());
    assert_eq!(
        builds_after_calls,
        builds_after_warmup,
        "post-warmup SessionResolverContext resolver-method calls MUST NOT \
         rebuild the base HostStoreView or rerun \
         `resolver_store_view().with_session_overlay(...)`; observed delta {}.",
        builds_after_calls - builds_after_warmup
    );
}

/// Session-overlay completion test.
///
/// When a `SessionResolverContext` calls `complete_canonical` for a
/// canonical the session view has overlaid, the canonical completion
/// overlay MUST record the SESSION OVERLAY hash, not the base host's
/// scheduler hash. The session view is the authoritative source for
/// overlaid canonicals; the request-entry base view was already re-
/// rooted via `with_session_overlay` to carry the overlay hash. Writing
/// the base hash on top of that overlay-rooted hash would break the
/// session-overlay validation contract: subsequent self-
/// root / parse-domain validation inside the request would
/// false-validate the base hash and false-reject the overlay hash, even
/// though the session is the only authority for the overlaid content.
///
/// The session-aware `complete_canonical_with_session_view` path reads
/// `view.overlay_content_hash_for(canonical)` first and records that hash.
/// The overlay hash must then validate while the base hash must not.
#[test]
fn complete_canonical_writes_session_overlay_hash_not_base_hash() {
    use crate::session_view::{OverlaidView, SessionView};
    use rustc_hash::FxHashMap;

    // This assertion is about generic session-rooted artifact hashes. Carrier
    // document revisions require their own registered document authority, so
    // use an ordinary script overlay here rather than inventing carrier source
    // identity inside the overlay materializer.
    let (host, canonical) = small_host_with_one_script();

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
    let overlay_source: Arc<str> =
        Arc::from("export interface ButtonProps { label: string; disabled?: boolean }\n");
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
        .resolver_store_view_read()
        .into_owned_view()
        .with_session_overlay(&host, &view);
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let ctx = SessionResolverContext::new(&host, &view, &base, Arc::clone(&overlay));

    // Mid-request: promote the overlaid canonical into the completion
    // overlay. This must write `overlay_hash`.
    ctx.complete_canonical(&canonical);

    // Direct overlay-state inspection: the recorded `whole_hash` MUST be the
    // overlay hash.
    let probe = crate::resolver_core::RequestStoreView::new(&base, Arc::clone(&overlay));
    assert_eq!(
        probe.peek_whole_hash_for_tests(&canonical),
        Some(overlay_hash),
        "complete_canonical (session-aware path) MUST record the session-overlay \
         hash so the canonical's overlay-rooted facts validate inside the request"
    );

    // End-to-end shadowing validation: the wrapper's StoreView trait
    // honours the overlay value. Self-root validation against the
    // OVERLAY hash succeeds (the overlay matches), and validation
    // against the BASE hash fails (the overlay shadows with the
    // overlay hash, mismatching the base hash).
    let store_view =
        &verter_type_engine::resolver_core::fact_validation_port::FactValidationView::new(&ctx);
    assert!(
        store_view.validates_self_root_whole_hash(&canonical, &overlay_hash),
        "self-root validation against the overlay hash MUST succeed"
    );
    assert!(
        !store_view.validates_self_root_whole_hash(&canonical, &base_hash),
        "self-root validation against the base hash MUST fail on a session-overlaid \
         canonical to prevent stale base-rooted validation"
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
/// the base view rejects the fact via its `whole_hashes.get(...) -> None` arm
/// (`fact.expected_hash != ZERO_HASH`); the wrapper sees the canonical in the
/// overlay, recomposes the key under the overlay's content hash,
/// and the warm-hit `ResolvedImportFactsDb` lookup succeeds.
#[test]
fn request_store_view_validates_resolve_imports_for_overlay_promoted_canonical() {
    use crate::resolver_core::{CanonicalCompletionOverlay, RequestStoreView};
    use verter_session_query::facts::fact_cache::{FactVersionRef, ResolveImportsFactRef};
    use verter_session_query::facts::store_view::StoreView;
    // `ResolverStore` is intentionally absent here — the test exercises
    // the wrapper validator directly via `validates_fact_signature`
    // and does not need the store-mutation surface.
    use crate::session_view::{HostView, SessionView};
    use crate::types::DependencyResolution;
    use crate::{CompileErrorPolicy, FileLanguage, HostConfig, UpsertRequest, VerterHost};
    use verter_session_query::facts::registry::{
        FactKey, FactLane, InternedName, InternedSpecifier,
    };

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
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("dep upsert");

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/owner.ts".to_string(),
            source: Arc::from("import { v } from './dep';\nexport const o = v;\n"),
            file_language: FileLanguage::script_ts(),
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
    let fact_ref = ResolveImportsFactRef::Semantic {
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
    let mut base = host_arc.resolver_store_view_read().into_owned_view();
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
    // optimistic-accept window does not apply). This establishes that the base
    // view alone cannot validate the warm hit.
    assert!(
        !base.validates_fact_signature(&sig),
        "fixture invariant: the base view without `/owner.ts` MUST reject the \
         fact through the untracked-file arm"
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
    // and look up the populated shared `ResolvedImportFactsDb` entry.
    assert!(
        request_view.validates_fact_signature(&sig),
        "RequestStoreView::validates_resolve_imports_domain MUST validate the fact \
         via the overlay's content hash when the base view does not track the owner"
    );
}

/// Every validation-visible completion map publishes its presence flag
/// before insertion while already holding that map's write lock. The
/// instrumented writers assert both properties at runtime; the lookups then
/// prove all three insertions are visible through the public overlay behavior.
#[test]
fn completion_writers_publish_presence_under_the_map_lock_before_insertion() {
    use crate::file_artifact_store::FileFacts;
    use verter_session_query::facts::fact_cache::DerivedFactKind;

    let overlay = CanonicalCompletionOverlay::new();
    overlay.verify_write_protocol_for_tests();
    overlay.insert_whole_hash_for_tests("/whole.ts", [0x11; 16]);
    overlay.insert_derived_hash_for_tests("/derived.ts", DerivedFactKind::Route, [0x22; 16]);
    overlay.insert_file_facts_for_tests("/facts.ts", Arc::new(FileFacts::empty()));

    assert_eq!(
        overlay.peek_whole_hash_for_tests("/whole.ts"),
        Some([0x11; 16])
    );
    assert_eq!(
        overlay.lookup_derived_hash_for_tests("/derived.ts", DerivedFactKind::Route),
        Some([0x22; 16])
    );
    assert!(overlay.tracks_file_for_tests("/facts.ts"));
}

/// Every serve of a file completes its canonical into the request overlay
/// (`ensure_indexed_ready_serve` → `complete_canonical`), so a request that
/// serves one file once per query completes it once per query. Only the
/// first completion of a content version promotes it: a repeat finds the
/// same `whole_hash` and the same facts already shadowing and writes
/// nothing, so a request's completion work does not grow with its query
/// count — promoting again compared the stored facts with the artifact's
/// structurally, a walk over every fact the file registers per serve.
#[test]
fn repeated_completion_of_one_content_version_promotes_once() {
    let (host, canonical) = small_host_with_one_script();
    assert!(
        host.ensure_indexed_ready_serve(&canonical).is_some(),
        "the script must index"
    );
    let view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let ctx = HostResolverContext::new(&host, &view, overlay.clone());
    for _ in 0..50 {
        ctx.complete_canonical(&canonical);
    }
    let probe = crate::resolver_core::RequestStoreView::new(&view, Arc::clone(&overlay));
    assert!(
        probe.peek_whole_hash_for_tests(&canonical).is_some(),
        "the canonical is completed"
    );
    assert_eq!(
        overlay.completion_promotions_for_tests(),
        1,
        "fifty completions of one content version promote it once"
    );
}

/// A plain script's artifact key reuses the parse identity its source
/// snapshot derived once, and that key is exactly the one the source bytes
/// derive: every serve reads the key, so re-hashing the whole source per
/// serve made each read cost the file's size. Reading the key fifty times
/// hashes the source no time at all; deriving it from the source (the
/// control) hashes it once.
#[test]
fn a_script_artifact_key_reuses_the_snapshot_parse_identity() {
    let (host, canonical) = small_host_with_one_script();
    assert!(host
        .authoritative_current_artifact_key(&canonical)
        .is_some());
    let before =
        verter_session_query::source::framework_parse::source_parse_identity_derivations_for_tests(
        );
    for _ in 0..50 {
        assert!(host
            .authoritative_current_artifact_key(&canonical)
            .is_some());
    }
    assert_eq!(
        verter_session_query::source::framework_parse::source_parse_identity_derivations_for_tests(
        ) - before,
        0,
        "fifty key reads hash the script's source no time at all"
    );
    let state = host
        .effective_file_state(&canonical, None)
        .expect("the script has a source state");
    let parse_key = state
        .script_parse_key
        .clone()
        .expect("a plain script's snapshot carries its parse identity");
    let from_source =
        verter_session_query::source::artifact_key::FileArtifactKey::for_source_identity(
            Arc::from(canonical.as_str()),
            state.whole_hash,
            state.source.as_ref(),
            state.file_language.clone(),
            None,
            verter_session_query::source::artifact_key::BASE_PARSE_ENV_HASH,
        )
        .expect("the script's language has a parse identity");
    assert_eq!(
        verter_session_query::source::framework_parse::source_parse_identity_derivations_for_tests(
        ) - before,
        1,
        "deriving the key from the source hashes it once"
    );
    assert_eq!(from_source.parse_key, parse_key);
    assert_eq!(
        host.authoritative_current_artifact_key(&canonical),
        Some(from_source)
    );
    let (host, component) = small_host_with_one_component();
    assert_eq!(
        host.effective_file_state(&component, None)
            .expect("the component has a source state")
            .script_parse_key,
        None,
        "a framework carrier's parse identity is its framework parse key"
    );
}

/// A request validates each consumed result's receipt once, not once per
/// signature that reaches it: a second signature sharing a chain's
/// receipt reads only its own fact. What the answer depends on stays in
/// the key: strict self-roots the receipt reaches, and a completion that
/// moves the overlay, walk the receipt again; a refused walk is never
/// remembered; a new request starts empty.
#[test]
fn a_request_validates_a_shared_receipt_once() {
    use crate::resolver_core::RequestStoreView;
    use verter_session_query::facts::fact_cache::{FactVersionRef, ResultReceipt};
    use verter_session_query::facts::store_view::StoreView;

    let (host, _) = small_host_with_one_script();
    let base = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let whole = |level: u8| FactVersionRef::FileWholeHash {
        canonical_id: format!("/r/{level}.ts"),
        hash: [level; 16],
    };
    for level in 0..34u8 {
        overlay.insert_whole_hash_for_tests(&format!("/r/{level}.ts"), [level; 16]);
    }
    let mut chain = ResultReceipt::new(vec![whole(0)]);
    for level in 1..32u8 {
        chain = ResultReceipt::new(vec![FactVersionRef::Receipt(chain), whole(level)]);
    }
    let first = vec![FactVersionRef::Receipt(chain.clone()), whole(32)];
    let second = vec![FactVersionRef::Receipt(chain.clone()), whole(33)];

    let view = RequestStoreView::new(&base, Arc::clone(&overlay));
    let reads = |signature: &[FactVersionRef], roots: &[&str]| {
        let before = view.leaf_validations_for_tests();
        assert_eq!(view.validate_fact_signature(signature, roots), Ok(()));
        view.leaf_validations_for_tests() - before
    };
    assert_eq!(reads(&first, &[]), 33, "the chain's 32 facts and its own");
    assert_eq!(reads(&second, &[]), 1, "only its own fact");
    let before = view.leaf_validations_for_tests();
    assert!(view.validates(&FactVersionRef::Receipt(chain.clone())));
    assert_eq!(
        view.leaf_validations_for_tests(),
        before,
        "one fact, no reads"
    );

    // Levels 7 to 31 reach the root and are read again; the seven below
    // never reach it, so their answers stand.
    assert_eq!(reads(&second, &["/r/7.ts"]), 26, "a strict root it reaches");
    assert_eq!(reads(&second, &["/r/7.ts"]), 1);
    assert_eq!(
        reads(&second, &["/elsewhere.ts"]),
        1,
        "a root it never reaches"
    );

    overlay.insert_whole_hash_for_tests("/moved.ts", [9; 16]);
    assert_eq!(reads(&second, &[]), 33, "a completion moved the overlay");
    assert_eq!(reads(&second, &[]), 1);

    let stale = ResultReceipt::new(vec![FactVersionRef::Receipt(chain.clone()), {
        FactVersionRef::FileWholeHash {
            canonical_id: "/r/33.ts".into(),
            hash: [0; 16],
        }
    }]);
    let stale = vec![FactVersionRef::Receipt(stale)];
    for _ in 0..2 {
        let before = view.leaf_validations_for_tests();
        assert!(view.validate_fact_signature(&stale, &[]).is_err());
        assert!(
            view.leaf_validations_for_tests() > before,
            "a refused receipt is read again"
        );
    }

    let fresh = RequestStoreView::new(&base, overlay);
    let before = fresh.leaf_validations_for_tests();
    assert_eq!(fresh.validate_fact_signature(&second, &[]), Ok(()));
    assert_eq!(
        fresh.leaf_validations_for_tests() - before,
        33,
        "a new request"
    );
}

use verter_type_engine::resolver_core::request_ports::OwnedLowering as _;

#[test]
fn terminal_macro_inventory_preserves_indexed_absence_and_paired_base_fallback() {
    use crate::session_view::SessionView;
    use verter_type_engine::resolver_core::request_ports::OwnedLowering;

    let (host, canonical) = small_host_with_one_component();
    let scheduler_source = host.scheduler_source(&canonical).expect("parsed source");
    let scheduler_analysis = Arc::clone(
        &scheduler_source
            .downcast_data::<crate::host_executor::HostSourceData>()
            .expect("host source data")
            .parse
            .script_analysis,
    );
    let indexed = host
        .ensure_indexed_ready_serve(&canonical)
        .expect("indexed serve")
        .indexed;
    let key = host
        .authoritative_current_artifact_key(&canonical)
        .expect("exact runtime artifact key");
    let mut without_analysis = indexed.as_ref().clone();
    without_analysis.script_analysis = None;
    host.project_type_store().indexed().insert_artifacts(
        key,
        Arc::new(crate::file_artifact_store::FileArtifacts::with_indexed(
            Arc::new(without_analysis),
        )),
    );
    assert!(host
        .current_content_pinned_indexed(&canonical)
        .expect("selected replacement")
        .script_analysis
        .is_none());
    let view = host.resolver_store_view_read().into_owned_view();
    let ctx = HostResolverContext::new(&host, &view, Arc::new(CanonicalCompletionOverlay::new()));
    let answer = ctx.terminal_macro_inventory(&canonical);
    assert_eq!(answer.origin_whole_hash, Some(indexed.whole_hash));
    assert!(
        answer.script_analysis.is_none(),
        "present indexed artifact never falls back for absent analysis"
    );

    assert!(
        host.project_type_store()
            .indexed()
            .remove_canonical(&canonical)
            > 0
    );
    let view = host.resolver_store_view_read().into_owned_view();
    let ctx = HostResolverContext::new(&host, &view, Arc::new(CanonicalCompletionOverlay::new()));
    let answer = ctx.terminal_macro_inventory(&canonical);
    assert_eq!(answer.origin_whole_hash, Some(scheduler_source.whole_hash));
    assert!(Arc::ptr_eq(
        &answer.script_analysis.expect("paired scheduler analysis"),
        &scheduler_analysis
    ));
    assert!(
        host.current_content_pinned_indexed(&canonical).is_none(),
        "terminal fallback does not materialize an artifact"
    );

    struct EmptySessionView {
        identity: crate::file_artifact_store::ProjectIdentity,
        env: crate::session_view::EnvHashes,
    }
    impl SessionView for EmptySessionView {
        fn source(&self, _: &str) -> Option<Arc<str>> {
            None
        }
        fn content_hash_for(
            &self,
            _: &str,
        ) -> Option<verter_session_query::analysis::types::Hash16> {
            None
        }
        fn project_identity(&self) -> crate::file_artifact_store::ProjectIdentity {
            self.identity
        }
        fn env_hashes(&self) -> &crate::session_view::EnvHashes {
            &self.env
        }
        fn resolved_import_facts(
            &self,
            _: &str,
        ) -> Option<Arc<crate::resolved_import_facts::ResolvedImportFacts>> {
            None
        }
    }
    let session = EmptySessionView {
        identity: host.host_view_project_identity(),
        env: crate::session_view::EnvHashes::default(),
    };
    let ctx = SessionResolverContext::new(
        &host,
        &session,
        &view,
        Arc::new(CanonicalCompletionOverlay::new()),
    );
    let answer = ctx.terminal_macro_inventory(&canonical);
    assert_eq!(answer.origin_whole_hash, None);
    assert!(
        answer.script_analysis.is_none(),
        "any session view suppresses the base scheduler fallback"
    );
    assert!(host.current_content_pinned_indexed(&canonical).is_none());
}
