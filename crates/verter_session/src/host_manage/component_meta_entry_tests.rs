//! Discriminating tests for `strip_owner_route_fact` — the narrow
//! owner-Route fact filter applied at the `ComponentMetaResultDb`
//! cache-admission site.
//!
//! ## What is under test
//!
//! Block 1.J.2 made the component-meta cache signature tracer-owned
//! and deleted the route-fact filter, on the premise that the
//! finalised tracer read set never observes the owner's own
//! `DerivedFactHash{Route}` fact. That premise is false: the cold
//! compute's macro-root route walk observes the owner's Route fact
//! whenever the owner is a route participant (see
//! `tests/component_meta_route_facts_flow_into_signature.rs`, which
//! characterises that the owner's Route fact reaches the tracer).
//!
//! The owner's own Route fact is non-round-tripping: `HostStoreView::build`
//! dual-sources `view.derived_hashes[(owner, Route)]` — from the
//! owner's `IndexedReady` shallow state AND from any `route_owned_shallow`
//! entry, with the route-owned source overwriting the indexed source.
//! The cold component-meta compute observes the Route hash via the
//! `IndexedReady` shallow state; a later warm-hit validation can read
//! the route-owned hash instead. When the two sources disagree, a
//! repeated IDENTICAL `get_component_meta` query misses the
//! final-result cache with no edit — a steady-state warm-cache miss /
//! perf regression. The owner's own export route is not a dependency
//! of the owner's own component-meta result in the first place (the
//! owner's `FileWholeHash` fact already covers owner-content edits),
//! so the fix drops the fact unconditionally rather than gambling on
//! whether the two sources happen to agree.
//!
//! `strip_owner_route_fact` drops exactly the owner's own
//! `DerivedFactHash{Route}` fact before cache admission. Cross-file
//! route facts (Route facts for OTHER canonicals the cold compute
//! walked) round-trip correctly and stay.
//!
//! ## Discrimination
//!
//! `repeated_query_with_route_owned_entry_is_warm_hit` plants a
//! `route_owned_shallow` entry for the owner via a genuine route-only
//! read BEFORE the owner has an `IndexedReady` (the codex
//! precondition), then runs `get_component_meta` and asserts:
//!
//! 1. **Pre/post discriminator.** The published `ComponentMetaResultEntry`
//!    signature does NOT contain `DerivedFactHash { canonical_id ==
//!    owner, kind: Route }`. PRE-FIX this fact IS present (the publish
//!    site admitted the raw tracer set verbatim) and this assertion
//!    FAILS. POST-FIX `strip_owner_route_fact` removes it.
//! 2. **Narrowness guard.** The published signature DOES still contain
//!    the cross-file route dep's `DerivedFactHash{Route}` fact —
//!    proving the filter is NARROW, not the broad route-fact removal.
//! 3. **Behavioural non-regression.** A repeated IDENTICAL query is a
//!    warm-cache HIT with no new miss. The fix must not break warm
//!    reuse for the route-owned-entry scenario.

use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;

use crate::resolver_core::{DerivedFactKind, FactVersionRef};
use crate::types::{FileKind, HostConfig, UpsertRequest};
use crate::VerterHost;

/// `/src/types.ts` — a cross-file dep imported by the owner. Its
/// export route is a genuine cross-file route dependency of the
/// owner's component-meta result.
const TYPES_TS: &str = "export interface RProps { a: number; b: string; }\n";

/// Owner SFC: `defineProps<RProps>()` over the imported `RProps`.
/// Resolving the macro root walks the named-type export route; the
/// route walk observes `DerivedFactHash{Route}` participant facts —
/// including the owner's own, as the importer is a route participant.
const OWNER_VUE: &str = "<script setup lang=\"ts\">\n\
     import type { RProps } from './types';\n\
     defineProps<RProps>();\n\
     </script>\n\
     <template><div /></template>\n";

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

/// Read the published `ComponentMetaResultEntry` signature `facts`
/// rail for `owner` — composed exactly as
/// `publish_component_meta_cache_entry` composes the key (owner
/// canonical + current `IndexedReady` whole-hash + default
/// `ComponentMetaOptions` fingerprint).
fn published_facts(host: &VerterHost, owner: &str) -> Vec<FactVersionRef> {
    let whole_hash = host
        .ensure_indexed_ready(owner)
        .map(|ir| ir.whole_hash)
        .expect("owner must have an IndexedReady entry");
    let key = crate::component_meta_result_db::ComponentMetaResultKey {
        owner_canonical: Arc::from(owner),
        owner_whole_hash: whole_hash,
        options_fingerprint: crate::host_manage::component_meta_options_fingerprint(
            &crate::host_manage::ComponentMetaOptions::default(),
        ),
    };
    host.project_type_store()
        .component_meta_results()
        .get(&key)
        .expect("a ComponentMetaResultEntry must be published for the owner")
        .read_set_signature
        .facts
        .to_vec()
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

#[test]
fn repeated_query_with_route_owned_entry_is_warm_hit() {
    let host = build_host();
    upsert(&host, "/src/types.ts", TYPES_TS, FileKind::NonSfc);
    upsert(&host, "/src/Comp.vue", OWNER_VUE, FileKind::VueSfc);

    // Plant a `route_owned_shallow` entry for the OWNER via a genuine
    // route-only read, BEFORE the owner has an `IndexedReady`. The
    // materialiser aborts NEW publishes when `IndexedReady` already
    // exists for the canonical, so this must happen first; the entry
    // then persists once `get_component_meta` builds `IndexedReady`.
    // This reproduces the codex precondition exactly: "an owner
    // already has a `route_owned_shallow` entry from an earlier
    // route-only read".
    let route_owned = host.ensure_route_owned_shallow_entry("/src/Comp.vue");
    assert!(
        route_owned.is_some(),
        "route-only read must materialise a route_owned_shallow entry \
         for the owner SFC — the codex precondition",
    );
    assert!(
        host.project_type_store()
            .route_owned_shallow()
            .get_any("/src/Comp.vue")
            .is_some(),
        "the route_owned_shallow DB must hold the owner entry after \
         the route-only read",
    );

    // Cold `get_component_meta` — builds `IndexedReady`, runs the cold
    // resolver, publishes the `ComponentMetaResultEntry`. The pre-
    // existing route_owned_shallow entry persists.
    let prime = host.get_component_meta("/src/Comp.vue");
    assert!(prime.is_some(), "cold get_component_meta must resolve");

    // Discriminator 1 (pre/post) — the published signature must NOT
    // carry the owner's own `DerivedFactHash{Route}` fact. PRE-FIX the
    // publish site admitted the finalised tracer set verbatim, so the
    // owner's Route fact (observed by the macro-root route walk) IS
    // present and this assertion FAILS. POST-FIX
    // `strip_owner_route_fact` removes exactly this fact.
    let facts = published_facts(&host, "/src/Comp.vue");
    assert!(
        !has_owner_route_fact(&facts, "/src/Comp.vue"),
        "the owner's own `DerivedFactHash{{Route}}` fact MUST be \
         filtered from the published signature — it is dual-sourced \
         on `HostStoreView::derived_hashes` and does not round-trip \
         on warm validation. facts = {facts:#?}",
    );

    // Discriminator 2 (narrowness) — the cross-file route dep's
    // `DerivedFactHash{Route}` fact MUST still be present. A broad
    // route-fact removal would strip this too.
    assert!(
        facts.iter().any(|f| matches!(
            f,
            FactVersionRef::DerivedFactHash {
                canonical_id,
                kind: DerivedFactKind::Route,
                ..
            } if canonical_id == "/src/types.ts"
        )),
        "the cross-file route dep `/src/types.ts` `DerivedFactHash{{Route}}` \
         fact MUST remain in the published signature — `strip_owner_route_fact` \
         filters ONLY the owner's own Route fact, not cross-file route \
         facts. facts = {facts:#?}",
    );

    // Discriminator 3 (behavioural non-regression) — a repeated
    // IDENTICAL query is a warm-cache HIT with no new miss. With the
    // owner-Route fact filtered, the published signature round-trips,
    // so the second query reuses the warm result. (For a fixture
    // whose route-owned and indexed Route hashes happen to coincide
    // this would pass even pre-fix; the hard pre/post discriminator
    // is Discriminator 1. This assertion guards that the fix does not
    // regress warm reuse.)
    let prov = host.provenance();
    let hits_before = prov.component_meta_result_cache_hits.load(Relaxed);
    let misses_before = prov.component_meta_result_cache_misses.load(Relaxed);

    let second = host.get_component_meta("/src/Comp.vue");
    assert!(second.is_some(), "second identical query must resolve");

    let hits_after = prov.component_meta_result_cache_hits.load(Relaxed);
    let misses_after = prov.component_meta_result_cache_misses.load(Relaxed);
    assert!(
        hits_after > hits_before,
        "a repeated IDENTICAL `get_component_meta` query MUST be a \
         warm-cache HIT even when the owner has a route_owned_shallow \
         entry — the published signature must round-trip. \
         hits {hits_before} -> {hits_after}, misses {misses_before} -> {misses_after}",
    );
    assert_eq!(
        misses_before, misses_after,
        "a repeated IDENTICAL query MUST NOT register a new \
         `ComponentMetaResultDb` miss — a miss here means the warm \
         validation rejected the published signature. \
         misses {misses_before} -> {misses_after}",
    );

    // The warm-hit payload must be the genuine component meta — the
    // owner's `RProps` props surface.
    let second_meta = second.unwrap();
    let prop_names: Vec<&str> = second_meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"a") && prop_names.contains(&"b"),
        "the warm-hit payload must carry the owner's `RProps` props \
         (a + b) — got {prop_names:?}",
    );
}

#[test]
fn editing_route_dep_still_invalidates_with_route_owned_entry() {
    // The filter is narrow — it removes ONLY the owner's own Route
    // fact. Cross-file route facts stay, so editing a route dep MUST
    // still invalidate the owner's warm hit. This guards against the
    // filter being widened into the broad route-fact removal that
    // would silently drop cross-file route invalidation.
    let host = build_host();
    upsert(&host, "/src/types.ts", TYPES_TS, FileKind::NonSfc);
    upsert(&host, "/src/Comp.vue", OWNER_VUE, FileKind::VueSfc);

    // Plant the owner's route_owned_shallow entry first (codex
    // precondition), then prime the component-meta cache.
    let _ = host.ensure_route_owned_shallow_entry("/src/Comp.vue");
    let prime = host.get_component_meta("/src/Comp.vue");
    assert!(prime.is_some(), "prime call must resolve");

    let prov = host.provenance();
    let misses_before = prov.component_meta_result_cache_misses.load(Relaxed);

    // Edit the route source type — `RProps` loses `b`.
    upsert(
        &host,
        "/src/types.ts",
        "export interface RProps { a: number; }\n",
        FileKind::NonSfc,
    );

    let after = host.get_component_meta("/src/Comp.vue");
    assert!(after.is_some(), "post-edit call must still resolve");
    let misses_after = prov.component_meta_result_cache_misses.load(Relaxed);
    assert!(
        misses_after > misses_before,
        "editing the cross-file route dep MUST invalidate the owner's \
         warm hit — the dep's `DerivedFactHash{{Route}}` fact survives \
         `strip_owner_route_fact` (only the OWNER's own Route fact is \
         filtered). misses {misses_before} -> {misses_after}",
    );

    // The post-edit result reflects the NEW shape: `RProps` now has
    // only `a`. A stale warm hit would still report `b`.
    let after_meta = after.unwrap();
    let prop_names: Vec<&str> = after_meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"a") && !prop_names.contains(&"b"),
        "post-edit component-meta MUST reflect the new `RProps` shape \
         (a, no b) — got {prop_names:?}",
    );
}
