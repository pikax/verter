//! Discriminating tests for stripping the owner's own
//! `DerivedFactHash{Route}` fact at the `resolve_component_meta`
//! warm-cache publish boundaries.
//!
//! ## What is under test
//!
//! `resolve_component_meta` publishes its result into TWO warm caches:
//!
//! 1. the validated `resolver_runtime().component_meta` cache, admitted
//!    via `insert_arc_with_kind` inside
//!    `store_cached_resolved_meta_for_view_fingerprint`, and
//! 2. the legacy `cached_resolved_meta` mirror, stored as a
//!    `ResolvedComponentMetaCacheEntry` by `mirror_cached_resolved_meta_arc`.
//!
//! Both warm caches re-validate their stored `fact_versions` against the
//! live `HostStoreView` on every warm read (the read deciding whether a
//! later request joins the in-flight result or re-leads as a fresh cold
//! `Flight::Leader`).
//!
//! The owner's OWN `DerivedFactHash{Route}` fact is non-round-tripping:
//! `HostStoreView::build` sources `view.derived_hashes[(owner,
//! Route)]` from the owner's `IndexedReady.shallow_state` route surface.
//! The non-`DirectSource`
//! `DerivedFactHash` validation arm rejects a MISSING `(owner, Route)`
//! entry (it uses `is_some_and`, NOT the permissive `None => true`
//! untracked-accept the `FileWholeHash` arm uses), so under concurrency a
//! straggler whose live view does not source the owner Route hash
//! false-misses the content-identical published entry and re-runs the
//! cold path as a second `Flight::Leader`. That breaks the cold-concurrent
//! per-joiner attribution contract
//! (`tests/g_cache/cache_layer_cold_concurrent_attribution.rs`).
//!
//! The `ComponentMetaResultDb` path already filters exactly this fact via
//! `strip_owner_route_fact` (see `component_meta_entry.rs` +
//! `component_meta_entry_tests.rs`). These tests pin that the SAME filter
//! is applied at the two `resolve_component_meta` publish boundaries.
//!
//! ## Discrimination
//!
//! `resolve_component_meta_warm_caches_strip_owner_route_fact`
//! materialises the owner's `IndexedReady` via a genuine route-only
//! read, runs a cold `resolve_component_meta`, and asserts:
//!
//! 1. **Pre/post discriminator (BOTH caches).** Neither the validated
//!    cache's admitted candidate signature NOR the legacy mirror's
//!    `fact_versions` contains `DerivedFactHash { canonical_id == owner,
//!    kind: Route }`. PRE-FIX both admit the raw `fact_versions` verbatim
//!    (empirically `owner_route=true` on both), so this assertion FAILS.
//!    POST-FIX `strip_owner_route_fact` removes exactly this fact.
//! 2. **Narrowness guard.** Both admitted signatures DO still carry the
//!    cross-file route dep's `DerivedFactHash{Route}` fact AND the owner's
//!    `FileWholeHash` fact — proving the filter is NARROW.
//!
//! The two negative guards
//! (`resolve_component_meta_owner_content_edit_still_invalidates` and
//! `resolve_component_meta_cross_file_edit_still_invalidates`) prove the
//! strip did not OVER-strip: an owner-content edit and a cross-file dep
//! edit each still invalidate the warm `resolve_component_meta` result.

use std::sync::Arc;

use crate::resolver_core::{DerivedFactKind, FactVersionRef};
use crate::types::{FileKind, HostConfig, ProjectionMode, UpsertRequest};
use crate::VerterHost;

/// `/src/types.ts` — a cross-file dep imported by the owner. Its export
/// route is a genuine cross-file route dependency of the owner's
/// component-meta result, so its `DerivedFactHash{Route}` fact MUST
/// survive `strip_owner_route_fact`.
const TYPES_TS: &str = "export interface RProps { a: number; b: string; }\n";

/// Owner SFC: `defineProps<RProps>()` over the imported `RProps`.
/// Resolving the macro root walks the named-type export route; the route
/// walk observes `DerivedFactHash{Route}` participant facts — including
/// the owner's own, as the importer is itself a route participant.
const OWNER_VUE: &str = "<script setup lang=\"ts\">\n\
     import type { RProps } from './types';\n\
     defineProps<RProps>();\n\
     </script>\n\
     <template><div /></template>\n";

const OWNER: &str = "/src/Comp.vue";
const DEP: &str = "/src/types.ts";

fn build_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig::default()))
}

fn upsert(host: &VerterHost, id: &str, src: &str, kind: FileKind) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_kind: kind,
            aliases: Vec::new(),
        })
        .expect("upsert");
}

/// The legacy `cached_resolved_meta` mirror's `fact_versions` for
/// `(owner, Expanded, view_fingerprint=0)`.
fn legacy_mirror_facts(host: &VerterHost, owner: &str) -> Vec<FactVersionRef> {
    host.derived_raw_cache()
        .get(owner)
        .and_then(|entry| {
            entry
                .cached_resolved_meta
                .get(&(ProjectionMode::Expanded, 0))
                .map(|cached| cached.fact_versions.to_vec())
        })
        .expect("a legacy cached_resolved_meta mirror entry must exist for the owner")
}

/// Every admitted candidate signature in the validated
/// `resolver_runtime().component_meta` cache for `(owner, Expanded,
/// view_fingerprint=0)`. Reads the ADMITTED set regardless of validation
/// (the strip contract is about what is admitted, not what currently
/// validates).
fn validated_cache_signatures(host: &VerterHost, owner: &str) -> Vec<Arc<[FactVersionRef]>> {
    let key =
        crate::host_manage::component_meta_request_impl::resolved_meta_cache_key_with_view_fingerprint(
            owner,
            ProjectionMode::Expanded,
            0,
        );
    host.resolver_runtime()
        .component_meta
        .candidate_signatures_for_key(&key)
}

fn has_owner_route_fact(facts: &[FactVersionRef], owner: &str) -> bool {
    facts.iter().any(|f| {
        matches!(
            f,
            FactVersionRef::DerivedFactHash {
                canonical_id,
                kind: DerivedFactKind::Route,
                ..
            } if canonical_id == owner
        )
    })
}

fn has_dep_route_fact(facts: &[FactVersionRef], dep: &str) -> bool {
    facts.iter().any(|f| {
        matches!(
            f,
            FactVersionRef::DerivedFactHash {
                canonical_id,
                kind: DerivedFactKind::Route,
                ..
            } if canonical_id == dep
        )
    })
}

fn has_owner_whole_hash_fact(facts: &[FactVersionRef], owner: &str) -> bool {
    facts.iter().any(|f| {
        matches!(
            f,
            FactVersionRef::FileWholeHash { canonical_id, .. } if canonical_id == owner
        )
    })
}

fn prop_names(
    meta: &verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
) -> Vec<String> {
    let mut names: Vec<String> = meta.props.iter().map(|p| p.name.clone()).collect();
    names.sort_unstable();
    names
}

/// Materialise the owner's `IndexedReady` via a genuine route-only read
/// BEFORE the cold `resolve_component_meta`. This reproduces the
/// precondition where an owner already has an `IndexedReady` from an
/// earlier route-only read, which sources the owner Route hash on the
/// live `HostStoreView`.
fn prime_with_indexed_ready(host: &VerterHost) {
    let indexed = host.ensure_indexed_ready(OWNER);
    assert!(
        indexed.is_some(),
        "route-only read must materialise an IndexedReady artifact for the owner SFC",
    );
    let resolved = host.resolve_component_meta(OWNER, ProjectionMode::Expanded);
    assert!(
        resolved.is_some(),
        "cold resolve_component_meta must resolve"
    );
}

#[test]
fn resolve_component_meta_warm_caches_strip_owner_route_fact() {
    let host = build_host();
    upsert(&host, DEP, TYPES_TS, FileKind::NonSfc);
    upsert(&host, OWNER, OWNER_VUE, FileKind::VueSfc);
    prime_with_indexed_ready(&host);

    // ── Legacy mirror ────────────────────────────────────────────────
    let mirror = legacy_mirror_facts(&host, OWNER);
    // Discriminator 1 (pre/post): owner self-Route fact MUST be absent.
    // PRE-FIX the mirror admitted `state.fact_versions` verbatim, so the
    // owner Route fact IS present (empirically owner_route=true) and this
    // FAILS. POST-FIX `strip_owner_route_fact` removes it.
    assert!(
        !has_owner_route_fact(&mirror, OWNER),
        "legacy cached_resolved_meta mirror MUST NOT carry the owner's own \
         `DerivedFactHash{{Route}}` fact — it is dual-sourced on \
         `HostStoreView::derived_hashes` and does not round-trip on warm \
         validation. mirror = {mirror:#?}",
    );
    // Discriminator 2 (narrowness): cross-file dep route fact + owner
    // FileWholeHash MUST remain.
    assert!(
        has_dep_route_fact(&mirror, DEP),
        "legacy mirror MUST still carry the cross-file route dep `{DEP}` \
         `DerivedFactHash{{Route}}` fact — the strip is NARROW (owner-only). \
         mirror = {mirror:#?}",
    );
    assert!(
        has_owner_whole_hash_fact(&mirror, OWNER),
        "legacy mirror MUST still carry the owner `FileWholeHash` fact so \
         owner-content edits invalidate. mirror = {mirror:#?}",
    );

    // ── Validated cache ──────────────────────────────────────────────
    let sigs = validated_cache_signatures(&host, OWNER);
    assert!(
        !sigs.is_empty(),
        "the validated resolver_runtime().component_meta cache MUST have an \
         admitted candidate for the owner after a cold resolve_component_meta",
    );
    for sig in &sigs {
        // Discriminator 1 (pre/post): owner self-Route fact MUST be absent.
        assert!(
            !has_owner_route_fact(sig, OWNER),
            "validated component_meta cache candidate MUST NOT carry the owner's \
             own `DerivedFactHash{{Route}}` fact (admitted via insert_arc_with_kind \
             in store_cached_resolved_meta_for_view_fingerprint). PRE-FIX this fact \
             IS admitted verbatim. sig = {sig:#?}",
        );
        // Discriminator 2 (narrowness).
        assert!(
            has_dep_route_fact(sig, DEP),
            "validated cache candidate MUST still carry the cross-file route dep \
             `{DEP}` Route fact. sig = {sig:#?}",
        );
        assert!(
            has_owner_whole_hash_fact(sig, OWNER),
            "validated cache candidate MUST still carry the owner `FileWholeHash` \
             fact. sig = {sig:#?}",
        );
    }
}

#[test]
fn resolve_component_meta_owner_content_edit_still_invalidates() {
    // Narrowness negative guard: stripping the owner's own Route fact MUST
    // NOT break owner-content-edit invalidation — the owner `FileWholeHash`
    // fact survives the strip and gates content edits.
    let host = build_host();
    upsert(&host, DEP, TYPES_TS, FileKind::NonSfc);
    upsert(&host, OWNER, OWNER_VUE, FileKind::VueSfc);
    prime_with_indexed_ready(&host);

    // Warm hit (no edit) baseline — the validated cache reuses the entry.
    let stale_before_warm = host.resolver_runtime().component_meta.stale_miss_count();
    let _ = host.resolve_component_meta(OWNER, ProjectionMode::Expanded);
    let stale_after_warm = host.resolver_runtime().component_meta.stale_miss_count();
    assert_eq!(
        stale_before_warm, stale_after_warm,
        "an UNEDITED re-resolve must NOT register a stale miss on the validated \
         resolve_component_meta cache — the stripped signature round-trips. \
         stale_miss {stale_before_warm} -> {stale_after_warm}",
    );

    // Edit the OWNER: intersect an extra local prop into the macro arg.
    let edited = "<script setup lang=\"ts\">\n\
         import type { RProps } from './types';\n\
         defineProps<RProps & { extra: boolean }>();\n\
         </script>\n\
         <template><div /></template>\n";
    upsert(&host, OWNER, edited, FileKind::VueSfc);

    let stale_before = host.resolver_runtime().component_meta.stale_miss_count();
    let after = host
        .get_component_meta(OWNER)
        .expect("post owner-edit component meta resolves");
    let stale_after = host.resolver_runtime().component_meta.stale_miss_count();

    // The recomputed surface MUST reflect the owner edit — `extra` appears.
    let names = prop_names(&after);
    assert!(
        names.contains(&"extra".to_string()),
        "post owner-content-edit component-meta MUST reflect the new owner shape \
         (the added `extra` prop) — a stale warm hit would omit it. props = {names:?}",
    );
    assert!(
        names.contains(&"a".to_string()) && names.contains(&"b".to_string()),
        "post-edit surface still carries the imported RProps members. props = {names:?}",
    );
    // And the validated resolve_component_meta cache observed the
    // invalidation (the owner FileWholeHash fact no longer validates).
    assert!(
        stale_after > stale_before,
        "an owner-content edit MUST invalidate the warm validated \
         resolve_component_meta entry — the owner `FileWholeHash` fact survives the \
         owner-Route strip and rejects the stale entry. \
         stale_miss {stale_before} -> {stale_after}",
    );
}

#[test]
fn resolve_component_meta_cross_file_edit_still_invalidates() {
    // Narrowness negative guard: stripping the owner's own Route fact MUST
    // NOT break cross-file invalidation — the cross-file dep's Route fact
    // (and its FileWholeHash fact) survive the strip and gate dep edits.
    let host = build_host();
    upsert(&host, DEP, TYPES_TS, FileKind::NonSfc);
    upsert(&host, OWNER, OWNER_VUE, FileKind::VueSfc);
    prime_with_indexed_ready(&host);

    let stale_before = host.resolver_runtime().component_meta.stale_miss_count();

    // Edit the cross-file dep: `RProps` loses `b`.
    upsert(
        &host,
        DEP,
        "export interface RProps { a: number; }\n",
        FileKind::NonSfc,
    );

    let after = host
        .get_component_meta(OWNER)
        .expect("post cross-file-edit component meta resolves");
    let stale_after = host.resolver_runtime().component_meta.stale_miss_count();

    // The recomputed surface MUST reflect the dep edit — `b` is gone.
    let names = prop_names(&after);
    assert!(
        names.contains(&"a".to_string()) && !names.contains(&"b".to_string()),
        "post cross-file-edit component-meta MUST reflect the new RProps shape \
         (a, no b) — a stale warm hit would still report `b`. props = {names:?}",
    );
    assert!(
        stale_after > stale_before,
        "a cross-file dep edit MUST invalidate the warm validated \
         resolve_component_meta entry — the dep's Route + FileWholeHash facts \
         survive the owner-Route strip and reject the stale entry. \
         stale_miss {stale_before} -> {stale_after}",
    );
}
