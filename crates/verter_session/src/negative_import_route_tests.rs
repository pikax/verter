//! Negative import-route reopens on new file.
//!
//! Root diagnosis: `IndexedReady.import_routes` can retain a stale
//! negative snapshot with no equivalent generation/fact validation.
//! After a new file appears, `authoritative_import_route` can still
//! observe the stale miss through `IndexedReady`.
//!
//! `authoritative_import_route` has two route sources:
//!  1. `cached_import_route_resolution` — reads
//!     `DerivedRawState.import_routes`, which DOES revalidate known-miss
//!     entries against the `import_routes_known_miss_recorded_at_generation`
//!     sidecar.
//!  2. The `IndexedReady.import_routes` fallback — a snapshot built once
//!     at materialisation time with NO generation tag.
//!
//! Every caller of `authoritative_import_route`
//! (`resolve_type_dependency_canonical` /
//! `resolve_loaded_dependency_canonical` /
//! `resolve_type_dependency_canonical_shallow`) maps a known-miss
//! resolution to an unconditional `return None` — it gives up WITHOUT
//! recomputing. So pre-fix, once `IndexedReady.import_routes` records a
//! specifier as a known-miss, that import is treated as permanently
//! unresolvable: a file appearing later never reopens the route.
//!
//! Post-fix the `IndexedReady` fallback FILTERS known-miss entries
//! (`!import_route_is_known_miss`). A negative is recomputed by the
//! caller's `resolve_workspace_dependency_and_cache` path, which
//! re-resolves against the current workspace and reopens the route the
//! moment the target file exists. Positive entries served through the
//! fallback are current by construction: `ensure_indexed_ready`'s reuse
//! is edge-currency-gated, so a surface whose baked edges predate a
//! dependency-set change takes the edge-refresh (re-resolving its
//! edges) before this read.
//!
//! Discriminating fixture: an owner imports `./late_dep`, which does
//! not exist when the owner's `IndexedReady` is materialised — so
//! `IndexedReady.import_routes['./late_dep']` is a known-miss snapshot.
//! `./late_dep.ts` is then upserted; the owner-upsert path has no
//! eager reverse-dependent cascade. Pre-fix
//! `resolve_type_dependency_canonical` kept returning `None`;
//! post-fix the route reopens (the gated read edge-refreshes the
//! owner's surface, and the known-miss filter backstops any
//! pre-refresh artifact).
use std::sync::Arc;

use crate::{HostConfig, UpsertRequest, VerterHost};

/// Upsert a `.ts` file into a standalone host.
fn upsert(host: &VerterHost, path: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(path.to_string()),
            input_id: path.to_string(),
            source: Arc::from(source),
            file_language: crate::LanguageRegistry::global()
                .classify_static(path)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .unwrap_or_else(|e| panic!("upsert {path} failed: {e:?}"));
}

/// An owner whose import does not resolve at index time records a
/// known-miss in `IndexedReady.import_routes`. After the target file
/// appears — without an eager dependent-eviction cascade — the import
/// route must reopen.
#[test]
fn negative_import_route_reopens_after_target_file_appears() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let owner = "/neg_route/owner.ts";
    // The owner imports a TYPE from `./late_dep`, which does not exist
    // yet. At `ensure_indexed_ready(owner)` time the route resolves to
    // a known-miss and is snapshotted into `IndexedReady.import_routes`.
    upsert(
        &host,
        owner,
        "import type { LateType } from './late_dep';\n\
         export const consumer: LateType = null as unknown as LateType;\n",
    );

    let owner_indexed = host
        .ensure_indexed_ready(owner)
        .expect("owner IndexedReady must materialise");
    // Fixture invariant: the unresolved import is snapshotted as a
    // known-miss in `IndexedReady.import_routes`.
    let snapshot = owner_indexed.import_routes.get("./late_dep").cloned();
    assert!(
        snapshot.is_some(),
        "fixture invariant: the materialiser must snapshot the unresolved \
         import './late_dep' into IndexedReady.import_routes",
    );
    assert!(
        VerterHost::import_route_is_known_miss(snapshot.as_ref().expect("snapshot present"),),
        "fixture invariant: './late_dep' must be snapshotted as a KNOWN-MISS \
         (no resolved_canonical_id, no candidates) — the target file does \
         not exist yet",
    );

    // Pre-condition: with the target file still absent, the import does
    // not resolve.
    let before = host.resolve_type_dependency_canonical(owner, "./late_dep");
    assert!(
        before.is_none(),
        "the import must not resolve before the target file exists",
    );

    // The target file appears. The owner-upsert path has no eager
    // reverse-dependent cascade, so the owner's `IndexedReady`
    // (carrying the stale `./late_dep` known-miss snapshot) is NOT
    // evicted — exactly the lingering-stale state that exposes the
    // negative-snapshot defect.
    let late_dep = "/neg_route/late_dep.ts";
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(late_dep.to_string()),
            input_id: late_dep.to_string(),
            source: Arc::from("export type LateType = { resolved: true };\n"),
            file_language: crate::LanguageRegistry::global()
                .classify_static(late_dep)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("late_dep upsert");

    // The gated read path is edge-currency-gated: the owner's surface
    // carries cross-file edges, so the target's appearance (a
    // `content_generation` advance) routes `ensure_indexed_ready`
    // through the edge-refresh, which re-resolves `./late_dep` against
    // the live file set. The refreshed surface records the POSITIVE
    // resolution — the stale known-miss never survives a gated read.
    // (The known-miss filter on the `IndexedReady` fallback remains the
    // backstop for any reader holding a pre-refresh artifact.)
    let owner_indexed_after = host
        .ensure_indexed_ready(owner)
        .expect("owner IndexedReady still present");
    if let Some(still) = owner_indexed_after.import_routes.get("./late_dep") {
        assert!(
            !VerterHost::import_route_is_known_miss(still),
            "the gated read must serve an EDGE-REFRESHED surface whose \
             './late_dep' entry re-resolved positively after the target \
             appeared — serving the stale known-miss snapshot means the \
             edge-currency gate failed to stale the owner's surface on \
             the dependency-set change",
        );
    }

    // Discriminator: the route must reopen. Two independent rails
    // enforce it — the edge-currency refresh (the gated read re-resolves
    // the owner's edges against the live file set) and the
    // `authoritative_import_route` known-miss filter (a stale negative
    // from a pre-refresh artifact is never served; the caller recomputes
    // against the current workspace). Pre-fix neither existed: the stale
    // negative was served, every caller mapped it to `return None`, and
    // the import stayed permanently unresolvable.
    let after = host.resolve_type_dependency_canonical(owner, "./late_dep");
    assert_eq!(
        after.as_deref(),
        Some(late_dep),
        "after './late_dep.ts' is upserted, resolve_type_dependency_canonical \
         MUST reopen the route and resolve to {late_dep}. Pre-fix the stale \
         IndexedReady.import_routes known-miss snapshot is served and the \
         import stays unresolvable forever — the exact stale-known-miss-snapshot defect.",
    );
}

/// A positive `IndexedReady.import_routes` resolution must STILL be
/// served from the fallback — the item-2 fix filters only known-miss
/// entries, never positive resolutions.
#[test]
fn positive_import_route_still_served_from_indexed_fallback() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let dep = "/pos_route/dep.ts";
    let owner = "/pos_route/owner.ts";
    // The dep exists BEFORE the owner is indexed, so the owner's
    // `IndexedReady.import_routes` records a POSITIVE resolution.
    upsert(&host, dep, "export type DepType = { ok: true };\n");
    upsert(
        &host,
        owner,
        "import type { DepType } from './dep';\n\
         export const consumer: DepType = null as unknown as DepType;\n",
    );

    let owner_indexed = host
        .ensure_indexed_ready(owner)
        .expect("owner IndexedReady must materialise");
    let snapshot = owner_indexed
        .import_routes
        .get("./dep")
        .cloned()
        .expect("fixture invariant: './dep' must be in IndexedReady.import_routes");
    assert!(
        !VerterHost::import_route_is_known_miss(&snapshot),
        "fixture invariant: './dep' resolves to an existing file, so its \
         IndexedReady.import_routes entry is a POSITIVE resolution",
    );

    // The positive resolution must be served — the item-2 fix's
    // `!import_route_is_known_miss` filter keeps positives. A regression
    // that filtered ALL `IndexedReady` entries would break this.
    let resolved = host.resolve_type_dependency_canonical(owner, "./dep");
    assert_eq!(
        resolved.as_deref(),
        Some(dep),
        "a positive IndexedReady.import_routes resolution must STILL be \
         served from the fallback — the item-2 filter targets known-miss \
         entries only, never positive resolutions",
    );
}
