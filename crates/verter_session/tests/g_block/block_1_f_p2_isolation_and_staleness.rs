//! Discriminating regression tests for Block 1.f P2 findings
//! ([Codex review of Block 1.f cleanup]).
//!
//! Both tests pin contract behaviour that exists only after the
//! Block 1.f P2 fixes land. Each test would FAIL against the
//! pre-fix tree and PASS against the post-fix tree (the
//! discriminator property required by the stub-prevention rule).
//!
//! ## P2.1 — overlay fallthrough uses the scheduler-authoritative
//! content-hash helper
//!
//! Before the fix, `OverlaidView::resolved_import_facts` (and
//! `OverlaidViewRef::resolved_import_facts`) composed
//! `ResolvedImportFactsKey.content_hash` through a `FileArtifactStore`
//! scan on base fallthrough. The producer
//! (`admit_resolved_import_facts_for_owner`) admits under
//! `parse.whole_hash` from the scheduler, which is available
//! immediately post-`upsert` before `IndexedReady` is materialised.
//! An overlay session whose overlay covered an *unrelated*
//! canonical (the queried owner was not overlaid) therefore missed
//! the producer's payload that a plain `HostView` returned, even
//! though both views should agree on the base host's resolution.
//!
//! After the fix, both overlay-view shapes route the base-
//! fallthrough hash through `current_content_hash_from_scheduler`
//! (matching the helper `resolved_import_facts_via_host` used for the
//! base-only views) — the scheduler-authoritative current content
//! hash, with no permissive `FileArtifactStore` fallback.
//!
//! ## P2.2 — later route snapshots replace earlier negative entries
//!
//! Before the fix, `ResolvedImportFactsKey` was
//! `(canonical, content_hash, parse_env_hash, resolve_env_hash,
//! resolver_version)` — no dependence on the owner's known-miss
//! sidecar. When the first `set_import_dependencies` for a fresh
//! owner admitted a known-miss bundle and a later
//! `set_import_dependencies` (for the SAME unchanged source) was
//! issued after the missing target canonical appeared, both calls
//! produced the same key value and `insert_if_absent` kept the
//! stale negative payload.
//!
//! After the fix, the key carries an additional
//! `known_miss_generation` dimension folded from the owner's
//! `DerivedRawState::import_routes_known_miss_recorded_at_generation`
//! sidecar. The first call admits under a non-zero tag; the second
//! call admits under `[0u8; 16]` (the sidecar is empty after a
//! successful resolution) and the view returns the resolved
//! bundle.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use verter_session::session_view::{OverlaidView, SessionView};
use verter_session::{
    CompileErrorPolicy, DependencyResolution, FileKind, HostConfig, UpsertRequest, VerterHost,
};

// ---------------------------------------------------------------------------
// P2.1 — overlay fallthrough scheduler hash
// ---------------------------------------------------------------------------

#[test]
fn overlay_view_with_unrelated_overlay_observes_admitted_facts_for_base_owner() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    }));

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/dep.ts".to_string(),
            source: Arc::from("export const used = 1;"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .expect("dep upsert");

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/owner.ts".to_string(),
            source: Arc::from("import { used } from './dep';\nexport const o = used;\n"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .expect("owner upsert");

    // Trigger the producer for `/owner.ts`. After this call the
    // producer admits the resolved-import facts under
    // `parse.whole_hash` taken from the scheduler.
    host.set_import_dependencies(
        "/owner.ts",
        vec![DependencyResolution {
            specifier: "./dep".to_string(),
            resolved_canonical_id: Some("/dep.ts".to_string()),
            possible_canonical_ids: vec!["/dep.ts".to_string()],
        }],
    );

    // Construct an OverlaidView that overlays an UNRELATED canonical
    // (`/scratch.ts`). The owner `/owner.ts` is NOT overlaid, so the
    // view falls through to the base host for its content hash and
    // its resolved-import facts. Pre-fix, this fallthrough went
    // through bare `content_hash_for`, which only consulted the
    // file-artifact store — empty post-upsert before
    // `IndexedReady` materialisation — and returned `None` from
    // `resolved_import_facts("/owner.ts")` even though the producer
    // had admitted the payload through the scheduler hash.
    let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    overlays.insert(
        "/scratch.ts".to_string(),
        Arc::from("export const scratch = 0;\n"),
    );
    let overlay_view = OverlaidView::new(Arc::clone(&host), overlays);

    let payload = overlay_view.resolved_import_facts("/owner.ts").expect(
        "OverlaidView with an unrelated overlay must observe the producer's payload for \
             the base-fallthrough owner (Codex P2.1 / Block 1.f-fix)",
    );

    let used_entry = payload
        .import_clauses
        .iter()
        .find(|e| e.binding.as_ref() == "used")
        .expect("the `used` binding must be present in the admitted payload");
    assert_eq!(
        used_entry.resolved_canonical.as_ref().map(|c| c.as_ref()),
        Some("/dep.ts"),
        "the overlay view's fallthrough lookup must return the resolved entry, not a stale miss",
    );
}

// ---------------------------------------------------------------------------
// P2.2 — known-miss generation in the cache key
// ---------------------------------------------------------------------------

#[test]
fn later_set_import_dependencies_replaces_prior_known_miss_in_view() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    }));

    // Stage 1 — upsert owner with an unresolvable specifier and admit
    // a known-miss bundle. The owner's
    // `import_routes_known_miss_recorded_at_generation` sidecar has
    // one entry → the key's `known_miss_generation` tag is non-zero.
    let owner_source = "import { Used } from './target';\nexport const o = 0;\n";
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/owner.ts".to_string(),
            source: Arc::from(owner_source),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .expect("owner upsert");

    host.set_import_dependencies(
        "/owner.ts",
        vec![DependencyResolution {
            specifier: "./target".to_string(),
            resolved_canonical_id: None,
            possible_canonical_ids: Vec::new(),
        }],
    );

    // Verify the cold cache learned the negative entry.
    let view = verter_session::session_view::HostView::new(Arc::clone(&host));
    let pre_resolve = view
        .resolved_import_facts("/owner.ts")
        .expect("producer must admit the negative bundle on the first call");
    let negative_entry = pre_resolve
        .import_clauses
        .iter()
        .find(|e| e.binding.as_ref() == "Used")
        .expect("`Used` binding admitted as negative fact");
    assert!(
        negative_entry.resolved_canonical.is_none(),
        "first admission must record `Used` as a negative (unresolved) entry",
    );

    // Stage 2 — the target file is created. The `upsert` advances
    // the workspace `content_generation`, so the next
    // `set_import_dependencies` for the same owner re-records the
    // (now zero-element) known-miss sidecar under a fresh
    // generation. The owner's source bytes are UNCHANGED — only the
    // known-miss generation tag in the key shifts.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/target.ts".to_string(),
            source: Arc::from("export const Used = 1;\n"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .expect("target upsert");

    host.set_import_dependencies(
        "/owner.ts",
        vec![DependencyResolution {
            specifier: "./target".to_string(),
            resolved_canonical_id: Some("/target.ts".to_string()),
            possible_canonical_ids: vec!["/target.ts".to_string()],
        }],
    );

    let post_resolve = view.resolved_import_facts("/owner.ts").expect(
        "view must observe the resolved bundle after the target file is created \
             (Codex P2.2 / Block 1.f-fix: known-miss generation in the cache key lets the \
             later snapshot win admission instead of being silently discarded against the \
             stale negative entry)",
    );
    let positive_entry = post_resolve
        .import_clauses
        .iter()
        .find(|e| e.binding.as_ref() == "Used")
        .expect("`Used` binding must be present in the post-resolve bundle");
    assert_eq!(
        positive_entry
            .resolved_canonical
            .as_ref()
            .map(|c| c.as_ref()),
        Some("/target.ts"),
        "post-resolve entry must carry the newly-resolved canonical, not the stale `None`",
    );
}
