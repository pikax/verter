//! Discriminator: the published `ComponentMetaResultEntry` signature's
//! `facts` rail is sourced from the FINALIZED fact-tracer read set, NOT
//! the curated `resolved.fact_versions`.
//!
//! A cold path that discarded the traced facts on
//! `FactReadSetFinalise::Ok` and published
//! `filter_owner_round_trippable_facts(resolved.fact_versions)` — the
//! curated accumulator set, route-filtered — could NOT produce a
//! published `facts` rail equal to `read_set.finalise()`: it would carry
//! `DerivedFactHash` Route/ImportRoute entries the tracer never
//! observes, and the route-filter would drop the owner's Route entry.
//!
//! Instead, the `FactReadSetFinalise::Ok(facts)` payload — minus
//! the owner's own non-round-tripping `DerivedFactHash{Route}` fact —
//! IS the published `read_set_signature.facts` rail. The signature is
//! sourced from the finalized tracer read set, then
//! `publish_component_meta_cache_entry` applies the narrow
//! `strip_owner_route_fact` filter at cache admission: exactly the
//! owner's own Route fact is dropped (it is dual-sourced on
//! `HostStoreView::derived_hashes` and does not round-trip on warm
//! validation). Every OTHER traced fact — cross-file route facts,
//! `FileWholeHash` facts, `Parse` / `ResolveImports` / `RouteSurface`
//! facts — lands in the published `facts` rail unchanged.
//!
//! Discrimination: this test asserts EXACT set equality between the
//! published `facts` rail and the finalized tracer read set with the
//! owner-Route fact removed. If the producer reverted to sourcing the
//! signature from `resolved.fact_versions`, the two sets would diverge
//! (the curated set carries `DerivedFactHash` variants absent from the
//! traced set) and `assert_eq!` on the sorted fact vectors would FAIL.

#![cfg(test)]

use std::sync::Arc;

use verter_session::resolver_core::{DerivedFactKind, FactReadSetFinalise, FactVersionRef};
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};

fn build_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig::default()))
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

/// Canonical-ordering sort for `FactVersionRef` so two fact vectors
/// observed/curated in different orders compare structurally. The
/// tracer's `finalise()` already sorts; the published rail mirrors
/// that sorted vector, but we sort defensively so the assertion is
/// order-independent.
fn sorted(facts: &[FactVersionRef]) -> Vec<String> {
    let mut v: Vec<String> = facts.iter().map(|f| format!("{f:?}")).collect();
    v.sort();
    v
}

/// `/src/types.ts` content — a cross-file dep imported by the owner.
const TYPES_TS: &str = "export interface Foo { a: number; b: string; }\n";
/// Owner SFC: `defineProps<Foo>()` over the imported `Foo`, single
/// native `<div>` root (no child-component fallthrough recursion).
const COMP_VUE: &str = "<script setup lang=\"ts\">\n\
     import type { Foo } from './types';\n\
     defineProps<Foo>();\n\
     </script>\n\
     <template><div /></template>\n";

#[test]
fn published_component_meta_signature_equals_finalized_tracer_read_set() {
    let host = build_host();
    // Cross-file `defineProps<Foo>()` — the cold compute observes the
    // dep `/src/types.ts` through the resolver tier; the tracer
    // captures the full read set.
    upsert(&host, "/src/types.ts", TYPES_TS, FileLanguage::script_ts());
    upsert(&host, "/src/Comp.vue", COMP_VUE, FileLanguage::vue());

    // Prime: cold compute publishes the entry. The publish path
    // sources the `facts` rail from the finalized tracer read set.
    let primed = host.get_component_meta("/src/Comp.vue");
    assert!(primed.is_some(), "prime call must resolve a component");

    // Read the published entry's `facts` rail.
    let published = verter_session::for_tests::component_meta_result_signature_for_owner(
        &host,
        "/src/Comp.vue",
    )
    .expect("a ComponentMetaResultEntry must be published for /src/Comp.vue");
    assert!(
        !published.facts.is_empty(),
        "the published `facts` rail must be non-empty — a cross-file \
         `defineProps<Foo>()` cold compute observes at least the dep \
         file's facts through the tracer",
    );

    // Compute the finalized tracer read set on a SEPARATE fresh host
    // with byte-identical fixtures, as that host's FIRST operation —
    // so the trace is a genuine cold compute (no warm
    // resolved-meta cache short-circuit skewing the observed reads).
    // Identical content ⇒ identical content hashes ⇒ the traced fact
    // set must equal the first host's published `facts` rail.
    let trace_host = build_host();
    upsert(
        &trace_host,
        "/src/types.ts",
        TYPES_TS,
        FileLanguage::script_ts(),
    );
    upsert(&trace_host, "/src/Comp.vue", COMP_VUE, FileLanguage::vue());
    let traced = verter_session::for_tests::component_meta_cold_traced_read_set_for_tests(
        &trace_host,
        "/src/Comp.vue",
    )
    .expect("cold traced read set must compute for /src/Comp.vue");
    let traced_facts: Vec<FactVersionRef> = match traced {
        FactReadSetFinalise::Ok(facts) => facts.to_vec(),
        FactReadSetFinalise::Overflow => {
            panic!("this small fixture must not overflow the fact-signature cap")
        }
    };

    // The publish site applies the narrow `strip_owner_route_fact`
    // filter at cache admission — exactly the owner's own
    // `DerivedFactHash{Route}` fact is dropped. To compare the
    // published rail against the tracer read set, remove the same one
    // fact from the traced set. This is the ONLY fact the publish
    // site filters; every other traced fact is published verbatim.
    let traced_minus_owner_route: Vec<FactVersionRef> = traced_facts
        .iter()
        .filter(|f| {
            !matches!(
                f,
                FactVersionRef::DerivedFactHash {
                    canonical_id,
                    kind: DerivedFactKind::Route,
                    ..
                } if canonical_id == "/src/Comp.vue"
            )
        })
        .cloned()
        .collect();

    // EXACT set equality — tracer-owned-signature discrimination. The
    // published `facts` rail IS the finalized tracer read set with the
    // owner-Route fact stripped: `get_component_meta`'s cold path takes
    // the `FactReadSetFinalise::Ok(facts)` payload, and
    // `publish_component_meta_cache_entry` removes only the owner's own
    // Route fact. A producer that instead sourced the signature from
    // the curated `resolved.fact_versions` would publish a DIFFERENT
    // set — the curated path drains the dispatch accumulator and adds
    // blanket `DerivedFactHash` Route/ImportRoute entries per
    // `current_dependency_fact_versions`, a set that does not match
    // what the tracer accumulated. The sorted vectors would diverge
    // and this `assert_eq!` would FAIL.
    assert_eq!(
        sorted(&published.facts),
        sorted(&traced_minus_owner_route),
        "the published `ComponentMetaResultEntry` `facts` \
         rail MUST equal the finalized fact-tracer read set minus the \
         owner's own `DerivedFactHash{{Route}}` fact. \
         published={:#?} traced(minus owner-Route)={:#?}",
        sorted(&published.facts),
        sorted(&traced_minus_owner_route),
    );

    // Warm sanity: an identical second call must hit the warm cache
    // (the tracer-owned signature round-trips). If the published
    // signature carried a non-round-tripping fact, this would miss
    // and cold-recompute.
    let prov = host.provenance();
    let hits_before = prov
        .component_meta_result_cache_hits
        .load(std::sync::atomic::Ordering::Relaxed);
    let _ = host.get_component_meta("/src/Comp.vue");
    let hits_after = prov
        .component_meta_result_cache_hits
        .load(std::sync::atomic::Ordering::Relaxed);
    assert!(
        hits_after > hits_before,
        "an unedited second call must hit the \
         warm `ComponentMetaResultDb` cache — the tracer-owned \
         signature must round-trip. hits {hits_before} -> {hits_after}",
    );
}
