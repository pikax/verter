//! Negative import-route reopens on new file.
//!
//! Root diagnosis: `IndexedReady.import_routes` can retain a stale
//! negative snapshot with no equivalent generation/fact validation.
//! After a new file appears, `authoritative_import_route` can still
//! observe the stale miss through `IndexedReady`.
//!
//! `authoritative_import_route` has two route sources:
//!  1. `cached_import_route_resolution` — reads
//!     `DerivedRawState.import_routes` and refuses every known-miss
//!     through the shared per-entry freshness oracle.
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
    // Fixture invariant: the unresolved import is in the owner's AUTHORED
    // inventory, and it does not resolve while the target is absent.
    assert!(
        owner_indexed
            .shallow_state
            .import_targets
            .values()
            .any(|target| target.source_specifier == "./late_dep"),
        "fixture invariant: the materialiser must publish the authored \
         './late_dep' specifier",
    );
    assert_eq!(
        host.resolve_type_dependency_canonical(owner, "./late_dep"),
        None,
        "fixture invariant: './late_dep' does not resolve — the target file \
         does not exist yet",
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

    // The owner's own bytes are unchanged, so it keeps serving the SAME
    // parse artifact — there is no route state on it to go stale.
    let owner_indexed_after = host
        .ensure_indexed_ready(owner)
        .expect("owner IndexedReady still present");
    assert_eq!(
        owner_indexed_after.whole_hash, owner_indexed.whole_hash,
        "the owner's parse artifact is unchanged — this is a dependency-set \
         move, not a content change",
    );

    // Discriminator: the route must reopen. The artifact snapshots no
    // resolution, and no host memo pins one, so the answer comes from the
    // one owner-edge authority against the live workspace. Pre-fix a
    // stale known-miss snapshot was served, every caller mapped it to
    // `return None`, and the import stayed permanently unresolvable.
    let after = host.resolve_type_dependency_canonical(owner, "./late_dep");
    assert_eq!(
        after.as_deref(),
        Some(late_dep),
        "after './late_dep.ts' is upserted, resolve_type_dependency_canonical \
         MUST reopen the route and resolve to {late_dep}. Pre-fix the stale \
         known-miss snapshot was served and the import stayed unresolvable \
         forever — the exact stale-known-miss-snapshot defect.",
    );
}

/// A positive resolution must STILL be served — the known-miss refusal
/// must not degenerate into refusing every resolution.
#[test]
fn positive_import_route_still_resolves() {
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
    assert!(
        owner_indexed
            .shallow_state
            .import_targets
            .values()
            .any(|target| target.source_specifier == "./dep"),
        "fixture invariant: the owner publishes its authored './dep' specifier",
    );

    // The positive resolution must be served — the known-miss refusal
    // must not degenerate into refusing every resolution.
    let resolved = host.resolve_type_dependency_canonical(owner, "./dep");
    assert_eq!(
        resolved.as_deref(),
        Some(dep),
        "a positive resolution must STILL be served — the known-miss refusal \
         targets negative answers only",
    );
}

/// BEHAVIOURAL successor to the `owner_import_surface_and_negative_route_facts`
/// source-text assertion that the reader still contains the literal
/// `if resolution.is_known_miss() {` branch.
///
/// A source scan for that branch cannot distinguish "the arm refuses"
/// from "the arm falls through to `Some(resolution)`": the text is
/// identical in both trees. This test calls
/// [`crate::VerterHost::cached_import_route_resolution`] directly and
/// pins the two outcomes that matter — a known-miss row is NEVER served
/// warm (so the caller re-resolves through the one owner-edge
/// authority), and a positive row in the SAME table still is (so the
/// refusal has not degenerated into refusing everything).
#[test]
fn cached_import_route_resolution_refuses_a_known_miss_and_serves_a_positive() {
    use crate::types::DependencyResolution;

    let host = VerterHost::new_standalone(HostConfig::default());
    let owner = "/known_miss_reader/owner.ts";
    let present = "/known_miss_reader/present.ts";
    upsert(&host, present, "export type A = { ok: true };\n");
    upsert(
        &host,
        owner,
        "import type { A } from './present';\n\
         import type { B } from './absent';\n\
         export type Re = A | B;\n",
    );

    host.set_import_dependencies(
        owner,
        vec![
            DependencyResolution {
                specifier: "./present".to_string(),
                resolved_canonical_id: Some(present.to_string()),
                possible_canonical_ids: vec![present.to_string()],
            },
            DependencyResolution {
                specifier: "./absent".to_string(),
                resolved_canonical_id: None,
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    // Fixture invariant: BOTH rows really are in the table the reader
    // consults, and the negative one really is classified a known-miss.
    // Without this the `is_none()` assertion below could pass because
    // the row was never recorded at all.
    {
        let derived = host
            .derived_raw_cache()
            .get(owner)
            .expect("fixture invariant: the owner must have a DerivedRawState entry");
        let positive = derived
            .import_routes
            .get("./present")
            .expect("fixture invariant: './present' must be recorded");
        assert!(
            !VerterHost::import_route_is_known_miss(positive),
            "fixture invariant: './present' must be recorded as a POSITIVE route"
        );
        let negative = derived
            .import_routes
            .get("./absent")
            .expect("fixture invariant: './absent' must be recorded");
        assert!(
            VerterHost::import_route_is_known_miss(negative),
            "fixture invariant: './absent' must be recorded as a KNOWN-MISS route"
        );
    }

    assert_eq!(
        host.cached_import_route_resolution(owner, "./present")
            .and_then(|resolution| resolution.resolved_canonical_id),
        Some(present.to_string()),
        "a caller-supplied authoritative POSITIVE must still be served warm — \
         the known-miss refusal must not degenerate into refusing every row",
    );
    assert!(
        host.cached_import_route_resolution(owner, "./absent")
            .is_none(),
        "a KNOWN-MISS row must never be served warm: a negative answer is not \
         evidence that the answer is still negative. Falling through to \
         `Some(resolution)` here re-serves the miss forever while the source \
         text of the `is_known_miss` arm stays byte-identical.",
    );
}
