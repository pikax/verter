//! Positive-resolution growth of an owner's import-route resolution
//! signature after Decision-DAG compaction.
//!
//! ## What this pins, and why it is POSITIVE-resolution
//!
//! An owner's import-route witness carries one derived `Decision` fact per
//! authored specifier rather than flattening every resolution's transitive
//! leaf set. Declaration-companion resolution drives two queries per authored
//! chunk (the authored `.mjs` request and its resolved `.d.mts` target), so the
//! measured 180-specifier fixture carries 360 decision facts. That stays below
//! `FACT_SIGNATURE_CAP`, roots the prepared declaration bundle, and lets
//! component-meta reuse its warm result.
//!
//! The driver is NOT mass negative probing. In the measured nuxt-ui
//! corpus 24 of 25 `./_chunks/*.mjs` specifiers RESOLVE, through the
//! declaration-companion substitution in
//! `verter_workspace::resolver::resolve_declaration_companion`
//! (`.mjs` -> `.d.mts`): the runtime `.mjs` target is absent from the
//! published package while the `.d.mts` sibling is present. This fixture
//! reproduces exactly that shape — every specifier resolves positively —
//! so the growth it demonstrates is the one the corpus actually pays.
//! This is deliberately the old cardinality-fixture size. On the flat-leaf
//! profile, 180 positive specifiers produced 1,084 observations and refused
//! admission. Keeping the same corpus shape makes the test discriminate DAG
//! compaction rather than a smaller workload.
//!
//! ## Required profile
//!
//! | Observable | Required (`SIG-1`, `SIG-3`, `PD-1`, `PERF-1`) |
//! |---|---|
//! | `owner_import_route_observation_count_for_tests` (180 chunks) | 360: two decisions per authored declaration-companion specifier |
//! | `owner_import_route_witness_for_tests` | `Some`; cardinality alone does not refuse |
//! | second direct prepared-bundle lookup | cache hit, zero new cold flights |
//! | warm `get_component_meta` pass | zero new cold flights, exactly one result-cache hit |

use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::{HostConfig, UpsertRequest, VerterHost};

/// The pre-compaction refusal-fixture size. The post-DAG calibration is two
/// decision facts per authored specifier, so this remains comfortably bounded.
const LARGE_SPECIFIERS: usize = 180;

/// Specifier count for the anti-vacuity control. Far enough below the
/// cap that the same shape roots and warms.
const SMALL_SPECIFIERS: usize = 8;

fn decision_fact_count(facts: &[crate::resolver_core::FactVersionRef]) -> usize {
    facts
        .iter()
        .filter(|fact| {
            matches!(
                fact,
                crate::resolver_core::FactVersionRef::ResolveImports(
                    crate::resolver_core::ResolveImportsFactRef::Resolution(resolution)
                ) if resolution.is_decision()
            )
        })
        .count()
}

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

/// The corpus shape: `./_chunks/cN.mjs` authored, the runtime `.mjs`
/// ABSENT, the `.d.mts` declaration sibling PRESENT.
fn chunk_declaration(root: &str, index: usize) -> String {
    format!("{root}/_chunks/c{index}.d.mts")
}

fn seed_chunks(host: &VerterHost, root: &str, count: usize) -> String {
    let mut imports = String::new();
    for index in 0..count {
        upsert(
            host,
            &chunk_declaration(root, index),
            &format!("export declare const V{index}: number;\n"),
        );
        imports.push_str(&format!(
            "import {{ V{index} }} from './_chunks/c{index}.mjs';\n"
        ));
    }
    imports
}

fn ts_owner(root: &str, count: usize) -> (VerterHost, String) {
    let host = VerterHost::new_standalone(HostConfig::default());
    let mut source = seed_chunks(&host, root, count);
    source.push_str("export type Wrapper = { inner: number };\n");
    let owner = format!("{root}/owner.ts");
    upsert(&host, &owner, &source);
    (host, owner)
}

fn vue_owner(root: &str, count: usize) -> (VerterHost, String) {
    let host = VerterHost::new_standalone(HostConfig::default());
    let mut source = String::from("<script setup lang=\"ts\">\n");
    source.push_str(&seed_chunks(&host, root, count));
    source.push_str("defineProps<{ a: string }>()\n</script>\n<template><div/></template>\n");
    let owner = format!("{root}/Comp.vue");
    upsert(&host, &owner, &source);
    (host, owner)
}

/// FIXTURE INVARIANT: every authored `./_chunks/cN.mjs` really does
/// resolve, and resolves to the `.d.mts` companion. Without this the
/// over-cap fixture below would be indistinguishable from the existing
/// unresolvable-bare-specifier overflow fixture.
fn assert_every_chunk_resolves_positively(
    host: &VerterHost,
    owner: &str,
    root: &str,
    count: usize,
) {
    for index in 0..count {
        let specifier = format!("./_chunks/c{index}.mjs");
        let publication = host.generation_current_route_resolution(owner, &specifier, None);
        let verter_workspace::ResolutionPublication::Admitted(admitted) = publication else {
            panic!(
                "fixture invariant: {specifier} must be ADMITTED — this fixture stages \
                 POSITIVE resolution growth, not the unresolvable-specifier overflow the \
                 negative fixture already covers"
            );
        };
        assert_eq!(
            admitted.into_result().as_deref(),
            Some(chunk_declaration(root, index).as_str()),
            "fixture invariant: {specifier} must resolve through the declaration-companion \
             substitution to its .d.mts sibling (the runtime .mjs is absent, exactly as in \
             the measured nuxt-ui corpus)"
        );
    }
}

/// The former over-cap positive fixture now carries two bounded Decisions per
/// declaration-companion specifier, stays rootable, and warm-hits the
/// prepared-bundle rail.
#[test]
fn large_positive_chunk_owner_is_compacted_and_warms() {
    const ROOT: &str = "/rc_growth_over";
    const UNIT_ROOT: &str = "/rc_growth_unit";

    let (host, owner) = ts_owner(ROOT, LARGE_SPECIFIERS);
    assert_every_chunk_resolves_positively(&host, &owner, ROOT, LARGE_SPECIFIERS);

    // Per-specifier cost, measured from a one-specifier owner of the
    // SAME shape, so the headroom below is stated in specifiers.
    let (unit_host, unit_owner) = ts_owner(UNIT_ROOT, 1);
    let per_specifier = unit_host
        .owner_import_route_observation_count_for_tests(&unit_owner)
        .expect("fixture invariant: one positive chunk specifier must be observed, not refused");
    assert!(
        per_specifier > 0,
        "fixture invariant: one positively-resolving chunk specifier must observe at least \
         one fact"
    );

    let observed = host
        .owner_import_route_observation_count_for_tests(&owner)
        .expect(
            "fixture invariant: every specifier resolved, so the observation set must be \
             BUILT; a `None` here means a resolution was REFUSED and the fixture is \
             measuring the wrong failure",
        );
    assert_eq!(
        per_specifier, 2,
        "post-DAG calibration: one positive declaration-companion chunk contributes two \
         Decision facts"
    );
    assert_eq!(
        observed,
        LARGE_SPECIFIERS * per_specifier,
        "the owner witness must grow by the bounded post-DAG unit; observed {observed}"
    );
    assert!(
        observed <= verter_workspace::FACT_SIGNATURE_CAP,
        "Decision compaction must keep the {LARGE_SPECIFIERS}-specifier witness within \
         FACT_SIGNATURE_CAP ({}); observed {observed}",
        verter_workspace::FACT_SIGNATURE_CAP
    );
    let witness = host
        .owner_import_route_witness_for_tests(&owner)
        .expect("a large fully-resolved owner must retain a rootable witness");
    assert_eq!(
        decision_fact_count(&witness),
        LARGE_SPECIFIERS * per_specifier,
        "every bounded witness fact must be a derived Decision node"
    );

    let first_view = host.resolver_store_view_read().into_owned_view();
    assert!(
        host.prepared_decl_bundle_with_store_view(&first_view, None, &owner)
            .is_some(),
        "the first prepared bundle must materialise"
    );
    let hits_before = host.provenance().bundle_cache_hits.load(Ordering::Relaxed);
    let flights_before = host
        .provenance()
        .bundle_cold_flight_runs
        .load(Ordering::Relaxed);
    let second_view = host.resolver_store_view_read().into_owned_view();
    assert!(
        host.prepared_decl_bundle_with_store_view(&second_view, None, &owner)
            .is_some(),
        "the warm prepared-bundle lookup must resolve"
    );
    assert_eq!(
        host.provenance().bundle_cache_hits.load(Ordering::Relaxed),
        hits_before + 1,
        "the second lookup through a fresh store view must hit the prepared-bundle cache"
    );
    assert_eq!(
        host.provenance()
            .bundle_cold_flight_runs
            .load(Ordering::Relaxed),
        flights_before,
        "the warm prepared-bundle lookup must run no new cold flight"
    );
    let candidates = host
        .resolver
        .runtime
        .prepared_decl_bundles
        .candidate_signatures_for_key(&owner.to_string());
    assert!(
        candidates.iter().any(|facts| {
            decision_fact_count(facts) == LARGE_SPECIFIERS * per_specifier
                && facts.iter().any(|fact| {
                    matches!(
                        fact,
                        crate::resolver_core::FactVersionRef::FileWholeHash {
                            canonical_id,
                            ..
                        } if canonical_id == &owner
                    )
                })
        }),
        "the compacted witness must admit a candidate retaining all Decision nodes and the \
         owner's precise file self-root; candidates: {candidates:?}"
    );
}

/// ANTI-VACUITY CONTROL for the case above: the same positive-chunk
/// shape BELOW the cap roots and warms. Without this, a tree that simply
/// never admits a prepared-decl bundle would satisfy the refusal
/// assertions.
#[test]
fn below_cap_positive_chunk_owner_roots_and_admits() {
    const ROOT: &str = "/rc_growth_under";
    let (host, owner) = ts_owner(ROOT, SMALL_SPECIFIERS);
    assert_every_chunk_resolves_positively(&host, &owner, ROOT, SMALL_SPECIFIERS);

    let observed = host
        .owner_import_route_observation_count_for_tests(&owner)
        .expect("the control owner's observation set must be built");
    assert!(
        observed <= verter_workspace::FACT_SIGNATURE_CAP,
        "control invariant: {SMALL_SPECIFIERS} chunk specifiers must stay within \
         FACT_SIGNATURE_CAP ({}); observed {observed}",
        verter_workspace::FACT_SIGNATURE_CAP
    );
    assert!(
        host.owner_import_route_witness_for_tests(&owner).is_some(),
        "the control owner's witness must be rootable — the refusal above must be caused by \
         cardinality, not by the chunk shape"
    );

    let view = host.resolver_store_view_read().into_owned_view();
    let _bundle = host
        .prepared_decl_bundle_with_store_view(&view, None, &owner)
        .expect("the control owner's bundle must materialise");
    assert!(
        !host
            .resolver
            .runtime
            .prepared_decl_bundles
            .candidate_signatures_for_key(&owner.to_string())
            .is_empty(),
        "the control owner must still warm its bundle slot"
    );
}

/// End-to-end consequence at the public boundary: the former over-cap
/// positive-chunk component takes one cold pass and then warm-hits.
#[test]
fn large_positive_chunk_owner_reuses_component_meta_after_first_pass() {
    const ROOT: &str = "/rc_growth_cm_over";
    let (host, owner) = vue_owner(ROOT, LARGE_SPECIFIERS);
    assert_every_chunk_resolves_positively(&host, &owner, ROOT, LARGE_SPECIFIERS);
    let witness = host
        .owner_import_route_witness_for_tests(&owner)
        .expect("the large SFC owner's compacted witness must be rootable");
    assert_eq!(
        decision_fact_count(&witness),
        LARGE_SPECIFIERS * 2,
        "the SFC witness must carry the two Decision nodes driven by each authored chunk"
    );

    let mut passes = Vec::new();
    for _ in 0..3 {
        let flights_before = host
            .provenance()
            .bundle_cold_flight_runs
            .load(Ordering::Relaxed);
        let hits_before = host
            .provenance()
            .component_meta_result_cache_hits
            .load(Ordering::Relaxed);
        let meta = host.get_component_meta(&owner);
        assert!(
            meta.is_some(),
            "the caller is still SERVED its component meta — refusal is about ADMISSION"
        );
        let flights = host
            .provenance()
            .bundle_cold_flight_runs
            .load(Ordering::Relaxed)
            - flights_before;
        let hits = host
            .provenance()
            .component_meta_result_cache_hits
            .load(Ordering::Relaxed)
            - hits_before;
        passes.push((flights, hits));
    }

    let (first_flights, _) = passes[0];
    assert!(
        first_flights > 0,
        "fixture invariant: the cold pass must run at least one cold bundle flight"
    );
    for (index, (flights, hits)) in passes.iter().enumerate().skip(1) {
        assert_eq!(
            *flights, 0,
            "warm pass {index} must run zero additional cold bundle flights"
        );
        assert_eq!(
            *hits, 1,
            "warm pass {index} must take exactly one component-meta result-cache hit"
        );
    }

    assert!(
        host.derived_raw_cache()
            .get(&owner)
            .map(|derived| derived.cached_resolved_meta.len())
            .unwrap_or(0)
            > 0,
        "the compacted witness must leave a warm component-meta candidate"
    );
}

/// ANTI-VACUITY CONTROL: the same SFC shape BELOW the cap warms — its
/// second pass performs ZERO additional cold bundle flights and takes a
/// component-meta result cache hit. This is the exact post-change
/// behaviour required of the over-cap case, proven reachable on this
/// tree, so the test above cannot be satisfied by a tree that never
/// warms component meta at all.
#[test]
fn below_cap_positive_chunk_owner_warms_with_no_additional_cold_flight() {
    const ROOT: &str = "/rc_growth_cm_under";
    let (host, owner) = vue_owner(ROOT, SMALL_SPECIFIERS);
    // Same reason as the over-cap SFC case: the control must prove the
    // warm profile for the POSITIVE shape, not for whatever shape a
    // broken declaration-companion substitution leaves behind.
    assert_every_chunk_resolves_positively(&host, &owner, ROOT, SMALL_SPECIFIERS);
    assert!(
        host.owner_import_route_witness_for_tests(&owner).is_some(),
        "control invariant: the below-cap SFC owner's witness must be rootable"
    );

    assert!(
        host.get_component_meta(&owner).is_some(),
        "the cold pass must resolve"
    );

    let flights_before = host
        .provenance()
        .bundle_cold_flight_runs
        .load(Ordering::Relaxed);
    let hits_before = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Ordering::Relaxed);
    assert!(
        host.get_component_meta(&owner).is_some(),
        "the warm pass must resolve"
    );
    assert_eq!(
        host.provenance()
            .bundle_cold_flight_runs
            .load(Ordering::Relaxed),
        flights_before,
        "control: a rootable owner's warm pass must perform ZERO additional cold bundle \
         flights"
    );
    assert_eq!(
        host.provenance()
            .component_meta_result_cache_hits
            .load(Ordering::Relaxed),
        hits_before + 1,
        "control: a rootable owner's warm pass must take exactly one component-meta result \
         cache hit"
    );
}
