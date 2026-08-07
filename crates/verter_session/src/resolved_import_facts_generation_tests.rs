//! Clock semantics for the `SemanticImports` compaction domain.
//!
//! The domain's terminal aggregate replaces every precise semantic-import
//! fact a scope observed with one claim: "the domain held as of this
//! generation". That claim is only worth anything if the generation moves
//! for exactly the events that can change what a recorded fact depends
//! on — and stays put for the events that cannot.
//!
//! Both halves are asserted, because each is independently satisfiable by
//! a degenerate implementation: a counter that never moves passes the
//! no-advance tests, and one that moves on every touch passes the advance
//! tests.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::resolved_import_facts::{
    ResolvedImportFacts, ResolvedImportFactsKey, RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
};
use crate::resolver_core::FactVersionRef;
use crate::types::{DependencyResolution, FileLanguage, HostConfig, UpsertRequest};
use crate::VerterHost;

fn make_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig::default()))
}

fn upsert_ts(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert ts");
}

fn owner_routes() -> FxHashMap<String, DependencyResolution> {
    let mut routes = FxHashMap::default();
    routes.insert(
        "./dep".to_string(),
        DependencyResolution {
            specifier: "./dep".to_string(),
            resolved_canonical_id: Some("/dep.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        },
    );
    routes
}

fn producer_key(host: &VerterHost, canonical: &str) -> ResolvedImportFactsKey {
    let content_hash = host
        .current_or_read_whole_hash(canonical)
        .expect("owner content hash");
    let env = host.host_view_env_hashes_for(canonical);
    ResolvedImportFactsKey {
        canonical: Arc::from(canonical),
        content_hash,
        parse_env_hash: env.parse_env_hash,
        resolve_env_hash: env.resolve_env_hash,
        resolver_version: RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
    }
}

/// A two-file owner/dep host whose producer has already run once.
fn seeded_host() -> Arc<VerterHost> {
    let host = make_host();
    upsert_ts(host.as_ref(), "/dep.ts", "export const a = 1;\n");
    upsert_ts(
        host.as_ref(),
        "/owner.ts",
        "import { a } from './dep'\nexport const b = a;\n",
    );
    assert!(
        host.admit_resolved_import_facts_for_owner("/owner.ts", &owner_routes()),
        "fixture invariant: the cold producer run must admit a candidate"
    );
    host
}

/// A candidate ENTERING the slot advances the domain generation.
///
/// Without this the aggregate is a witness nothing can invalidate: a
/// scope that compacted its semantic-import facts would keep validating
/// across every subsequent admission.
///
/// Mutation recipe: in `ResolvedImportFactsDb::admit`, return
/// `(admitted, false)` from the `mutate` closure instead of
/// `(admitted, admitted)`. This test fails while the refusal and skip
/// controls below stay green.
#[test]
fn a_candidate_entering_the_slot_advances_the_domain_generation() {
    let host = seeded_host();
    let db = host.project_type_store().resolved_import_facts();
    let key = producer_key(&host, "/owner.ts");

    let before = db
        .stable_generation()
        .expect("a quiescent store reports a stable generation");

    let witness: Vec<FactVersionRef> = host
        .resolved_import_facts_witness_for(key.canonical.as_ref(), key.content_hash)
        .expect("the production witness must be rootable for the owner");
    let admitted = db.admit(key, Arc::new(ResolvedImportFacts::new()), witness);
    assert!(
        admitted,
        "fixture invariant: this admission must succeed, or the assertion below is about a \
         refusal rather than about an admission"
    );

    assert_ne!(
        db.stable_generation(),
        Some(before),
        "a candidate entered the slot, so every scope that compacted this domain before now \
         describes a membership that no longer exists and must be able to detect it"
    );
}

/// A REFUSED admission does not advance the generation.
///
/// Nothing entered the slot, so advancing would refuse every concurrent
/// scope's compaction while describing no change at all.
///
/// Mutation recipe: return `(admitted, true)` from `admit`'s `mutate`
/// closure. This test fails while the advance test above stays green.
#[test]
fn a_refused_admission_does_not_advance_the_domain_generation() {
    let host = seeded_host();
    let db = host.project_type_store().resolved_import_facts();
    let key = producer_key(&host, "/owner.ts");

    let before = db.stable_generation().expect("quiescent");

    // Strict admission refuses an EMPTY witness: nothing enters the slot.
    let refused = db.admit(
        key.clone(),
        Arc::new(ResolvedImportFacts::new()),
        Vec::new(),
    );
    assert!(
        !refused,
        "fixture invariant: an empty witness must be REFUSED under strict admission, or this \
         test is measuring a successful admission"
    );

    assert_eq!(
        db.stable_generation(),
        Some(before),
        "a refused admission changed no membership, so advancing for it would cost every \
         concurrent scope its compaction and describe nothing"
    );
}

/// A producer recomputation that reproduces a retained candidate WHOLE
/// does not advance the generation — the dedupe returns before any
/// admission, so there is no membership change to describe.
///
/// This is the hot case: a bundler re-pushes identical import
/// dependencies on every build. If it advanced, warm reuse of every
/// compacted semantic-import witness would be destroyed by a no-op.
///
/// Mutation recipe: delete the `holds_candidate_matching` early return in
/// `admit_resolved_import_facts_for_owner`. This test fails.
#[test]
fn an_identical_producer_recomputation_does_not_advance_the_domain_generation() {
    let host = seeded_host();
    let db = host.project_type_store().resolved_import_facts();

    let before = db.stable_generation().expect("quiescent");

    let readmitted = host.admit_resolved_import_facts_for_owner("/owner.ts", &owner_routes());
    assert!(
        !readmitted,
        "fixture invariant: an identical recomputation must be SKIPPED by the producer's \
         dedupe, or this test measures a genuine admission"
    );

    assert_eq!(
        db.stable_generation(),
        Some(before),
        "pure producer churn changed no membership; advancing here would make every rebuild \
         invalidate every compacted semantic-import witness in the host"
    );
}

/// A `clear` advances the generation.
///
/// Removal is a membership change every bit as much as admission, and a
/// compacted witness that survived one would vouch for candidates the
/// store no longer holds. The store has no production `clear` caller
/// today, which is exactly why the rule is pinned here rather than left
/// to be rediscovered by whoever adds one.
///
/// Mutation recipe: return `((), false)` from `clear`'s `mutate` closure.
/// This test fails.
#[test]
fn clearing_the_store_advances_the_domain_generation() {
    let host = seeded_host();
    let db = host.project_type_store().resolved_import_facts();

    let before = db.stable_generation().expect("quiescent");
    assert!(
        !db.is_empty(),
        "fixture invariant: the store must hold something for the clear to remove"
    );

    db.clear();

    assert_ne!(
        db.stable_generation(),
        Some(before),
        "a clear removed every candidate a compacted witness could have stood for, so a scope \
         holding one must be able to detect it"
    );
}

/// An eviction is NOT a second validity dimension: it happens inside the
/// same insertion that caused it, so the one advance that insertion makes
/// covers it.
///
/// The store retains at most `CANDIDATE_CAP` candidates per key and
/// drains the oldest inside the same `rcu` that pushes the new one. This
/// pins that there is no admit-free window in which a candidate silently
/// ages out — five distinct admissions produce five advances, not five
/// plus an extra for the eviction.
#[test]
fn eviction_rides_the_insertion_that_causes_it_rather_than_advancing_separately() {
    use verter_workspace::CANDIDATE_CAP;

    let host = seeded_host();
    let db = host.project_type_store().resolved_import_facts();
    let key = producer_key(&host, "/owner.ts");
    let witness: Vec<FactVersionRef> = host
        .resolved_import_facts_witness_for(key.canonical.as_ref(), key.content_hash)
        .expect("rootable witness");

    let before = db.stable_generation().expect("quiescent");

    // More admissions than the slot can hold, so at least one eviction
    // happens. `admit` itself performs no dedupe — the skip lives in the
    // producer — so every one of these enters the slot.
    let admissions = CANDIDATE_CAP + 2;
    for _ in 0..admissions {
        assert!(
            db.admit(
                key.clone(),
                Arc::new(ResolvedImportFacts::new()),
                witness.clone()
            ),
            "each admission must enter the slot"
        );
    }
    assert_eq!(
        db.candidate_signatures_for_tests(&key).len(),
        CANDIDATE_CAP,
        "fixture invariant: the slot must be capped, or no eviction happened and this test is \
         not about eviction at all"
    );

    let after = db.stable_generation().expect("quiescent");
    assert_eq!(
        after - before,
        (admissions as u64) * 2,
        "each admission advances the generation exactly once (the counter steps by two per \
         advance, one for entering the in-flight window and one for leaving it). A larger delta \
         would mean eviction advanced separately — a second dimension the errata established \
         does not exist, because the drain happens inside the same insertion transaction."
    );
}
