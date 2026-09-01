//! Discriminating tests for stripping the owner's own
//! Parse-owned route-interface facts at the `resolve_component_meta`
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
//! Route-only resolution now records `FactKey::SyntacticRouteInterface`
//! from `FileFacts`. Owner and dependency observations both round-trip and
//! stay in the tracer-owned warm signatures. The legacy whole-content Route
//! fact remains constructible for explicit compatibility consumers, but is
//! not the component-meta route observation.
//!
//! ## Discrimination
//!
//! `resolve_component_meta_warm_caches_preserve_parse_owned_route_facts`
//! materialises the owner's `IndexedReady` via a genuine route-only
//! read, runs a cold `resolve_component_meta`, and asserts:
//!
//! 1. Both warm caches retain owner and dependency syntactic-route facts.
//! 2. Both retain the owner `FileWholeHash`, preserving content invalidation.
//!
//! The two negative guards
//! (`resolve_component_meta_owner_content_edit_still_invalidates` and
//! `resolve_component_meta_cross_file_edit_still_invalidates`) prove the
//! strip did not OVER-strip: an owner-content edit and a cross-file dep
//! edit each still invalidate the warm `resolve_component_meta` result.

use std::sync::Arc;

use crate::resolver_core::{FactVersionRef, StoreView};
use crate::types::{FileLanguage, Hash16, HostConfig, ProjectionMode, UpsertRequest};
use crate::VerterHost;

/// `/src/types.ts` — a cross-file dep imported by the owner. Its export
/// route is a genuine cross-file route dependency of the owner's
/// component-meta result.
const TYPES_TS: &str = "export interface RProps { a: number; b: string; }\n";

/// Owner SFC: `defineProps<RProps>()` over the imported `RProps`.
/// Resolving the macro root walks the named-type export route; the route
/// walk observes parse-owned syntactic route-interface facts.
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

fn build_reloadable_host(
    owner: &str,
    source: &'static str,
) -> (Arc<verter_workspace::MemoryWorkspace>, Arc<VerterHost>) {
    use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.inject_file(owner.to_string(), Arc::from(source));
    let access: Arc<dyn WorkspaceAccess> = workspace.clone();
    (
        workspace,
        Arc::new(VerterHost::new(HostConfig::default(), access)),
    )
}

fn upsert(host: &VerterHost, id: &str, src: &str, kind: FileLanguage) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_language: kind,
            aliases: Vec::new(),
        })
        .expect("upsert");
}

/// The legacy `cached_resolved_meta` mirror's `fact_versions` for
/// `(owner, Expanded, view_fingerprint=0)`.
fn legacy_mirror_facts(host: &VerterHost, owner: &str) -> Vec<FactVersionRef> {
    legacy_mirror_facts_for_view(host, owner, 0)
}

fn legacy_mirror_facts_for_view(
    host: &VerterHost,
    owner: &str,
    view_fingerprint: u64,
) -> Vec<FactVersionRef> {
    host.derived_raw_cache()
        .get(owner)
        .and_then(|entry| {
            entry
                .cached_resolved_meta
                .get(&(ProjectionMode::Expanded, view_fingerprint))
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
    validated_cache_signatures_for_view(host, owner, 0)
}

fn validated_cache_signatures_for_view(
    host: &VerterHost,
    owner: &str,
    view_fingerprint: u64,
) -> Vec<Arc<[FactVersionRef]>> {
    let key =
        crate::host_manage::component_meta_request_impl::resolved_meta_cache_key_with_view_fingerprint(
            owner,
            ProjectionMode::Expanded,
            view_fingerprint,
        );
    host.resolver_runtime()
        .component_meta
        .candidate_signatures_for_key(&key)
}

#[test]
fn overlaid_component_meta_result_is_rooted_in_overlay_owner_hash() {
    use crate::session_view::{OverlaidView, SessionView};

    let host = build_host();
    let owner = "/src/Overlay.vue";
    upsert(
        &host,
        owner,
        "<script setup lang=\"ts\">\ndefineProps<{ baseOnly: number }>();\n</script>\n",
        FileLanguage::vue(),
    );
    let base_hash = host.current_or_read_whole_hash(owner).expect("base hash");
    let overlay_source: Arc<str> = Arc::from(
        "<script setup lang=\"ts\">\ndefineProps<{ overlayOnly: string }>();\n</script>\n",
    );
    let mut overlays = rustc_hash::FxHashMap::default();
    overlays.insert(owner.to_string(), overlay_source);
    let view = OverlaidView::new(Arc::clone(&host), overlays);
    let overlay_hash = view.content_hash_for(owner).expect("overlay hash");
    assert_ne!(overlay_hash, base_hash);

    let meta = host
        .get_component_meta_via_view(owner, &view)
        .expect("overlay component meta resolves");
    assert_eq!(prop_names(&meta), vec!["overlayOnly"]);

    let signatures = validated_cache_signatures_for_view(&host, owner, view.fingerprint());
    assert!(
        !signatures.is_empty(),
        "overlay resolution must admit an inner candidate"
    );
    assert!(
        signatures
            .iter()
            .all(|facts| owner_whole_hash(facts, owner) == Some(overlay_hash)),
        "overlay candidates must be rooted in the view-authoritative owner hash: {signatures:#?}",
    );
    let mirror = legacy_mirror_facts_for_view(&host, owner, view.fingerprint());
    assert_eq!(owner_whole_hash(&mirror, owner), Some(overlay_hash));
}

#[test]
fn host_view_ref_uncaptured_owner_reloads_once_and_warms_exact_hash() {
    use crate::session_view::{HostViewRef, SessionView};

    const SOURCE: &str =
        "<script setup lang=\"ts\">\ndefineProps<{ reloaded: string }>();\n</script>\n";
    let (_workspace, host) = build_reloadable_host("/src/Reload.vue", SOURCE);
    let owner = "/src/Reload.vue";
    assert!(host.ensure_loaded(owner));
    host.evict(owner);
    assert!(host.is_canonical_evicted(owner));

    let before = host.provenance_snapshot().ensure_loaded_calls;
    let view = HostViewRef::new(&host);
    let meta = host
        .get_component_meta_via_view(owner, &view)
        .expect("an unmasked evicted owner reloads through the base host");
    assert_eq!(prop_names(&meta), vec!["reloaded"]);
    let after = host.provenance_snapshot().ensure_loaded_calls;
    assert_eq!(after, before + 1, "the uncaptured fallback reads once");
    assert!(!host.is_canonical_evicted(owner));

    let current_hash = host
        .authoritative_current_content_hash(owner)
        .expect("reload restores scheduler authority");
    let signatures = validated_cache_signatures_for_view(&host, owner, view.fingerprint());
    assert!(
        !signatures.is_empty()
            && signatures
                .iter()
                .all(|facts| owner_whole_hash(facts, owner) == Some(current_hash)),
        "the inner cache must carry the exact reloaded scheduler hash: {signatures:#?}",
    );
    assert_eq!(
        owner_whole_hash(
            &legacy_mirror_facts_for_view(&host, owner, view.fingerprint()),
            owner,
        ),
        Some(current_hash),
    );

    let warm_before = host.provenance_snapshot().ensure_loaded_calls;
    let warm = host
        .get_component_meta_via_view(owner, &view)
        .expect("the repaired cache is reusable");
    assert_eq!(prop_names(&warm), vec!["reloaded"]);
    assert_eq!(host.provenance_snapshot().ensure_loaded_calls, warm_before);
}

#[test]
fn unmasked_overlaid_view_ref_uncaptured_owner_reloads_base_once() {
    use crate::session_view::{OverlaidViewRef, SessionView};

    const SOURCE: &str =
        "<script setup lang=\"ts\">\ndefineProps<{ throughBase: number }>();\n</script>\n";
    let (_workspace, host) = build_reloadable_host("/src/Unmasked.vue", SOURCE);
    let owner = "/src/Unmasked.vue";
    assert!(host.ensure_loaded(owner));
    host.evict(owner);

    let overlays = rustc_hash::FxHashMap::default();
    let overlay_hashes = rustc_hash::FxHashMap::default();
    let tombstones = std::collections::HashSet::new();
    let view = OverlaidViewRef::new(&host, &overlays, &overlay_hashes, &tombstones);
    let before = host.provenance_snapshot().ensure_loaded_calls;
    let meta = host
        .get_component_meta_via_view(owner, &view)
        .expect("an unmasked overlay view falls through to the load-capable base host");
    assert_eq!(prop_names(&meta), vec!["throughBase"]);
    assert_eq!(host.provenance_snapshot().ensure_loaded_calls, before + 1,);
    let current_hash = host
        .authoritative_current_content_hash(owner)
        .expect("reload restores scheduler authority");
    assert!(
        validated_cache_signatures_for_view(&host, owner, view.fingerprint())
            .iter()
            .all(|facts| owner_whole_hash(facts, owner) == Some(current_hash)),
    );
    assert_eq!(
        owner_whole_hash(
            &legacy_mirror_facts_for_view(&host, owner, view.fingerprint()),
            owner,
        ),
        Some(current_hash),
    );
}

#[test]
fn explicit_overlay_owner_never_reloads_evicted_base_and_uses_overlay_hash() {
    use crate::session_view::OverlaidViewRef;

    const BASE: &str =
        "<script setup lang=\"ts\">\ndefineProps<{ baseOnly: number }>();\n</script>\n";
    const OVERLAY: &str =
        "<script setup lang=\"ts\">\ndefineProps<{ overlayOnly: string }>();\n</script>\n";
    let (_workspace, host) = build_reloadable_host("/src/Explicit.vue", BASE);
    let owner = "/src/Explicit.vue";
    assert!(host.ensure_loaded(owner));
    host.evict(owner);

    let overlay_source: Arc<str> = Arc::from(OVERLAY);
    let overlay_hash = crate::hash::hash_16(OVERLAY.as_bytes());
    let mut overlays = rustc_hash::FxHashMap::default();
    overlays.insert(owner.to_string(), overlay_source);
    let mut overlay_hashes = rustc_hash::FxHashMap::default();
    overlay_hashes.insert(owner.to_string(), overlay_hash);
    let tombstones = std::collections::HashSet::new();
    let view = OverlaidViewRef::new(&host, &overlays, &overlay_hashes, &tombstones);

    let before = host.provenance_snapshot().ensure_loaded_calls;
    let resolved = crate::host_manage::component_meta_request_impl::uncaptured_component_meta_owner_hash_for_view_for_test(
        &host,
        &view,
        owner,
    );
    assert_eq!(resolved, Some(overlay_hash));
    assert_eq!(
        host.provenance_snapshot().ensure_loaded_calls,
        before,
        "an explicit overlay must not reload masked base bytes",
    );
    assert!(
        host.is_canonical_evicted(owner),
        "the masked base remains evicted"
    );
}

#[test]
fn tombstoned_owner_never_reloads_evicted_base() {
    use crate::session_view::OverlaidViewRef;

    const BASE: &str =
        "<script setup lang=\"ts\">\ndefineProps<{ hidden: number }>();\n</script>\n";
    let (_workspace, host) = build_reloadable_host("/src/Deleted.vue", BASE);
    let owner = "/src/Deleted.vue";
    assert!(host.ensure_loaded(owner));
    host.evict(owner);

    let overlays = rustc_hash::FxHashMap::default();
    let overlay_hashes = rustc_hash::FxHashMap::default();
    let mut tombstones = std::collections::HashSet::new();
    tombstones.insert(owner.to_string());
    let view = OverlaidViewRef::new(&host, &overlays, &overlay_hashes, &tombstones);

    let before = host.provenance_snapshot().ensure_loaded_calls;
    assert!(host.get_component_meta_via_view(owner, &view).is_none());
    assert_eq!(
        host.provenance_snapshot().ensure_loaded_calls,
        before,
        "a session tombstone is authoritative absence and performs no base reload",
    );
    assert!(host.is_canonical_evicted(owner));
}

fn has_syntactic_route_fact(facts: &[FactVersionRef], canonical: &str) -> bool {
    facts.iter().any(|f| {
        matches!(
            f,
            FactVersionRef::Parse(parse)
                if parse.canonical_id == canonical
                    && parse.key == verter_semantic::facts::FactKey::SyntacticRouteInterface
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

fn owner_whole_hash(facts: &[FactVersionRef], owner: &str) -> Option<Hash16> {
    facts.iter().find_map(|fact| match fact {
        FactVersionRef::FileWholeHash { canonical_id, hash } if canonical_id == owner => {
            Some(*hash)
        }
        _ => None,
    })
}

fn syntactic_route_hash(facts: &[FactVersionRef], owner: &str) -> Option<Hash16> {
    facts.iter().find_map(|fact| match fact {
        FactVersionRef::Parse(parse)
            if parse.canonical_id == owner
                && parse.key == verter_semantic::facts::FactKey::SyntacticRouteInterface =>
        {
            Some(parse.expected_hash)
        }
        _ => None,
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
fn resolve_component_meta_warm_caches_preserve_parse_owned_route_facts() {
    let host = build_host();
    upsert(&host, DEP, TYPES_TS, FileLanguage::script_ts());
    upsert(&host, OWNER, OWNER_VUE, FileLanguage::vue());
    prime_with_indexed_ready(&host);

    // ── Legacy mirror ────────────────────────────────────────────────
    let mirror = legacy_mirror_facts(&host, OWNER);
    assert!(
        has_syntactic_route_fact(&mirror, OWNER),
        "legacy mirror must carry the owner's parse-owned syntactic route fact. \
         mirror = {mirror:#?}",
    );
    assert!(
        has_syntactic_route_fact(&mirror, DEP),
        "legacy mirror must carry the dependency's parse-owned syntactic route fact. \
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
        assert!(
            has_syntactic_route_fact(sig, OWNER),
            "validated cache candidate must carry the owner's parse-owned \
             syntactic route fact. sig = {sig:#?}",
        );
        assert!(
            has_syntactic_route_fact(sig, DEP),
            "validated cache candidate must carry the dependency's parse-owned \
             syntactic route fact. sig = {sig:#?}",
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
    // The owner `FileWholeHash` independently gates content edits.
    let host = build_host();
    upsert(&host, DEP, TYPES_TS, FileLanguage::script_ts());
    upsert(&host, OWNER, OWNER_VUE, FileLanguage::vue());
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
    upsert(&host, OWNER, edited, FileLanguage::vue());

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
         resolve_component_meta entry through the owner `FileWholeHash`. \
         stale_miss {stale_before} -> {stale_after}",
    );
}

#[test]
fn malformed_owner_result_is_self_rooted_and_same_canonical_repair_recovers() {
    let host = build_host();
    let owner = "/src/Broken.vue";
    upsert(
        &host,
        owner,
        "<script setup lang=\"ts\">\ndefineProps<{ a: number }(;\n</script>\n<template><div/></template>\n",
        FileLanguage::vue(),
    );
    let broken_hash = host
        .ensure_indexed_ready(owner)
        .expect("malformed owner still has an indexed artifact")
        .whole_hash;
    let broken_project_generation = host.project_type_store().current_project_generation();
    assert_eq!(
        broken_project_generation, 0,
        "standalone host starts at PG0"
    );
    let first = host
        .get_component_meta(owner)
        .expect("malformed owner may return a degraded component surface");
    assert!(prop_names(&first).is_empty());
    let broken_sigs = validated_cache_signatures(&host, owner);
    let broken_mirror = legacy_mirror_facts(&host, owner);
    assert!(
        !broken_sigs.is_empty(),
        "malformed result must exercise admission"
    );
    assert!(
        broken_sigs
            .iter()
            .all(|facts| owner_whole_hash(facts, owner) == Some(broken_hash)),
        "every malformed inner candidate must be rooted in the malformed owner bytes: {broken_sigs:#?}",
    );
    assert_eq!(owner_whole_hash(&broken_mirror, owner), Some(broken_hash));
    let broken_route_hash = syntactic_route_hash(&broken_mirror, owner)
        .expect("malformed result must retain its syntactic route observation");

    upsert(
        &host,
        owner,
        "<script setup lang=\"ts\">\ndefineProps<{ fixed: string }>();\n</script>\n<template><div/></template>\n",
        FileLanguage::vue(),
    );
    assert_eq!(
        host.project_type_store().current_project_generation(),
        broken_project_generation,
        "same-canonical content repair must not restore blanket project invalidation",
    );
    let fixed_hash = host
        .ensure_indexed_ready(owner)
        .expect("fixed owner has an indexed artifact")
        .whole_hash;
    assert_ne!(fixed_hash, broken_hash);
    let current_view = host
        .resolver_store_view_read()
        .current()
        .expect("current view");
    let key = crate::host_manage::component_meta_request_impl::resolved_meta_cache_key_with_view_fingerprint(
        owner,
        ProjectionMode::Expanded,
        0,
    );
    let warm_before_query = host
        .resolver_runtime()
        .component_meta
        .get_if_valid(&key, current_view.view());
    assert!(
        warm_before_query.is_none(),
        "the inner malformed candidate must miss"
    );
    assert!(
        !current_view.view().validates_fact_signature(&broken_mirror),
        "the malformed legacy mirror must miss on its owner whole hash",
    );
    let broken_route_fact = broken_mirror
        .iter()
        .find(|fact| syntactic_route_hash(std::slice::from_ref(*fact), owner).is_some())
        .expect("malformed mirror carries the route fact");
    assert!(
        current_view.view().validates(broken_route_fact),
        "the route interface intentionally remains unchanged; owner content is the discriminating fact",
    );

    let fixed = host
        .get_component_meta(owner)
        .expect("the repaired canonical resolves");
    assert_eq!(prop_names(&fixed), vec!["fixed"]);
    let fixed_mirror = legacy_mirror_facts(&host, owner);
    assert_eq!(owner_whole_hash(&fixed_mirror, owner), Some(fixed_hash));
    assert_eq!(
        syntactic_route_hash(&fixed_mirror, owner),
        Some(broken_route_hash)
    );
    let fixed_sigs = validated_cache_signatures(&host, owner);
    assert!(
        fixed_sigs
            .iter()
            .any(|facts| owner_whole_hash(facts, owner) == Some(fixed_hash)),
        "a repaired inner candidate must carry the repaired owner hash: {fixed_sigs:#?}",
    );

    let warm_hits_before = host.resolver_runtime().component_meta.warm_hit_count();
    let _warm = host
        .resolve_component_meta(owner, ProjectionMode::Expanded)
        .expect("unedited repaired owner resolves warm");
    assert!(
        host.resolver_runtime().component_meta.warm_hit_count() > warm_hits_before,
        "the repaired unedited owner must reuse the validated inner cache",
    );
}

#[test]
fn resolve_component_meta_cross_file_edit_still_invalidates() {
    // Cross-file parse-owned route and content facts gate dependency edits.
    let host = build_host();
    upsert(&host, DEP, TYPES_TS, FileLanguage::script_ts());
    upsert(&host, OWNER, OWNER_VUE, FileLanguage::vue());
    prime_with_indexed_ready(&host);

    let stale_before = host.resolver_runtime().component_meta.stale_miss_count();

    // Edit the cross-file dep: `RProps` loses `b`.
    upsert(
        &host,
        DEP,
        "export interface RProps { a: number; }\n",
        FileLanguage::script_ts(),
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
         resolve_component_meta entry through the dependency's exact facts. \
         stale_miss {stale_before} -> {stale_after}",
    );
}
