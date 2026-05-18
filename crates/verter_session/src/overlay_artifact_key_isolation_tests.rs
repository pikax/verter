//! Discriminating tests — a byte-identical session overlay does NOT
//! collide with the base file's `FileArtifactStore` artifact.
//!
//! ## The defect these tests pin
//!
//! A session "overlays" every opened-but-unmodified file in an LSP
//! session with content byte-identical to disk. The overlay's content
//! hash therefore equals the base file's content hash. Before the
//! Block 2.S-F fix, the view-aware overlay materialiser published its
//! `IndexedReady` candidate under `FileArtifactKey::legacy(canonical,
//! hash)` — the *same* key the base `ensure_indexed_ready` uses. Two
//! distinct artifacts then occupy one slot.
//!
//! The two artifacts genuinely diverge: the overlay materialiser can
//! resolve a relative import (`./helper`) to an **overlay-only** file
//! the base workspace cannot see (`resolve_relative_overlay_candidate`),
//! so the overlay `IndexedReady.import_routes` carry session-specific
//! resolutions the base `IndexedReady.import_routes` do not. With a
//! colliding key:
//!
//! - overlay-materialised-first → a later base `ensure_indexed_ready`
//!   fast-path `get(canonical, hash)` returns the overlay artifact, so
//!   a base read observes the session's overlay-only routes;
//! - base-materialised-first → the overlay materialiser's fast-path
//!   `get(canonical, hash)` returns the base artifact, so the session
//!   silently loses its overlay-only route.
//!
//! ## The fix
//!
//! The view-aware overlay materialiser keys its candidate under
//! `FileArtifactKey::overlay_scoped(canonical, hash, discriminator)` —
//! the discriminator (derived from the session view's overlay-set
//! fingerprint) occupies the `parse_env_hash` dimension. The base
//! artifact keeps `parse_env_hash = LEGACY_PARSE_ENV_HASH`. A base
//! `get` / `get_for_current_content` read never reaches the overlay
//! candidate; a session-view `get_overlay_scoped` read never reaches
//! the base artifact.
//!
//! ## Discrimination property
//!
//! Each test FAILS against the pre-fix tree (the colliding legacy key)
//! and PASSES against the post-fix tree. The fixtures use a
//! byte-identical overlay (overlay hash == base hash) plus an
//! overlay-only relative dependency so the two artifacts' import
//! routes provably diverge.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::session_view::{OverlaidView, SessionView};
use crate::{FileKind, HostConfig, UpsertRequest, VerterHost};

/// Base owner SFC-free `.ts` file. It imports `./helper`, which is NOT
/// upserted into the workspace — so the base `ensure_indexed_ready`
/// cannot resolve the specifier, while a session that overlays
/// `/helper.ts` can.
const OWNER_SOURCE: &str = "import { helper } from './helper';\nexport const owner = helper;\n";

/// Overlay-only dependency body. Present ONLY as a session overlay.
const HELPER_SOURCE: &str = "export const helper = 1;\n";

fn host_with_owner() -> (Arc<VerterHost>, [u8; 16]) {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/owner.ts".to_string()),
            input_id: "/owner.ts".to_string(),
            source: Arc::from(OWNER_SOURCE),
            file_kind: FileKind::from_path("/owner.ts"),
            aliases: Vec::new(),
        })
        .expect("owner upsert succeeds");
    let host = Arc::new(host);
    let base = host
        .ensure_indexed_ready("/owner.ts")
        .expect("base IndexedReady materialises");
    (host, base.whole_hash)
}

/// Build an `OverlaidView` that overlays `/owner.ts` **byte-identically**
/// (overlay hash == base hash) and overlays the overlay-only
/// `/helper.ts`.
fn byte_identical_overlay_view(host: &Arc<VerterHost>) -> OverlaidView {
    let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    // Byte-identical overlay of the base owner — the LSB
    // "opened-but-unmodified file" case.
    overlays.insert("/owner.ts".to_string(), Arc::from(OWNER_SOURCE));
    // Overlay-only dependency — no disk / workspace presence.
    overlays.insert("/helper.ts".to_string(), Arc::from(HELPER_SOURCE));
    OverlaidView::new(Arc::clone(host), overlays)
}

/// The `resolved_canonical_id` the artifact recorded for the `./helper`
/// specifier, or `None` when the route is unresolved / absent.
fn helper_route(indexed: &crate::project_type_store::IndexedReady) -> Option<String> {
    indexed
        .import_routes
        .get("./helper")
        .and_then(|resolution| resolution.resolved_canonical_id.clone())
}

#[test]
fn byte_identical_overlay_hash_equals_base_hash_fixture_invariant() {
    // Fixture invariant: the overlay is byte-identical to the base, so
    // the overlay content hash MUST equal the base content hash —
    // otherwise the `content_hash` key dimension would already isolate
    // the two artifacts and there would be no collision to fix.
    let (host, base_hash) = host_with_owner();
    let view = byte_identical_overlay_view(&host);
    let overlay_hash = view
        .overlay_content_hash_for("/owner.ts")
        .expect("the view carries an explicit overlay for /owner.ts");
    assert_eq!(
        overlay_hash, base_hash,
        "fixture invariant: a byte-identical overlay must hash to the base hash — \
         this is the precondition under which the legacy key collides",
    );
    // And the discriminator is non-zero, so an `overlay_scoped` key can
    // never alias the base `legacy` key (`parse_env_hash = [0u8; 16]`).
    let discriminator = view
        .overlay_artifact_discriminator("/owner.ts")
        .expect("an overlaid canonical has an overlay-artifact discriminator");
    assert_ne!(
        discriminator, [0u8; 16],
        "the overlay discriminator must be non-zero so the overlay-scoped key \
         cannot alias the base legacy key",
    );
}

#[test]
fn overlay_materialized_first_does_not_poison_base_artifact_routes() {
    // Order A — the overlay materialises BEFORE the base read.
    //
    // Pre-fix: the overlay artifact (with `./helper` resolved to the
    // overlay-only `/helper.ts`) lands under `legacy(/owner.ts, hash)`.
    // A later base `ensure_indexed_ready("/owner.ts")` fast-path
    // `get(/owner.ts, hash)` returns THAT artifact — so the base read
    // observes the session's overlay-only route. Post-fix the overlay
    // artifact is keyed `overlay_scoped`, the base read keeps the
    // base `legacy` key, and the base route stays unresolved.
    let (host, _base_hash) = host_with_owner();
    let view = byte_identical_overlay_view(&host);
    let overlay_hash = view
        .overlay_content_hash_for("/owner.ts")
        .expect("overlay hash present");
    let overlay_source = view.source("/owner.ts").expect("overlay source present");

    // Materialise the overlay candidate first.
    let overlay_indexed = host
        .materialize_overlay_indexed_ready_with_view(
            "/owner.ts",
            &overlay_source,
            overlay_hash,
            &view,
        )
        .expect("overlay IndexedReady materialises");
    assert_eq!(
        helper_route(&overlay_indexed).as_deref(),
        Some("/helper.ts"),
        "fixture invariant: the overlay materialiser resolves `./helper` to the \
         overlay-only `/helper.ts` (this is the session-specific route)",
    );

    // Now read the BASE artifact through the base-only path.
    let base_indexed = host
        .ensure_indexed_ready("/owner.ts")
        .expect("base IndexedReady is available");
    assert_eq!(
        helper_route(&base_indexed),
        None,
        "BASE-ARTIFACT POISONING: the base `ensure_indexed_ready` read returned an \
         artifact whose `./helper` route resolves to the overlay-only `/helper.ts`. \
         The base workspace has no `/helper.ts`, so the base artifact's route MUST \
         stay unresolved — a resolved route means the overlay candidate collided \
         onto the base `FileArtifactKey::legacy` slot. The overlay-scoped key \
         dimension keeps the two artifacts isolated.",
    );
}

#[test]
fn base_materialized_first_does_not_starve_overlay_artifact_routes() {
    // Order B — the base read happens BEFORE the overlay materialises.
    //
    // Pre-fix: the base artifact (with `./helper` unresolved) sits
    // under `legacy(/owner.ts, hash)`. The overlay materialiser's
    // fast-path `get(/owner.ts, hash)` then HITS that base artifact and
    // returns it — so the session silently loses its overlay-only
    // route. Post-fix the overlay materialiser's fast path reads the
    // `overlay_scoped` key (a miss), materialises its own candidate,
    // and the overlay route resolves to `/helper.ts`.
    let (host, _base_hash) = host_with_owner();

    // Base artifact already materialised by `host_with_owner`; assert
    // its `./helper` route is unresolved (the control).
    let base_indexed = host
        .ensure_indexed_ready("/owner.ts")
        .expect("base IndexedReady is available");
    assert_eq!(
        helper_route(&base_indexed),
        None,
        "control: the base artifact's `./helper` route is unresolved (no \
         `/helper.ts` in the base workspace)",
    );

    // Now materialise the overlay candidate.
    let view = byte_identical_overlay_view(&host);
    let overlay_hash = view
        .overlay_content_hash_for("/owner.ts")
        .expect("overlay hash present");
    let overlay_source = view.source("/owner.ts").expect("overlay source present");
    let overlay_indexed = host
        .materialize_overlay_indexed_ready_with_view(
            "/owner.ts",
            &overlay_source,
            overlay_hash,
            &view,
        )
        .expect("overlay IndexedReady materialises");

    assert_eq!(
        helper_route(&overlay_indexed).as_deref(),
        Some("/helper.ts"),
        "OVERLAY ROUTE STARVATION: the overlay materialiser returned an artifact \
         whose `./helper` route is unresolved. The session overlays `/helper.ts`, \
         so the overlay artifact's route MUST resolve to it — an unresolved route \
         means the overlay materialiser's fast path hit the base artifact under a \
         colliding `FileArtifactKey::legacy` slot instead of materialising the \
         session's own candidate. The overlay-scoped key keeps them isolated.",
    );
}

#[test]
fn base_and_overlay_artifacts_coexist_under_distinct_keys() {
    // After both artifacts exist, the `FileArtifactStore` must hold
    // BOTH — the base under the legacy key and the overlay under the
    // overlay-scoped key — and each strict-key read must return its
    // OWN artifact. A pre-fix tree keeps exactly one entry under the
    // shared legacy key (the second writer overwrites the first).
    let (host, base_hash) = host_with_owner();
    let view = byte_identical_overlay_view(&host);
    let overlay_hash = view
        .overlay_content_hash_for("/owner.ts")
        .expect("overlay hash present");
    let discriminator = view
        .overlay_artifact_discriminator("/owner.ts")
        .expect("overlay discriminator present");
    let overlay_source = view.source("/owner.ts").expect("overlay source present");
    let _ = host
        .materialize_overlay_indexed_ready_with_view(
            "/owner.ts",
            &overlay_source,
            overlay_hash,
            &view,
        )
        .expect("overlay IndexedReady materialises");

    // The base legacy-key read returns the base artifact: `./helper`
    // unresolved.
    let base_via_legacy = host
        .project_type_store()
        .indexed()
        .get("/owner.ts", base_hash)
        .expect("base artifact present under the legacy key");
    assert_eq!(
        helper_route(&base_via_legacy),
        None,
        "the legacy-key read MUST return the base artifact (unresolved `./helper`) — \
         the overlay candidate must not occupy the legacy slot",
    );

    // The overlay-scoped read returns the overlay artifact: `./helper`
    // resolved to the overlay-only `/helper.ts`.
    let overlay_via_scoped = host
        .project_type_store()
        .indexed()
        .get_overlay_scoped("/owner.ts", overlay_hash, discriminator)
        .expect("overlay artifact present under the overlay-scoped key");
    assert_eq!(
        helper_route(&overlay_via_scoped).as_deref(),
        Some("/helper.ts"),
        "the overlay-scoped read MUST return the overlay artifact (resolved \
         `./helper`) — both artifacts coexist as distinct candidates",
    );

    // The two artifacts are genuinely different objects.
    assert!(
        !Arc::ptr_eq(&base_via_legacy, &overlay_via_scoped),
        "the base and overlay artifacts must be distinct `IndexedReady` instances — \
         a shared instance means the legacy and overlay-scoped keys collapsed onto \
         one slot",
    );
}
