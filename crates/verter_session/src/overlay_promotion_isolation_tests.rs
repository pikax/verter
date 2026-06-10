//! Discriminator: route-owned-shallow bundle materialiser MUST
//! promote `(whole_hash, route_hash, import_route_hash)` triples into
//! the per-request [`CanonicalCompletionOverlay`] before publishing
//! the bundle.
//!
//! ## Why this test exists
//!
//! Without overlay promotion, the request-entry
//! [`HostStoreView`](crate::resolver_store::HostStoreView) snapshot
//! misses route-owned canonicals materialised mid-request (the
//! snapshot is built ONCE at request entry; later
//! `ensure_route_owned_shallow_entry` publications are invisible to
//! it). Every subsequent warm-read validation of the bundle's stored
//! `(FileWholeHash, ImportRoute)` facts then routes through the base
//! view's untracked-canonical reject and triggers a cold rebuild
//! every probe — the leak overlay promotion closes.
//!
//! ## Discriminating contract
//!
//! Unit-style: drive
//! `materialize_prepared_decl_bundle_from_route_owned_shallow` via a
//! `prepared_decl_bundle_with_store_view` call against a hermetic
//! host with a `.d.ts` route-owned dependency, then assert the
//! request-scoped overlay carries the canonical's
//! `(whole_hash, route_hash, import_route_hash)` triple via the
//! direct `lookup_*` test accessors.
//!
//! Pre-fix the
//! `materialize_prepared_decl_bundle_from_route_owned_shallow`
//! signature did not take a `view` argument and the
//! `StoreView::promote_route_owned_completion` method did not exist
//! on the trait. The test does not compile against the pre-fix
//! tree.
//!
//! Post-fix the materialiser threads the view through, calls
//! `view.promote_route_owned_completion(...)` before the bundle
//! insert, and the overlay's `whole_hashes` + `derived_hashes` maps
//! observe the canonical via the direct lookup test accessors. The
//! test compiles and passes.

use std::sync::Arc;

use crate::resolver_core::request_store_view::{CanonicalCompletionOverlay, RequestStoreView};
use crate::resolver_core::{DerivedFactKind, StoreView};
use crate::{HostConfig, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

const TYPEDEFS_DTS: &str = r#"export interface Outer {
  readonly id: number;
  readonly name: string;
}
export type Variant = "primary" | "secondary" | "tertiary";
"#;

fn build_host_with_dts() -> Arc<VerterHost> {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.inject_file("/typedefs.d.ts".into(), Arc::from(TYPEDEFS_DTS));
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    Arc::new(VerterHost::new(HostConfig::default(), ws_access))
}

#[test]
fn route_owned_bundle_materialisation_populates_overlay_whole_hash() {
    // Hermetic host with a single `.d.ts` declaration file. The
    // declaration file is loaded mid-request via the
    // `prepared_decl_bundle_with_store_view` path — which dispatches
    // through `materialize_prepared_decl_bundle_from_route_owned_shallow`
    // for `.d.ts` extensions.
    let host = build_host_with_dts();

    // Build the per-request view + overlay PAIR exactly like the
    // audited entry does. The overlay starts empty.
    let base = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let view = RequestStoreView::new(&base, Arc::clone(&overlay));

    // Sanity: the overlay is empty before the materialiser runs.
    assert_eq!(
        overlay.peek_whole_hash_for_tests("/typedefs.d.ts"),
        None,
        "overlay must start empty before the materialiser runs"
    );

    // Drive the prepared-decl-bundle path on the route-owned `.d.ts`
    // canonical. The first call cold-materialises via the route-
    // owned-shallow producer; the producer promotes the canonical's
    // `(whole_hash, route_hash, import_route_hash)` triple into the
    // overlay before publishing the bundle.
    let bundle = host.prepared_decl_bundle_with_store_view(&view, "/typedefs.d.ts");
    assert!(
        bundle.is_some(),
        "the materialiser must produce a bundle for an existing `.d.ts` file"
    );

    // Discriminator: the overlay's `whole_hashes` MUST now carry
    // `/typedefs.d.ts`. Pre-fix the materialiser did NOT take a
    // `view` argument (its signature was
    // `materialize_prepared_decl_bundle_from_route_owned_shallow(&self, canonical_id)`)
    // and there was no overlay-promotion code path; the overlay
    // stayed empty.
    let overlay_whole_hash = overlay.peek_whole_hash_for_tests("/typedefs.d.ts");
    assert!(
        overlay_whole_hash.is_some(),
        "Overlay must carry `/typedefs.d.ts`'s `whole_hash` after the \
         route-owned-shallow bundle materialiser runs. The promotion is \
         what closes the perpetual cold-rebuild loop: without it, the \
         base view's snapshot misses the just-published canonical and \
         every warm validation rejects the bundle's stored `(FileWholeHash, \
         ImportRoute)` facts."
    );

    // The overlay's `whole_hash` for the canonical MUST match the
    // host-side `whole_hash` for the file (consistency with the
    // producer's view of the file's content).
    let host_whole_hash = host
        .get_whole_hash("/typedefs.d.ts")
        .expect("host must track the canonical after the materialiser ran");
    assert_eq!(
        overlay_whole_hash.unwrap(),
        host_whole_hash,
        "overlay-promoted `whole_hash` must match the host's view of \
         the canonical's content hash — a stale promotion would steer \
         warm validation toward the wrong content version"
    );
}

#[test]
fn route_owned_bundle_materialisation_view_lookup_observes_overlay_whole_hash() {
    // Discriminating companion: the request-scoped `RequestStoreView`
    // — the same view threaded through resolver-tier callers — MUST
    // observe the overlay-promoted whole hash via its
    // `validates_self_root_whole_hash` accessor (the strict
    // self-root validator that the bundle's warm-read path consults).
    //
    // Without overlay promotion, the view's
    // `validates_self_root_whole_hash` falls through to the base
    // view's strict reject (the base view never saw the route-owned
    // canonical because it was published mid-request).
    let host = build_host_with_dts();
    let base = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let view = RequestStoreView::new(&base, Arc::clone(&overlay));

    let _bundle = host
        .prepared_decl_bundle_with_store_view(&view, "/typedefs.d.ts")
        .expect("materialiser must produce a bundle");

    let host_whole_hash = host
        .get_whole_hash("/typedefs.d.ts")
        .expect("host must track the canonical");

    assert!(
        StoreView::validates_self_root_whole_hash(&view, "/typedefs.d.ts", &host_whole_hash),
        "RequestStoreView::validates_self_root_whole_hash MUST accept \
         the route-owned canonical's whole hash after the bundle \
         materialiser runs. The overlay promotion is what enables this \
         — pre-fix the route-owned canonical is untracked by both the \
         base view (snapshot is too old) and the overlay (promotion \
         wasn't wired), so the strict self-root validator rejects every \
         warm-read of the bundle and forces a cold rebuild."
    );
}

#[test]
fn route_owned_bundle_materialisation_optional_import_route_promotion() {
    // The route-owned-shallow producer pushes an `ImportRoute`
    // derived-fact entry into the bundle's `fact_dep_signature`
    // only when `host.generation_current_import_route_hash` returns
    // `Some`. For a leaf `.d.ts` with no imports this is `None`, in
    // which case the promotion correctly skips the `ImportRoute`
    // arm. The discriminating contract is therefore conditional:
    // *when* the host carries an import-route hash for the route-
    // owned canonical, the overlay MUST carry the same hash.
    let host = build_host_with_dts();
    let base = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let view = RequestStoreView::new(&base, Arc::clone(&overlay));

    let _bundle = host
        .prepared_decl_bundle_with_store_view(&view, "/typedefs.d.ts")
        .expect("materialiser must produce a bundle");

    let host_import_route_hash = host.generation_current_import_route_hash("/typedefs.d.ts");
    let overlay_import_route_hash =
        StoreView::derived_hash_for(&view, "/typedefs.d.ts", DerivedFactKind::ImportRoute);

    match host_import_route_hash {
        Some(expected) => assert_eq!(
            overlay_import_route_hash,
            Some(expected),
            "When the host carries an `ImportRoute` derived-fact hash \
             for the route-owned canonical, the overlay MUST carry the \
             same hash — otherwise the bundle's stored `ImportRoute` fact \
             will mismatch on warm validation and trigger a cold rebuild. \
             host_import_route_hash={host_import_route_hash:?} \
             overlay_import_route_hash={overlay_import_route_hash:?}"
        ),
        None => {
            // The producer correctly skipped the ImportRoute arm; the
            // overlay's `derived_hashes` map should remain empty for
            // this canonical OR the entry should not carry an
            // `ImportRoute` slot. Either is acceptable — a `Some`
            // overlay hash with a `None` host hash would itself be a
            // defect (stale promotion).
        }
    }
}
