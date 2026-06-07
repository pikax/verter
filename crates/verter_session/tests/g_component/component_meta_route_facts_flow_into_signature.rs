//! Route facts in the published `ComponentMetaResultEntry`
//! signature: CROSS-FILE route facts flow in, the OWNER's own Route
//! fact is filtered.
//!
//! The signature is sourced from the finalised fact tracer read set.
//! The cold compute's macro-root route walk genuinely observes the
//! route's participant facts — including the owner's own
//! `DerivedFactHash{Route}` — into the active tracer.
//!
//! The owner's own Route fact does not round-trip on warm validation:
//! `HostStoreView::build` dual-sources `derived_hashes[(owner, Route)]`
//! (the `IndexedReady` shallow state AND any `route_owned_shallow`
//! entry), and the two can disagree. `publish_component_meta_cache_entry`
//! therefore drops exactly the owner's own `DerivedFactHash{Route}`
//! fact via `strip_owner_route_fact` before cache admission. Cross-file
//! route facts — Route facts for the route DEPS the cold compute
//! walked — round-trip correctly and stay in the published signature.
//!
//! Discrimination:
//!  1. `cross_file_route_facts_flow_owner_route_filtered` asserts the
//!     published `facts` rail carries the route DEP's
//!     `DerivedFactHash{Route}` fact but NOT the owner's own. Without
//!     the publish-site filter the owner's Route fact would be present
//!     and the negative assertion would FAIL.
//!  2. `editing_route_dep_invalidates_warm_hit` asserts that editing
//!     the route's source type invalidates the warm hit and the
//!     post-edit result reflects the new shape — the cross-file route
//!     facts in the signature drive correct invalidation.

#![cfg(test)]

use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;

use verter_session::resolver_core::{DerivedFactKind, FactVersionRef};
use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};

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

const TYPES_A: &str = "export interface RProps { a: number; }\n";
const TYPES_B: &str = "export interface RProps { a: number; b: string; }\n";
const OWNER_VUE: &str = "<script setup lang=\"ts\">\n\
     import type { RProps } from './types';\n\
     defineProps<RProps>();\n\
     </script>\n\
     <template><div /></template>\n";

#[test]
fn cross_file_route_facts_flow_owner_route_filtered() {
    // `defineProps<RProps>()` over an imported type. Resolving the
    // macro root walks the named-type export route; the route walk
    // observes `DerivedFactHash{Route}` participant facts (including
    // the owner's, as the importer is a route participant) into the
    // fact tracer.
    let host = build_host();
    upsert(&host, "/src/types.ts", TYPES_A, FileKind::NonSfc);
    upsert(&host, "/src/Comp.vue", OWNER_VUE, FileKind::VueSfc);

    let meta = host.get_component_meta("/src/Comp.vue");
    assert!(meta.is_some(), "cold get_component_meta must resolve");

    let sig = verter_session::for_tests::component_meta_result_signature_for_owner(
        &host,
        "/src/Comp.vue",
    )
    .expect("a ComponentMetaResultEntry must be published for /src/Comp.vue");

    // The cross-file route dep's Route fact MUST be present — route
    // facts for every walked DEP flow into the signature unfiltered
    // and drive correct cross-file invalidation.
    assert!(
        sig.facts.iter().any(|f| matches!(
            f,
            FactVersionRef::DerivedFactHash {
                canonical_id,
                kind: DerivedFactKind::Route,
                ..
            } if canonical_id == "/src/types.ts"
        )),
        "the route source dep's `DerivedFactHash{{Route}}` fact MUST \
         be in the published `facts` rail — cross-file route facts \
         are NOT filtered. facts = {:#?}",
        sig.facts,
    );

    // EXACT discrimination: the OWNER's own `DerivedFactHash{Route}`
    // fact MUST be filtered from the published `facts` rail.
    // `publish_component_meta_cache_entry` strips it via
    // `strip_owner_route_fact` because the owner's own export route is
    // dual-sourced on `HostStoreView::derived_hashes` and does not
    // round-trip on warm validation. Without the publish-site filter
    // the owner's Route fact would be present and this assertion FAILS.
    let owner_route_facts: Vec<&FactVersionRef> = sig
        .facts
        .iter()
        .filter(|f| {
            matches!(
                f,
                FactVersionRef::DerivedFactHash {
                    canonical_id,
                    kind: DerivedFactKind::Route,
                    ..
                } if canonical_id == "/src/Comp.vue"
            )
        })
        .collect();
    assert!(
        owner_route_facts.is_empty(),
        "the owner's own `DerivedFactHash{{Route}}` fact MUST be \
         filtered from the published `facts` rail — it does not \
         round-trip on warm validation. The narrow `strip_owner_route_fact` \
         filter drops exactly this fact at cache admission. \
         offending facts = {owner_route_facts:#?}",
    );
}

#[test]
fn editing_route_dep_invalidates_warm_hit() {
    let host = build_host();
    upsert(&host, "/src/types.ts", TYPES_A, FileKind::NonSfc);
    upsert(&host, "/src/Comp.vue", OWNER_VUE, FileKind::VueSfc);

    // Prime — cold compute publishes the entry with the route facts.
    let prime = host.get_component_meta("/src/Comp.vue");
    assert!(prime.is_some(), "prime call must resolve");

    let prov = host.provenance();
    let hits_before = prov.component_meta_result_cache_hits.load(Relaxed);
    // Warm sanity: an unedited second call hits the warm cache — the
    // tracer-owned signature (with the owner-Route fact filtered)
    // round-trips.
    let _ = host.get_component_meta("/src/Comp.vue");
    let hits_after = prov.component_meta_result_cache_hits.load(Relaxed);
    assert!(
        hits_after > hits_before,
        "warm sanity: unedited second call must hit the warm cache — \
         the published signature must round-trip \
         (hits {hits_before} -> {hits_after})",
    );

    let misses_before = prov.component_meta_result_cache_misses.load(Relaxed);

    // Edit the route source type — `RProps` gains `b`.
    upsert(&host, "/src/types.ts", TYPES_B, FileKind::NonSfc);

    let after = host.get_component_meta("/src/Comp.vue");
    assert!(after.is_some(), "post-edit call must still resolve");
    let misses_after = prov.component_meta_result_cache_misses.load(Relaxed);
    assert!(
        misses_after > misses_before,
        "editing the route source type MUST invalidate the owner's \
         warm `ComponentMetaResultDb` hit — the cross-file route \
         facts flow into the published signature and \
         `validates_fact_signature` catches the change. \
         misses {misses_before} -> {misses_after}",
    );

    // The post-edit result must reflect the NEW shape: `RProps` now
    // has `a` AND `b`. A stale warm hit would report only `a`.
    let props = after.unwrap();
    let prop_names: Vec<&str> = props.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"a") && prop_names.contains(&"b"),
        "post-edit component-meta MUST reflect the new `RProps` shape \
         (a + b) — got props {prop_names:?}",
    );
}
