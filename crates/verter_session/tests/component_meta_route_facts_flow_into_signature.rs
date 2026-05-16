//! Block 1.J.2 — item 3 discriminator: route facts FLOW into the
//! published `ComponentMetaResultEntry` signature now that the
//! route-fact filtering workaround is removed.
//!
//! Pre-1.J.2: `get_component_meta`'s cold path published
//! `filter_owner_round_trippable_facts(resolved.fact_versions)`. That
//! workaround EXPLICITLY stripped the owner's
//! `FactVersionRef::DerivedFactHash { kind: Route }` entry from the
//! published signature, because the curated signature could carry a
//! non-round-tripping copy of it. Route facts therefore did NOT fully
//! flow into the published `facts` rail.
//!
//! Post-1.J.2: the signature is sourced from the finalized fact
//! tracer read set. The cold compute's macro-root route walk
//! (`RouteDb::get_or_resolve_route_observing_facts`) genuinely
//! observes the route's participant facts — including the owner's
//! `DerivedFactHash{Route}` — into the active tracer, and the
//! `filter_owner_round_trippable_facts` workaround is deleted. Every
//! route fact the cold compute actually observed lands in the
//! published `facts` rail unfiltered.
//!
//! Discrimination:
//!  1. `route_facts_flow_unfiltered_into_signature` asserts the
//!     published `facts` rail carries a `DerivedFactHash{Route}` fact
//!     for the OWNER canonical — the EXACT fact the deleted
//!     `filter_owner_round_trippable_facts` stripped. If the filter
//!     were still applied (or the curated source restored), the
//!     owner's Route fact would be absent and this assertion FAILS.
//!  2. `editing_route_dep_invalidates_warm_hit` asserts that editing
//!     the route's source type invalidates the warm hit and the
//!     post-edit result reflects the new shape — the route facts in
//!     the signature drive correct invalidation.

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
fn route_facts_flow_unfiltered_into_signature() {
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

    // EXACT discrimination: the OWNER's `DerivedFactHash{Route}` fact
    // — the precise fact `filter_owner_round_trippable_facts`
    // stripped — MUST now be present in the published `facts` rail.
    // The cold compute's macro-root route walk observes it; with the
    // filter deleted, it flows through unfiltered.
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
        !owner_route_facts.is_empty(),
        "Block 1.J.2 item 3: the owner's `DerivedFactHash{{Route}}` \
         fact MUST be present in the published `facts` rail — the \
         `filter_owner_round_trippable_facts` route-fact filter is \
         deleted, so the route facts the cold compute observed flow \
         in unfiltered. facts = {:#?}",
        sig.facts,
    );

    // The route-dep's Route fact must also be present — route facts
    // for every walked participant flow into the signature.
    assert!(
        sig.facts.iter().any(|f| matches!(
            f,
            FactVersionRef::DerivedFactHash {
                canonical_id,
                kind: DerivedFactKind::Route,
                ..
            } if canonical_id == "/src/types.ts"
        )),
        "Block 1.J.2 item 3: the route source dep's \
         `DerivedFactHash{{Route}}` fact MUST also be in the \
         published `facts` rail. facts = {:#?}",
        sig.facts,
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
    // route facts in the tracer-owned signature round-trip.
    let _ = host.get_component_meta("/src/Comp.vue");
    let hits_after = prov.component_meta_result_cache_hits.load(Relaxed);
    assert!(
        hits_after > hits_before,
        "warm sanity: unedited second call must hit the warm cache — \
         the tracer-owned route facts must round-trip \
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
        "Block 1.J.2 item 3: editing the route source type MUST \
         invalidate the owner's warm `ComponentMetaResultDb` hit — \
         the route facts flow into the published signature and \
         `validates_fact_signature` catches the change. \
         misses {misses_before} -> {misses_after}",
    );

    // The post-edit result must reflect the NEW shape: `RProps` now
    // has `a` AND `b`. A stale warm hit would report only `a`.
    let props = after.unwrap();
    let prop_names: Vec<&str> = props.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"a") && prop_names.contains(&"b"),
        "Block 1.J.2 item 3: post-edit component-meta MUST reflect \
         the new `RProps` shape (a + b) — got props {prop_names:?}",
    );
}
