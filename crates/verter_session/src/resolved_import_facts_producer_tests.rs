//! Producer-side admission-dedupe invariants for
//! [`VerterHost::admit_resolved_import_facts_for_owner`].
//!
//! The producer skips re-admitting a recomputation that is pure churn. That
//! skip decision must name ONE candidate: the slot must retain a single
//! candidate carrying BOTH the witness the producer just observed AND the
//! payload it just built. A decision assembled from two independent slot
//! queries — "does ANY candidate hold this witness" and, separately, "is the
//! LAST candidate's payload equal to mine" — can be satisfied by two DIFFERENT
//! candidates, and in that state the producer drops a genuinely new
//! `(witness, payload)` pair that nothing in the slot retains.
//!
//! ## Scope of what this proves
//!
//! These fixtures prove CORRELATION — that the decision names one candidate
//! rather than combining two. They do NOT prove ATOMICITY against a
//! concurrent mutation; that property is held structurally by the
//! implementation (a single `entries.get` + `candidates.load()`, with the
//! conjunction evaluated per candidate and no second slot read), not by a
//! race-exercising test.
//!
//! ## Mutation recipe — applicable to the landed tree as written
//!
//! In `ResolvedImportFactsDb::holds_candidate_matching`, replace the body
//! `self.entries.holds_candidate_with_signature_and_value(key, facts, value)`
//! with the uncorrelated pair over the two substrate methods that still
//! exist:
//!
//! ```text
//! self.entries.holds_candidate_with_signature(key, facts)
//!     && self
//!         .entries
//!         .lookup_any_candidate(key)
//!         .is_some_and(|retained| retained.as_ref() == value)
//! ```
//!
//! ⇒ `producer_readmits_when_no_single_candidate_holds_both_the_witness_and_the_payload`
//! FAILS (the producer skips, and the slot never gains the pair) while the
//! churn control stays GREEN. Applied to the landed tree, confirmed present /
//! unique / new, run, and reverted by inverse edit.
//!
//! Note the recipe targets the DB method, not the producer: the producer's
//! former two-query form named `retained_bundle` and a db-level
//! `holds_candidate_with_signature` wrapper, and both were deleted as dead in
//! the same change that landed this guard — a recipe naming them would not
//! apply to this tree.

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

/// The owner's import routes, in the shape the production caller
/// (`set_import_dependencies`) hands the producer.
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

/// Recompose the key the producer composes for `canonical` — same env
/// accessor, same content hash source, same resolver version.
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

/// The producer's admission-dedupe decision must be CANDIDATE-CORRELATED: it
/// may skip only when ONE retained candidate carries both the witness the
/// producer observed and the payload it built.
///
/// The slot is arranged so that the two halves of the uncorrelated decision are
/// satisfied by two DIFFERENT candidates:
///
/// | candidate | witness  | payload  |
/// |-----------|----------|----------|
/// | first     | `W_real` | `P_other`|
/// | last      | `W_fake` | `P_real` |
///
/// The producer then recomputes `(W_real, P_real)` — a pair NO candidate holds.
/// "Any candidate holds `W_real`" is true (the first), and "the last
/// candidate's payload equals `P_real`" is also true (the last), so an
/// uncorrelated decision skips and the fresh pair is silently dropped. A
/// correlated decision finds no single candidate holding both and admits.
///
/// Discriminating: the arrangement asserts its own preconditions (two
/// candidates, the cross-pair genuinely absent, `P_other != P_real`), so the
/// test cannot pass vacuously by failing to set the trap. The post-state
/// assertion is independent of the skip decision itself — it reads the slot and
/// requires the `(W_real, P_real)` pair to be retained afterwards.
#[test]
fn producer_readmits_when_no_single_candidate_holds_both_the_witness_and_the_payload() {
    let host = make_host();
    upsert_ts(host.as_ref(), "/dep.ts", "export const a = 1;\n");
    upsert_ts(
        host.as_ref(),
        "/owner.ts",
        "import { a } from './dep'\nexport const b = a;\n",
    );

    let routes = owner_routes();

    // Cold: the real producer admits its own `(W_real, P_real)` candidate.
    assert!(
        host.admit_resolved_import_facts_for_owner("/owner.ts", &routes),
        "the cold producer run must admit a candidate"
    );

    let key = producer_key(&host, "/owner.ts");
    let db = host.project_type_store().resolved_import_facts();

    let w_real: Vec<FactVersionRef> = host
        .resolved_import_facts_witness_for(key.canonical.as_ref(), key.content_hash)
        .expect("the production witness must be rootable for the owner");
    let p_real = db
        .retained_bundle_for_tests(&key)
        .expect("the cold admission must retain its payload");
    assert!(
        !w_real.is_empty(),
        "the production witness must be non-empty or strict admission would refuse the seeds"
    );
    assert!(
        !p_real.import_clauses.is_empty(),
        "the owner must produce a non-trivial payload so `P_other` is genuinely different"
    );

    // Rebuild the slot into the cross-pair arrangement.
    db.clear();

    // `P_other` — a payload that is NOT what the producer will recompute.
    let p_other = Arc::new(ResolvedImportFacts::new());
    assert_ne!(
        p_other, p_real,
        "the decoy payload must differ from the producer's, or the trap is not set"
    );
    // `W_fake` — a witness that is NOT what the producer will observe, and is
    // non-empty so strict admission accepts it.
    let mut w_fake = w_real.clone();
    w_fake.push(FactVersionRef::FileWholeHash {
        canonical_id: "/dep.ts".to_string(),
        hash: [0xAB; 16],
    });
    assert_ne!(
        w_fake, w_real,
        "the decoy witness must differ from the producer's, or the trap is not set"
    );

    assert!(
        db.insert_if_absent(key.clone(), Arc::clone(&p_other), w_real.clone()),
        "the first seed (W_real, P_other) must be admitted"
    );
    assert!(
        db.insert_if_absent(key.clone(), Arc::clone(&p_real), w_fake.clone()),
        "the second seed (W_fake, P_real) must be admitted and become the LAST candidate"
    );

    // TRAP PROOF: both halves of an uncorrelated decision hold, yet NO single
    // candidate carries the pair the producer is about to recompute.
    let signatures = db.candidate_signatures_for_tests(&key);
    assert_eq!(
        signatures.len(),
        2,
        "the arrangement must retain exactly two candidates; got {}",
        signatures.len()
    );
    assert!(
        signatures
            .iter()
            .any(|sig| sig.as_ref() == w_real.as_slice()),
        "some candidate must carry W_real — the first half of the uncorrelated decision"
    );
    assert_eq!(
        db.retained_bundle_for_tests(&key),
        Some(Arc::clone(&p_real)),
        "the LAST candidate's payload must equal P_real — the second half of the \
         uncorrelated decision"
    );
    assert!(
        !db.holds_candidate_matching(&key, &w_real, p_real.as_ref()),
        "NO single candidate may hold both W_real and P_real — that absence is the \
         defect this test exists to catch"
    );

    // The producer recomputes exactly `(W_real, P_real)`.
    let admitted = host.admit_resolved_import_facts_for_owner("/owner.ts", &routes);
    assert!(
        admitted,
        "the producer must ADMIT: no retained candidate holds both the witness it \
         observed and the payload it built. Skipping here drops a fresh resolution \
         state on the strength of two unrelated candidates."
    );
    assert!(
        db.holds_candidate_matching(&key, &w_real, p_real.as_ref()),
        "after admission the slot must retain ONE candidate carrying both W_real and \
         P_real"
    );
}

/// CONTROL: the genuine-churn skip still skips. A recomputation whose witness
/// AND payload are both carried by the SAME retained candidate must not
/// re-admit — otherwise the correlated guard would have turned a dedupe into an
/// unconditional push and aged concurrent candidates out of the bounded slot.
///
/// This is the anti-vacuity partner of the test above: together they pin the
/// guard to "exactly one candidate holds both", not "always admit".
#[test]
fn producer_still_skips_a_recomputation_one_candidate_already_holds_whole() {
    let host = make_host();
    upsert_ts(host.as_ref(), "/dep.ts", "export const a = 1;\n");
    upsert_ts(
        host.as_ref(),
        "/owner.ts",
        "import { a } from './dep'\nexport const b = a;\n",
    );

    let routes = owner_routes();

    assert!(
        host.admit_resolved_import_facts_for_owner("/owner.ts", &routes),
        "the cold producer run must admit a candidate"
    );

    let key = producer_key(&host, "/owner.ts");
    let db = host.project_type_store().resolved_import_facts();
    let before = db.candidate_signatures_for_tests(&key).len();
    assert_eq!(before, 1, "the cold run must leave exactly one candidate");

    assert!(
        !host.admit_resolved_import_facts_for_owner("/owner.ts", &routes),
        "an identical recomputation is pure churn and must be skipped — the retained \
         candidate already holds BOTH this witness and this payload"
    );
    assert_eq!(
        db.candidate_signatures_for_tests(&key).len(),
        before,
        "a skipped recomputation must not push a duplicate candidate into the bounded slot"
    );
}
