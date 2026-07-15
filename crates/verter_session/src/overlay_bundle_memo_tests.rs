//! Request-scoped session-overlay prepared-decl bundle memo —
//! discriminating regression tests.
//!
//! ## The behavior these tests pin
//!
//! An overlay-bearing `prepared_decl_bundle_with_context` read cannot use
//! the host's shared bundle cache (R17: the shared slot is keyed by
//! canonical alone and would alias the base bundle), so pre-memo every
//! read re-ran `materialize_prepared_decl_bundle_via_ctx` — including the
//! full per-import re-export-chain walk — on EVERY bundle touch.
//!
//! The memo lives on the request-scoped
//! [`crate::resolver_core::CanonicalCompletionOverlay`] (created once per
//! top-level request, threaded into every `SessionResolverContext` the
//! request builds, dropped with the request) and is keyed by
//! `(raw overlay owner, overlay content hash, store-view compat token)`:
//!
//! - the overlay content hash pins the memo to the session view's frozen
//!   overlay bytes;
//! - the [`crate::resolver_core::StoreViewCompatToken`] pins it to ONE
//!   externally-coherent base-world snapshot (the same complete validity
//!   oracle singleflight lanes coalesce on), so a retry attempt against a
//!   fresh view after an external supersession NEVER reuses a bundle
//!   materialised against the superseded world.
//!
//! Success-only: a materialisation that consumed a NON-CACHEABLE read
//! (fenced overlay serve, unrootable route, broken lease — the
//! `with_cacheability_scope` verdict) is returned to its caller but never
//! memoised, so the per-call re-materialisation (and its per-call
//! non-cacheable fan-out into enclosing tracer scopes) is preserved for
//! exactly the reads where it is load-bearing.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::resolver_core::{CanonicalCompletionOverlay, ResolverContext, SessionResolverContext};
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
fn same_request_second_overlay_bundle_read_is_memo_hit() {
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
        first.prepared_type_decls.get("Foo").is_some(),
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
        bundle_b.prepared_type_decls.get("FooChanged").is_some(),
        "the new request's bundle carries the NEW overlay's declaration",
    );
    assert!(
        bundle_b.prepared_type_decls.get("Foo").is_none(),
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

/// (c) A canonical the view does NOT overlay keeps the base
/// (`prepared_decl_bundle_with_store_view`) path and never touches the
/// memo.
#[test]
fn base_canonical_without_overlay_never_enters_memo() {
    let host = host_with_base_files();
    // The view overlays the OWNER only; DEP stays base-served.
    let view = overlaid_view(&host, OWNER, OVERLAY_OWNER_A);
    let (store_view, overlay) = request_pieces(&host, &view);
    let ctx = SessionResolverContext::new(&host, &view, &store_view, Arc::clone(&overlay));

    let first = ResolverContext::prepared_decl_bundle(&ctx, DEP)
        .expect("base-path bundle must serve for the non-overlaid dep");
    let second = ResolverContext::prepared_decl_bundle(&ctx, DEP)
        .expect("base-path bundle must serve again");
    assert!(first.prepared_type_decls.get("Dep").is_some());
    assert!(second.prepared_type_decls.get("Dep").is_some());

    assert_eq!(
        overlay.overlay_bundle_memo_len_for_tests(),
        0,
        "a base-path (non-overlaid) canonical must never populate the \
         overlay bundle memo",
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
        overlay.overlay_bundle_memo_len_for_tests(),
        0,
        "the tombstone branch must never populate the overlay bundle memo",
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

/// (f) SUCCESS-ONLY: a materialisation whose cacheability scope reports
/// non-cacheable is returned to its caller but NEVER memoised — the next
/// read re-materialises (per-call semantics preserved for the fenced /
/// unrootable class).
///
/// Staged through the shared test knob that makes every tracer scope
/// report overflow — `with_cacheability_scope`'s verdict folds
/// `would_overflow`, the same non-cacheable rail a fenced serve or an
/// unrootable route marks.
#[test]
fn non_cacheable_materialization_is_not_memoized() {
    let host = host_with_base_files();
    host.test_force
        .force_fact_tracer_overflow_observations
        .store(
            crate::resolver_core::FACT_SIGNATURE_CAP + 1,
            std::sync::atomic::Ordering::Relaxed,
        );

    let view = overlaid_view(&host, OWNER, OVERLAY_OWNER_A);
    let (store_view, overlay) = request_pieces(&host, &view);
    let ctx = SessionResolverContext::new(&host, &view, &store_view, Arc::clone(&overlay));

    let first = ResolverContext::prepared_decl_bundle(&ctx, OWNER)
        .expect("a non-cacheable materialisation still serves its caller");
    let second = ResolverContext::prepared_decl_bundle(&ctx, OWNER)
        .expect("the second read re-materialises");

    assert!(
        !Arc::ptr_eq(&first, &second),
        "a non-cacheable materialisation must NOT be memoised — the second \
         read re-materialises per-call",
    );
    assert_eq!(
        overlay.overlay_bundle_memo_len_for_tests(),
        0,
        "the success-only gate must keep non-cacheable bundles out of the memo",
    );
}

/// (g) END-TO-END WIRING: a real session-view component-meta request —
/// the production `resolve_component_meta_with_view` boundary that
/// builds the `ViewBoundRequestHost` and threads ITS request-scoped
/// completion overlay into every resolver context — hits the memo
/// (provenance `overlay_bundle_memo_hits` moves). This pins the memo to
/// the benchmark's actual flow (compat checker `updateFile` overlays +
/// `getComponentMeta`), not only to hand-built contexts.
#[test]
fn session_view_component_meta_request_hits_overlay_bundle_memo() {
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
        .overlay_bundle_memo_hits
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
        .overlay_bundle_memo_hits
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
