//! The request-world prepared-decl bundle memo — discriminating
//! regression tests.
//!
//! ## The behaviour these tests pin
//!
//! A prepared-decl bundle that the SHARED cache cannot hold still costs a
//! full cold materialisation on every touch. Two classes hit this:
//!
//! * an overlay-bearing bundle (R17: the shared slot is keyed by
//!   canonical alone and would alias the base bundle);
//! * a `RequestOnly` bundle, whose materialisation consumed a
//!   deterministic non-cacheable read (a FENCED serve, an unrootable
//!   import-route witness), so the shared admission gate declines it.
//!
//! Both are COMPLETE and deterministic under the request's immutable
//! view. [`RequestBundleMemo`](crate::resolver_core::request_store_view::RequestBundleMemo)
//! is the request-scoped home for exactly those values. It lives on the
//! request-scoped
//! [`CanonicalCompletionOverlay`](crate::resolver_core::CanonicalCompletionOverlay)
//! (created once per top-level request, threaded into every resolver
//! context the request builds, dropped with the request) and is keyed by
//! `(canonical, world)` with the
//! [`StoreViewCompatToken`](crate::resolver_core::StoreViewCompatToken)
//! on the entry:
//!
//! - the world (`Base` / `Overlay(content hash)`) keeps the two
//!   namespaces distinct, so a base consumer is never served the
//!   session's edit and vice versa;
//! - the compat token pins entries to ONE externally-coherent base-world
//!   snapshot (the same complete validity oracle singleflight lanes
//!   coalesce on), so a retry attempt after an external supersession
//!   NEVER reuses a bundle materialised against the superseded world.
//!
//! Admission is STRUCTURAL: `RequestBundleMemo::insert` itself refuses
//! anything that is not request-reusable, so a cancelled, partial,
//! lease-missed, mutation-unstable or overflow-refused materialisation
//! cannot be memoised even by a caller that asks. A `RequestOnly` entry
//! replays its stored refusal on EVERY hit, so reuse never launders the
//! taint the cold return carried.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::resolver_core::request_store_view::BundleMemoWorld;
use crate::resolver_core::reuse::ReuseClass;
use crate::resolver_core::{
    CanonicalCompletionOverlay, ResolverContext, SessionResolverContext, StoreView,
};
use crate::session_view::{OverlaidView, SessionView};
use crate::{HostConfig, VerterHost};

const OWNER: &str = "/proj/owner.ts";
const DEP: &str = "/proj/dep.ts";
const UNRELATED: &str = "/proj/unrelated.ts";

const BASE_OWNER: &str = "import { Dep } from './dep';\nexport interface Foo { a: Dep; }\n";
const BASE_DEP: &str = "export interface Dep { x: number }\n";
const OVERLAY_OWNER_A: &str =
    "import { Dep } from './dep';\nexport interface Foo { a: Dep; b: string; }\n";
const OVERLAY_OWNER_B: &str =
    "import { Dep } from './dep';\nexport interface FooChanged { c: boolean; }\n";

fn upsert_base(host: &VerterHost, canonical: &str, source: &str) {
    let result = host.upsert(crate::UpsertRequest {
        canonical_id: Some(canonical.to_string()),
        input_id: canonical.to_string(),
        source: Arc::from(source),
        file_language: crate::LanguageRegistry::global()
            .classify_static(canonical)
            .static_resolution(),
        aliases: Vec::new(),
    });
    assert!(
        result.is_ok(),
        "base upsert of {canonical} failed: {:?}",
        result.err()
    );
}

/// Base workspace: an owner with a resolvable surface + one import, and
/// the imported dep.
fn host_with_base_files() -> Arc<VerterHost> {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_base(&host, OWNER, BASE_OWNER);
    upsert_base(&host, DEP, BASE_DEP);
    Arc::new(host)
}

fn overlaid_view(host: &Arc<VerterHost>, canonical: &str, source: &str) -> OverlaidView {
    let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    overlays.insert(canonical.to_string(), Arc::from(source));
    OverlaidView::new(Arc::clone(host), overlays)
}

/// One "request": an owned session-rooted store view + a fresh
/// request-scoped completion overlay. The returned pieces are what a
/// `SessionResolverContext` borrows for the request's lifetime.
fn request_pieces(
    host: &Arc<VerterHost>,
    view: &dyn SessionView,
) -> (
    crate::resolver_store::HostStoreView,
    Arc<CanonicalCompletionOverlay>,
) {
    let store_view = host
        .resolver_store_view_read()
        .into_owned_view()
        .with_session_overlay(host, view);
    (store_view, Arc::new(CanonicalCompletionOverlay::new()))
}

/// (a) Same request + same overlay: the second bundle read returns the
/// SAME `Arc` (memo hit) — the per-touch re-materialisation is gone.
///
/// Pre-memo this FAILS: every overlay-branch read materialised a fresh
/// bundle (`Arc::new` per call), so the two reads are never `ptr_eq`.
#[test]
fn overlay_bundle_computes_once_sequentially_per_request_world() {
    let host = host_with_base_files();
    let view = overlaid_view(&host, OWNER, OVERLAY_OWNER_A);
    let overlay_hash = view
        .overlay_content_hash_for(OWNER)
        .expect("view must report an overlay content hash for the masked owner");
    let (store_view, overlay) = request_pieces(&host, &view);
    let ctx = SessionResolverContext::new(&host, &view, &store_view, Arc::clone(&overlay));

    let first = ResolverContext::prepared_decl_bundle(&ctx, OWNER)
        .expect("overlay-bearing bundle must materialise");
    assert_eq!(
        first.owner_whole_hash, overlay_hash,
        "the overlay-bearing bundle is built from the OVERLAY content",
    );
    assert!(
        first
            .prepared_type_decls
            .get("Foo")
            .expect("Foo preparation should succeed")
            .is_some(),
        "the overlay bundle carries the owner's prepared type decl",
    );

    let second = ResolverContext::prepared_decl_bundle(&ctx, OWNER)
        .expect("second read must serve the bundle");
    assert!(
        Arc::ptr_eq(&first, &second),
        "the second overlay bundle read within the SAME request must be a \
         request-scoped memo hit (Arc::ptr_eq), not a re-materialisation",
    );
}

/// The prepared-bundle memo is not validation-visible shadowing. A
/// memo-only overlay therefore remains the parent population even though
/// the memo itself is non-empty.
#[test]
fn memo_only_population_remains_empty() {
    let host = host_with_base_files();
    let view = overlaid_view(&host, OWNER, OVERLAY_OWNER_A);
    let overlay_hash = view
        .overlay_content_hash_for(OWNER)
        .expect("session overlay hash");
    let (store_view, producer_overlay) = request_pieces(&host, &view);
    let ctx = SessionResolverContext::new(&host, &view, &store_view, producer_overlay);
    let bundle = ResolverContext::prepared_decl_bundle(&ctx, OWNER).expect("prepared bundle");

    let memo_only = CanonicalCompletionOverlay::new();
    memo_only.bundle_memo().insert(
        OWNER,
        BundleMemoWorld::Overlay(overlay_hash),
        store_view.compat_token(),
        ReuseClass::Shared,
        bundle,
    );

    assert_eq!(memo_only.bundle_memo().len_for_tests(), 1);
    assert_eq!(
        memo_only.completion_state_for_tests(),
        verter_workspace::CompletionOverlayState::Empty,
        "memo contents never affect fact validation and must not partition aggregate reuse"
    );
}

/// (b) A NEW request after an overlay content change observes the NEW
/// overlay content — the memo never leaks across requests.
#[test]
fn fresh_request_observes_new_overlay_content() {
    let host = host_with_base_files();

    // Request 1 under overlay content A.
    let view_a = overlaid_view(&host, OWNER, OVERLAY_OWNER_A);
    let hash_a = view_a.overlay_content_hash_for(OWNER).unwrap();
    let (store_view_a, overlay_a) = request_pieces(&host, &view_a);
    let bundle_a = {
        let ctx = SessionResolverContext::new(&host, &view_a, &store_view_a, overlay_a);
        ResolverContext::prepared_decl_bundle(&ctx, OWNER)
            .expect("request 1 bundle must materialise")
    };
    assert_eq!(bundle_a.owner_whole_hash, hash_a);

    // Request 2 under overlay content B (fresh view + fresh completion
    // overlay — a genuinely new request).
    let view_b = overlaid_view(&host, OWNER, OVERLAY_OWNER_B);
    let hash_b = view_b.overlay_content_hash_for(OWNER).unwrap();
    assert_ne!(hash_a, hash_b, "fixture invariant: contents A and B differ");
    let (store_view_b, overlay_b) = request_pieces(&host, &view_b);
    let bundle_b = {
        let ctx = SessionResolverContext::new(&host, &view_b, &store_view_b, overlay_b);
        ResolverContext::prepared_decl_bundle(&ctx, OWNER)
            .expect("request 2 bundle must materialise")
    };

    assert!(
        !Arc::ptr_eq(&bundle_a, &bundle_b),
        "a new request must never be served the previous request's bundle",
    );
    assert_eq!(
        bundle_b.owner_whole_hash, hash_b,
        "the new request's bundle reflects the NEW overlay content",
    );
    assert!(
        bundle_b
            .prepared_type_decls
            .get("FooChanged")
            .expect("FooChanged preparation should succeed")
            .is_some(),
        "the new request's bundle carries the NEW overlay's declaration",
    );
    assert!(
        bundle_b
            .prepared_type_decls
            .get("Foo")
            .expect("Foo lookup should not fail")
            .is_none(),
        "the new request's bundle must NOT carry the OLD overlay's declaration",
    );
}

/// (b2) A NEW request with the SAME overlay content re-materialises: the
/// memo dies with its request (no cross-request bundle sharing even when
/// the key would match).
#[test]
fn fresh_request_same_overlay_content_rematerializes() {
    let host = host_with_base_files();

    let view_1 = overlaid_view(&host, OWNER, OVERLAY_OWNER_A);
    let (store_view_1, overlay_1) = request_pieces(&host, &view_1);
    let bundle_1 = {
        let ctx = SessionResolverContext::new(&host, &view_1, &store_view_1, overlay_1);
        ResolverContext::prepared_decl_bundle(&ctx, OWNER).expect("request 1 bundle")
    };

    let view_2 = overlaid_view(&host, OWNER, OVERLAY_OWNER_A);
    let (store_view_2, overlay_2) = request_pieces(&host, &view_2);
    let bundle_2 = {
        let ctx = SessionResolverContext::new(&host, &view_2, &store_view_2, overlay_2);
        ResolverContext::prepared_decl_bundle(&ctx, OWNER).expect("request 2 bundle")
    };

    assert!(
        !Arc::ptr_eq(&bundle_1, &bundle_2),
        "the memo is request-scoped: a fresh request (fresh completion \
         overlay) re-materialises even for identical overlay content",
    );
}

/// (c) `RM-3` / namespace isolation — the base and overlay worlds are
/// SEPARATE memo namespaces.
///
/// The same request overlays the OWNER and leaves DEP base-served. Both
/// canonicals enter the memo, in DIFFERENT worlds, and neither is served
/// out of the other's namespace. Collapsing the two namespaces would
/// serve a base consumer the session's uncommitted edit inside the same
/// request — the reason the world discriminant exists.
#[test]
fn base_and_overlay_bundle_memos_are_isolated() {
    let host = host_with_base_files();
    let view = overlaid_view(&host, OWNER, OVERLAY_OWNER_A);
    let overlay_hash = view
        .overlay_content_hash_for(OWNER)
        .expect("the view must report an overlay content hash for the masked owner");
    let (store_view, overlay) = request_pieces(&host, &view);
    let ctx = SessionResolverContext::new(&host, &view, &store_view, Arc::clone(&overlay));

    let dep_first = ResolverContext::prepared_decl_bundle(&ctx, DEP)
        .expect("base-path bundle must serve for the non-overlaid dep");
    let dep_second = ResolverContext::prepared_decl_bundle(&ctx, DEP)
        .expect("base-path bundle must serve again");
    let owner_bundle = ResolverContext::prepared_decl_bundle(&ctx, OWNER)
        .expect("overlay-path bundle must serve for the masked owner");

    assert!(dep_first
        .prepared_type_decls
        .get("Dep")
        .expect("first Dep preparation should succeed")
        .is_some());
    assert!(
        Arc::ptr_eq(&dep_first, &dep_second),
        "the base-path canonical is memoised too — the second read must serve the SAME \
         value rather than re-validating and re-materialising",
    );

    let memo = overlay.bundle_memo();
    assert_eq!(
        memo.len_in_world_for_tests(BundleMemoWorld::Base),
        1,
        "the non-overlaid dep occupies the BASE namespace",
    );
    assert_eq!(
        memo.len_in_world_for_tests(BundleMemoWorld::Overlay(overlay_hash)),
        1,
        "the overlaid owner occupies its OWN overlay namespace",
    );
    assert_eq!(
        memo.len_for_tests(),
        2,
        "two canonicals, two worlds, two entries — a shared namespace would have \
         collided them",
    );

    assert_eq!(
        owner_bundle.owner_whole_hash, overlay_hash,
        "the owner's memoised bundle is the OVERLAY one, never the base one",
    );
    assert_ne!(
        dep_first.owner_whole_hash, overlay_hash,
        "and the dep's is the base one",
    );
}

/// (d) A session-tombstoned canonical is never memoised (and yields no
/// bundle: the session deleted the file).
#[test]
fn tombstoned_canonical_is_not_memoized() {
    let host = host_with_base_files();

    let overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    let overlay_hashes: FxHashMap<String, crate::types::Hash16> = FxHashMap::default();
    let mut tombstones: std::collections::HashSet<String> = std::collections::HashSet::new();
    tombstones.insert(OWNER.to_string());
    let view =
        crate::session_view::OverlaidViewRef::new(&host, &overlays, &overlay_hashes, &tombstones);

    let store_view = host
        .resolver_store_view_read()
        .into_owned_view()
        .with_session_overlay(&host, &view);
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let ctx = SessionResolverContext::new(&host, &view, &store_view, Arc::clone(&overlay));

    let bundle = ResolverContext::prepared_decl_bundle(&ctx, OWNER);
    assert!(
        bundle.is_none(),
        "a session-tombstoned canonical has no current content — no bundle",
    );
    assert_eq!(
        overlay.bundle_memo().len_for_tests(),
        0,
        "the tombstone branch must never populate the request bundle memo",
    );
}

/// (e) An EXTERNAL supersession between two view snapshots inside the
/// same request-overlay lifetime forces a memo MISS: the compat token in
/// the memo key differs, so the second context re-materialises against
/// its fresh view instead of reusing a bundle whose import
/// canonicalization walked the superseded world.
///
/// This is the retry-attempt safety rail: `run_stable_request` retries
/// re-snapshot the base view while the request-scoped completion overlay
/// (and thus the memo) is shared across attempts.
#[test]
fn external_supersession_between_snapshots_misses_memo() {
    let host = host_with_base_files();
    let view = overlaid_view(&host, OWNER, OVERLAY_OWNER_A);

    // Attempt 1.
    let (store_view_1, overlay) = request_pieces(&host, &view);
    let bundle_1 = {
        let ctx = SessionResolverContext::new(&host, &view, &store_view_1, Arc::clone(&overlay));
        ResolverContext::prepared_decl_bundle(&ctx, OWNER).expect("attempt 1 bundle")
    };

    // External mutation: a base upsert moves the external-supersession
    // dimensions (store-view epoch), so a fresh snapshot carries a
    // different compat token.
    upsert_base(&host, UNRELATED, "export const unrelated = 1;\n");

    // Attempt 2: fresh base snapshot, SAME session view, SAME
    // request-scoped completion overlay (the retry shape).
    let store_view_2 = host
        .resolver_store_view_read()
        .into_owned_view()
        .with_session_overlay(&host, &view);
    {
        use crate::resolver_core::StoreView;
        assert_ne!(
            store_view_1.compat_token(),
            store_view_2.compat_token(),
            "fixture invariant: the base upsert must move the compat token",
        );
    }
    let bundle_2 = {
        let ctx = SessionResolverContext::new(&host, &view, &store_view_2, Arc::clone(&overlay));
        ResolverContext::prepared_decl_bundle(&ctx, OWNER).expect("attempt 2 bundle")
    };

    assert!(
        !Arc::ptr_eq(&bundle_1, &bundle_2),
        "a retry attempt under a fresh (externally-moved) view must NOT \
         reuse the superseded attempt's memoised bundle — the compat token \
         keys the memo to one externally-coherent world",
    );
}

/// (e2) The same retry shape, superseded by a RESOLUTION retarget.
///
/// `set_exact_resolutions` retargets the owner's `./dep` edge and moves
/// NOTHING else — not the store-view epoch, not the project or content
/// generation, not the env fold or identity. Only the resolution-fact
/// generation advances. So this is the case (e) cannot reach: if that
/// dimension is left out of the external-supersession fold, the two
/// attempts share a compat token, the retry HITS, and the memo re-serves a
/// bundle whose resolved import edges the retarget just invalidated —
/// defeating the retry the memo exists to make possible.
#[test]
fn resolution_retarget_between_snapshots_misses_memo() {
    let host = host_with_base_files();
    let view = overlaid_view(&host, OWNER, OVERLAY_OWNER_A);

    let (store_view_1, overlay) = request_pieces(&host, &view);
    let bundle_1 = {
        let ctx = SessionResolverContext::new(&host, &view, &store_view_1, Arc::clone(&overlay));
        ResolverContext::prepared_decl_bundle(&ctx, OWNER).expect("attempt 1 bundle")
    };

    let applied = host.ws().set_exact_resolutions(
        OWNER,
        vec![verter_workspace::ExactResolution {
            specifier: "./dep".to_string(),
            phase: verter_workspace::ResolvePhase::ProviderGraph,
            kind: verter_workspace::ResolveRequestKind::EsmImport,
            resolved_canonical_id: Some(UNRELATED.to_string()),
            possible_canonical_ids: vec![UNRELATED.to_string()],
        }],
    );
    assert!(
        applied.changed,
        "fixture invariant: the retarget must change workspace state"
    );

    let store_view_2 = host
        .resolver_store_view_read()
        .into_owned_view()
        .with_session_overlay(&host, &view);
    {
        let token_1 = store_view_1.validation_token();
        let token_2 = store_view_2.validation_token();
        assert_eq!(
            (
                token_1.store_view_epoch,
                token_1.project_generation,
                token_1.content_generation,
                token_1.env_hash_fold,
                token_1.project_identity,
                token_1.overlay_identity,
            ),
            (
                token_2.store_view_epoch,
                token_2.project_generation,
                token_2.content_generation,
                token_2.env_hash_fold,
                token_2.project_identity,
                token_2.overlay_identity,
            ),
            "fixture invariant: an exact retarget must move NO other \
             external dimension — otherwise this degenerates into case (e)"
        );
        assert_ne!(
            token_1.resolution_fact_generation, token_2.resolution_fact_generation,
            "fixture invariant: the retarget must mint a resolution fact version"
        );
        use crate::resolver_core::StoreView;
        assert_ne!(
            store_view_1.compat_token(),
            store_view_2.compat_token(),
            "the compat token must move on a resolution retarget: it folds \
             the external-supersession dimensions, and after the retarget \
             the two views are NOT validation-equivalent",
        );
    }

    let bundle_2 = {
        let ctx = SessionResolverContext::new(&host, &view, &store_view_2, Arc::clone(&overlay));
        ResolverContext::prepared_decl_bundle(&ctx, OWNER).expect("attempt 2 bundle")
    };

    assert!(
        !Arc::ptr_eq(&bundle_1, &bundle_2),
        "a retry attempt after a resolution retarget must NOT reuse the \
         pre-retarget memoised bundle — its resolved import edges were \
         computed against the world the retarget replaced",
    );
}

/// (f) An unattributed refusal is returned to its caller but is never
/// memoised. The seam marks the cacheability scope directly, without
/// recording a typed reason in the refusal observer, which forces the
/// conservative `NoReuse(UnattributedRefusal)` class.
#[test]
fn unattributed_refusal_is_not_memoized() {
    let host = host_with_base_files();
    *host.materialize_seam_hook.lock() = Some(Arc::new(|| {
        crate::resolver_core::resolver_context::note_non_cacheable_propagation(
            verter_workspace::NonCacheablePropagation::Transitive,
        );
    }));

    let view = overlaid_view(&host, OWNER, OVERLAY_OWNER_A);
    let overlay_hash = view
        .overlay_content_hash_for(OWNER)
        .expect("session overlay hash");
    let (store_view, overlay) = request_pieces(&host, &view);
    let ctx = SessionResolverContext::new(&host, &view, &store_view, Arc::clone(&overlay));

    let first = ResolverContext::prepared_decl_bundle(&ctx, OWNER)
        .expect("an unattributed refusal still serves its caller");
    *host.materialize_seam_hook.lock() = None;
    assert_eq!(
        overlay
            .bundle_memo()
            .len_in_world_for_tests(BundleMemoWorld::Overlay(overlay_hash)),
        0,
        "the typed gate must keep the refused owner out of the memo; unrelated base-world \
         dependencies may have their own entries",
    );

    let second = ResolverContext::prepared_decl_bundle(&ctx, OWNER)
        .expect("the second read re-materialises");

    assert!(
        !Arc::ptr_eq(&first, &second),
        "an unattributed refusal must NOT be memoised — the second \
         read re-materialises per-call",
    );
}

/// (g) END-TO-END WIRING: a real session-view component-meta request —
/// the production `resolve_component_meta_with_view` boundary that
/// builds the `ViewBoundRequestHost` and threads ITS request-scoped
/// completion overlay into every resolver context — hits the memo
/// (provenance `bundle_request_memo_hits` moves). This pins the memo to
/// the benchmark's actual flow (compat checker `updateFile` overlays +
/// `getComponentMeta`), not only to hand-built contexts.
#[test]
fn session_view_component_meta_request_hits_the_request_bundle_memo() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_base(
        &host,
        "/proj/types.ts",
        "export interface Props { a: number; b: string }\n",
    );
    upsert_base(
        &host,
        "/proj/Comp.vue",
        "<script setup lang=\"ts\">\nimport type { Props } from './types';\ndefineProps<Props>();\n</script>\n<template><div/></template>\n",
    );
    let host = Arc::new(host);

    // Session overlays BOTH the owner SFC and its imported types file —
    // the meta-ui checker shape (every component + helper upserted as a
    // session overlay).
    let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    overlays.insert(
        "/proj/Comp.vue".to_string(),
        Arc::from(
            "<script setup lang=\"ts\">\nimport type { Props } from './types';\ndefineProps<Props>();\n</script>\n<template><span/></template>\n",
        ),
    );
    overlays.insert(
        "/proj/types.ts".to_string(),
        Arc::from("export interface Props { a: number; b: string; c: boolean }\n"),
    );
    let view = OverlaidView::new(Arc::clone(&host), overlays);

    let hits_before = host
        .provenance()
        .bundle_request_memo_hits
        .load(std::sync::atomic::Ordering::Relaxed);

    let resolved = host.resolve_component_meta_with_view(
        "/proj/Comp.vue",
        crate::types::ProjectionMode::Expanded,
        &view,
    );
    assert!(
        resolved.is_some(),
        "the session-view component-meta request must resolve",
    );

    let hits_after = host
        .provenance()
        .bundle_request_memo_hits
        .load(std::sync::atomic::Ordering::Relaxed);
    assert!(
        hits_after > hits_before,
        "a real session-view component-meta request must serve at least one \
         overlay prepared-decl bundle read from the request-scoped memo \
         (hits before={hits_before} after={hits_after}) — zero hits means \
         the ViewBoundRequestHost's completion overlay is not reaching the \
         producer",
    );
}

// ---------------------------------------------------------------------
// The BASE world.
//
// The overlay cases above are keyed on `Arc::ptr_eq` because the overlay
// branch never enters the singleflight lane, so `bundle_cold_flight_runs`
// does not move for it at all. The BASE branch is the one where that
// counter is genuinely the oracle — but only for a value the SHARED
// cache is not allowed to hold. For a clean (`Shared`) base bundle the
// shared cache already serves every later touch at zero cold flights, so
// the flight counter cannot discriminate the memo there and the
// `bundle_request_memo_hits` counter is the oracle instead. Each test
// below states which oracle it is using and why.
// ---------------------------------------------------------------------

use std::sync::atomic::Ordering;

const BASE_ROOT: &str = "/rc_base_memo";

fn cold_flight_runs(host: &VerterHost) -> u64 {
    host.provenance()
        .bundle_cold_flight_runs
        .load(Ordering::Relaxed)
}

fn request_memo_hits(host: &VerterHost) -> u64 {
    host.provenance()
        .bundle_request_memo_hits
        .load(Ordering::Relaxed)
}

/// A base-only host: an owner with a resolvable surface plus one import.
fn base_host(root: &str) -> (Arc<VerterHost>, String) {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_base(&host, &format!("{root}/dep.ts"), BASE_DEP);
    let owner = format!("{root}/owner.ts");
    upsert_base(&host, &owner, BASE_OWNER);
    (Arc::new(host), owner)
}

/// One base touch through the request `memo`, bracketed by a cacheability
/// scope. Returns `(served, enclosing_scope_is_non_cacheable, flights)`.
fn base_touch(
    host: &Arc<VerterHost>,
    memo: &CanonicalCompletionOverlay,
    owner: &str,
) -> (
    Option<Arc<crate::resolver_core::prepared_decl::PreparedDeclBundle>>,
    bool,
    u64,
) {
    let view = host.resolver_store_view_read().into_owned_view();
    let before = cold_flight_runs(host);
    let (bundle, non_cacheable) = crate::fact_signature_helpers::with_cacheability_scope(
        &crate::fact_signature_helpers::FactTracerBasisSource::unbound(host.as_ref()),
        |_probe| host.prepared_decl_bundle_with_store_view(&view, Some(memo.bundle_memo()), owner),
    );
    (bundle, non_cacheable, cold_flight_runs(host) - before)
}

/// A published owner whose import witness takes the typed refusal arm.
/// Decision-DAG contract tests cover the upstream refusal producers;
/// this fixture isolates the stable, joinable `RequestOnly` value that
/// the session memo may retain but the shared cache may not.
fn refused_resolution_bundle_host(root: &str) -> (Arc<VerterHost>, String) {
    let host = VerterHost::new_standalone(HostConfig::default());
    let owner = format!("{root}/owner.ts");
    upsert_base(
        &host,
        &owner,
        "import type { Missing } from './missing';\n\
         export interface Wrapper { inner: Missing }\n",
    );
    host.test_force
        .force_import_route_witness_refusal_for_tests
        .store(true, Ordering::Relaxed);
    let host = Arc::new(host);
    assert!(
        host.owner_import_route_witness_for_tests(&owner).is_none(),
        "fixture invariant: the forced publication refusal must decline the durable \
         import witness"
    );
    assert!(
        host.resolver
            .runtime
            .prepared_decl_bundles
            .candidate_signatures_for_key(&owner)
            .is_empty(),
        "fixture invariant: nothing is shared-admitted for this owner, so the request \
         memo is the ONLY tier that can serve a later touch"
    );
    (host, owner)
}

/// `RM-1` / `PD-1` — a clean BASE bundle computes once per immutable
/// request world.
///
/// **Oracle note.** `bundle_cold_flight_runs` is NOT the discriminator
/// here: a `Shared` bundle is admitted to `prepared_decl_bundles`, so the
/// pre-memo tree already served touches 2..n at zero cold flights. The
/// memo's contribution is skipping the per-touch fact revalidation, and
/// `bundle_request_memo_hits` is what measures it. Both are asserted, so
/// a change that reintroduces per-touch cold flights also fails.
#[test]
fn base_bundle_computes_once_sequentially_per_request_world() {
    let (host, owner) = base_host(&format!("{BASE_ROOT}_clean"));
    let request = CanonicalCompletionOverlay::new();

    let hits_before = request_memo_hits(&host);
    let mut flights = Vec::new();
    let mut bundles = Vec::new();
    for _ in 0..3 {
        let (bundle, non_cacheable, ran) = base_touch(&host, &request, &owner);
        assert!(
            !non_cacheable,
            "control invariant: a clean base bundle must leave its reader's tracer clean"
        );
        bundles.push(bundle.expect("every touch must be served"));
        flights.push(ran);
    }
    let hits = request_memo_hits(&host) - hits_before;

    assert_eq!(
        flights,
        vec![1, 0, 0],
        "the first touch runs the cold flight and no later touch runs one"
    );
    assert_eq!(
        hits, 2,
        "`RM-1`: touches 2 and 3 are served by the REQUEST memo — the tier that makes \
         'computes once per immutable request world' true independently of whether the \
         shared cache happens to hold the value"
    );
    assert!(
        Arc::ptr_eq(&bundles[0], &bundles[1]) && Arc::ptr_eq(&bundles[1], &bundles[2]),
        "every touch inside one request world serves the SAME value"
    );
    assert_eq!(
        request
            .bundle_memo()
            .len_in_world_for_tests(BundleMemoWorld::Base),
        1,
        "one canonical, one base-world entry"
    );
}

/// `RM-2` — a request-memo HIT replays the stored refusal into the
/// enclosing tracer's signature.
///
/// The cold touch's `note_non_cacheable_read_fan_out(FencedServe)` fired
/// inside the FIRST touch's cacheability scope. A later touch runs no
/// materialisation at all, so without the replay its own scope reads
/// clean and its enclosing compute may warm a shared cache with a value
/// derived from a superseded artifact.
#[test]
fn request_only_memo_hit_replays_taint_to_enclosing_signature() {
    let (host, owner) = base_host(&format!("{BASE_ROOT}_taint"));
    // Materialise the artifact first so the force below fences the SERVE
    // rather than the build.
    let _ = host.ensure_indexed_ready(&owner);
    host.test_force
        .force_indexed_ready_serve_fence_for_tests
        .store(true, Ordering::Relaxed);

    let request = CanonicalCompletionOverlay::new();
    let hits_before = request_memo_hits(&host);
    let (cold_bundle, cold_non_cacheable, cold_flights) = base_touch(&host, &request, &owner);
    let (warm_bundle, warm_non_cacheable, warm_flights) = base_touch(&host, &request, &owner);
    let hits = request_memo_hits(&host) - hits_before;

    host.test_force
        .force_indexed_ready_serve_fence_for_tests
        .store(false, Ordering::Relaxed);

    assert!(
        cold_bundle.is_some() && warm_bundle.is_some(),
        "both served"
    );
    assert_eq!(cold_flights, 1, "sanity: the first touch is the cold one");
    assert_eq!(warm_flights, 0, "sanity: the second touch is a memo hit");
    assert_eq!(hits, 1, "sanity: exactly one memo hit was served");
    assert!(
        cold_non_cacheable,
        "sanity: the cold touch consumed the fenced serve"
    );
    assert!(
        warm_non_cacheable,
        "`RM-2`: the MEMO HIT must replay the stored refusal into ITS OWN enclosing \
         tracer. The cold touch's fan-out fired in the cold touch's scope; a memo hit \
         that returns without replaying launders the taint and lets the enclosing \
         compute warm a shared cache with a superseded-basis value."
    );
}

/// `RM-1` / `RM-2` — a deterministic `RequestOnly` bundle computes once
/// per request world and replays on every touch. This three-touch profile
/// complements the single-hit assertion above.
#[test]
fn request_only_bundle_computes_once_and_replays_each_touch() {
    let (host, owner) = refused_resolution_bundle_host("/rc_base_memo_refused_replay");
    let owner = owner.as_str();

    let request = CanonicalCompletionOverlay::new();
    let mut flights = Vec::new();
    let mut taints = Vec::new();
    for _ in 0..3 {
        let (bundle, non_cacheable, ran) = base_touch(&host, &request, owner);
        assert!(bundle.is_some(), "every touch is still SERVED");
        flights.push(ran);
        taints.push(non_cacheable);
    }

    assert_eq!(
        flights,
        vec![1, 0, 0],
        "`RM-1`: one cold flight per immutable request world"
    );
    assert_eq!(
        taints,
        vec![true, true, true],
        "`RM-2`: EVERY touch — cold and memoised alike — marks its enclosing tracer \
         non-cacheable"
    );
    assert!(
        host.resolver
            .runtime
            .prepared_decl_bundles
            .candidate_signatures_for_key(&owner.to_string())
            .is_empty(),
        "`RM-3`: request-scoped reuse never becomes shared publication"
    );
}

/// `RM-3` — a materialisation whose basis includes a TRANSIENT refusal (a
/// broken decl-body lease) is served to its caller and NEVER
/// request-memoised.
///
/// A lease miss is recoverable on a later demand under a live lease.
/// Freezing the degraded answer for the rest of the request would turn a
/// transient miss into a permanent one, which is exactly why the reuse
/// rail splits deterministic from transient refusals instead of treating
/// "non-cacheable" as one bucket.
#[test]
fn lease_missed_bundle_is_not_request_memoized() {
    assert_transient_refusal_is_not_memoized(
        "/rc_base_memo_lease",
        crate::resolver_core::resolver_context::NonCacheableReadReason::LeaseMiss,
    );
}

/// `RM-3` — the budget/partial sibling of the lease-miss case: a
/// safety-budget stop leaves the result degraded rather than definite, so
/// it is never request-memoised either.
#[test]
fn cancelled_or_partial_bundle_is_not_request_memoized() {
    assert_transient_refusal_is_not_memoized(
        "/rc_base_memo_budget",
        crate::resolver_core::resolver_context::NonCacheableReadReason::InferenceBudgetExceeded,
    );
}

/// Drive a REAL cold bundle materialisation whose window fans `reason`
/// through the REAL production marking chokepoint, and assert the memo
/// refuses it — with an anti-vacuity control proving the same shape IS
/// memoised without the refusal.
fn assert_transient_refusal_is_not_memoized(
    root: &str,
    reason: crate::resolver_core::resolver_context::NonCacheableReadReason,
) {
    let (host, owner) = base_host(root);
    // The seam fires inside the IndexedReady materialise flight, which
    // runs inside the bundle's cold flight body — so the refusal lands in
    // the bundle producer's own observation scope, exactly as a real
    // nested producer's would.
    *host.materialize_seam_hook.lock() = Some(Arc::new(move || {
        crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(reason);
    }));

    let request = CanonicalCompletionOverlay::new();
    let (bundle, non_cacheable, flights) = base_touch(&host, &request, &owner);
    *host.materialize_seam_hook.lock() = None;

    assert!(
        bundle.is_some(),
        "a transient refusal still SERVES its caller — the refusal is about reuse, never \
         about the answer"
    );
    assert_eq!(flights, 1, "sanity: the touch ran a genuine cold flight");
    assert!(
        non_cacheable,
        "sanity: the planted refusal reached the enclosing tracer"
    );
    assert_eq!(
        request.bundle_memo().len_for_tests(),
        0,
        "`RM-3`: a {reason:?} basis is NOT request-reusable — memoising it would freeze a \
         recoverable miss for the rest of the request"
    );

    // Anti-vacuity control: the SAME owner shape, SAME harness, no
    // refusal. Without it, a memo that never admitted anything would
    // satisfy the assertion above.
    let (control_host, control_owner) = base_host(&format!("{root}_control"));
    let control_request = CanonicalCompletionOverlay::new();
    let (control_bundle, control_non_cacheable, _) =
        base_touch(&control_host, &control_request, &control_owner);
    assert!(control_bundle.is_some(), "the control must be served");
    assert!(
        !control_non_cacheable,
        "control invariant: the unplanted materialisation must stay clean"
    );
    assert_eq!(
        control_request.bundle_memo().len_for_tests(),
        1,
        "the control proves the memo DOES admit this shape when nothing refuses it"
    );
}

/// `RM-3` — a SUPERSEDED request world is never served from the memo, and
/// a FENCED non-reproducible MISS never enters it.
///
/// The two arms are the negative halves of `RM-1`: reuse is bounded by
/// ONE immutable request world, and it only ever covers a definite
/// ANSWER. A miss concluded from a fenced (superseded) surface is not an
/// answer — no replayed taint repairs "nothing here" for a canonical that
/// has live content — so it stays out of the memo entirely.
#[test]
fn superseded_or_fenced_bundle_is_not_request_memoized() {
    // Arm (a): SUPERSEDED. An entry recorded under one token is not
    // served under another.
    let (host, owner) = base_host(&format!("{BASE_ROOT}_superseded"));
    let request = CanonicalCompletionOverlay::new();
    let (first, _, first_flights) = base_touch(&host, &request, &owner);
    assert_eq!(first_flights, 1, "sanity: the first touch is cold");
    let first = first.expect("first touch served");

    upsert_base(
        &host,
        &format!("{BASE_ROOT}_superseded/other.ts"),
        "export const other = 1;\n",
    );

    let hits_before = request_memo_hits(&host);
    let (second, _, _) = base_touch(&host, &request, &owner);
    let second = second.expect("second touch served");
    assert_eq!(
        request_memo_hits(&host) - hits_before,
        0,
        "a memo entry recorded under a superseded store-view token must NOT be served — \
         the token is the request world's identity, and reuse is bounded by it"
    );
    assert!(
        !Arc::ptr_eq(&first, &second),
        "the superseded world's value must not be reused"
    );

    // Arm (b): FENCED non-reproducible MISS. A surface-empty owner served
    // from a FENCED artifact describes the superseded artifact, not live
    // content.
    let miss_host = VerterHost::new_standalone(HostConfig::default());
    let miss_owner = "/rc_base_memo_fenced_miss/owner.ts";
    upsert_base(&miss_host, miss_owner, "// no declaration surface\n");
    let miss_host = Arc::new(miss_host);
    let _ = miss_host.ensure_indexed_ready(miss_owner);
    miss_host
        .test_force
        .force_indexed_ready_serve_fence_for_tests
        .store(true, Ordering::Relaxed);

    let miss_request = CanonicalCompletionOverlay::new();
    let (miss, miss_non_cacheable, _) = base_touch(&miss_host, &miss_request, miss_owner);
    miss_host
        .test_force
        .force_indexed_ready_serve_fence_for_tests
        .store(false, Ordering::Relaxed);

    assert!(
        miss.is_none(),
        "sanity: the surface-empty owner has no bundle"
    );
    assert!(
        miss_non_cacheable,
        "sanity: the fenced serve reached the enclosing tracer"
    );
    assert_eq!(
        miss_request.bundle_memo().len_for_tests(),
        0,
        "`RM-3`: a fenced-derived miss is NOT reproducible — reusing it would answer \
         'nothing here' for a canonical that has live content, and no replayed taint \
         repairs a wrong answer"
    );
}

/// `RM-1` — the request world's identity is the store-view compat token:
/// when it moves, the next touch materialises a NEW bundle rather than
/// serving the entry the previous world recorded.
///
/// This is the retry-attempt safety rail on the BASE namespace. A
/// `run_stable_request` retry re-snapshots the base view while SHARING
/// the request-scoped completion overlay, so without the token in the
/// entry the fresh attempt would be served a bundle whose import
/// canonicalization walked the world the retry exists to escape.
#[test]
fn request_world_token_change_forces_new_bundle() {
    // Staged on the `RequestOnly` owner deliberately: a `Shared` bundle
    // is admitted to `prepared_decl_bundles`, which stays valid across an
    // UNRELATED upsert, so a post-token touch would warm-hit the shared
    // cache at zero cold flights and the flight counter would prove
    // nothing about the memo. With nothing shared-admitted the memo is
    // the only tier that could serve the stale entry.
    let (host, owner) = refused_resolution_bundle_host("/rc_base_memo_token");
    let owner = owner.as_str();
    let request = CanonicalCompletionOverlay::new();

    let (first, _, _) = base_touch(&host, &request, owner);
    let first = first.expect("attempt 1 served");
    let (memo_hit, _, memo_flights) = base_touch(&host, &request, owner);
    assert!(
        Arc::ptr_eq(&first, &memo_hit.expect("attempt 1 replay served")),
        "fixture invariant: with the token unchanged the memo DOES serve the entry — \
         otherwise the miss below proves nothing about the token"
    );
    assert_eq!(memo_flights, 0, "fixture invariant: that was a memo hit");

    // A base upsert moves the external-supersession dimensions, so the
    // next snapshot carries a different compat token.
    upsert_base(
        &host,
        &format!("{BASE_ROOT}_token/unrelated.ts"),
        "export const u = 1;\n",
    );

    let (second, _, second_flights) = base_touch(&host, &request, owner);
    let second = second.expect("attempt 2 served");
    assert!(
        !Arc::ptr_eq(&first, &second),
        "a touch under a MOVED request-world token must not be served the previous \
         world's memoised bundle"
    );
    assert_eq!(
        second_flights, 1,
        "and it must run its own cold flight against the fresh world"
    );
}

/// Concurrent callers inside one request world join ONE cold flight and
/// end with ONE memo entry.
#[test]
fn concurrent_callers_join_one_cold_bundle_flight() {
    let (host, owner) = base_host(&format!("{BASE_ROOT}_concurrent"));
    let request = Arc::new(CanonicalCompletionOverlay::new());
    // ONE view shared by every thread, so all claims land on the same
    // singleflight lane (the lane key folds the compat token).
    let view = host.resolver_store_view_read().into_owned_view();

    let before = cold_flight_runs(&host);
    let bundles: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let host = Arc::clone(&host);
                let request = Arc::clone(&request);
                let owner = owner.clone();
                let view = &view;
                scope.spawn(move || {
                    host.prepared_decl_bundle_with_store_view(
                        view,
                        Some(request.bundle_memo()),
                        &owner,
                    )
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("worker must not panic"))
            .collect()
    });
    let flights = cold_flight_runs(&host) - before;

    assert!(
        bundles.iter().all(Option::is_some),
        "every concurrent caller must be served"
    );
    assert_eq!(
        flights, 1,
        "concurrent callers coalesce onto ONE cold flight — the singleflight lane, not \
         one flight per caller"
    );
    assert_eq!(
        request
            .bundle_memo()
            .len_in_world_for_tests(BundleMemoWorld::Base),
        1,
        "and the burst leaves exactly ONE memo entry, not one per caller"
    );
}
