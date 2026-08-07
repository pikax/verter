//! The prepared-decl bundle's import-route rooting: the owner's
//! resolve-domain RESOLUTION WITNESS.
//!
//! ## Why this test exists
//!
//! A prepared-decl bundle is admitted for one owner and reused warm
//! until its recorded facts stop validating. Its import-route
//! dependency is the hard case: `import type { LateType } from
//! './late_dep'` where `/late_dep.ts` does not exist yet resolves to a
//! known miss, and when the dependency later appears the owner's own
//! bytes never change — so neither the owner's `FileWholeHash` nor its
//! `Route` surface digest moves. Without a rail that observes the
//! RESOLUTION itself, the bundle validates forever against a miss that
//! is no longer true.
//!
//! The legacy rail summarised the owner's resolved route table into a
//! `DerivedFactKind::ImportRoute` digest that the store-view build
//! recomputed for every published owner — re-resolving known-miss
//! specifiers to do so, which made view construction a resolution
//! producer and forced producer and validator to re-derive the same
//! digest from two different route-table source orders.
//!
//! The rail is now the witness itself: resolving the owner's authored
//! specifiers through the shared route-edge policy fans each sealed
//! transaction's derived Decision node onto the bundle's read set. A store
//! view validates those nodes against the immutable resolution world whose
//! DAG retains the exhausted probe set for a miss.
//!
//! ## Pinned properties
//!
//! 1. An admitted bundle for an owner with an unresolvable import
//!    records resolve-domain resolution facts — not just its own
//!    content root.
//! 2. Those recorded facts STOP validating once the dependency appears,
//!    with the owner's content untouched. This is the property the
//!    whole rail exists for.
//! 3. An unrelated appearance leaves them validating (path-precise, not
//!    a global file-set stamp).
//! 4. Re-reading the bundle through one view warm-hits — producer and
//!    validator agree by construction, so there is no re-materialise
//!    loop.

use std::sync::Arc;

use crate::resolver_core::{FactVersionRef, StoreView};
use crate::{HostConfig, UpsertRequest, VerterHost};

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

fn is_resolution_fact(fact: &FactVersionRef) -> bool {
    matches!(
        fact,
        FactVersionRef::ResolveImports(inner) if inner.resolution_fact().is_some()
    )
}

/// Every resolution fact of every admitted candidate signature for the
/// owner's bundle slot.
fn admitted_resolution_facts(host: &VerterHost, owner: &str) -> Vec<FactVersionRef> {
    host.resolver
        .runtime
        .prepared_decl_bundles
        .candidate_signatures_for_key(&owner.to_string())
        .into_iter()
        .flat_map(|signature| signature.to_vec())
        .filter(is_resolution_fact)
        .collect()
}

fn view_validates(host: &VerterHost, facts: &[FactVersionRef]) -> bool {
    let view = host.resolver_store_view_read().into_owned_view();
    facts.iter().all(|fact| StoreView::validates(&view, fact))
}

/// Owner importing a type from a specifier that does not resolve yet.
fn owner_with_absent_dependency(owner: &str) -> VerterHost {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        owner,
        "import type { LateType } from './late_dep';\n\
         export type Wrapper = { inner: LateType };\n",
    );
    let view = host.resolver_store_view_read().into_owned_view();
    let _bundle = host
        .prepared_decl_bundle_with_store_view(&view, None, owner)
        .expect("the owner's bundle must materialise");
    host
}

#[test]
fn bundle_admission_records_the_owner_resolution_witness() {
    let owner = "/proj_irw/owner.ts";
    let host = owner_with_absent_dependency(owner);

    let facts = admitted_resolution_facts(&host, owner);
    assert!(
        !facts.is_empty(),
        "the admitted bundle's fact signature must carry the owner's \
         resolve-domain resolution witness. Rooting on the owner's own \
         content alone leaves the bundle blind to a specifier that \
         becomes resolvable while the owner's bytes stay put — the exact \
         class the deleted ImportRoute digest existed to cover. \
         Recorded resolution facts: {facts:?}"
    );
}

#[test]
fn the_recorded_witness_stops_validating_once_the_dependency_appears() {
    let owner = "/proj_irw2/owner.ts";
    let host = owner_with_absent_dependency(owner);

    let facts = admitted_resolution_facts(&host, owner);
    assert!(
        !facts.is_empty(),
        "precondition: the bundle must have recorded a resolution witness"
    );
    assert!(
        view_validates(&host, &facts),
        "precondition: the witness must validate before the dependency appears"
    );

    // The late dependency appears. The owner's content does NOT change.
    upsert(
        &host,
        "/proj_irw2/late_dep.ts",
        "export type LateType = { resolved: true };\n",
    );

    assert!(
        !view_validates(&host, &facts),
        "ADMISSION RAIL REGRESSION: the bundle's recorded witness must \
         stop validating once `./late_dep` appears. The owner's bytes \
         never changed, so its `FileWholeHash` and `Route` facts are \
         unmoved — the resolution witness is the ONLY rail that can make \
         the appearance observable to a warm read."
    );
}

#[test]
fn an_unrelated_appearance_leaves_the_recorded_witness_valid() {
    let owner = "/proj_irw3/owner.ts";
    let host = owner_with_absent_dependency(owner);

    let facts = admitted_resolution_facts(&host, owner);
    assert!(
        !facts.is_empty(),
        "precondition: the bundle must have recorded a resolution witness"
    );

    upsert(
        &host,
        "/proj_irw3/unrelated.ts",
        "export type Unrelated = { other: true };\n",
    );

    assert!(
        view_validates(&host, &facts),
        "an unrelated appearance must leave the witness valid — the rail \
         is path-precise, not a global file-set stamp. A regression here \
         re-invalidates every edge-bearing owner on every new file."
    );
}

#[test]
fn a_second_bundle_read_through_one_view_warm_hits() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let owner = "/proj_irw4/owner.ts";
    upsert(
        &host,
        owner,
        "import type { LateType } from './late_dep';\n\
         export type Wrapper = { inner: LateType };\n",
    );
    // The dependency appears BEFORE the bundle is built, so the bundle's
    // witness observes the positive resolution.
    upsert(
        &host,
        "/proj_irw4/late_dep.ts",
        "export type LateType = { resolved: true };\n",
    );

    let view = host.resolver_store_view_read().into_owned_view();
    host.provenance().reset();

    let _first = host
        .prepared_decl_bundle_with_store_view(&view, None, owner)
        .expect("first prepared_decl_bundle call must materialise a bundle");
    assert_eq!(
        host.provenance().snapshot().bundle_materializations,
        1,
        "first prepared_decl_bundle call must materialise exactly 1 bundle"
    );

    let _second = host
        .prepared_decl_bundle_with_store_view(&view, None, owner)
        .expect("second prepared_decl_bundle call must return a bundle");
    let after_second = host.provenance().snapshot();
    assert_eq!(
        after_second.bundle_materializations, 1,
        "second prepared_decl_bundle call MUST warm-hit. Producer and \
         validator observe the SAME witness — the producer records the \
         fact versions it observed and the view compares them to the \
         world it captured, with no digest for the two sides to \
         re-derive differently. Observed bundle_materializations = {}",
        after_second.bundle_materializations
    );
    assert!(
        after_second.bundle_cache_hits >= 1,
        "second call must register at least one bundle cache hit; \
         observed cache hits = {}",
        after_second.bundle_cache_hits
    );
}

/// FAIL-CLOSED: an import-bearing owner whose witness is REFUSED must leave no
/// warm bundle candidate.
///
/// Decision facts make ordinary large witnesses bounded. This fixture uses
/// the typed test seam for the surviving upstream-refusal outcome and proves
/// the session producer still propagates it as `UnrootableRoute`. The bundle's
/// values are RESOLVED canonicals, so
/// the owner's own `FileWholeHash` is not a validity oracle for them: a
/// dependency appearing or retargeting moves no byte of the owner. A
/// bundle admitted on `FileWholeHash` alone therefore keeps serving its
/// pre-appearance edges forever — the precise invariant this rail exists
/// to establish.
///
/// The optional-extend shape that used to sit here (`if let Some(witness)
/// = … { facts.extend(witness) }` followed by an unconditional insert)
/// admits exactly that uninvalidatable candidate. `decline_import_route_witness`
/// marks the enclosing compute non-cacheable, but `insert_arc_with_kind`
/// never consults that mark — a lone `FileWholeHash` is a well-formed,
/// non-empty, non-overflowing signature — so the producer is the only
/// correct refusal point.
///
#[test]
fn an_unrootable_witness_leaves_no_warm_bundle_candidate() {
    const OWNER: &str = "/proj_irw_refused/owner.ts";

    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        OWNER,
        "import type { T } from '@absent/pkg/sub';\n\
         export type Wrapper = { inner: number };\n",
    );
    assert!(host.owner_import_route_witness_for_tests(OWNER).is_some());
    host.test_force
        .force_import_route_witness_refusal_for_tests
        .store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(
        host.owner_import_route_witness_for_tests(OWNER).is_none(),
        "the typed refusal seam must reach the producer's unrootable-witness arm"
    );

    let view = host.resolver_store_view_read().into_owned_view();
    let bundle = host.prepared_decl_bundle_with_store_view(&view, None, OWNER);
    assert!(
        bundle.is_some(),
        "the caller must still be SERVED its bundle — fail-closed refuses \
         ADMISSION, it does not refuse the answer"
    );

    let candidates = host
        .resolver
        .runtime
        .prepared_decl_bundles
        .candidate_signatures_for_key(&OWNER.to_string());
    assert!(
        candidates.is_empty(),
        "an owner whose import-route witness is unrootable must admit NO \
         warm bundle candidate. Admitted signatures: {candidates:?}"
    );
}

/// Anti-vacuity control for the case above: the SAME producer, with a
/// rootable witness, still admits. Without this, deleting the whole
/// admission would satisfy the refusal test.
#[test]
fn a_rootable_witness_still_admits_a_warm_bundle_candidate() {
    const OWNER: &str = "/proj_irw_rootable/owner.ts";
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        OWNER,
        "import type { LateType } from './late_dep';\n\
         export type Wrapper = { inner: LateType };\n",
    );
    assert!(
        host.owner_import_route_witness_for_tests(OWNER).is_some(),
        "fixture invariant: one unresolved specifier must stay well within \
         FACT_SIGNATURE_CAP and yield a rootable witness"
    );

    let view = host.resolver_store_view_read().into_owned_view();
    let _bundle = host
        .prepared_decl_bundle_with_store_view(&view, None, OWNER)
        .expect("the owner's bundle must materialise");

    assert!(
        !host
            .resolver
            .runtime
            .prepared_decl_bundles
            .candidate_signatures_for_key(&OWNER.to_string())
            .is_empty(),
        "the fail-closed gate must refuse ONLY the unrootable case — a \
         rootable witness must still warm the bundle slot"
    );
}

/// The component-meta sibling of the fail-closed gate above.
///
/// `append_dependency_fact_versions` composes its signature with the same
/// optional-extend shape (`if let Some(witness) = … { facts.extend(…) }`),
/// so read as source text it looks like the same defect. It is not: its
/// admission gate consults the fact tracer, and
/// `decline_import_route_witness` marks the enclosing compute
/// non-cacheable through `note_non_cacheable_read_fan_out`. The
/// prepared-decl producer had no such gate — `insert_arc_with_kind` takes
/// an explicit fact vector and never reads that mark, which is exactly why
/// it needed an explicit refusal.
///
/// The difference is behavioural, not textual, so it is pinned
/// behaviourally, with a control that discriminates the two arms.
#[test]
fn component_meta_admits_nothing_for_an_owner_with_an_unrootable_witness() {
    fn meta_candidates(owner: &str, force_refusal: bool) -> usize {
        let host = VerterHost::new_standalone(HostConfig::default());
        let source = "<script setup lang=\"ts\">\n\
                      import type { T } from '@absent/pkg/sub';\n\
                      defineProps<{ a: string }>()\n\
                      </script>\n<template><div/></template>\n";
        upsert(&host, owner, source);
        assert!(host.owner_import_route_witness_for_tests(owner).is_some());
        host.test_force
            .force_import_route_witness_refusal_for_tests
            .store(force_refusal, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(
            host.owner_import_route_witness_for_tests(owner).is_none(),
            force_refusal,
            "the fixture must select the intended witness disposition"
        );

        let _meta = host.get_component_meta(owner);
        host.derived_raw_cache()
            .get(owner)
            .map(|derived| derived.cached_resolved_meta.len())
            .unwrap_or(0)
    }

    assert_eq!(
        meta_candidates("/proj_irw_meta_open/Comp.vue", true),
        0,
        "an owner whose import-route witness is unrootable must leave no warm \
         component-meta candidate — the result's values are resolved \
         canonicals, and nothing left in the signature can invalidate them"
    );
    assert_eq!(
        meta_candidates("/proj_irw_meta_rootable/Comp.vue", false),
        1,
        "CONTROL: the same producer with a ROOTABLE witness must still admit. \
         Without this the assertion above would pass on a tree that simply \
         never caches component-meta at all."
    );
}

// Sibling fixture modules, declared here rather than in `lib.rs`: both
// pin behaviour of the same import-route-witness / prepared-decl-bundle
// rail this module documents, and `lib.rs` carries a line ceiling that
// exists to keep module declarations from accumulating there.
#[path = "resolution_signature_growth_tests.rs"]
mod resolution_signature_growth_tests;

#[path = "non_cacheable_bundle_reuse_tests.rs"]
mod non_cacheable_bundle_reuse_tests;

#[path = "request_only_reuse_tests.rs"]
mod request_only_reuse_tests;

#[path = "parse_env_asymmetry_tests.rs"]
mod parse_env_asymmetry_tests;

#[path = "file_language_capability_tests.rs"]
mod file_language_capability_tests;
